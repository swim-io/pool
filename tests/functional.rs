#![cfg(feature = "test-bpf")]

mod helpers;

use arrayvec::ArrayVec;
use helpers::*;
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::str::FromStr;
use pool::decimal::*;
use pool::entrypoint::TOKEN_COUNT;
use borsh::de::BorshDeserialize;

async fn get_account(banks_client: &mut BanksClient, pubkey: &Pubkey) -> Account {
    banks_client
        .get_account(*pubkey)
        .await
        .expect("account not found")
        .expect("account empty")
}

#[tokio::test]
async fn test_pool_init () {

    let mut test = ProgramTest::new(
        "pool",
        pool::id(),
        processor!(pool::processor::Processor::<{ TOKEN_COUNT }>::process),
    );

    // limit to track compute unit increase. 
    // Mainnet compute budget as of 08/25/2021 is 200_000
    test.set_bpf_compute_max_units(200_000);

    //TODO: not sure if needed
    let user_accounts_owner = Keypair::new(); 

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    const RESERVE_AMOUNT: u64 = 42;


    let amp_factor = DecimalU64::new(1000, 0).unwrap();
    let lp_fee = DecimalU64::new(1000, 4).unwrap();
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let pool = TestPoolAccountInfo::<{ TOKEN_COUNT }>::new();
    let mint_pubkeys = &pool.token_mint_keypairs.iter().map(|kp| kp.pubkey()).collect::<ArrayVec<_, { TOKEN_COUNT }>>().into_inner().unwrap();
    let token_pubkeys = &pool.token_account_keypairs.iter().map(|kp| kp.pubkey()).collect::<ArrayVec<_, { TOKEN_COUNT }>>().into_inner().unwrap();
    println!("[DEV] pool.token_mint_keypairs: {:#?}", mint_pubkeys);
    println!("[DEV] pool.token_pubkeys: {:#?}", token_pubkeys);
    pool.init_pool(&mut banks_client, &payer, &user_accounts_owner, amp_factor, lp_fee, governance_fee).await;

    let pool_account_data = get_account(&mut banks_client, &pool.pool_keypair.pubkey()).await;
    println!("[DEV] pool_account_data.data.len: {}", pool_account_data.data.len());
    assert_eq!(pool_account_data.owner, pool::id());

    let pool = pool::state::PoolState::<{ TOKEN_COUNT }>::try_from_slice(pool_account_data.data.as_slice()).unwrap();
    assert!(pool.is_initialized());
}

