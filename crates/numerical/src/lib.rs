//! Numerical kernels for EEE Workbench.
//! Std-only so the core can be built without Python, BLAS or network access.

pub mod linalg;
pub mod mna;
pub mod transient;

pub use linalg::{DenseLu, LinearSolver, LuError};
pub use mna::{Circuit, DcResult, Device, NodeId, SolveError};
pub use transient::{TransientConfig, TransientInitialConditions, TransientResult};

#[cfg(test)]
mod tests;
