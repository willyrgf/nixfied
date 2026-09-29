use std::collections::BTreeMap;
use std::path::Path;

use super::endpoint::{CompleteWitnessSet, ListenerWitness};
use crate::cancellation::{CancellationToken, canceled_error};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::ProbePolicy;
use crate::redaction::Redactor;
use crate::service::process::{
    CapturedExec, CapturedExecOutcome, Invocation, RenderedInvocation, resolve_exec_cwd,
    spawn_gated_captured_exec,
};
use crate::service::registry::{InvocationOwner, TaskTerminalStatus, mark_invocation_finished};

include!("../generated/readiness.rs");

impl ProbePhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Health => "health",
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoundFailure {
    pub(crate) phase: ProbePhase,
    pub(crate) reason: RoundReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) endpoint_id: Option<String>,
}

/// Minted only after exit zero and successful process/capture settlement.
#[derive(Debug)]
pub(crate) struct SuccessfulProbe(String);
pub(crate) enum ProbeAttempt {
    Succeeded(SuccessfulProbe),
    Failed,
    TimedOut,
}

/// One attempt owns its witnesses and successful invocations. This value is
/// consumed on completion; retries cannot inherit previous successes.
pub(crate) struct CheckingRound {
    phase: ProbePhase,
    initial: Option<CompleteWitnessSet>,
    probes: BTreeMap<Option<String>, SuccessfulProbe>,
}
impl CheckingRound {
    pub(crate) fn begin(phase: ProbePhase, initial: Option<CompleteWitnessSet>) -> Self {
        Self {
            phase,
            initial,
            probes: BTreeMap::new(),
        }
    }
    pub(crate) fn initial(&self) -> Option<&CompleteWitnessSet> {
        self.initial.as_ref()
    }
    pub(crate) fn record(
        &mut self,
        endpoint: Option<String>,
        probe: SuccessfulProbe,
    ) -> RuntimeResult<()> {
        if self.probes.insert(endpoint, probe).is_some() {
            return Err(RuntimeError::new(
                ErrorCode::LifecycleFailed,
                "duplicate probe credit in check round",
            ));
        }
        Ok(())
    }
    pub(crate) fn complete(
        self,
        final_set: Option<CompleteWitnessSet>,
    ) -> RuntimeResult<CompletedRound> {
        let evidence = match (self.initial, final_set) {
            (Some(initial), Some(final_set))
                if initial.iter().next().is_some()
                    && initial.matches(&final_set)
                    && self
                        .probes
                        .keys()
                        .filter_map(Option::as_ref)
                        .eq(final_set.iter().map(|(id, _)| id))
                    && !self.probes.contains_key(&None) =>
            {
                let mut probes = self.probes;
                let mut entries = Vec::new();
                for (id, witness) in final_set.into_entries() {
                    let probe = probes.remove(&Some(id)).expect("checked probe key set");
                    entries.push((witness, probe));
                }
                RoundEvidence::Endpoints(entries)
            }
            (None, None) if self.probes.len() == 1 && self.probes.contains_key(&None) => {
                RoundEvidence::Endpointless(
                    self.probes.into_values().next().expect("one scalar probe"),
                )
            }
            _ => {
                return Err(RuntimeError::new(
                    ErrorCode::LifecycleFailed,
                    "incomplete or mismatched check round",
                ));
            }
        };
        Ok(CompletedRound {
            phase: self.phase,
            evidence,
        })
    }
}

enum RoundEvidence {
    Endpointless(SuccessfulProbe),
    Endpoints(Vec<(ListenerWitness, SuccessfulProbe)>),
}
/// The registry accepts only this private, complete proof, never parallel arrays.
pub(crate) struct CompletedRound {
    phase: ProbePhase,
    evidence: RoundEvidence,
}
impl CompletedRound {
    pub(crate) fn phase(&self) -> ProbePhase {
        self.phase
    }
    pub(crate) fn endpoints(&self) -> impl Iterator<Item = (&ListenerWitness, &str)> {
        match &self.evidence {
            RoundEvidence::Endpoints(entries) => Some(entries.iter()),
            RoundEvidence::Endpointless(_) => None,
        }
        .into_iter()
        .flatten()
        .map(|(witness, probe)| (witness, probe.0.as_str()))
    }
    pub(crate) fn probes(&self) -> Vec<&str> {
        match &self.evidence {
            RoundEvidence::Endpointless(probe) => vec![probe.0.as_str()],
            RoundEvidence::Endpoints(entries) => {
                entries.iter().map(|(_, probe)| probe.0.as_str()).collect()
            }
        }
    }
}

pub(crate) struct ExecProbe<'a> {
    pub command: &'a RenderedInvocation,
    pub source_root: &'a Path,
    pub logs_dir: &'a Path,
    pub redactor: &'a Redactor,
    pub registry: &'a mut crate::registry::Registry,
    pub launcher: &'a Path,
    pub run_id: &'a str,
    pub service_name: &'a str,
    pub manifest_hash: &'a str,
    pub occurrence: u64,
}

