pub mod amp_factor;
pub mod decimal;
#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod error;
pub mod instruction;
pub mod invariant;
pub mod pool_fee;
pub mod processor;
pub mod state;

pub const TOKEN_COUNT: usize = 4; //TODO find a proper way to set/configure this
solana_program::declare_id!("4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM");
