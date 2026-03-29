# RFC Layer Cleanup Plan

Date: 2026-03-28

Branch reviewed: `rfc-layer`

Scope: framework test architecture review only, no implementation changes

## Basis

Commands used:

- `nix run .#features`
- `nix run .#help`
- `rg --files tests/framework nixfied/framework/presets | sort`
- `rg -n "covers =|canonical =|proofKind =|layer =" tests/framework/default.nix`
- `rg -n "model\\.features|views\\.features|feature|coverageRequired|coverageLayer|ownerFiles" tests/framework/*.nix`
- `rg -n "builtins\\.readFile|pathExists|hasInfix|grep -Fq|require_contains|require_not_contains|jq" tests/framework/*.nix`
- `sed -n` and `nl -ba` on the files listed below

Primary files inspected:

- `tests/framework/default.nix`
- `tests/framework/feature-coverage-validation.nix`
- `tests/framework/framework-test-shards.nix`
- `tests/framework/framework-test-shard-validation.nix`
- `nixfied/framework/presets/framework-test.nix`
- `tests/framework/compiler-validation.nix`
- `tests/framework/discovery-command-surfaces-smoke.nix`
- `tests/framework/package-output-contract.nix`
- `tests/framework/introspect-contract.nix`
- `tests/framework/help-snapshot.nix`
- `tests/framework/contract-migration-guard.nix`
- `tests/framework/workflow-modes-contract.nix`
- `tests/framework/service-api-surface-contract.nix`
- `tests/framework/framework-test-coverage-contract.nix`
- `tests/framework/runtime-manifest-fixture-contract.nix`
- `tests/framework/selected-app-manifest-contract.nix`
- `tests/framework/workflow-ref-app-manifest-contract.nix`
- `tests/framework/service-set-surface-contract.nix`
- `tests/framework/workflow-service-set-adapter-smoke.nix`
- `tests/framework/service-hook-env-smoke.nix`
- `tests/framework/log-prefix-contract.nix`
- `tests/framework/disabled-service-runtime-surface-smoke.nix`
- `tests/framework/ephemeral-copy-mode-smoke.nix`
- `tests/framework/ephemeral-env-file-mode-smoke.nix`
- `tests/framework/ephemeral-nix-source-smoke.nix`
- `tests/framework/ephemeral-registry-run-isolation-smoke.nix`
- `tests/framework/discovery-runtime-contract.nix`
- `tests/framework/service-surface-catalog-contract.nix`
- `tests/framework/launcher-surface-contract.nix`
- `tests/framework/framework-utility-launcher-contract.nix`
- `tests/framework/framework-test-cli-contract-smoke.nix`
- `tests/framework/service-requirements-contract.nix`
- `tests/framework/excluded-service-evaluation.nix`
- `tests/framework/registry-helper-contract.nix`
- `tests/framework/no-legacy-project-modules.nix`

## Executive Call

- Current feature-proof accounting is inflated.
- The suite already has metadata for `layer`, `proofKind`, `canonical`, and `covers`, but the current accounting treats those labels as truth instead of checking each test's actual oracle.
- The only strong direct proof of the compiled feature model today is `tests/framework/compiler-validation.nix`.
- There is still no direct framework test of the public `nix run .#features` surface.
- `nix run .#features` currently reports 36 `coverage=required` features, but `tests/framework/default.nix` declares only 9 canonical owners, and most of those 9 are not real `.#features` proofs.
- `services` is not a real architecture layer. It is already just a cost bucket over compile, adapter, kernel, and e2e behavior.
- The migration lane is overloaded with deleted-seam and source-shape freezes that should not be treated as architecture centerpieces.

## Key Findings

1. The current canonical feature owners are mostly not feature-model proofs.

- `tests/framework/help-snapshot.nix` is canonical for all exposed task and workflow features through `tests/framework/default.nix`, but its only assertion is `model.views.help.lines == snapshots/help.txt`.
- `tests/framework/runtime-manifest-fixture-contract.nix` is canonical for `runtime.app-execution-manifests` and `runtime.service-set-surfaces`, but it proves manifest narrowing and service-set manifest shape, not the feature model.
- `tests/framework/service-hook-env-smoke.nix` is canonical for `runtime.service-hooks`, but it proves hook env/app behavior, not `model.features`.
- `tests/framework/log-prefix-contract.nix` is canonical for `runtime.output.prefix-contract`, but it is source-grep over logging implementations.
- The four canonical ephemeral tests are real runtime behavior checks, but not feature inventory proofs:
  - `tests/framework/ephemeral-copy-mode-smoke.nix`
  - `tests/framework/ephemeral-env-file-mode-smoke.nix`
  - `tests/framework/ephemeral-nix-source-smoke.nix`
  - `tests/framework/ephemeral-registry-run-isolation-smoke.nix`

