//! Enrollment and guarded shared establishment. Only Establish transfers a
//! writable registry into the existing session executor; selection never starts
//! recovery or a successor.
use std::time::Duration;

use crate::admission::ControlAdmission;
use crate::background::Establishment;
use crate::cancellation::CancellationToken;
use crate::error::{
    ErrorCode, ObservationSelection, ObservationStage, ObservationWait, RuntimeError,
    RuntimeResult, SessionUnavailableReason, session_unavailable,
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
    request: &DeclaredRunRequest<'_>,
    location: &SlotLocation,
    wait: &ObservationWait<'_>,
) -> RuntimeResult<Enrollment> {
    let expected = request.recorded();
    let scope = Scope::new(request.admission().common(), location)?;
    Context::from_location(location)?;
    let mut history = History::default();
    loop {
        wait.checkpoint(
            ObservationStage::Selection,
            &ObservationSelection::unenrolled(),
        )?;
        let Some(snapshot) = Snapshot::read(&scope, &mut history)? else {
            std::thread::sleep(wait.budget.remaining().min(Duration::from_millis(10)));
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
        compare_request(expected, &recorded, &ObservationSelection::pinned(&pinned))?;
        wait.checkpoint(
            ObservationStage::Selection,
            &ObservationSelection::pinned(&pinned),
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
        actual.recorded(),
        &ObservationSelection::seeking(&seeking),
    )?;
    loop {
        cancellation.check()?;
        precommit.check_abandonment()?;
        let Some(snapshot) = seeking.read()? else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        if let Some(publication) = snapshot.first_after(seeking.watermark()) {
            let recorded = snapshot.run(publication.run_id())?.request;
            drop(snapshot);
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
        if let Some(publication) = snapshot.first_after(seeking.watermark()) {
            let recorded = snapshot.run(publication.run_id());
            drop(snapshot);
            // Guard acquisition proves this selected publication released
            // authority. Release without opening a writer or running recovery.
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
            return crate::error::both(operation, guard.release());
        }
        drop(snapshot);
        if let Err(error) = precommit.learn_shared_cursor(seeking.cursor()) {
            return crate::error::both(Err(error), guard.release());
        }
        return Registry::open_or_create(guard, scope.identity()).map(SharedDecision::Establish);
    }
}

/// Validate a bounded helper selection against canonical facts and the original
/// enrollment, then transfer that one fixed selection to observation.
pub fn select_reply(
    mut seeking: Seeking,
    reply: &crate::background::SharedSelection,
    expected: &DeclaredRunRequest<'_>,
    wait: &ObservationWait<'_>,
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
        wait.checkpoint(
            ObservationStage::Establishment,
            &ObservationSelection::seeking(&seeking),
        )?;
        if let Some(snapshot) = seeking.read()? {
            let publication = snapshot
                .first_after(seeking.watermark())
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
                expected.recorded(),
                &recorded,
                &ObservationSelection::pinned(&pinned),
            )?;
            return Ok(pinned);
        }
        std::thread::sleep(wait.budget.remaining().min(Duration::from_millis(10)));
    }
}

fn corrupt(message: &'static str) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admission::RunAdmission;
    use crate::cancellation::WaitBudget;
    use crate::error::ObservationEstablishment;
    use crate::registry::session::{
        ExecutionOutcome, record_execution_outcome, record_finalization_complete,
        record_run_created,
    };
    use nixfied_manifest::{
        TaskId,
        fixtures::{SyntheticManifestOptions, synthetic_manifest},
    };

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
            let admission = crate::test_support::admit_manifest(&root, &manifest);
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
                &DeclaredRunRequest::new(&self.admission, &TaskId::new("smoke"), 5000).unwrap(),
                &self.location,
                &ObservationWait {
                    budget: &WaitBudget::new(std::num::NonZeroU64::new(1000).unwrap()),
                    cancellation: &CancellationToken::new(),
                    establishment: &ObservationEstablishment::not_requested(),
                    step: None,
                },
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
                None,
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
    fn committed_publication_carries_history_without_an_acknowledgement_reader() {
        use std::io::Read;
        use std::os::unix::fs::MetadataExt;

        let fixture = Fixture::new();
        let mut writer = fixture.writer();
        writer
            .append_event(crate::registry::EventInsert::new("before-enrollment", "{}"))
            .unwrap();
        let seeking = fixture.seeking();
        let (mut launcher, mut owner) = Establishment::shared_fixture(seeking.cursor());
        let mut purpose = crate::background::SessionPurpose::Detached(&mut owner);
        // Recovery can extend learned history without advancing enrollment.
        writer
            .append_event(crate::registry::EventInsert::new("recovery", "{}"))
            .unwrap();
        let placement = fixture.location.for_run("published").unwrap();
        record_run_created(
            &mut writer,
            "published",
            &DeclaredRunRequest::new(&fixture.admission, &TaskId::new("smoke"), 5000).unwrap(),
            &placement,
            purpose.shared_cursor(),
        )
        .unwrap();
        purpose.mark_committed();
        let sequence: i64 = writer
            .connection()
            .query_row(
                "SELECT seq FROM events WHERE event_type = 'run.created' AND run_id = 'published'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sequence, 3);
        let file = std::fs::metadata(writer.path()).unwrap();
        writer
            .connection()
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        let blocker = rusqlite::Connection::open(writer.path()).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
        // A reader here cannot make progress; acknowledgement uses only the
        // publication already validated in the committing transaction.
        purpose
            .acknowledge(crate::background::Acknowledgement {
                run_id: "published".to_owned(),
                run_dir: placement.run_dir(),
                logs_dir: placement.logs_dir(),
            })
            .unwrap();
        launcher.set_nonblocking(false).unwrap();
        launcher
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut header = [0; 8];
        launcher.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"NXD2");
        let mut body = vec![0; u32::from_be_bytes(header[4..].try_into().unwrap()) as usize];
        launcher.read_exact(&mut body).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({
                "result":"established", "runId":"published",
                "runDir":placement.run_dir(), "logsDir":placement.logs_dir(),
                "cursor":{"watermark":1,"history":{"state":"complete",
                    "device":file.dev(),"inode":file.ino(),"greatestSeq":3}}
            })
        );
        blocker.execute_batch("ROLLBACK").unwrap();
        writer.close().unwrap();
    }

    #[test]
    fn rejected_publication_rolls_back_run_and_keeps_the_learned_cursor() {
        for fault in [
            "event",
            "commit",
            "earlier",
            "rewind",
            "replacement",
            "duplicate",
            "schema",
            "identity",
            "version",
        ] {
            let fixture = Fixture::new();
            let mut cursor = fixture.seeking().cursor();
            let mut writer = fixture.writer();
            match fault {
                "event" => writer.connection().execute_batch("CREATE TRIGGER deny_creation BEFORE INSERT ON events WHEN NEW.event_type = 'run.created' BEGIN SELECT RAISE(ABORT, 'injected'); END").unwrap(),
                "commit" => {
                    unsafe extern "C" fn veto(_: *mut std::ffi::c_void) -> libc::c_int { 1 }
                    // Veto the actual native commit without changing schema.
                    unsafe { rusqlite::ffi::sqlite3_commit_hook(writer.connection().handle(), Some(veto), std::ptr::null_mut()); }
                }
                "earlier" => fixture.publish(&mut writer, "first-failed", 5000, true),
                "rewind" => {
                    // Learn actual high water, then regress the native file.
                    let mut seeking = fixture.seeking();
                    writer.append_event(crate::registry::EventInsert::new("learned", "{}")).unwrap();
                    writer.append_event(crate::registry::EventInsert::new("learned-again", "{}")).unwrap();
                    drop(seeking.read().unwrap().unwrap());
                    cursor = seeking.cursor();
                    writer.connection().execute_batch("DELETE FROM events").unwrap();
                }
                "replacement" => {
                    std::fs::rename(writer.path(), writer.path().with_extension("old")).unwrap();
                    std::fs::write(writer.path(), b"replacement").unwrap();
                }
                "duplicate" => {
                    fixture.publish(&mut writer, "prior", 5000, true);
                    writer.connection().execute_batch("INSERT INTO events(at,event_type,run_id,payload_json) VALUES ('now','run.created','prior','{}')").unwrap();
                }
                "schema" => writer.connection().execute_batch("CREATE TABLE unexpected (value TEXT)").unwrap(),
                "identity" => { writer.connection().execute("UPDATE registry_meta SET project_id = 'other'", []).unwrap(); }
                "version" => writer.connection().pragma_update(None, "user_version", 19).unwrap(),
                _ => unreachable!(),
            }
            let before = serde_json::to_value(&cursor).unwrap();
            let result = record_run_created(
                &mut writer,
                "rejected",
                &DeclaredRunRequest::new(&fixture.admission, &TaskId::new("smoke"), 5000).unwrap(),
                &fixture.location.for_run("rejected").unwrap(),
                Some(&mut cursor),
            );
            assert_eq!(
                result.unwrap_err().code,
                if fault == "identity" {
                    ErrorCode::StateUnowned
                } else {
                    ErrorCode::RegistryCorrupt
                },
                "{fault}"
            );
            assert_eq!(serde_json::to_value(&cursor).unwrap(), before, "{fault}");
            let count: i64 = writer.connection().query_row(
                "SELECT (SELECT count(*) FROM runs WHERE run_id = 'rejected') + (SELECT count(*) FROM events WHERE run_id = 'rejected')",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(count, 0, "{fault}");
            writer.close().unwrap();
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
                actual.recorded(),
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
                            let raw = rusqlite::Connection::open(fixture.location.registry_path())
                                .unwrap();
                            let mode: String = raw
                                .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
                                .unwrap();
                            assert_eq!(mode, "delete");
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
            let mode: String = raw
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            assert_eq!(mode, "delete", "selection must not initialize a writer");
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
            actual.recorded(),
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
