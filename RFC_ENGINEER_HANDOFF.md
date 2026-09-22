# Adopter API implementation handoff

Status: implemented. The current simplification supersedes the original private
schema/default and maintenance-fixture choices; public behavior is unchanged.
The active design is [RFC_EXPOSE_ADOPTER_FACING_API.md](RFC_EXPOSE_ADOPTER_FACING_API.md).
Product behavior remains governed by [docs/CONTRACT.md](docs/CONTRACT.md).

The original delivery and its detailed coverage/recovery receipts are preserved
in commit `48fc137`; the reviewed implementation starting point was `d5562f1`
and the production baseline was `c3d9111`. These immutable historical receipts
are not a second active implementation assignment or proof of the current tree.

The abandoned implementation archive is architect-only recovery material.
Do not locate, read, copy, port, compare against, or ask another agent to consult
it. Current committed code and independent fixtures are the working baseline.

## Active ownership

- Native Nixpkgs options own configuration and merge behavior; the raw metadata
  audit checks descriptions, visibility and nested option mounts before rendering.
- Shared structural declarations own fields and inventory-linked vocabularies.
  Fields select coherent presence alternatives. Record-level producer ownership
  distinguishes Nix constructors from output-only records. Empty collection and
  explicit enum-member defaults are the only supported defaults; required-nullable
  and recursive decoder-default interpretation are removed.
- Rust definitions stay in native include scopes. Serde owns casing, collection
  defaults and serialization; explicit enum wire defaults retain separate ownership
  from convenience Default impls. Native IDs, paths, LoopbackHost and UniqueVec stay
  native. Borrowed views remain temporary projections, not competing stored state.
- Command syntax supplies shared tokens/types/defaults and help. Shared argument
  identities are checked across commands; native parsing and effects remain native.
- Publication metadata is checked once and indexed by kind/scope/name. Native
  bindings remain lazy; final audits observe actual exports and injected arguments.
- Reference packaging discards context only from presentation data. Product builds
  compile checked-in generated sources; source checks separately verify freshness.

## Proof and delivery

Keep independent raw model/output bytes, seven exact helps, parser precedence and
encoding cases, no-child admission tests, redaction/write failures and real host
lifecycle tests. Poisoned defaults/bindings and mounted-option regressions remain
required. Do not substitute generated expectations for independent oracles.

Maintenance fixtures compile an added wire field and command argument through
normal projections and small native consumers. They no longer rewrite production
parser source at textual anchors or grade invented failure-selection branches.
Fixture Cargo build plumbing is shared; product source isolation is unchanged.

Use the focused proofs and cross-layer `nix run .#ci -- --dirty` route in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). Report actual checks and unverified
platform/release coverage. Preserve unrelated work and do not commit unless asked.
