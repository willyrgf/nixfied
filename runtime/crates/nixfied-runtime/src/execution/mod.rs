//! The execution layer: the executor's own input type (`ExecutionModel`) and the
//! total lowering from `nixfied_model::Model` that produces it.
//!
//! The seam: the schema (`Model`) defines what is *expressible*; the
//! `ExecutionModel` defines what is *executable*. `lower` is the only bridge, and
//! it destructures every `Model` field with no `..`, so a new schema field is a
//! compile error until the lowering consciously maps or refuses it.

mod lower;
mod plan;
mod types;

pub use lower::lower;
pub use plan::{PlanNode, RunPlan, ServiceBinding, plan, prove_all_plans_feasible};
pub use types::*;
