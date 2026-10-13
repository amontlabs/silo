pub mod operation_gate;
pub mod image_cache;

#[cfg(test)]
pub(crate) mod contract_tests;

/// Process-wide ordered admission for work that changes computer runtime state.
pub static OPERATIONS: operation_gate::OperationGate = operation_gate::OperationGate::new();
