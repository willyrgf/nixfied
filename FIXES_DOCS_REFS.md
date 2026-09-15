# Discoverable authoring reference at the project's Nixfied revision

Status: proposal, not implemented. Commands below describe the proposed surface.
Origin: adopter report from MFM, "Reading Nixfied From the Store", against
pin `6b3a70b`.

## 1. Problem and required outcome

An adopter asked: **what are the alternatives to `stateRefs = [ "slot" ]`?**
Answering took seven hops through a pinned `/nix/store` checkout, including four
visits to Nix compiler and Rust runtime sources.

This is a product discovery failure. An adopter should be able to learn what
Nixfied supports, which options express it, and which constraints apply through
the commands Nixfied supplies. Source inspection must not be a prerequisite for
ordinary authoring questions.

The report proposed exporting the option-reference derivation already consumed
by checks and packaging the prose docs. That is a reasonable distribution
improvement. The required outcome is broader than obtaining a readable file:

| Requirement | What the adopter needs |
| --- | --- |
| Access | A discoverable command leading from project help to authoring reference material. |
| Coverage | Supported choices, types, defaults, constraints, examples, and explicit limitations. |
| Accuracy | Explanations that agree with the selected implementation. |
| Revision binding | Content from the same Nixfied input that generates the project's apps. |

Incorrect descriptions compound the problem, but do not explain it away. Even a
perfectly accurate Markdown file fails the access requirement if the adopter
must first locate framework sources in the store or on GitHub.

### Governing invariant

Starting from generated project help, an adopter can discover the authoring
surface and answer supported authoring questions using reference content from
the project's selected Nixfied input, without inspecting framework internals.

This is a proposed public integration guarantee. Documentation remains a human
reference, not another required model artifact or runtime authority.

## 2. Evidence and the answer currently missing

The current sources establish these concrete gaps:

- [`nix/project-apps.nix`](nix/project-apps.nix) exports command discovery and
  runtime controls, but no authoring-reference app.
- [`nix/docs/options.nix`](nix/docs/options.nix) generates the reference from
  the module evaluator. Its use in [`flake.nix`](flake.nix) is private to checks.
  The comparison in
  [`nix/packages/rust-workspace-check.nix`](nix/packages/rust-workspace-check.nix)
  proves that the checked-in reference matches the generator, not that every
  description matches behavior.
- [`nix/modules/primitives.nix`](nix/modules/primitives.nix) describes
  `stateRefs` as participating in service identity. It does not appear in
  [`service/identity.rs`](runtime/crates/nixfied-runtime/src/service/identity.rs),
  and [`execution/lower.rs`](runtime/crates/nixfied-runtime/src/execution/lower.rs)
  discards it.
- The `requires` description does not explain task-side placeholder names.
  [`nix/compiler/validate.nix`](nix/compiler/validate.nix) admits named endpoint
  references only to required services that have endpoints.

For the original question, the current reference needs to say:

> `services.<name>.stateRefs` is declared as a list of strings, with default
> `[ "slot" ]`. This is not an enum of storage backends or selectable state
> roots. Changing the labels does not select another state directory or change
> service reuse identity: execution lowering discards them. They remain
> serialized model data and appear in the generated model view, so changing them
> changes the raw model hash. Use the state policy and the documented
> `${stateDir}` convention to understand runtime-owned slot state.

Syntactically accepted strings and meaningful runtime alternatives are different
questions. The reference must answer both. It must not invent a registry of
legal state roots from the example string `"slot"`.

The sibling service/task `logRefs`, `artifactRefs`, and `summaryRefs` fields also
need accurate descriptions of their current role. Removal is separate contract
work (§7), not a prerequisite for telling adopters the truth today.

## 3. Proposed command experience

Add a framework-owned `docs` app to the generated project surface, with a visible
entry in `.#help` and a pointer in the scaffold's authoring comments.
The following syntax is the proposed first-delivery interface:

```sh
nix run .#docs
nix run .#docs -- options
nix run .#docs -- options nixfied.services
nix run .#docs -- option 'nixfied.services.<name>.stateRefs'
nix run .#docs -- topic state
nix run .#docs -- topic placeholders
```

- No arguments print a short index, the framework source identity, and examples
  of the next command to run. `-h` and `--help` explain the query syntax.
- `options [prefix]` lists canonical option paths, optionally restricted to a
  namespace. Discovery must not require knowing an exact name first.