2. The strongest existing feature proof is under-credited.

- `tests/framework/compiler-validation.nix` directly asserts:
  - `model.features` exists
  - feature ids, kinds, summaries, `ownerFiles`, `modelPaths`, `coverageRequired`, and `coverageLayer` are well-formed
  - service, workflow, exposed-task, and runtime feature ids are present
  - the `features` command surface exists
- Despite that, `tests/framework/default.nix` only credits it with service features.

3. The public `.#features` surface is not actually tested.

- `tests/framework/package-output-contract.nix` only asserts that the `features` app exists.
- `tests/framework/compiler-validation.nix` only asserts that the feature view and command surface exist.
- No test currently executes `apps.features.program` or snapshots the `nix run .#features` output.

4. Metadata and execution layout are being mistaken for proof.

- `tests/framework/feature-coverage-validation.nix` validates bookkeeping only.
- `tests/framework/framework-test-shard-validation.nix` validates shard completeness and layer alignment only.
- `tests/framework/framework-test-coverage-contract.nix` freezes shard names and deleted bucket names only.
- `nixfied/framework/presets/framework-test.nix` and `tests/framework/framework-test-shards.nix` define execution layout, not feature truth.

5. The current profiles distort what "covered" means.

- `tests/framework/framework-test-shards.nix` keeps `services` as a separate shard even though the file itself says it is not a real second architecture model.
- The `ci` profile excludes `services` and `e2e`.
- Canonical owners for `runtime.service-hooks` and the four canonical ephemeral runtime features live in `services` or `e2e`, so the default `ci` profile does not even run several currently credited canonical feature owners.

## Ranked List: Real `.#features` Proofs

1. `tests/framework/compiler-validation.nix`

- Strongest current proof.
- Directly inspects `model.features` and the compiled feature model guarantees.
- This should be the core canonical owner for feature inventory correctness.

2. `tests/framework/discovery-command-surfaces-smoke.nix`

- Real feature export/discovery proof, but weaker.
- It validates rendered feature inventory behavior in discovery output.
- Its limitation is that it uses fixture inventory rather than the repo's actual compiled feature inventory.

3. Inline `introspection-schema` check in `tests/framework/default.nix`

- Secondary export-shape proof only.
- Useful for guarding that exported schema still includes `features`.
- Not sufficient as a standalone feature proof.

## Ranked List: Over-Counted As Feature Proof

1. `tests/framework/help-snapshot.nix`

- Canonical today, but the oracle is help text.
- This is the worst mislabel in the suite.

2. `tests/framework/runtime-manifest-fixture-contract.nix`

- Valuable manifest architecture test.
- Not a feature-model proof.

3. `tests/framework/service-hook-env-smoke.nix`

- Valuable adapter/runtime test.
- Not a feature-model proof.

4. `tests/framework/log-prefix-contract.nix`

- Source-shape and output-prefix freeze.
- Not a feature-model proof.

5. The canonical ephemeral runtime tests:

- `tests/framework/ephemeral-copy-mode-smoke.nix`
- `tests/framework/ephemeral-env-file-mode-smoke.nix`
- `tests/framework/ephemeral-nix-source-smoke.nix`
- `tests/framework/ephemeral-registry-run-isolation-smoke.nix`

These are valid runtime behavior tests, but they are being used as feature accounting owners when they should be architecture proofs only.

## Current Classification

### Real `.#features` Proofs

- `tests/framework/compiler-validation.nix`
- `tests/framework/discovery-command-surfaces-smoke.nix`
- inline `introspection-schema` check in `tests/framework/default.nix`

If the project wants a clean feature-proof subset today, it should contain exactly those checks. That subset is thin, and the missing public proof is a direct `features` surface contract.

### Metadata / Governance Only

- `tests/framework/feature-coverage-validation.nix`
- `tests/framework/framework-test-shard-validation.nix`
- `tests/framework/framework-test-coverage-contract.nix`
- `tests/framework/default.nix`
- `tests/framework/framework-test-shards.nix`
- `nixfied/framework/presets/framework-test.nix`

These are important, but they only govern labeling, placement, and execution layout.

### Valid Non-Feature Architecture Tests

Public/help/export surfaces:

- `tests/framework/help-snapshot.nix`
- `tests/framework/package-output-contract.nix`
- `tests/framework/introspect-contract.nix`
- `tests/framework/discovery-runtime-contract.nix`

