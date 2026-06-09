{ lib, system }:

config:
let
  fail = message: throw "nixfied model validation failed: ${message}";
  expect = condition: message: if condition then null else fail message;
  slotPolicy = config.nixfied.slotPolicy;
  portPolicy = config.nixfied.placement.ports;
  slots = lib.range slotPolicy.min slotPolicy.max;
  slotWindow = slot: {
    start = portPolicy.base + (slot * portPolicy.slotStride);
    end = portPolicy.base + (slot * portPolicy.slotStride) + portPolicy.windowSize - 1;
  };
  windows = map slotWindow slots;
  windowsInRange = lib.all (window: window.start >= 1 && window.end <= 65535) windows;
  windowsDoNotOverlap =
    portPolicy.slotStride >= portPolicy.windowSize;
  checks = [
    (expect (config.nixfied.target.system == system) "target.system must match the compile system in M0")
    (expect (config.nixfied.secrets == [ ]) "secrets are unsupported")
    (expect (slotPolicy.min >= 0) "slotPolicy.min must be non-negative")
    (expect (slotPolicy.max >= slotPolicy.min) "slotPolicy.max must be >= min")
    (expect (
      slotPolicy.default >= slotPolicy.min
      && slotPolicy.default <= slotPolicy.max
    ) "slotPolicy.default must be within the slot range")
    (expect windowsInRange "per-slot candidate port windows must be in 1..65535")
    (expect windowsDoNotOverlap "per-slot candidate port windows must not overlap")
    (expect (
      builtins.attrNames config.nixfied.environments == [ "dev" ]
    ) "a single 'dev' environment is supported")
    (expect (config.nixfied.services != { }) "at least one service must be declared")
    (expect (
      lib.all (
        service: builtins.hasAttr service config.nixfied.services
      ) config.nixfied.environments.dev.services
    ) "dev environment services must be declared")
    (expect (
      lib.all (task: builtins.hasAttr task config.nixfied.tasks) config.nixfied.environments.dev.tasks
    ) "dev environment tasks must be declared")
  ];
in
lib.foldl' (acc: check: lib.seq check acc) config checks
