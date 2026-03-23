{ contracts }:
let
  t = contracts;

  nonEmptyString = t.string { minLength = 1; };
  anyString = t.string { };
  nullableString = t.union {
    options = [
      nonEmptyString
      (t.null { })
    ];
  };
  nonNegativeInt = t.integer { minimum = 0; };
  positiveInt = t.integer { minimum = 1; };
  exitCode = t.integer {
    minimum = 0;
    maximum = 255;
  };
  nullableInt = t.union {
    options = [
      nonNegativeInt
      (t.null { })
    ];
  };
  nullableExitCode = t.union {
    options = [
      exitCode
      (t.null { })
    ];
  };
  nullableBool = t.union {
    options = [
      (t.bool { })
      (t.null { })
    ];
  };
  nullablePid = t.union {
    options = [
      nonNegativeInt
      nonEmptyString
      (t.null { })
    ];
  };
  runState = t.enum {
    values = [
      "queued"
      "running"
      "passed"
      "failed"
      "canceled"
    ];
  };
  stepStatus = t.enum {
    values = [
      "passed"
      "skipped"
      "failed"
      "canceled"
    ];
  };
  eventState = t.enum {
    values = [
      "queued"
      "running"
      "passed"
      "failed"
      "canceled"
      "busy"
      "released"
      "starting"
      "ready"
      "stopped"
      "orphaned"
      "waiting"
      "unknown"
    ];
  };