/// Execute one invocation probe attempt: run the probe's bound exec
/// (args/env already substituted at service start) in its own process group
/// with the probe's per-attempt deadline; exit 0 is success. Each attempt's
/// output is retained in exclusive occurrence files, preserving earlier attempts.
pub(crate) fn exec_probe_attempt(
    probe: &ProbePolicy,
    invocation: ExecProbe<'_>,
    cancellation: &CancellationToken,
    checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
) -> RuntimeResult<ProbeAttempt> {
    let ExecProbe {
        command,
        source_root,
        logs_dir,
        redactor,
        registry,
        launcher,
        run_id,
        service_name,
        manifest_hash,
        occurrence,
    } = invocation;
    let command_cwd = resolve_exec_cwd(source_root, &command.cwd)?;
    // Each probe attempt owns new logs outside the default live display.
    let evidence = crate::output::EvidenceSource::in_logs(
        logs_dir,
        service_name,
        crate::output::SourcePresentation::Hidden,
        &format!(
            "lifecycle.{service_name}.{}.probe.{occurrence}",
            probe.label
        ),
    );
    let stdout_path = evidence.stdout.clone();
    let stderr_path = evidence.stderr.clone();
    cancellation.check()?;
    checkpoint()?;
    let pending = spawn_gated_captured_exec(
        &CapturedExec {
            authority: registry.authority(),
            executable: &command.executable,
            args: &command.args,
            env: &command.env,
            cwd: &command_cwd,
            stdin: command.stdin,
            timeout: Some(probe.timeout),
            stdout_path: &stdout_path,
            stderr_path: &stderr_path,
            redactor,
            label: &format!("lifecycle operation {}", probe.label),
        },
        launcher,
    )?;
    let command_json = serde_json::json!({
        "label": probe.label, "serviceId": service_name, "executable": command.executable,
        "args": command.args, "cwd": command_cwd, "stdoutPath": stdout_path, "stderrPath": stderr_path,
    }).to_string();
    let invocation = Invocation {
        owner: InvocationOwner::Probe(service_name),
        run_id,
        manifest_hash,
        source: &evidence,
        command_json: &command_json,
        terminal: &probe_terminal,
        canceling: &|_, _| "{}".to_string(),
    };
    let (process_key, outcome) = invocation
        .run(
            registry,
            pending,
            |pid| format!("process-{run_id}-probe-{service_name}-{occurrence}-{pid}"),
            cancellation,
            checkpoint,
        )
        .map_err(|failure| *failure.error)?;
    mark_invocation_finished(
        registry,
        invocation.identity(&process_key),
        probe_terminal(&outcome).1,
        "{}",
        Some(crate::redaction::CaptureOutcome::Complete),
    )?;
    Ok(match outcome {
        CapturedExecOutcome::Canceled => return Err(canceled_error()),
        CapturedExecOutcome::Exited(status) if status.success() => {
            ProbeAttempt::Succeeded(SuccessfulProbe(process_key))
        }
        CapturedExecOutcome::Exited(_) => ProbeAttempt::Failed,
        CapturedExecOutcome::TimedOut => ProbeAttempt::TimedOut,
    })
}

fn probe_terminal(outcome: &CapturedExecOutcome) -> (Option<i32>, TaskTerminalStatus) {
    match outcome {
        CapturedExecOutcome::Exited(status) if status.success() => {
            (status.code(), TaskTerminalStatus::Succeeded)
        }
        CapturedExecOutcome::Exited(status) => (status.code(), TaskTerminalStatus::Failed),
        CapturedExecOutcome::Canceled => (None, TaskTerminalStatus::Canceled),
        CapturedExecOutcome::TimedOut => (None, TaskTerminalStatus::TimedOut),
    }
}

// Registry fault tests use real kernel witnesses while inserting probe outcomes
// directly into their isolated database.
#[cfg(test)]
pub(super) fn observed_test_round(
    endpoint: &crate::service::SelectedEndpoint,
    phase: ProbePhase,
    probe_key: &str,
) -> CompletedRound {
    use super::endpoint::{ExpectedOwner, ListenerObservation, observe_listeners};
    let pid = std::process::id();
    let pgid = super::process::process_group(pid).unwrap().unwrap();
    let start = super::process::platform_start_identity(pid).unwrap();
    let owner = ExpectedOwner {
        pid,
        pgid,
        platform_start: &start,
        containment: nixfied_manifest::ContainmentRequirement::ProcessGroup,
    };
    let ListenerObservation::Complete(initial) =
        observe_listeners([endpoint], &owner, None).unwrap()
    else {
        panic!("test listener missing")
    };
    let ListenerObservation::Complete(final_set) =
        observe_listeners([endpoint], &owner, Some(&initial)).unwrap()
    else {
        panic!("test listener changed")
    };
    let mut round = CheckingRound::begin(phase, Some(initial));
    round
        .record(
            Some(endpoint.endpoint_id.clone()),
            SuccessfulProbe(probe_key.into()),
        )
        .unwrap();
    round.complete(Some(final_set)).unwrap()
}
