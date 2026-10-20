//! Read-only facts and invocation-local history. No fact here proves successful
//! task observation or grants slot mutation authority.
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::admission::ControlAdmission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::filesystem::FileIdentity;
use crate::state::SlotLocation;

use super::session::{OwnerIdentity, RecordedRunRequest, SessionProgress, valid_run_id};
use super::sqlite::{RegistryRead, read_existing};
use super::status::{DbStatus, ExecutionOutcome, ProcessRole, ProcessStatus};
use super::{RegistryIdentity, RegistryReader, sql_error};
use crate::output::EvidenceRelativePath;
use crate::redaction::CaptureOutcome;

#[derive(Debug, Clone)]
pub(crate) struct Scope {
    location: SlotLocation,
    identity: RegistryIdentity,
}

impl Scope {
    pub(crate) fn new(
        admission: &ControlAdmission,
        location: &SlotLocation,
    ) -> RuntimeResult<Self> {
        let slot = crate::slot::select_slot(admission.manifest(), Some(location.slot().slot()))?;
        if location.slot().project() != admission.project_id()
            || location.slot().environment() != slot.environment
        {
            return Err(RuntimeError::new(
                ErrorCode::StateUnowned,
                "observation scope differs from admitted slot",
            ));
        }
        Ok(Self {
            location: location.clone(),
            identity: RegistryIdentity::for_slot(
                admission.project_id(),
                slot.environment,
                slot.slot,
                admission.runtime_abi(),
                admission.toolchain_id(),
            ),
        })
    }
    pub(crate) fn location(&self) -> &SlotLocation {
        &self.location
    }
    pub(crate) fn identity(&self) -> &RegistryIdentity {
        &self.identity
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) enum History {
    #[default]
    Unseen,
    Bootstrap(FileIdentity),
    Complete {
        file: FileIdentity,
        greatest: i64,
    },
}

impl History {
    fn absent(&self) -> RuntimeResult<()> {
        match self {
            Self::Unseen => Ok(()),
            Self::Bootstrap(_) | Self::Complete { .. } => {
                Err(invalid("checked registry history disappeared"))
            }
        }
    }
    fn advance(&mut self, next: Self) -> RuntimeResult<()> {
        match (&*self, &next) {
            (Self::Unseen, _) => {}
            (
                Self::Bootstrap(before),
                Self::Bootstrap(after) | Self::Complete { file: after, .. },
            ) if before == after => {}
            (
                Self::Complete {
                    file: before,
                    greatest: old,
                },
                Self::Complete {
                    file: after,
                    greatest: new,
                },
            ) if before == after && new >= old => {}
            _ => {
                return Err(invalid(
                    "checked registry history was replaced, reset or regressed",
                ));
            }
        }
        *self = next;
        Ok(())
    }
}

/// Closed private transport data. Decoding cannot mint enrollment or admission.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CursorWire {
    watermark: i64,
    history: HistoryWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
enum HistoryWire {
    Unseen {},
    Bootstrap {
        device: u64,
        inode: u64,
    },
    Complete {
        device: u64,
        inode: u64,
        #[serde(rename = "greatestSeq")]
        greatest: i64,
    },
}

impl CursorWire {
    fn from_checked(watermark: i64, history: &History) -> Self {
        let history = match history {
            History::Unseen => HistoryWire::Unseen {},
            History::Bootstrap(file) => HistoryWire::Bootstrap {
                device: file.device,
                inode: file.inode,
            },
            History::Complete { file, greatest } => HistoryWire::Complete {
                device: file.device,
                inode: file.inode,
                greatest: *greatest,
            },
        };
        Self { watermark, history }
    }
    pub(crate) fn checked(&self) -> RuntimeResult<History> {
        if self.watermark < 0 {
            return Err(invalid("invalid observation cursor watermark"));
        }
        match self.history {
            HistoryWire::Unseen {} if self.watermark == 0 => Ok(History::Unseen),
            HistoryWire::Bootstrap { device, inode } if self.watermark == 0 => {
                Ok(History::Bootstrap(FileIdentity { device, inode }))
            }
            HistoryWire::Complete {
                device,
                inode,
                greatest,
            } if greatest >= self.watermark => Ok(History::Complete {
                file: FileIdentity { device, inode },
                greatest,
            }),
            _ => Err(invalid("incoherent observation cursor history")),
        }
    }
    pub(crate) fn advance_to(&mut self, next: &Self) -> RuntimeResult<()> {
        if self.watermark != next.watermark {
            return Err(invalid("bootstrap changed the frozen watermark"));
        }
        let mut history = self.checked()?;
        history.advance(next.checked()?)?;
        *self = Self::from_checked(self.watermark, &history);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Publication {
    run_id: String,
    sequence: i64,
}

impl Publication {
    pub(crate) fn run_id(&self) -> &str {
        &self.run_id
    }
    pub(crate) fn sequence(&self) -> i64 {
        self.sequence
    }
}

/// A fixed selection with no acquisition or recovery interface.
pub struct PinnedRun {
    scope: Scope,
    publication: Publication,
    watermark: i64,
    history: History,
}

impl PinnedRun {
    pub fn run_id(&self) -> &str {
        self.publication.run_id()
    }
    pub fn created_sequence(&self) -> i64 {
        self.publication.sequence()
    }
    pub fn cursor(&self) -> CursorWire {
        CursorWire::from_checked(self.watermark, &self.history)
    }
    pub(crate) fn scope(&self) -> &Scope {
        &self.scope
    }
    pub(crate) fn read(&mut self) -> RuntimeResult<Option<Snapshot>> {
        let read = Snapshot::read(&self.scope, &mut self.history)?;
        if let Some(snapshot) = &read
            && snapshot.publication(self.run_id()) != Some(&self.publication)
        {
            return Err(invalid("selected creation marker disappeared or changed"));
        }
        Ok(read)
    }
}

/// A non-cloneable enrollment with its one frozen native event watermark.
pub struct Seeking {
    scope: Scope,
    watermark: i64,
    history: History,
}

impl Seeking {
    pub fn watermark(&self) -> i64 {
        self.watermark
    }
    pub fn cursor(&self) -> CursorWire {
        CursorWire::from_checked(self.watermark, &self.history)
    }
    pub(crate) fn scope(&self) -> &Scope {
        &self.scope
    }
    pub(crate) fn resume(scope: Scope, cursor: &CursorWire) -> RuntimeResult<Option<Self>> {
        let mut history = cursor.checked()?;
        if Snapshot::read(&scope, &mut history)?.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            scope,
            watermark: cursor.watermark,
            history,
        }))
    }
    pub(crate) fn merge_cursor(&mut self, cursor: &CursorWire) -> RuntimeResult<()> {
        if cursor.watermark != self.watermark {
            return Err(invalid("bootstrap changed the frozen watermark"));
        }
        self.history.advance(cursor.checked()?)
    }
    pub(crate) fn read(&mut self) -> RuntimeResult<Option<Snapshot>> {
        Snapshot::read(&self.scope, &mut self.history)
    }
    pub(crate) fn pin(self, publication: Publication) -> PinnedRun {
        PinnedRun {
            scope: self.scope,
            publication,
            watermark: self.watermark,
            history: self.history,
        }
    }
}