in
{
  "runtime.summary" = t.record {
    doc = "Validated workflow summary envelope.";
    fields = {
      kind = t.field {
        schema = t.literal { value = "workflow-summary"; };
      };
      version = t.field {
        schema = t.literal { value = 1; };
      };
      payload = t.field {
        schema = t.ref { name = "runtime.summary.payload"; };
      };
    };
  };

  "runtime.summary.payload" = t.record {
    fields = {
      run_id = t.field { schema = nonEmptyString; };
      attempt_id = t.field { schema = nonEmptyString; };
      workflow_id = t.field { schema = nonEmptyString; };
      mode = t.field { schema = nonEmptyString; };
      exit_code = t.field { schema = exitCode; };
      started_at = t.field { schema = nonEmptyString; };
      finished_at = t.field { schema = nonEmptyString; };
      duration_seconds = t.field { schema = nonNegativeInt; };
      counts = t.field {
        schema = t.ref { name = "runtime.summary.counts"; };
      };
      steps = t.field {
        schema = t.list {
          elem = t.ref { name = "runtime.summary.step"; };
        };
      };
      timing = t.field {
        schema = t.ref { name = "runtime.summary.timing"; };
      };
    };
  };

  "runtime.summary.counts" = t.record {
    fields = {
      passed = t.field { schema = nonNegativeInt; };
      failed = t.field { schema = nonNegativeInt; };
      skipped = t.field { schema = nonNegativeInt; };
      canceled = t.field { schema = nonNegativeInt; };
    };
  };

  "runtime.summary.step" = t.record {
    fields = {
      name = t.field { schema = nonEmptyString; };
      status = t.field { schema = stepStatus; };
      state = t.field { schema = runState; };
      duration = t.field { schema = nonNegativeInt; };
      order = t.field { schema = nonNegativeInt; };
      workflow_id = t.field {
        schema = nullableString;
      };
      reason = t.field {
        schema = nullableString;
      };
      exit_code = t.field {
        schema = nullableExitCode;
      };
    };
  };

  "runtime.summary.timing" = t.record {
    fields = {
      total_duration = t.field { schema = nonNegativeInt; };
      setup_duration = t.field { schema = nonNegativeInt; };
      steps_duration = t.field { schema = nonNegativeInt; };
      teardown_duration = t.field { schema = nonNegativeInt; };
      accounted_duration = t.field { schema = nonNegativeInt; };
      untracked_duration = t.field { schema = nonNegativeInt; };
      parallelism = t.field {
        schema = t.ref { name = "runtime.summary.parallelism"; };
      };
    };
  };

  "runtime.summary.parallelism" = t.record {
    fields = {
      max_workers = t.field { schema = nullableInt; };
      peak_workers = t.field { schema = nullableInt; };
      canceled_count = t.field { schema = nullableInt; };
    };
  };

  "runtime.runRecord" = t.record {
    doc = "Validated orchestrator run-record envelope.";
    fields = {
      kind = t.field {
        schema = t.literal { value = "run-record"; };
      };
      version = t.field {
        schema = t.literal { value = 1; };
      };
      payload = t.field {
        schema = t.ref { name = "runtime.runRecord.payload"; };
      };
    };
  };

  "runtime.runRecord.payload" = t.record {
    fields = {
      run_id = t.field { schema = nonEmptyString; };
      attempt_id = t.field { schema = nonEmptyString; };
      command = t.field { schema = nonEmptyString; };
      workflow_id = t.field { schema = nullableString; };
      task_id = t.field { schema = nullableString; };
      execution_mode = t.field { schema = nonEmptyString; };
      process_mode = t.field {
        schema = t.enum {
          values = [
            "fg"
            "bg"
          ];
        };
      };
      ephemeral_enabled = t.field {
        schema = t.bool { };
      };
      state = t.field { schema = runState; };
      pid = t.field { schema = nullableInt; };
      pgid = t.field { schema = nullableInt; };
      exit_code = t.field { schema = nullableExitCode; };
      stop_reason = t.field { schema = nullableString; };
      created_at = t.field { schema = nonEmptyString; };
      started_at = t.field { schema = nullableString; };
      finished_at = t.field { schema = nullableString; };
      updated_at = t.field { schema = nonEmptyString; };
      args = t.field {
        schema = t.list {
          elem = anyString;
        };
      };
      history = t.field {
        schema = t.list {
          elem = t.ref { name = "runtime.runRecord.historyEntry"; };
          minItems = 1;
        };
      };
    };
  };

  "runtime.runRecord.historyEntry" = t.record {
    fields = {
      state = t.field { schema = runState; };
      at = t.field { schema = nonEmptyString; };
    };
  };

  "runtime.registryEvent" = t.record {
    doc = "Validated runtime registry event envelope.";
    fields = {
      kind = t.field {
        schema = t.literal { value = "runtime-event"; };
      };
      version = t.field {
        schema = t.literal { value = 1; };
      };
      payload = t.field {
        schema = t.ref { name = "runtime.registryEvent.payload"; };
      };
    };
  };

  "runtime.registryEvent.payload" = t.record {
    fields = {
      runId = t.field { schema = nonEmptyString; };
      attemptId = t.field { schema = nullableString; };
      workflowId = t.field { schema = nullableString; };
      taskId = t.field { schema = nullableString; };
      seq = t.field { schema = positiveInt; };
      ts = t.field { schema = nonEmptyString; };
      state = t.field { schema = eventState; };
      detail = t.field {
        schema = t.ref { name = "runtime.registryEvent.detail"; };
      };
    };
  };

  "runtime.registryEvent.detail" = t.record {
    closed = false;
    fields = {
      kind = t.field {
        required = false;
        schema = t.union {
          options = [
            (t.enum {
              values = [
                "slotLifecycle"
                "serviceLifecycle"
              ];
            })
            (t.null { })
          ];
        };
      };
      eventType = t.field {
        required = false;
        schema = nullableString;
      };
      commandName = t.field {
        required = false;
        schema = nullableString;
      };
      projectId = t.field {
        required = false;
        schema = nullableString;
      };
      service = t.field {
        required = false;
        schema = nullableString;
      };
      slot = t.field {
        required = false;
        schema = nullableString;
      };
      env = t.field {
        required = false;
        schema = nullableString;
      };
      profile = t.field {
        required = false;
        schema = nullableString;
      };
      pid = t.field {
        required = false;
        schema = nullablePid;
      };
      pgid = t.field {
        required = false;
        schema = nullablePid;
      };
      planId = t.field {
        required = false;
        schema = nullableString;
      };
      unitId = t.field {
        required = false;
        schema = nullableString;
      };
      attempt = t.field {
        required = false;
        schema = t.union {
          options = [
            positiveInt
            (t.null { })
          ];
        };
      };
      ownerScope = t.field {
        required = false;
        schema = nullableString;
      };
      reusePolicy = t.field {
        required = false;
        schema = nullableString;
      };
      discoveryScope = t.field {
        required = false;
        schema = nullableString;
      };
      ephemeralRoot = t.field {
        required = false;
        schema = nullableString;
      };
      readiness = t.field {
        required = false;
        schema = t.ref { name = "runtime.registryEvent.readiness"; };
      };
      waitReason = t.field {
        required = false;
        schema = nullableString;
      };
      logPath = t.field {
        required = false;
        schema = nullableString;
      };
      mode = t.field {
        required = false;
        schema = nullableString;
      };
      suffixReason = t.field {
        required = false;
        schema = nullableString;
      };
      produces = t.field {
        required = false;
        schema = t.ref { name = "runtime.registryEvent.produces"; };
      };
      exitCode = t.field {
        required = false;
        schema = nullableExitCode;
      };
      reason = t.field {
        required = false;
        schema = nullableString;
      };
      dependency = t.field {
        required = false;
        schema = nullableString;
      };
      serviceName = t.field {
        required = false;
        schema = nullableString;
      };
      signal = t.field {
        required = false;
        schema = nullableString;
      };
      missing = t.field {
        required = false;
        schema = nullableString;
      };
    };
  };

  "runtime.registryEvent.readiness" = t.record {
    fields = {
      healthOk = t.field {
        schema = nullableBool;
      };
      readyOk = t.field {
        schema = nullableBool;
      };
      lastError = t.field {
        schema = nullableString;
      };
    };
  };

  "runtime.registryEvent.produces" = t.record {
    closed = false;
    fields = {
      artifacts = t.field {
        required = false;
        schema = t.list {
          elem = anyString;
        };
      };
      stateKeys = t.field {
        required = false;
        schema = t.list {
          elem = anyString;
        };
      };
    };
  };
}