Manifest and app narrowing:

- `tests/framework/runtime-manifest-fixture-contract.nix`
- `tests/framework/selected-app-manifest-contract.nix`
- `tests/framework/workflow-ref-app-manifest-contract.nix`
- `tests/framework/machine-output-app-smoke.nix`

Service-set / hook / launcher / adapter behavior:

- `tests/framework/service-set-surface-contract.nix`
- `tests/framework/workflow-service-set-adapter-smoke.nix`
- `tests/framework/service-hook-env-smoke.nix`
- `tests/framework/disabled-service-runtime-surface-smoke.nix`
- `tests/framework/service-surface-catalog-contract.nix`
- `tests/framework/launcher-surface-contract.nix`
- `tests/framework/framework-utility-launcher-contract.nix`

Runtime semantics:

- `tests/framework/ephemeral-copy-mode-smoke.nix`
- `tests/framework/ephemeral-env-file-mode-smoke.nix`
- `tests/framework/ephemeral-nix-source-smoke.nix`
- `tests/framework/ephemeral-registry-run-isolation-smoke.nix`
- `tests/framework/service-requirements-contract.nix`
- `tests/framework/excluded-service-evaluation.nix`
- `tests/framework/framework-test-cli-contract-smoke.nix`

These tests are valuable. They should stay. They just should not be counted as `.#features` proofs.

### Old Dirty / Migration / Delete Candidates

Keep as migration-only:

- `tests/framework/contract-migration-guard.nix`
- `tests/framework/no-legacy-project-modules.nix`

Fold into `contract-migration-guard` or delete:

- `tests/framework/workflow-modes-contract.nix`
- `tests/framework/registry-helper-contract.nix`

Demote to governance/admin only:

- `tests/framework/framework-test-coverage-contract.nix`
- `tests/framework/framework-test-shard-validation.nix`

Split or relabel if kept:

- `tests/framework/service-api-surface-contract.nix`
- `tests/framework/log-prefix-contract.nix`
- deleted-helper assertions inside `tests/framework/introspect-contract.nix`

These checks are mostly deleted-seam guards or source-shape freezes. They are not feature proofs, and several of them are not even good long-lived architecture proofs.

## Tests With `covers` That Should Not Count Toward Canonical Feature Coverage

Canonical today, but should be removed from canonical feature accounting:

- `tests/framework/help-snapshot.nix`
- `tests/framework/log-prefix-contract.nix`
- `tests/framework/ephemeral-copy-mode-smoke.nix`
- `tests/framework/ephemeral-env-file-mode-smoke.nix`
- `tests/framework/ephemeral-nix-source-smoke.nix`
- `tests/framework/ephemeral-registry-run-isolation-smoke.nix`
- `tests/framework/runtime-manifest-fixture-contract.nix`
- `tests/framework/service-hook-env-smoke.nix`

Carry `covers`, but should remain architecture-only:

- `tests/framework/selected-app-manifest-contract.nix`
- `tests/framework/service-set-surface-contract.nix`
- `tests/framework/workflow-service-set-adapter-smoke.nix`
- `tests/framework/machine-output-app-smoke.nix`
- `tests/framework/workflow-ref-app-manifest-contract.nix`
- `tests/framework/disabled-service-runtime-surface-smoke.nix`

The blunt rule is simple: `covers` is not evidence. The test body is evidence.

## Proposed Rule For Canonical Feature Ownership

A test may carry canonical feature ownership only when its primary failure condition is one or more of:

- direct assertions over `model.features`
- direct assertions over `model.views.features`
- direct assertions over the public `features` surface
- compiled feature inventory stability
- feature ids, kinds, owners, `coverageRequired`, or `coverageLayer`
- canonical proof ownership metadata itself

A test must not carry canonical feature ownership when its primary oracle is:

- help text
- source grep
- deleted file/helper presence or absence
- manifest narrowing
- machine-output wrapper behavior
- launcher wrapping
- runtime shell output shape
- shard wiring or profile wiring
- service/runtime behavior that does not directly assert the feature model

## Proposed Clean Split

### Feature Proofs

Current clean subset:

- `tests/framework/compiler-validation.nix`
- `tests/framework/discovery-command-surfaces-smoke.nix`
- inline `introspection-schema` check in `tests/framework/default.nix`

Recommended missing proof to add later:

- a dedicated public `features` surface contract that executes `nix run .#features` or `apps.features.program` and proves the exported inventory directly

### Architecture Proofs

