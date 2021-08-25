#![cfg(feature = "test-bpf")]
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::str::FromStr;

#[tokio::test]
async fn test_pool () {
    const TOKEN_COUNT:usize = 6;
    let mut test = ProgramTest::new(
        "pool",
        pool::id(),
        processor!(pool::processor::Processor::<TOKEN_COUNT>::process),
    );

    // limit to track compute unit increase. 
    // Mainnet compute budget as of 08/25/2021 is 200_000
    test.set_bpf_compute_max_units(50_000);
}