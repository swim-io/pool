#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::str::FromStr;
use pool::decimal::*;

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

    //TODO: not sure if needed
    let user_accounts_owner = Keypair::new(); 

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    const RESERVE_AMOUNT: u64 = 42;

    // let _sol_user_liquidity_account = create_and_mint_to_token_account(
    //     &mut banks_client,
    //     spl_token::native_mint::id(),
    //     None,
    //     &payer,
    //     user_accounts_owner.pubkey(),
    //     RESERVE_AMOUNT,
    // )
    // .await;

    let amp_factor = DecimalU64::new(1000, 0).unwrap();
    let lp_fee = DecimalU64::new(1000, 4).unwrap();
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let pool = TestPoolAccountInfo::<TOKEN_COUNT>::init(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee
    ).await
    .unwrap();
}