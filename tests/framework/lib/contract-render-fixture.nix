{ contracts }:
let
  t = contracts.types;
in
t.mkBundle {
  version = 1;
  definitions = {
    "runtime.event" = t.taggedUnion {
      tag = "kind";
      variants = {
        queued = t.record {
          fields = {
            run_id = t.field {
              schema = t.string { minLength = 1; };
            };
            kind = t.field {
              schema = t.literal { value = "queued"; };
            };
          };
        };

        passed = t.record {
          fields = {
            exit_code = t.field {
              schema = t.integer {
                minimum = 0;
                maximum = 255;
              };
            };
            kind = t.field {
              schema = t.literal { value = "passed"; };
            };
            message = t.field {
              required = false;
              schema = t.union {
                options = [
                  (t.string { minLength = 1; })
                  (t.null { })
                ];
              };
            };
          };
        };
      };
    };

    "runtime.summary" = t.record {
      doc = "Workflow summary envelope.";
      fields = {
        version = t.field {
          schema = t.literal { value = 1; };
        };
        payload = t.field {
          schema = t.ref { name = "runtime.summary.payload"; };
        };
        kind = t.field {
          schema = t.literal { value = "workflow-summary"; };
        };
      };
    };

    "machineOutput.result" = t.record {
      doc = "Validated machine-output payload.";
      fields = {
        output_path = t.field {
          required = false;
          schema = t.string {
            minLength = 1;
            pattern = "^/.*$";
          };
        };
        metadata = t.field {
          required = false;
          schema = t.map {
            key = t.string { pattern = "^[a-z][a-z0-9_]*$"; };
            value = t.string { };
          };
        };
        ok = t.field {
          schema = t.bool { };
        };
        kind = t.field {
          schema = t.enum {
            values = [
              "task"
              "workflow"
            ];
          };
        };
      };
    };

    "runtime.summary.payload" = t.record {
      fields = {
        workflow_id = t.field {
          schema = t.string { minLength = 1; };
        };
        unit_ids = t.field {
          schema = t.list {
            elem = t.string { minLength = 1; };
            minItems = 1;
          };
        };
        run_id = t.field {
          schema = t.string { minLength = 1; };
        };
        labels = t.field {
          required = false;
          schema = t.map {
            key = t.string { pattern = "^[a-z][a-z0-9_]*$"; };
            value = t.string { };
          };
        };
        exit_code = t.field {
          schema = t.integer {
            minimum = 0;
            maximum = 255;
          };
        };
        duration_ms = t.field {
          required = false;
          schema = t.union {
            options = [
              (t.integer { minimum = 0; })
              (t.null { })
            ];
          };
        };
        attempt_id = t.field {
          schema = t.string { minLength = 1; };
        };
      };
    };
  };
}