- `option <path>` shows the declaration's type, default or absence of a default,
  description, examples where supplied, and relevant constraints or topic
  pointers. Open string types must not be presented as closed enums.
- `topic <name>` reads an indexed authoring topic. The index must cover the
  authoring model and its limits, tasks and services, state, endpoints and
  placeholders, secrets, adapters, and operation/recovery guidance.
- Unknown commands, option paths, topics, and unmatched prefixes fail with a
  nonzero status and useful valid choices or lookup guidance. They must not
  silently print an unrelated document or choose an approximate match.
- Successful reference content goes to stdout and can be redirected. Errors go
  to stderr. Reading requires no interactive pager or browser.

Dumping the entire option reference is insufficient. Prose must connect options
to workflows and limitations; the index must make exact declarations findable.

Framework reference and project configuration are distinct scopes. A future
project-only `docs model` may expose the existing `views/docs.md`, but that view
cannot enumerate every authoring possibility and is not needed for this delivery.

### Correct placeholder guidance

The first delivery must include examples for both task and service contexts:

- A task addresses an endpoint-bearing required service by **service id** using
  `${port:<serviceId>}` and `${host:<serviceId>}`. These select that service's
  primary endpoint. A task has no own endpoint-id namespace.
- Bare `${port}` and `${host}` in a task refer to the first `requires` entry's
  primary endpoint. They are invalid if that entry has no endpoint; resolution
  does not skip ahead to another dependency.
- Endpoint-less services can still be required for ordering and lifecycle
  purposes. They cannot be addressed through endpoint placeholders.
- Service lifecycle invocations can name their own endpoint ids and address
  endpoint-bearing `connectsTo` services by service id. Bare forms select the
  service's own primary endpoint.
- `${stateDir}` is the runtime-materialised slot state root. Adapter-specific
  subdirectories are conventions described in adapter prose.
- Substitution applies to invocation arguments and environment values.
  `stdin` is a `null`/`inherit` policy, not a template string. Secret placeholders
  are restricted to invocation environment values.

Document the supported grammar and rejection rules in
[`docs/CONTRACT.md`](docs/CONTRACT.md), with usage guidance in the guide and
option descriptions. Review the existing Nix and Rust boundaries before
promoting observed behavior into normative language.

## 4. Ownership, packaging, and revision binding

### One source for each fact

| Fact or responsibility | Owner |
| --- | --- |
| Option paths, types, defaults, descriptions | Typed Nix modules, rendered through the existing evaluator. |
| Behavioral guarantees and constraints | Contract and its enforcing validators/runtime; focused tests prove agreement. |
| Workflow explanations, examples, supported limits | Existing authored guide and adapter documentation. |
| Topic and option lookup | Nix-built static reference app and derived index. |
| Configured project facts | Existing compiler-derived `views/docs.md`. |

Extend or factor the existing reference builder rather than independently
reconstructing option definitions. A generated lookup index is disposable
presentation data, not an authored schema or model sidecar. Shell may dispatch
queries over it; shell must not reimplement authoring validation or graph rules.

### An artifact and an app serve different needs

Build the app's content from existing prose and the generated option reference.
An exported documentation derivation can make the same content available for
direct reading, archiving, or tooling. It may back the app too. There is no
architectural conflict between the two.

The required first-delivery surface is the project docs app. Public artifact
names and layout can be chosen during implementation if the artifact is also
exported; they must be documented and reuse the same content builder. Packaging
alone does not satisfy command discovery, and an app alone does not prove that
it reads the right revision.

### Bind content at evaluation

`projectApps` must bake in content from the Nixfied input providing that function.
It must require no additional adopter wiring beyond the existing generated app
integration. Do not resolve a default branch or registry entry when the app runs.

Also expose the generic docs app on Nixfied's own flake for readers using an
explicit framework reference without a checkout. That app describes the selected
framework reference; it must not claim to describe an unrelated project's lock.
It does not need the current-directory guard used by contextual project help.

Derive displayed provenance from the supplying input. Show the source/store
identity and a revision when available; do not require every source type to have
a Git revision or hard-code a version label.

The realised app reads static content without invoking Nix or the Rust runtime.
The outer `nix run` still evaluates and realises its app as usual. Framework
reference queries should not force project model validation or executable
closure builds: authoring errors are a reason to read the docs. Verify this
property explicitly rather than assuming Nix laziness provides it.

## 5. Boundaries and atomic integration

