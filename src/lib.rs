#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod error;
pub mod instruction;
pub mod amp_factor;
pub mod pool_fee;
pub mod processor;
pub mod state;
pub mod decimal;
mod invariant;
solana_program::declare_id!("4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM");
