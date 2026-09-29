# Independent authoring vectors: construct actual native modules and force the
# emitted wire and closure roots, not a parallel validator.
{ lib, pkgs, system }:
let
  start = { tools = [ "server" ]; run = [ "sleep" "30" ]; };
  probe = { tools = [ pkgs.hello ]; run = [ "hello" "\${host}:\${port}" ]; };
  endpoint = { readyProbe = probe; healthProbe = probe; };
  base = {
    lifecycle.start.invocation = start;
    endpoint = endpoint // { endpointId = "web"; };
  };
  compile = service:
    let
      evaluated = import ../compiler/resolve.nix {
        inherit lib pkgs system;
        module.nixfied = {
          project = { projectId = "probe-vectors"; name = "Probe vectors"; };
          closures.server = {
            package = pkgs.coreutils;
            executable = "bin/sleep";
            effects = if service ? endpoint || service ? endpoints then [ "process" "network-listener" ] else [ "process" ];
          };
          services.web = service;
        };
      };
      config = import ../compiler/validate.nix { inherit lib system; } evaluated.config;
    in import ../compiler/derive.nix {
      inherit lib pkgs system config;
      constants = import ../spec/constants.nix;
    };
  accepts = service: (builtins.tryEval (builtins.deepSeq (compile service).manifest true)).success;
  wire = (compile base).manifest;
  multi = {
    lifecycle.start.invocation = start;
    endpoints = { a = endpoint; z = endpoint; };
    primaryEndpoint = "a";
  };
  endpointless = {
    lifecycle = {
      start.invocation = start;
      ready.probe = start;
      health.probe = start;
    };
  };
  phaseChange = phase: change: lib.recursiveUpdate base { lifecycle.${phase} = change; };
  endpointChange = change: base // { endpoint = base.endpoint // change; };
  checks = [
    (accepts base)
    (accepts multi)
    (accepts endpointless)
    (!(wire.services.web.lifecycle.ready ? probe))
    (wire.services.web.lifecycle.ready.policy == { timeoutMs = 1000; retryIntervalMs = 100; maxAttempts = 20; })
    (wire.services.web.endpoints.web.readyProbe.run == [ "hello" "\${host}:\${port}" ])
    (wire.services.web.endpoints.web.readyProbe == (compile multi).manifest.services.web.endpoints.z.readyProbe)
    # The package occurs only in endpoint probes; traversal must realise it and
    # synthesize the closure used by both phases on both endpoints.
    (builtins.length (builtins.attrNames wire.closures) == 2)
    (builtins.elem pkgs.hello (compile base).packages)
    (!(accepts (endpointChange { readyProbe = null; })))
    (!(accepts (base // { endpoint = { endpointId = "web"; }; })))
    (!(accepts (multi // { endpoints = multi.endpoints // { z = { readyProbe = probe; }; }; })))
    (!(accepts (multi // { primaryEndpoint = "missing"; })))
    (!(accepts (base // { primaryEndpoint = "wrong"; })))
    (!(accepts (endpointless // { primaryEndpoint = "web"; })))
    (!(accepts { lifecycle.start.invocation = start; }))
    (!(accepts (lib.recursiveUpdate endpointless { lifecycle.ready.probe.run = [ "sleep" "\${port}" ]; })))
  ] ++ lib.concatMap (phase: [
    (!(accepts (phaseChange phase { probe = probe; })))
    (!(accepts (phaseChange phase { probe = { kind = "tcp"; }; })))
    (!(accepts (phaseChange phase { policy.timeoutMs = 0; })))
    (!(accepts (phaseChange phase { policy.retryIntervalMs = 0; })))
    (!(accepts (phaseChange phase { policy.maxAttempts = 4294967296; })))
    (!(accepts (endpointChange { ${phase + "Probe"} = probe // { stdin = "inherit"; }; })))
    (!(accepts (endpointChange { ${phase + "Probe"} = probe // { timeoutMs = 1; }; })))
    (!(accepts (endpointChange { ${phase + "Probe"} = probe // { run = [ "hello" "\${port:absent}" ]; }; })))
    (!(accepts (endpointChange { ${phase + "Probe"} = probe // { tools = [ "absent" ]; }; })))
    (!(accepts (lib.recursiveUpdate endpointless { lifecycle.${phase}.probe.stdin = "inherit"; })))
    (!(accepts (lib.recursiveUpdate endpointless { lifecycle.${phase}.probe.timeoutMs = 1; })))
  ]) [ "ready" "health" ];
in
assert lib.imap0 (index: ok: if ok then true else throw "endpoint readiness vector ${toString index} failed") checks == map (_: true) checks;
true
