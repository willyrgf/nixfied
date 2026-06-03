{ lib, system }:

config:
let
  fail = message: throw "nixfied M0 model validation failed: ${message}";
  expect = condition: message: if condition then null else fail message;
  checks = [
    (expect (config.nixfied.target.system == system) "target.system must match the compile system in M0")
    (expect (config.nixfied.secrets == [ ]) "secrets are unsupported in M0")
    (expect (config.nixfied.workflows == { }) "workflows are unsupported in M0")
    (expect (
      config.nixfied.state.cleanupPolicy == "delete-on-clean"
      && config.nixfied.state.persistence == "run-scoped"
    ) "M0 state policy must be delete-on-clean and run-scoped")
    (expect (
      config.nixfied.slotPolicy.min == 0
      && config.nixfied.slotPolicy.default == 0
      && config.nixfied.slotPolicy.max == 0
    ) "M0 supports only slot 0")
    (expect (
      config.nixfied.environments.dev.services == [ "synthetic" ]
      && config.nixfied.environments.dev.tasks == [ "smoke" ]
    ) "M0 dev environment must contain only synthetic service and smoke task")
    (expect (
      config.nixfied.services.synthetic.portWindow.start
      <= config.nixfied.services.synthetic.portWindow.end
    ) "synthetic service port window start must be <= end")
  ];
in
lib.foldl' (acc: check: lib.seq check acc) config checks