The documentation dispatcher owns query rejection before reading a selected
entry. The compiler owns rejection of exported task verbs that collide with
`docs`. Explicit query forms keep topic, namespace, and exact-option lookup
distinct; an unknown name never falls back to another scope.

The first delivery changes the Nix-only public integration surface. Update
VERB-1 and SURFACE-1 in the contract, reserved-name validation, generated apps,
help expectations, scaffold guidance, README, guide, and development reference
instructions together. No runtime ABI rotation is required unless implementation
also changes model data or runtime behavior.

Correct inaccurate descriptions in module sources and regenerate
`docs/OPTIONS.md`; never patch the generated reference by hand. Add examples and
topic navigation in their existing documentation owners. Keep authored prose
under the existing documentation tree so the upgrade documentation report
continues to cover it.

## 6. Proof and delivery order

First establish accurate content and the reference builder. Then connect lookup,
project and framework apps, help, namespace rejection, and scaffold discovery as
one public-surface delivery. Validate with an adopter-shaped fixture.

Acceptance requires these proofs:

1. **Discovery:** project help exposes `docs`; its index leads to option listing,
   exact lookup, and the state and placeholder topics.
2. **Original question:** `stateRefs` lookup exposes its type, default, current
   execution limitation, and state guidance without requiring source inspection.
3. **Reference consistency:** lookup and the full reference derive paths, types,
   defaults, and descriptions from the same evaluator. The existing drift gate
   still passes.
4. **Behavioral accuracy:** focused cases cover task service-id addressing,
   rejection of endpoint-id addressing from a task, endpoint-less dependencies,
   and the first-dependency rule for bare placeholders. Reuse existing proofs
   where they already establish these guarantees.
5. **Revision binding:** adopter fixtures selecting distinguishable framework
   sources receive corresponding content and provenance. A changed default
   source or caller directory cannot redirect a realised docs app.
6. **Read-only independence:** generic lookup works without runtime admission,
   state preparation, service startup, or project executable builds. It remains
   available with an invalid authoring declaration when the flake wiring itself
   is evaluable. The framework app works outside a checkout.
7. **Rejection:** invalid query shapes and unknown names fail usefully; a task
   verb named `docs` is rejected at Nix evaluation rather than shadowing the app.

Checking whether a runtime field is read does not prove its documented meaning.
A read can be irrelevant or implement different behavior. Use focused tests of
accepted construction, rejected inputs, and promised effects alongside the
reference-generation check.

Follow [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md): start with focused generation,
lookup, app, and Nix validation checks, then run the affected model build and
adopter gate. Use `--dirty` when downstream fixtures must consume uncommitted
changes. Run `.#ci -- --dirty` for a contract or cross-layer implementation.
Report checks actually run and unverified platform or integration coverage.

## 7. Separate architectural follow-ups

These findings deserve review, but are not dependencies of authoring discovery:

- **Remove unused execution refs.** The five service/task ref fields are dropped
  during execution lowering but remain in the model, generated view, and ABI.
  Removal requires the contract procedure: descriptor and ABI snapshot,
  producers, consumers, validation, adapters, examples, fixtures, vectors, and
  docs change together. Accurate current descriptions can ship first.
- **Review state-policy consolidation.** `cleanupPolicy` and `persistence`
  collapse to two permission outcomes at the cleanup gate. They also enter
  service identity and marker matching separately. This supports a simplification
  review, not a claim that combinations are invalid or globally indistinguishable.
  Specify adoption, identity, cleanup, and existing-state consequences first.
- **Review placeholder typo handling.** Exact `${stateDir}` is substituted;
  arbitrary misspellings are not recognized. Before rejecting unknown `${...}`
  forms, define the reserved grammar and representation of literal child-program
  syntax. Enforce the chosen contract at the earliest sufficient boundary with
  paired Nix/Rust proofs where applicable.
- **Improve declaration diagnostics.** Naming the offending reference and legal
  alternatives improves recovery. An error cannot replace discovery before an
  adopter writes code.
- **Consider a derived adapter catalog.** Evaluated adapters can supply concrete
  default model facts. Distinguish those defaults from supported module overrides.
  Keep wrapper-owned state layout in prose unless a separate design establishes
  a reason for new metadata.

Do not bundle these changes merely to share an ABI rotation. Each needs its own
invariant, owner, rejection boundary, and proof. The discovery delivery succeeds
when adopters can answer authoring questions through the supplied commands at
their selected revision.
