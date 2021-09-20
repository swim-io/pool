pub mod amp_factor;
pub mod decimal;
#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod error;
pub mod instruction;
mod invariant;
pub mod pool_fee;
pub mod processor;
pub mod state;
solana_program::declare_id!("4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM");
//solana_program::declare_id!("DF7HRRECYMAkkwanarcantNwnC7ehiw8sD53u5ZBMUyM");
//solana_program::declare_id!("6tWRQFov1NU1NmgQpd2WzN4RoPcR9MMTMt1pWAJtoCTF");
