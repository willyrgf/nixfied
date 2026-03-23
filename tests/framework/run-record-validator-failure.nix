{ pkgs }:
let
  runtimeArtifactContracts = import ../../nixfied/framework/contracts/runtime-artifact-contracts.nix {
    inherit pkgs;
  };
  runRecordValidator = import ../../nixfied/framework/contracts/mkValidator.nix {
    inherit
      pkgs
      ;
    contractBundle = runtimeArtifactContracts;
    contractRef = "runtime.runRecord";
  };
in
pkgs.runCommand "run-record-validator-failure" { } ''
  set -euo pipefail

  cat > "$TMPDIR/valid.json" <<'EOF'
  {
    "kind": "run-record",
    "version": 1,
    "payload": {
      "run_id": "run-123",
      "attempt_id": "attempt-123",
      "command": "run-workflow",
      "workflow_id": "workflow.ci.basic",
      "task_id": null,
      "execution_mode": "workflow",
      "process_mode": "fg",
      "ephemeral_enabled": false,
      "state": "passed",
      "pid": null,
      "pgid": null,
      "exit_code": 0,
      "stop_reason": null,
      "created_at": "2026-03-23T00:00:00Z",
      "started_at": "2026-03-23T00:00:01Z",
      "finished_at": "2026-03-23T00:00:02Z",
      "updated_at": "2026-03-23T00:00:02Z",
      "args": [],
      "history": [
        {
          "state": "queued",
          "at": "2026-03-23T00:00:00Z"
        },
        {
          "state": "passed",
          "at": "2026-03-23T00:00:02Z"
        }
      ]
    }
  }
  EOF

  cat > "$TMPDIR/invalid-state.json" <<'EOF'
  {
    "kind": "run-record",
    "version": 1,
    "payload": {
      "run_id": "run-123",
      "attempt_id": "attempt-123",
      "command": "run-workflow",
      "workflow_id": "workflow.ci.basic",
      "task_id": null,
      "execution_mode": "workflow",
      "process_mode": "fg",
      "ephemeral_enabled": false,
      "state": "unknown",
      "pid": null,
      "pgid": null,
      "exit_code": 0,
      "stop_reason": null,
      "created_at": "2026-03-23T00:00:00Z",
      "started_at": "2026-03-23T00:00:01Z",
      "finished_at": "2026-03-23T00:00:02Z",
      "updated_at": "2026-03-23T00:00:02Z",
      "args": [],
      "history": [
        {
          "state": "queued",
          "at": "2026-03-23T00:00:00Z"
        }
      ]
    }
  }
  EOF

  cat > "$TMPDIR/unknown-field.json" <<'EOF'
  {
    "kind": "run-record",
    "version": 1,
    "payload": {
      "run_id": "run-123",
      "attempt_id": "attempt-123",
      "command": "run-workflow",
      "workflow_id": "workflow.ci.basic",
      "task_id": null,
      "execution_mode": "workflow",
      "process_mode": "fg",
      "ephemeral_enabled": false,
      "state": "passed",
      "pid": null,
      "pgid": null,
      "exit_code": 0,
      "stop_reason": null,
      "created_at": "2026-03-23T00:00:00Z",
      "started_at": "2026-03-23T00:00:01Z",
      "finished_at": "2026-03-23T00:00:02Z",
      "updated_at": "2026-03-23T00:00:02Z",
      "args": [],
      "history": [
        {
          "state": "queued",
          "at": "2026-03-23T00:00:00Z"
        }
      ],
      "unexpected": true
    }
  }
  EOF

  "${runRecordValidator}" "$TMPDIR/valid.json" > /dev/null

  if "${runRecordValidator}" "$TMPDIR/invalid-state.json" > /dev/null 2>"$TMPDIR/invalid-state.err"; then
    echo "run-record validator accepted an invalid state payload"
    exit 1
  fi

  if "${runRecordValidator}" "$TMPDIR/unknown-field.json" > /dev/null 2>"$TMPDIR/unknown-field.err"; then
    echo "run-record validator accepted an unknown-field payload"
    exit 1
  fi

  echo "OK: run-record validator rejects invalid payloads and unknown fields" > "$out"
''