- help and command surfaces
- introspection CLI/runtime contracts
- manifest narrowing and machine-output runtime behavior
- launcher wrapping and service surface catalog behavior
- service hooks, service-set surfaces, runtime isolation, ephemeral behavior
- kernel/runtime/service architecture checks that do not directly assert `model.features`

Representative tests:

- `tests/framework/help-snapshot.nix`
- `tests/framework/introspect-contract.nix`
- `tests/framework/runtime-manifest-fixture-contract.nix`
- `tests/framework/selected-app-manifest-contract.nix`
- `tests/framework/workflow-ref-app-manifest-contract.nix`
- `tests/framework/service-set-surface-contract.nix`
- `tests/framework/workflow-service-set-adapter-smoke.nix`
- `tests/framework/service-hook-env-smoke.nix`
- `tests/framework/ephemeral-copy-mode-smoke.nix`
- `tests/framework/ephemeral-env-file-mode-smoke.nix`
- `tests/framework/ephemeral-nix-source-smoke.nix`
- `tests/framework/ephemeral-registry-run-isolation-smoke.nix`

### Migration Guards

- `tests/framework/contract-migration-guard.nix`
- `tests/framework/no-legacy-project-modules.nix`
- any temporary deleted-seam assertions that are still justified during the refactor window

### Delete / Deprecate

- `tests/framework/workflow-modes-contract.nix`
- `tests/framework/registry-helper-contract.nix`
- `tests/framework/framework-test-coverage-contract.nix` once shard governance is reduced to one source of truth
- source-shape fragments that can be folded into `contract-migration-guard`

## Shard And Profile Cleanup

- Keep `services` only as a cost bucket, not as a layer.
- Do not describe `services` as proof ownership. The current `tests/framework/README.md` already says it is not a second architecture model.
- Derive shard placement from real architecture ownership, or at minimum stop using shard placement as implicit evidence of feature coverage.
- If `ci` remains the default confidence profile, do not place canonical feature owners exclusively in `services` or `e2e`.
- Better target shape:
  - `feature-proof`: the actual feature-model proofs
  - `architecture-proof`: compile/manifest/kernel/adapter/e2e behavior proofs
  - `migration-only`: deleted seam guards
  - `full`: cost-expensive superset, including `services`

## Practical Cleanup Plan

### Phase 1: Correct The Accounting

- Remove canonical feature ownership from:
  - `tests/framework/help-snapshot.nix`
  - `tests/framework/log-prefix-contract.nix`
  - `tests/framework/runtime-manifest-fixture-contract.nix`
  - `tests/framework/service-hook-env-smoke.nix`
  - the four canonical ephemeral tests
- Strip `covers` from non-feature tests where the metadata currently invites misreading.
- Re-credit `tests/framework/compiler-validation.nix` as the primary feature-model owner.
- Treat `tests/framework/feature-coverage-validation.nix` as governance only, not as behavior proof.

### Phase 2: Split The Suite Cleanly

- Explicitly separate:
  - feature proofs
  - architecture proofs
  - migration guards
  - delete/deprecate
- Move `tests/framework/service-api-surface-contract.nix` out of `migration` if it is kept.
- Move deleted-helper checks out of mixed public-contract tests such as `tests/framework/introspect-contract.nix`.
- Keep `services` only as an execution-cost bucket.

### Phase 3: Delete Redundant Dirty Tests

- Fold `tests/framework/workflow-modes-contract.nix` into `tests/framework/contract-migration-guard.nix`.
- Fold `tests/framework/registry-helper-contract.nix` into `tests/framework/contract-migration-guard.nix`.
- Demote or delete `tests/framework/framework-test-coverage-contract.nix`.
- Keep `tests/framework/framework-test-shard-validation.nix` only if shard governance still needs an admin check.
- Delete temporary grep guards once the refactor window closes.

### Phase 4: Strengthen Real Feature Proofs

- Add a direct `features` surface proof.
- Keep `tests/framework/compiler-validation.nix` as the core `model.features` proof.
- Optionally let `tests/framework/discovery-command-surfaces-smoke.nix` own discovery/export behavior, not the core feature model.
- Make canonical ownership narrow and boring.

## Stale Or Corrected Claims

- Any claim that `covers` implies feature proof is wrong.
- Any claim that `services` is a real architecture layer is wrong.
- Any claim that the current `ci` profile runs all canonical feature owners is wrong.
- Any claim that help snapshots or manifest narrowing are sufficient `.#features` proofs is wrong.

## Next Time

- Add an explicit `features` surface test before expanding feature coverage bookkeeping further.
- Derive shard layout from real ownership metadata instead of maintaining multiple registries by hand.
- Give migration-only checks an expiry rule so source-grep fences do not become permanent architecture centers.
