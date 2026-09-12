//! u50_tools: application-wide tool provisioning and resolution.
//!
//! The uv provisioning pipeline, the resolver plugin system, and the
//! shared plugin scaffolding live here so every domain (style50,
//! check50, submit50) consumes them rather than carrying its own copy
//! (Phases 5-8 of docs/U50_CHECK_PYTHON_PLAN.md).

pub mod fs;
pub mod proc;
pub mod resolver;
pub mod uv;

pub use resolver::{Resolved, ResolverConfig, ResolverPlugin, ToolSpec};
