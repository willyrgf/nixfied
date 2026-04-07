{
  pkgs,
  model,
}:
let
  coverageMap = import ../../proof-workspace/scenarios/coverage-map.nix;
  testCatalog = import ../../nixfied/framework/testing/catalog.nix;
  features = model.features or { };
  featureIds = builtins.attrNames features;
  capabilities = coverageMap.capabilities or { };
  scenarios = coverageMap.scenarios or { };
  scenarioIds = builtins.attrNames scenarios;
  profileNames = coverageMap.profileNames or testCatalog.profileNames;

  allProfileChecks =
    profileName:
    builtins.concatLists (
      map (shardName: testCatalog.profileShardChecks.${profileName}.${shardName} or [ ]) testCatalog.order
    );

  allScheduledChecks =
    builtins.foldl'
      (
        acc: checkName:
        if builtins.elem checkName acc then acc else acc ++ [ checkName ]
      )
      [ ]
      (builtins.concatLists (map allProfileChecks profileNames));

  requiresCoverageMapping =
    featureId:
    let
      feature = features.${featureId};
      layer = feature.coverageLayer or "";
    in
    (feature.coverageRequired or false) && layer != "compile";

  missingCoverageMappings = builtins.filter (
    featureId: requiresCoverageMapping featureId && !(builtins.hasAttr featureId capabilities)
  ) featureIds;

  scenariosWithMissingOwnership = builtins.filter (
    scenarioId:
    let
      scenario = scenarios.${scenarioId};
      ownerFile = scenario.ownerFile or "";
      checkName = scenario.checkName or "";
    in
    ownerFile == "" || checkName == ""
  ) scenarioIds;

  scenariosWithInvalidProfiles = builtins.filter (
    scenarioId:
    let
      scenario = scenarios.${scenarioId};
      scenarioProfiles = scenario.profiles or [ ];
    in
    builtins.any (profileName: !(builtins.elem profileName profileNames)) scenarioProfiles
  ) scenarioIds;

  scenarioNotScheduledForProfile =
    scenarioId:
    let
      scenario = scenarios.${scenarioId};
      enabled = scenario.enabled or false;
      scenarioProfiles = scenario.profiles or [ ];
      checkName = scenario.checkName or "";
    in
    enabled
    && builtins.any (
      profileName: !(builtins.elem checkName (allProfileChecks profileName))
    ) scenarioProfiles;

  scenariosMissingProfileScheduling = builtins.filter scenarioNotScheduledForProfile scenarioIds;

  capabilitiesMissingScenarioOwnership = builtins.filter (
    capabilityId:
    let
      capability = capabilities.${capabilityId};
      scenarioRefs = capability.scenarios or [ ];
      layer = capability.layer or "";
    in
    layer == "proof-workspace"
    && (
      scenarioRefs == [ ]
      || builtins.any (scenarioId: !(builtins.hasAttr scenarioId scenarios)) scenarioRefs
    )
  ) (builtins.attrNames capabilities);

  capabilityHasAssertionOwnership =
    capabilityId:
    let
      capability = capabilities.${capabilityId};
      scenarioRefs = capability.scenarios or [ ];
    in
    builtins.any (
      scenarioId:
      if !(builtins.hasAttr scenarioId scenarios) then
        false
      else
        let
          scenario = scenarios.${scenarioId};
          assertedCapabilities = scenario.assertedCapabilities or [ ];
        in
        builtins.elem capabilityId assertedCapabilities
    ) scenarioRefs;

  capabilitiesMissingAssertionOwnership = builtins.filter (
    capabilityId:
    let
      capability = capabilities.${capabilityId};
      layer = capability.layer or "";
      scenarioRefs = capability.scenarios or [ ];
    in
    layer == "proof-workspace"
    && scenarioRefs != [ ]
    && !(capabilityHasAssertionOwnership capabilityId)
  ) (builtins.attrNames capabilities);

  deletionEligibility = coverageMap.deletionEligibility or { };
  defaultRequiredProfiles = deletionEligibility.requiredProfiles or [ ];
  deletionEntries = (deletionEligibility.firstBlock or [ ]) ++ (deletionEligibility.remaining or [ ]);

  checkIsDeleted = checkName: !(builtins.elem checkName allScheduledChecks);

  deletionMissingReplacementCapabilities = builtins.filter (
    entry:
    let
      checkName = entry.checkName or "";
      replacementCapabilities = entry.replacementCapabilities or [ ];
    in
    checkIsDeleted checkName
    && (
      replacementCapabilities == [ ]
      || builtins.any (capabilityId: !(builtins.hasAttr capabilityId capabilities)) replacementCapabilities
    )
  ) deletionEntries;

  replacementCapabilityIsGreen =
    requiredProfiles: capabilityId:
    let
      capability = capabilities.${capabilityId};
      layer = capability.layer or "";
      scenarioRefs = capability.scenarios or [ ];
      scenarioCheckForProfile =
        profileName:
        builtins.any (
          scenarioId:
          if !(builtins.hasAttr scenarioId scenarios) then
            false
          else
            let
              scenario = scenarios.${scenarioId};
              enabled = scenario.enabled or false;
              checkName = scenario.checkName or "";
            in
            enabled && builtins.elem checkName (allProfileChecks profileName)
        ) scenarioRefs;
    in
    layer == "proof-workspace"
    && scenarioRefs != [ ]
    && requiredProfiles != [ ]
    && builtins.all scenarioCheckForProfile requiredProfiles;

  deletionMissingGreenCoverage = builtins.filter (
    entry:
    let
      checkName = entry.checkName or "";
      replacementCapabilities = entry.replacementCapabilities or [ ];
      requiredProfiles = entry.requiredProfiles or defaultRequiredProfiles;
    in
    checkIsDeleted checkName
    && builtins.any (
      capabilityId:
      !(builtins.hasAttr capabilityId capabilities)
      || !(replacementCapabilityIsGreen requiredProfiles capabilityId)
    ) replacementCapabilities
  ) deletionEntries;

  showList = values: builtins.concatStringsSep ", " values;
  showDeletionEntries = entries: showList (builtins.map (entry: entry.checkName or "") entries);