pub(crate) struct Snapshot {
    scope: Scope,
    history: History,
    contents: Contents,
}
enum Contents {
    Empty,
    Complete {
        reader: RegistryReader,
        maximum: i64,
        publications: BTreeMap<String, Publication>,
    },
}

impl Snapshot {
    /// None means only explicit SQLite contention; it establishes no watermark.
    pub(crate) fn read(scope: &Scope, history: &mut History) -> RuntimeResult<Option<Self>> {
        let contents = match read_existing(
            &scope.location.registry_path(),
            &scope.identity,
            Duration::ZERO,
        )? {
            RegistryRead::Busy(_) => return Ok(None),
            RegistryRead::Absent => {
                history.absent()?;
                Contents::Empty
            }
            RegistryRead::BootstrapEmpty(file) => {
                history.advance(History::Bootstrap(file))?;
                Contents::Empty
            }
            RegistryRead::Snapshot { reader, file } => {
                let (maximum, publications) = publications(&reader)?;
                history.advance(History::Complete {
                    file,
                    greatest: maximum,
                })?;
                Contents::Complete {
                    reader,
                    maximum,
                    publications,
                }
            }
        };
        Ok(Some(Self {
            scope: scope.clone(),
            history: history.clone(),
            contents,
        }))
    }
    pub(crate) fn maximum(&self) -> i64 {
        match self.contents {
            Contents::Empty => 0,
            Contents::Complete { maximum, .. } => maximum,
        }
    }
    pub(crate) fn publication(&self, run: &str) -> Option<&Publication> {
        match &self.contents {
            Contents::Empty => None,
            Contents::Complete { publications, .. } => publications.get(run),
        }
    }
    pub(crate) fn current(&self) -> RuntimeResult<Option<Publication>> {
        let Contents::Complete { reader, .. } = &self.contents else {
            return Ok(None);
        };
        let run: Option<String> = reader.connection().query_row(
            "SELECT run_id FROM events WHERE event_type = 'run.created' AND run_id IN (SELECT run_id FROM runs WHERE finalization = 'unfinished') ORDER BY seq DESC LIMIT 1",
            [], |row| row.get(0)).optional().map_err(sql_error)?;
        run.map(|run| {
            self.publication(&run)
                .cloned()
                .ok_or_else(|| invalid("unfinished run has no creation marker"))
        })
        .transpose()
    }
    pub(crate) fn first_after(&self, watermark: i64) -> RuntimeResult<Option<Publication>> {
        let Contents::Complete { reader, .. } = &self.contents else {
            return Ok(None);
        };
        let marker: Option<(i64, String)> = reader.connection().query_row(
            "SELECT seq, run_id FROM events WHERE event_type = 'run.created' AND seq > ?1 ORDER BY seq ASC LIMIT 1",
            [watermark], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(sql_error)?;
        marker
            .map(|(sequence, run)| {
                self.publication(&run)
                    .filter(|publication| publication.sequence == sequence)
                    .cloned()
                    .ok_or_else(|| invalid("creation marker has inconsistent provenance"))
            })
            .transpose()
    }
    pub(crate) fn into_seeking(self) -> Seeking {
        Seeking {
            watermark: self.maximum(),
            scope: self.scope,
            history: self.history,
        }
    }
    pub(crate) fn pin(self, publication: Publication) -> PinnedRun {
        PinnedRun {
            watermark: self.maximum(),
            scope: self.scope,
            publication,
            history: self.history,
        }
    }
    pub(crate) fn run(&self, run: &str) -> RuntimeResult<RunFacts> {
        let Contents::Complete { reader, .. } = &self.contents else {
            return Err(invalid("selected run is absent"));
        };
        let (manifest, hash, root, source, timeout, owner, outcome, finalization):
            (String, String, String, String, String, String, Option<String>, String) = reader.connection().query_row(
                "SELECT manifest_path, computed_manifest_hash, root_task, source_json, operation_timeout_ms, owner_identity, execution_outcome, finalization FROM runs WHERE run_id = ?1",
                [run], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?))).map_err(sql_error)?;
        if manifest.is_empty() || manifest.contains('\0') {
            return Err(invalid("invalid recorded manifest_path"));
        }
        Ok(RunFacts {
            manifest_path: manifest,
            request: RecordedRunRequest::decode(&hash, &root, &source, &timeout)?,
            owner: serde_json::from_str(&owner)
                .map_err(|_| invalid("invalid recorded owner_identity fields"))?,
            progress: super::session::decode(outcome.as_deref(), &finalization)
                .map_err(|_| invalid("invalid recorded session progress fields"))?,
        })
    }
    pub(crate) fn unfinished(&self) -> RuntimeResult<Vec<(Publication, RunFacts)>> {
        let Contents::Complete { publications, .. } = &self.contents else {
            return Ok(Vec::new());
        };
        let mut unfinished = Vec::new();
        // Decode every run before interpreting any as an idle predecessor.
        for publication in publications.values() {
            let facts = self.run(publication.run_id())?;
            if !matches!(facts.progress, SessionProgress::Finalized(_)) {
                unfinished.push((publication.clone(), facts));
            }
        }
        Ok(unfinished)
    }

    pub(crate) fn candidate(&self, run: &str) -> RuntimeResult<CandidateSnapshot> {
        let facts = self.run(run)?;
        let Contents::Complete { reader, .. } = &self.contents else {
            return Err(invalid("selected run is absent"));
        };
        let connection = reader.connection();
        let rows = connection.prepare(
            "SELECT process_key, pid, pgid, start_identity, role, service_name, root_step_path, status, ownership, capture, execution_outcome, exit_code, stdout_path, stderr_path FROM processes WHERE run_id = ?1 ORDER BY process_key COLLATE BINARY"
        ).map_err(sql_error)?.query_map([run], |row| Ok(RawProcess {
            key: row.get(0)?, pid: row.get(1)?, pgid: row.get(2)?, start: row.get(3)?,
            role: row.get(4)?, service: row.get(5)?, root: row.get(6)?, status: row.get(7)?,
            ownership: row.get(8)?, capture: row.get(9)?, outcome: row.get(10)?, exit: row.get(11)?,
            stdout: row.get(12)?, stderr: row.get(13)?,
        })).map_err(sql_error)?.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
        let mut roots = BTreeMap::new();
        let mut services = BTreeMap::new();
        for row in rows {
            let (attribution, process) = row.decode()?;
            let endpoints = connection.prepare("SELECT endpoint_id, address, port FROM ports WHERE owner_process_key = ?1 ORDER BY endpoint_id COLLATE BINARY").map_err(sql_error)?
                .query_map([&process.key], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, u16>(2)?)))
                .map_err(sql_error)?.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
            if !matches!(attribution, Attribution::Service(_)) && !endpoints.is_empty() {
                return Err(invalid(
                    "endpoint evidence belongs to a non-service process",
                ));
            }
            match attribution {
                Attribution::Root(path) => {
                    if roots.insert(path, process).is_some() {
                        return Err(invalid("duplicate root task attribution"));
                    }
                }
                Attribution::Service(service_id) => {
                    let mut coordinates = Vec::new();
                    let mut endpoint_ids = BTreeSet::new();
                    for (endpoint_id, host, port) in endpoints {
                        if crate::execution::EndpointId::parse(&endpoint_id).is_none()
                            || port == 0
                            || !endpoint_ids.insert(endpoint_id.clone())
                        {
                            return Err(invalid("invalid or duplicate service endpoint identity"));
                        }
                        let host = RecordedHost::parse(host)?;
                        coordinates.push(EndpointFacts {
                            endpoint_id,
                            host,
                            port,
                        });
                    }
                    if services
                        .insert(
                            service_id.clone(),
                            ServiceFacts {
                                service_id,
                                process,
                                endpoints: coordinates,
                            },
                        )
                        .is_some()
                    {
                        return Err(invalid("duplicate declared service process in one run"));
                    }
                }
                Attribution::Preparation | Attribution::Probe => {}
            }
        }
        Ok(CandidateSnapshot {
            run: facts,
            roots,
            services: services.into_values().collect(),
            successor: self
                .first_after(
                    self.publication(run)
                        .ok_or_else(|| invalid("selected run has no publication"))?
                        .sequence(),
                )?
                .is_some(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunFacts {
    pub(crate) manifest_path: String,
    pub(crate) request: RecordedRunRequest,
    pub(crate) owner: OwnerIdentity,
    pub(crate) progress: SessionProgress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CandidateSnapshot {
    pub(crate) run: RunFacts,
    pub(crate) roots: BTreeMap<String, ProcessFacts>,
    pub(crate) services: Vec<ServiceFacts>,
    pub(crate) successor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ownership {
    Unresolved,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureState {
    Pending,
    Finished(CaptureOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessFacts {
    pub(crate) key: String,
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) start: String,
    pub(crate) status: ProcessStatus,
    pub(crate) ownership: Ownership,
    pub(crate) capture: CaptureState,
    pub(crate) outcome: Option<ExecutionOutcome>,
    pub(crate) exit: Option<i32>,
    pub(crate) stdout: EvidenceRelativePath,
    pub(crate) stderr: EvidenceRelativePath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceFacts {
    pub(crate) service_id: String,
    pub(crate) process: ProcessFacts,
    pub(crate) endpoints: Vec<EndpointFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndpointFacts {
    pub(crate) endpoint_id: String,
    pub(crate) host: RecordedHost,
    pub(crate) port: u16,
}

/// Preserve the registry's exact address spelling while sharing the native
/// loopback validator. Coordinate comparison must not normalize stored bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct RecordedHost(String);

impl RecordedHost {
    fn parse(host: String) -> RuntimeResult<Self> {
        nixfied_manifest::LoopbackHost::parse(&host)
            .map_err(|_| invalid("invalid recorded endpoint address"))?;
        Ok(Self(host))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

enum Attribution {
    Root(String),
    Preparation,
    Service(String),
    Probe,
}

struct RawProcess {
    key: String,
    pid: u32,
    pgid: i32,
    start: String,
    role: String,
    service: Option<String>,
    root: Option<String>,
    status: String,
    ownership: String,
    capture: String,
    outcome: Option<String>,
    exit: Option<i32>,
    stdout: String,
    stderr: String,
}

impl RawProcess {
    fn decode(self) -> RuntimeResult<(Attribution, ProcessFacts)> {
        let role =
            ProcessRole::from_db(&self.role).ok_or_else(|| invalid("invalid process role"))?;
        let attribution = match (role, self.service, self.root) {
            (ProcessRole::Task, None, Some(path))
                if path
                    .split('.')
                    .all(nixfied_manifest::validation::is_step_safe_id) =>
            {
                Attribution::Root(path)
            }
            (ProcessRole::Task, None, None) => Attribution::Preparation,
            (ProcessRole::Service, Some(service), None)
                if nixfied_manifest::validation::is_step_safe_id(&service) =>
            {
                Attribution::Service(service)
            }
            (ProcessRole::Probe, Some(service), None)
                if nixfied_manifest::validation::is_step_safe_id(&service) =>
            {
                Attribution::Probe
            }
            _ => {
                return Err(invalid(
                    "process role and root/service attribution disagree",
                ));
            }
        };
        if self.key.is_empty()
            || self.key.contains('\0')
            || self.pid == 0
            || self.pid > i32::MAX as u32
            || self.pgid <= 0
            || self.pgid as u32 != self.pid
        {
            return Err(invalid("invalid canonical process identity fields"));
        }
        let identity: crate::service::StoredProcessIdentity = serde_json::from_str(&self.start)
            .map_err(|_| invalid("invalid process start_identity fields"))?;
        let start = identity
            .platform_start
            .filter(|start| !start.is_empty())
            .ok_or_else(|| invalid("process leader start identity is absent"))?;
        let status = ProcessStatus::from_db(&self.status)
            .ok_or_else(|| invalid("invalid process status"))?;
        let ownership = match self.ownership.as_str() {
            "unresolved" => Ownership::Unresolved,
            "settled" => Ownership::Settled,
            _ => return Err(invalid("invalid process ownership")),
        };
        let capture = match self.capture.as_str() {
            "pending" => CaptureState::Pending,
            "complete" => CaptureState::Finished(CaptureOutcome::Complete),
            "incomplete" => CaptureState::Finished(CaptureOutcome::Incomplete),
            "unknown" => CaptureState::Finished(CaptureOutcome::Unknown),
            _ => return Err(invalid("invalid process capture outcome")),
        };
        let outcome = self
            .outcome
            .map(|outcome| {
                ExecutionOutcome::from_db(&outcome)
                    .ok_or_else(|| invalid("invalid process execution outcome"))
            })
            .transpose()?;
        if (self.exit.is_some() && outcome.is_none())
            || (outcome == Some(ExecutionOutcome::Succeeded) && self.exit.is_none())
            || (ownership == Ownership::Settled
                && matches!(status, ProcessStatus::Running | ProcessStatus::Ready))
            || (role != ProcessRole::Service
                && status == ProcessStatus::Succeeded
                && capture == CaptureState::Pending)
        {
            return Err(invalid("contradictory canonical process evidence"));
        }
        Ok((
            attribution,
            ProcessFacts {
                key: self.key,
                pid: self.pid,
                pgid: self.pgid,
                start,
                status,
                ownership,
                capture,
                outcome,
                exit: self.exit,
                stdout: EvidenceRelativePath::task(&self.stdout)?,
                stderr: EvidenceRelativePath::task(&self.stderr)?,
            },
        ))
    }
}

fn publications(reader: &RegistryReader) -> RuntimeResult<(i64, BTreeMap<String, Publication>)> {
    let connection = reader.connection();
    let invalid_sequence: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE seq <= 0)",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if invalid_sequence {
        return Err(invalid(
            "event sequence is outside the positive native domain",
        ));
    }
    let maximum = connection
        .query_row("SELECT coalesce(max(seq), 0) FROM events", [], |row| {
            row.get(0)
        })
        .map_err(sql_error)?;
    let runs = connection
        .prepare("SELECT run_id FROM runs")
        .map_err(sql_error)?
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(sql_error)?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(sql_error)?;
    let markers = connection.prepare("SELECT seq, run_id, process_key FROM events WHERE event_type = 'run.created' ORDER BY seq").map_err(sql_error)?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)))
        .map_err(sql_error)?.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
    let mut publications = BTreeMap::new();
    for (sequence, run, process) in markers {
        if !valid_run_id(&run)
            || process.is_some()
            || !runs.contains(&run)
            || publications
                .insert(
                    run.clone(),
                    Publication {
                        run_id: run,
                        sequence,
                    },
                )
                .is_some()
        {
            return Err(invalid("invalid or duplicate run creation provenance"));
        }
    }
    if publications.len() != runs.len() {
        return Err(invalid("run is missing its creation marker"));
    }
    Ok((maximum, publications))
}

fn invalid(message: &'static str) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancellation::CancellationToken;
    use crate::registry::Registry;
    use crate::state::ownership::SlotGuard;

    struct Fixture {
        _root: crate::test_support::TestDir,
        scope: Scope,
    }
    impl Fixture {
        fn new() -> Self {
            let root = crate::test_support::TestDir::new("observation-history");
            let location = crate::state::derive_slot_location("project", "dev", 0, &root).unwrap();
            let identity = RegistryIdentity::for_slot("project", "dev", 0, "abi", "toolchain");
            SlotGuard::acquire(&location, &CancellationToken::new())
                .unwrap()
                .release()
                .unwrap();
            Self {
                _root: root,
                scope: Scope { location, identity },
            }
        }
        fn writer(&self) -> Registry {
            Registry::open_or_create(
                SlotGuard::acquire(self.scope.location(), &CancellationToken::new()).unwrap(),
                self.scope.identity(),
            )
            .unwrap()
        }
        fn read(&self, history: &mut History) -> RuntimeResult<Snapshot> {
            Snapshot::read(&self.scope, history)?
                .ok_or_else(|| invalid("unexpected contention in isolated history fixture"))
        }
    }

    fn publish(registry: &Registry, run: &str, failed: bool) {
        let connection = registry.connection();
        connection.execute(
            "INSERT INTO runs(run_id, execution_outcome, finalization, manifest_path, computed_manifest_hash, runtime_abi, toolchain_id, generator_json, target_json, source_json, owner_identity, diagnostic_path, root_task, operation_timeout_ms)
             VALUES (?1, ?2, ?3, '/manifest.json', ?4, 'abi', 'toolchain', '{}', '{}', ?5, '{\"pid\":1,\"platformStart\":\"recorded\"}', 'diagnostics.log', 'owner', '5000')",
            rusqlite::params![run, failed.then_some("failed"), if failed { "complete" } else { "unfinished" }, "a".repeat(64),
                r#"{"codebaseId":"main","logicalRoot":".","observedRoot":"/historical","sourceMode":"live-workspace","sourceIdentity":"workspace","dirtyPolicy":"allow","admissionFingerprintPolicy":""}"#],
        ).unwrap();
        connection.execute("INSERT INTO events(at, event_type, run_id, payload_json) VALUES ('now', 'run.created', ?1, '{}')", [run]).unwrap();
    }

    #[test]
    fn history_keeps_complete_zero_distinct_from_missing_and_bootstrap() {
        use std::os::unix::fs::MetadataExt;
        for fault in ["missing", "bootstrap", "replacement"] {
            let fixture = Fixture::new();
            let writer = fixture.writer();
            let path = writer.path().to_path_buf();
            let mut history = History::default();
            let checked = fixture.read(&mut history).unwrap();
            assert_eq!(checked.maximum(), 0);
            drop(checked);
            writer.close().unwrap();
            let inode = std::fs::metadata(&path).unwrap().ino();
            match fault {
                "missing" => std::fs::remove_file(&path).unwrap(),
                "bootstrap" => {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(&path)
                        .unwrap();
                    assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
                }
                "replacement" => {
                    std::fs::rename(&path, path.with_extension("old")).unwrap();
                    fixture.writer().close().unwrap();
                    assert_ne!(std::fs::metadata(&path).unwrap().ino(), inode);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                fixture.read(&mut history).err().unwrap().code,
                ErrorCode::RegistryCorrupt,
                "{fault}"
            );
        }
    }

    #[test]
    fn bootstrap_progresses_only_in_place_and_event_high_water_never_rewinds() {
        let fixture = Fixture::new();
        let mut raw = rusqlite::Connection::open(fixture.scope.location.registry_path()).unwrap();
        let mut history = History::default();
        drop(fixture.read(&mut history).unwrap());
        assert!(matches!(history, History::Bootstrap(_)));
        super::super::schema::initialize(&mut raw, fixture.scope.identity()).unwrap();
        drop(fixture.read(&mut history).unwrap());
        assert!(matches!(history, History::Complete { greatest: 0, .. }));
        raw.execute("INSERT INTO events(at,event_type,payload_json) VALUES ('now','independent-marker','{}')", []).unwrap();
        drop(fixture.read(&mut history).unwrap());
        raw.execute("DELETE FROM events", []).unwrap();
        assert_eq!(
            fixture.read(&mut history).err().unwrap().code,
            ErrorCode::RegistryCorrupt
        );
    }

    #[test]
    fn resumption_keeps_frozen_h_and_selects_first_publication_even_after_failure() {
        let fixture = Fixture::new();
        let writer = fixture.writer();
        for _ in 0..5 {
            writer.connection().execute("INSERT INTO events(at,event_type,payload_json) VALUES ('now','independent-marker','{}')", []).unwrap();
        }
        let mut history = History::default();
        let seeking = fixture.read(&mut history).unwrap().into_seeking();
        assert_eq!(seeking.watermark(), 5);
        let cursor = seeking.cursor();
        publish(&writer, "first-failed", true);
        publish(&writer, "later-owner", false);
        let mut resumed = Seeking::resume(fixture.scope.clone(), &cursor)
            .unwrap()
            .unwrap();
        assert_eq!(resumed.watermark(), 5);
        let snapshot = resumed.read().unwrap().unwrap();
        let selected = snapshot.first_after(resumed.watermark()).unwrap().unwrap();
        assert_eq!(selected.run_id(), "first-failed");
        assert_eq!(selected.sequence(), 6);
        let facts = snapshot.run(selected.run_id()).unwrap();
        assert_eq!(
            facts.progress,
            SessionProgress::Finalized(ExecutionOutcome::Failed)
        );
        drop(snapshot);
        let reply_cursor = resumed.cursor();
        let mut parent = seeking;
        parent.merge_cursor(&reply_cursor).unwrap();
        assert_eq!(parent.watermark(), 5);
        assert!(
            parent.merge_cursor(&cursor).is_err(),
            "learned high water cannot regress across reply"
        );
        let pinned = parent.pin(selected);
        assert_eq!(pinned.run_id(), "first-failed");
        assert_eq!(pinned.created_sequence(), 6);
    }

    #[test]
    fn every_creation_marker_is_validated_before_selection() {
        for fault in ["duplicate", "missing", "dangling", "null-run"] {
            let fixture = Fixture::new();
            let writer = fixture.writer();
            publish(&writer, "first", false);
            let connection = writer.connection();
            match fault {
                "duplicate" => {
                    connection.execute("INSERT INTO events(at,event_type,run_id,payload_json) VALUES ('now','run.created','first','{}')", []).unwrap();
                }
                "missing" => {
                    connection.execute("DELETE FROM events", []).unwrap();
                }
                "dangling" => {
                    connection
                        .pragma_update(None, "foreign_keys", false)
                        .unwrap();
                    connection
                        .execute("UPDATE events SET run_id = 'absent'", [])
                        .unwrap();
                }
                "null-run" => {
                    connection
                        .execute("UPDATE events SET run_id = NULL", [])
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(
                fixture.read(&mut History::default()).err().unwrap().code,
                ErrorCode::RegistryCorrupt,
                "{fault}"
            );
        }
    }

    #[test]
    fn cursor_is_closed_data_and_cannot_reset_known_history() {
        for invalid in [
            r#"{"watermark":1,"history":{"state":"unseen"}}"#,
            r#"{"watermark":1,"history":{"state":"bootstrap","device":1,"inode":2}}"#,
            r#"{"watermark":2,"history":{"state":"complete","device":1,"inode":2,"greatestSeq":1}}"#,
            r#"{"watermark":-1,"history":{"state":"unseen"}}"#,
        ] {
            let cursor: CursorWire = serde_json::from_str(invalid).unwrap();
            assert_eq!(
                cursor.checked().err().unwrap().code,
                ErrorCode::RegistryCorrupt
            );
        }
        for invalid in [
            r#"{"watermark":0,"history":null}"#,
            r#"{"watermark":0,"history":{"state":"unseen","extra":0}}"#,
            r#"{"watermark":0,"history":{"state":"complete","device":1,"inode":2}}"#,
        ] {
            assert!(serde_json::from_str::<CursorWire>(invalid).is_err());
        }
    }
}
