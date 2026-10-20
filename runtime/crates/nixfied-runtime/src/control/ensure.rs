//! Enrollment and guarded shared establishment. Only Establish transfers a
//! writable registry into the existing session executor; selection never starts
//! recovery or a successor.
use std::time::Duration;

use nixfied_manifest::TaskId;

use crate::admission::{ControlAdmission, RunAdmission};
use crate::background::Establishment;
use crate::cancellation::{CancellationToken, WaitBudget};
use crate::error::{
    ErrorCode, ObservationEstablishment, ObservationSelection, ObservationStage, RuntimeError,
    RuntimeResult, SessionUnavailableReason, observation_checkpoint, session_unavailable,
};
use crate::registry::Registry;
use crate::registry::observation::{CursorWire, History, PinnedRun, Scope, Seeking, Snapshot};
use crate::registry::session::{DeclaredRunRequest, RecordedRunRequest};
use crate::service::{LeaderSample, sample_leader};
use crate::state::SlotLocation;
use crate::state::ownership::SlotGuard;

use super::observation::{Context, compare_request};

pub enum Enrollment {
    Pinned(PinnedRun),
    Seeking(Seeking),
}
pub enum SharedDecision {
    Selected(PinnedRun),
    Establish(Registry),
}

#[derive(serde::Serialize)]
pub struct EnsureOutput {
    pub disposition: super::observation::EnsureDisposition,
    pub observation: super::observation::TaskObservation,
}

pub fn enroll(
    admission: &RunAdmission,
    location: &SlotLocation,
    selected_task: &TaskId,
    operation_timeout_ms: u64,
    budget: &WaitBudget,
    cancellation: &CancellationToken,
) -> RuntimeResult<Enrollment> {
    let expected =
        DeclaredRunRequest::new(admission, selected_task, operation_timeout_ms)?.recorded()?;
    let scope = Scope::new(admission.common(), location)?;
    Context::from_location(location)?;
    let mut history = History::default();
    loop {
        observation_checkpoint(
            budget,
            cancellation,
            ObservationStage::Selection,
            &ObservationSelection::unenrolled(),
            &ObservationEstablishment::not_requested(),
            None,
        )?;
        let Some(snapshot) = Snapshot::read(&scope, &mut history)? else {
            std::thread::sleep(budget.remaining().min(Duration::from_millis(10)));
            continue;
        };
        let unfinished = snapshot.unfinished()?;
        let seeking = snapshot.into_seeking();
        let mut live = None;
        for (publication, facts) in unfinished {
            match sample_leader(facts.owner.pid())? {
                LeaderSample::Present {
                    start_identity,
                    zombie: false,
                    ..
                } if start_identity == facts.owner.start_identity() => {
                    if live.replace((publication, facts.request)).is_some() {
                        return Err(corrupt("multiple live unfinished owners in one slot"));
                    }
                }
                LeaderSample::Absent | LeaderSample::Present { .. } => {}
            }
        }
        let Some((publication, recorded)) = live else {
            return Ok(Enrollment::Seeking(seeking));
        };
        let pinned = seeking.pin(publication);
        compare_request(&expected, &recorded, &ObservationSelection::pinned(&pinned))?;
        observation_checkpoint(
            budget,
            cancellation,
            ObservationStage::Selection,
            &ObservationSelection::pinned(&pinned),
            &ObservationEstablishment::not_requested(),
            None,
        )?;
        return Ok(Enrollment::Pinned(pinned));
    }
}