in
assert (missingCoverageMappings == [ ])
  || throw "proof-workspace coverage validation failed: missing mappings for required capabilities: ${showList missingCoverageMappings}";
assert (scenariosWithMissingOwnership == [ ])
  || throw "proof-workspace coverage validation failed: scenario ownership missing ownerFile/checkName: ${showList scenariosWithMissingOwnership}";
assert (scenariosWithInvalidProfiles == [ ])
  || throw "proof-workspace coverage validation failed: scenario profiles reference unknown profile names: ${showList scenariosWithInvalidProfiles}";
assert (scenariosMissingProfileScheduling == [ ])
  || throw "proof-workspace coverage validation failed: enabled scenario profiles are not scheduled in catalog: ${showList scenariosMissingProfileScheduling}";
assert (capabilitiesMissingScenarioOwnership == [ ])
  || throw "proof-workspace coverage validation failed: proof-workspace capabilities reference missing scenarios: ${showList capabilitiesMissingScenarioOwnership}";
assert (capabilitiesMissingAssertionOwnership == [ ])
  || throw "proof-workspace coverage validation failed: proof-workspace capabilities missing explicit scenario assertion ownership: ${showList capabilitiesMissingAssertionOwnership}";
assert (deletionMissingReplacementCapabilities == [ ])
  || throw "proof-workspace coverage validation failed: deleted checks missing replacement capabilities: ${showDeletionEntries deletionMissingReplacementCapabilities}";
assert (deletionMissingGreenCoverage == [ ])
  || throw "proof-workspace coverage validation failed: deleted checks missing green replacement scenario coverage: ${showDeletionEntries deletionMissingGreenCoverage}";
pkgs.runCommand "proof-workspace-coverage-validation" { } ''
  echo "OK: proof workspace coverage map validation passed" > "$out"
''
