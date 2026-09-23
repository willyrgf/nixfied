//! The execution layer: the executor's own input type (`ExecutionManifest`) and the
//! total lowering from `nixfied_manifest::Manifest` that produces it.
//!
//! The seam: the schema (`Manifest`) defines what is *expressible*; the
//! `ExecutionManifest` defines what is *executable*. `lower` is the only bridge, and
//! it destructures every `Manifest` field with no `..`, so a new schema field is a
//! compile error until the lowering consciously maps or refuses it.

mod lower;
mod plan;
mod types;

pub use lower::lower;
pub use plan::{PlanNode, RunPlan, ServiceBinding, plan};
pub use types::*;

/// Borrow raw invocation positions before relational lowering, in secret-check order.
pub(crate) fn invocations(
    manifest: &nixfied_manifest::Manifest,
) -> impl Iterator<Item = &nixfied_manifest::InvocationSpec> {
    let service_values = manifest.services.values().flat_map(|service| {
        let lifecycle = &service.lifecycle;
        std::iter::once(&lifecycle.start.invocation)
            .chain(lifecycle.ready.probe.invocation.as_ref())
            .chain(lifecycle.health.probe.invocation.as_ref())
    });
    let task_values = manifest
        .tasks
        .values()
        .filter_map(|task| task.invocation.as_ref());
    service_values.chain(task_values)
}
