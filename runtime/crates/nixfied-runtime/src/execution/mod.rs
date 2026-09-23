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
pub use plan::{PlanNode, RunPlan, ServiceBinding, flatten_task, plan, prove_all_plans_feasible};
pub use types::*;
