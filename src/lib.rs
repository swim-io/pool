pub mod amp_factor;
pub mod common;
pub mod decimal;
#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod error;
pub mod instruction;
pub mod invariant;
pub mod pool_fee;
pub mod processor;
pub mod state;

pub const TOKEN_COUNT: usize = 6;

// 4 Pool
//solana_program::declare_id!("SWiMBJS9iBU1rMLAKBVfp73ThW1xPPwKdBHEU2JFpuo");

// 6 Pool
solana_program::declare_id!("SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC");