/// Resumption binds wire data to authoritative admission and current native
/// history. Busy reads neither reenroll nor advance the frozen watermark.
pub fn resume_from_wire(
    admission: &ControlAdmission,
    location: &SlotLocation,
    cursor: &CursorWire,
    precommit: &mut Establishment,
    cancellation: &CancellationToken,
) -> RuntimeResult<Seeking> {
    let scope = Scope::new(admission, location)?;
    loop {
        cancellation.check()?;
        precommit.check_abandonment()?;
        if let Some(seeking) = Seeking::resume(scope.clone(), cursor)? {
            return Ok(seeking);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn acquire_shared(
    actual: &DeclaredRunRequest<'_>,
    location: &SlotLocation,
    seeking: Seeking,
    expected: &RecordedRunRequest,
    precommit: &mut Establishment,
    cancellation: &CancellationToken,
) -> RuntimeResult<SharedDecision> {
    acquire_shared_at_boundaries(
        actual,
        location,
        seeking,
        expected,
        precommit,
        cancellation,
        |_| Ok(()),
    )
}

#[derive(Clone, Copy)]
enum AcquisitionBoundary {
    BeforeGuard,
    UnderGuard,
}

fn acquire_shared_at_boundaries(
    actual: &DeclaredRunRequest<'_>,
    location: &SlotLocation,
    mut seeking: Seeking,
    expected: &RecordedRunRequest,
    precommit: &mut Establishment,
    cancellation: &CancellationToken,
    mut boundary: impl FnMut(AcquisitionBoundary) -> RuntimeResult<()>,
) -> RuntimeResult<SharedDecision> {
    let scope = Scope::new(actual.admission().common(), location)?;
    if seeking.scope().location() != scope.location()
        || seeking.scope().identity() != scope.identity()
    {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "resumed seeking scope differs from actual admission",
        ));
    }
    compare_request(
        expected,
        &actual.recorded()?,
        &ObservationSelection::seeking(&seeking),
    )?;
    loop {
        cancellation.check()?;
        precommit.check_abandonment()?;
        let Some(snapshot) = seeking.read()? else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        if let Some(publication) = snapshot.first_after(seeking.watermark())? {
            let recorded = snapshot.run(publication.run_id())?.request;
            drop(snapshot);
            precommit.learn_shared_cursor(seeking.cursor())?;
            let pinned = seeking.pin(publication);
            compare_request(expected, &recorded, &ObservationSelection::pinned(&pinned))?;
            return Ok(SharedDecision::Selected(pinned));
        }
        drop(snapshot);
        boundary(AcquisitionBoundary::BeforeGuard)?;
        let Some(guard) = SlotGuard::try_acquire(location, cancellation)? else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        // This check precedes writer initialization or journal conversion.
        let checked = (|| {
            boundary(AcquisitionBoundary::UnderGuard)?;
            loop {
                cancellation.check()?;
                precommit.check_abandonment()?;
                if let Some(snapshot) = seeking.read()? {
                    return Ok(snapshot);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        let snapshot = match checked {
            Ok(snapshot) => snapshot,
            Err(error) => return crate::error::both(Err(error), guard.release()),
        };
        let publication = snapshot.first_after(seeking.watermark());
        let publication = match publication {
            Ok(publication) => publication,
            Err(error) => {
                drop(snapshot);
                return crate::error::both(Err(error), guard.release());
            }
        };
        if let Some(publication) = publication {
            let recorded = snapshot.run(publication.run_id());
            drop(snapshot);
            // Guard acquisition proves this selected publication released
            // authority. Close the checked registry without any recovery.
            let registry = Registry::open_or_create(guard, scope.identity())?;
            let pinned = seeking.pin(publication);
            let operation = recorded.and_then(|facts| {
                compare_request(
                    expected,
                    &facts.request,
                    &ObservationSelection::pinned(&pinned),
                )?;
                Err(session_unavailable(
                    SessionUnavailableReason::AuthorityReleased,
                    &ObservationSelection::pinned(&pinned),
                    None,
                    None,
                ))
            });
            return crate::error::both(operation, registry.close());
        }
        drop(snapshot);
        if let Err(error) = precommit.learn_shared_cursor(seeking.cursor()) {
            return crate::error::both(Err(error), guard.release());
        }
        return Registry::open_or_create(guard, scope.identity()).map(SharedDecision::Establish);
    }
}

/// Recheck the helper's learned continuity and exact creation marker after the
/// commit. Observer abandonment is no longer a cancellation input here.
pub fn committed_cursor(
    admission: &ControlAdmission,
    location: &SlotLocation,
    cursor: &CursorWire,
    run_id: &str,
    cancellation: &CancellationToken,
) -> RuntimeResult<CursorWire> {
    let scope = Scope::new(admission, location)?;
    loop {
        cancellation.check()?;
        if let Some(mut seeking) = Seeking::resume(scope.clone(), cursor)?
            && let Some(snapshot) = seeking.read()?
        {
            let publication = snapshot
                .first_after(seeking.watermark())?
                .ok_or_else(|| corrupt("established run has no post-enrollment creation marker"))?;
            if publication.run_id() != run_id {
                return Err(corrupt(
                    "established run is not the first post-enrollment publication",
                ));
            }
            drop(snapshot);
            return Ok(seeking.cursor());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Validate a bounded helper selection against canonical facts and the original
/// enrollment, then transfer that one fixed selection to observation.
pub fn select_reply(
    mut seeking: Seeking,
    reply: &crate::background::SharedSelection,
    expected: &DeclaredRunRequest<'_>,
    budget: &WaitBudget,
    cancellation: &CancellationToken,
) -> RuntimeResult<PinnedRun> {
    let (run_id, created_sequence, cursor) = match reply {
        crate::background::SharedSelection::Established {
            acknowledgement,
            cursor,
        } => (acknowledgement.run_id.as_str(), None, cursor),
        crate::background::SharedSelection::Existing {
            run_id,
            created_sequence,
            cursor,
        } => (run_id.as_str(), Some(created_sequence.get()), cursor),
    };
    seeking.merge_cursor(cursor)?;
    loop {
        observation_checkpoint(
            budget,
            cancellation,
            ObservationStage::Establishment,
            &ObservationSelection::seeking(&seeking),
            &ObservationEstablishment::not_requested(),
            None,
        )?;
        if let Some(snapshot) = seeking.read()? {
            let publication = snapshot
                .first_after(seeking.watermark())?
                .ok_or_else(|| corrupt("bootstrap selection has no canonical publication"))?;
            if publication.run_id() != run_id
                || created_sequence.is_some_and(|sequence| publication.sequence() != sequence)
            {
                return Err(corrupt(
                    "bootstrap selection differs from the first publication",
                ));
            }
            let recorded = snapshot.run(publication.run_id())?.request;
            drop(snapshot);
            let pinned = seeking.pin(publication);
            compare_request(
                &expected.recorded()?,
                &recorded,
                &ObservationSelection::pinned(&pinned),
            )?;
            return Ok(pinned);
        }
        std::thread::sleep(budget.remaining().min(Duration::from_millis(10)));
    }
}

fn corrupt(message: &'static str) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::session::{
        ExecutionOutcome, record_execution_outcome, record_finalization_complete,
        record_run_created,
    };
    use nixfied_manifest::fixtures::{SyntheticManifestOptions, synthetic_manifest};

    struct Fixture {
        root: crate::test_support::TestDir,
        admission: RunAdmission,
        location: SlotLocation,
    }
    impl Fixture {
        fn new() -> Self {
            let root = crate::test_support::TestDir::new("shared-acquisition");
            let manifest = synthetic_manifest(&SyntheticManifestOptions {
                executable: std::env::var("NIXFIED_TEST_CHILD").unwrap(),
                ..Default::default()
            });
            let path = root.join("manifest.json");
            std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            let mut context =
                crate::AdmissionContext::current(crate::StoreOriginPolicy::AllowNonStoreForTests);
            context.invocation_root = crate::admission::InvocationRoot::Path(root.to_path_buf());
            let admission = crate::admit_run(&path, &context).unwrap();
            let location = crate::state::derive_slot_location(
                admission.common().project_id(),
                "dev",
                0,
                root.join("state"),
            )
            .unwrap();
            Self {
                root,
                admission,
                location,
            }
        }
        fn writer(&self) -> Registry {
            Registry::open_or_create(
                SlotGuard::acquire(&self.location, &CancellationToken::new()).unwrap(),
                Scope::new(self.admission.common(), &self.location)
                    .unwrap()
                    .identity(),
            )
            .unwrap()
        }
        fn seeking(&self) -> Seeking {
            let Enrollment::Seeking(seeking) = enroll(
                &self.admission,
                &self.location,
                &TaskId::new("smoke"),
                5000,
                &WaitBudget::new(std::num::NonZeroU64::new(1000).unwrap()),
                &CancellationToken::new(),
            )
            .unwrap() else {
                panic!("new slot was not idle");
            };
            seeking
        }
        fn publish(&self, writer: &mut Registry, run: &str, timeout: u64, failed: bool) {
            record_run_created(
                writer,
                run,
                &DeclaredRunRequest::new(&self.admission, &TaskId::new("smoke"), timeout).unwrap(),
                &self.location.for_run(run).unwrap(),
            )
            .unwrap();
            if failed {
                record_execution_outcome(
                    writer,
                    run,
                    self.admission.common().computed_manifest_hash(),
                    ExecutionOutcome::Failed,
                )
                .unwrap();
                record_finalization_complete(
                    writer,
                    run,
                    self.admission.common().computed_manifest_hash(),
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn guarded_recheck_refuses_first_failed_publication_without_recovery_or_new_run() {
        for mismatch in [false, true] {
            let fixture = Fixture::new();
            let seeking = fixture.seeking();
            let (_launcher, mut precommit) = Establishment::shared_fixture(seeking.cursor());
            let actual =
                DeclaredRunRequest::new(&fixture.admission, &TaskId::new("smoke"), 5000).unwrap();
            let mut published = false;
            let mut guarded = false;
            let result = acquire_shared_at_boundaries(
                &actual,
                &fixture.location,
                seeking,
                &actual.recorded().unwrap(),
                &mut precommit,
                &CancellationToken::new(),
                |phase| {
                    match phase {
                        AcquisitionBoundary::BeforeGuard if !published => {
                            let mut writer = fixture.writer();
                            fixture.publish(
                                &mut writer,
                                "first-failed",
                                if mismatch { 0 } else { 5000 },
                                true,
                            );
                            fixture.publish(&mut writer, "later-matching", 5000, false);
                            writer.close().unwrap();
                            published = true;
                        }
                        AcquisitionBoundary::UnderGuard => guarded = true,
                        AcquisitionBoundary::BeforeGuard => {}
                    }
                    Ok(())
                },
            );
            let error = result.err().expect("released publication cannot establish");
            assert!(guarded);
            assert_eq!(
                error.code,
                if mismatch {
                    ErrorCode::RequestMismatch
                } else {
                    ErrorCode::SessionUnavailable
                }
            );
            if !mismatch {
                assert_eq!(
                    error.details["sessionObservation"]["reason"],
                    "authority-released"
                );
            }
            assert_eq!(
                error.details["sessionObservation"]["selection"],
                serde_json::json!({"state":"pinned","runId":"first-failed"})
            );
            let raw = rusqlite::Connection::open(fixture.location.registry_path()).unwrap();
            let counts: (i64,i64,i64) = raw.query_row("SELECT (SELECT count(*) FROM runs),
                (SELECT count(*) FROM processes), (SELECT count(*) FROM events WHERE event_type NOT IN ('run.created','run.execution-settled','run.finalized'))",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
            assert_eq!(
                counts,
                (2, 0, 0),
                "selection must not recover or start a successor"
            );
            assert!(!fixture.location.state_root().exists());
            assert!(fixture.root.exists());
        }
    }

    #[test]
    fn publication_while_authority_is_held_selects_first_even_if_it_failed() {
        let fixture = Fixture::new();
        let seeking = fixture.seeking();
        let (_launcher, mut precommit) = Establishment::shared_fixture(seeking.cursor());
        let actual =
            DeclaredRunRequest::new(&fixture.admission, &TaskId::new("smoke"), 5000).unwrap();
        let mut writer = None;
        let result = acquire_shared_at_boundaries(
            &actual,
            &fixture.location,
            seeking,
            &actual.recorded().unwrap(),
            &mut precommit,
            &CancellationToken::new(),
            |phase| {
                if matches!(phase, AcquisitionBoundary::BeforeGuard) && writer.is_none() {
                    let mut publisher = fixture.writer();
                    fixture.publish(&mut publisher, "failed-first", 5000, true);
                    fixture.publish(&mut publisher, "live-later", 5000, false);
                    writer = Some(publisher);
                }
                Ok(())
            },
        )
        .unwrap();
        let SharedDecision::Selected(pinned) = result else {
            panic!("selected work was restarted");
        };
        assert_eq!(pinned.run_id(), "failed-first");
        assert_eq!(pinned.created_sequence(), 1);
        writer.unwrap().close().unwrap();
    }
}
