#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod state;
pub mod error;
pub mod instruction;
pub mod processor;
pub mod pool_fee;
pub mod amp_factor;

solana_program::declare_id!("4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM");