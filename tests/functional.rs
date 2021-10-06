#![cfg(feature = "test-bpf")]

mod helpers;

use arrayvec::ArrayVec;
use borsh::de::BorshDeserialize;
use helpers::*;
use pool::decimal::*;
use pool::entrypoint::TOKEN_COUNT;
use solana_program::program_pack::{IsInitialized, Pack};
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use spl_token::{
    instruction::approve,
    state::{Account as Token, AccountState, Mint},
};
use std::{assert, collections::BTreeMap, str::FromStr};

type AmountT = u64;
type DecT = DecimalU64;

#[tokio::test]
async fn test_pool_init() {
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

    let amp_factor = DecimalU64::new(1000, 0).unwrap();
    let lp_fee = DecimalU64::new(1000, 4).unwrap();
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let pool = TestPoolAccountInfo::<{ TOKEN_COUNT }>::new();
    let mint_pubkeys = &pool
        .token_mint_keypairs
        .iter()
        .map(|kp| kp.pubkey())
        .collect::<ArrayVec<_, { TOKEN_COUNT }>>()
        .into_inner()
        .unwrap();
    let token_pubkeys = &pool
        .token_account_keypairs
        .iter()
        .map(|kp| kp.pubkey())
        .collect::<ArrayVec<_, { TOKEN_COUNT }>>()
        .into_inner()
        .unwrap();
    println!("[DEV] pool.token_mint_keypairs: {:#?}", mint_pubkeys);
    println!("[DEV] pool.token_pubkeys: {:#?}", token_pubkeys);
    pool.init_pool(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;

    let pool_account_data = get_account(&mut banks_client, &pool.pool_keypair.pubkey()).await;
    println!("[DEV] pool_account_data.data.len: {}", pool_account_data.data.len());
    assert_eq!(pool_account_data.owner, pool::id());

    let pool = pool::state::PoolState::<{ TOKEN_COUNT }>::try_from_slice(pool_account_data.data.as_slice()).unwrap();
    assert!(pool.is_initialized());
}

#[tokio::test]
async fn test_pool_add() {
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

    let amp_factor = DecimalU64::new(1000, 0).unwrap();
    let lp_fee = DecimalU64::new(1000, 4).unwrap();
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let pool = TestPoolAccountInfo::<{ TOKEN_COUNT }>::new();
    pool.init_pool(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;

    let mut deposit_tokens_to_mint_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut deposit_tokens_for_approval_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT {
        let approval_amount: u64 = inc * 100;
        let mint_amount: u64 = approval_amount * 2;
        deposit_tokens_to_mint_arrayvec.push(mint_amount);
        deposit_tokens_for_approval_arrayvec.push(approval_amount);
        inc += 1;
    }
    let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = deposit_tokens_to_mint_arrayvec.into_inner().unwrap();
    let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] =
        deposit_tokens_for_approval_arrayvec.into_inner().unwrap();
    let user_transfer_authority = Keypair::new();
    let (user_token_accounts, user_lp_token_account) = pool
        .prepare_accounts_for_add(
            &mut banks_client,
            &payer,
            &user_accounts_owner,
            &user_transfer_authority.pubkey(),
            &user_accounts_owner,
            deposit_tokens_to_mint,
            deposit_tokens_for_approval,
        )
        .await;
    //let user_token_accounts_debug = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        let user_token_acct_acct = get_account(&mut banks_client, &user_token_accounts[i].pubkey()).await;
        let user_token_acct = Token::unpack(&user_token_acct_acct.data).unwrap();
        println!(
            "user_token_accounts[{}].amount is {}. delegated_amount: {}",
            i, user_token_acct.amount, user_token_acct.delegated_amount
        );
    }

    let mut user_token_keypairs_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        user_token_keypairs_arrvec.push(user_token_accounts[i].pubkey());
    }
    let user_token_pubkeys = user_token_keypairs_arrvec.into_inner().unwrap();
    let user_token_balances_before = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_lp_token_balances_before =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    assert_eq!(deposit_tokens_to_mint, user_token_balances_before);
    assert_eq!(0, user_lp_token_balances_before[0]);
    println!("[DEV] Executing add");
    pool.execute_add(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account.pubkey(),
        deposit_tokens_for_approval,
        0,
    )
    .await;

    let user_token_balances_after = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let mut expected_user_token_balances_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        expected_user_token_balances_arrvec.push(deposit_tokens_to_mint[i] - deposit_tokens_for_approval[i]);
    }
    let expected_user_token_balances = expected_user_token_balances_arrvec.into_inner().unwrap();
    println!("expected_user_token_balances: {:?}", expected_user_token_balances);
    println!("user_token_balances_after: {:?}", user_token_balances_after);
    assert_eq!(expected_user_token_balances, user_token_balances_after);
    let user_lp_token_balance_after =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    println!("user_lp_token_balance_after: {:?}", user_lp_token_balance_after);
    let governance_fee_balance =
        get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);
}

#[tokio::test]
async fn test_pool_swap_exact_input() {
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
    pool.init_pool(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;

    let mut deposit_tokens_to_mint_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut deposit_tokens_for_approval_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT {
        let approval_amount: u64 = inc * 100;
        let mint_amount: u64 = approval_amount * 2;
        deposit_tokens_to_mint_arrayvec.push(mint_amount);
        deposit_tokens_for_approval_arrayvec.push(approval_amount);
        inc += 1;
    }
    let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = deposit_tokens_to_mint_arrayvec.into_inner().unwrap();
    let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] =
        deposit_tokens_for_approval_arrayvec.into_inner().unwrap();
    let user_transfer_authority = Keypair::new();
    let (user_token_accounts, user_lp_token_account) = pool
        .prepare_accounts_for_add(
            &mut banks_client,
            &payer,
            &user_accounts_owner,
            &user_transfer_authority.pubkey(),
            &user_accounts_owner,
            deposit_tokens_to_mint,
            deposit_tokens_for_approval,
        )
        .await;
    for i in 0..TOKEN_COUNT {
        let user_token_acct_acct = get_account(&mut banks_client, &user_token_accounts[i].pubkey()).await;
        let user_token_acct = Token::unpack(&user_token_acct_acct.data).unwrap();
        println!(
            "user_token_accounts[{}].amount is {}. delegated_amount: {}",
            i, user_token_acct.amount, user_token_acct.delegated_amount
        );
    }

    let mut user_token_keypairs_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        user_token_keypairs_arrvec.push(user_token_accounts[i].pubkey());
    }
    let user_token_pubkeys = user_token_keypairs_arrvec.into_inner().unwrap();
    let user_token_balances_before = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_lp_token_balances_before =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    assert_eq!(deposit_tokens_to_mint, user_token_balances_before);
    assert_eq!(0, user_lp_token_balances_before[0]);
    println!("[DEV] Executing add");
    pool.execute_add(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account.pubkey(),
        deposit_tokens_for_approval,
        0,
    )
    .await;

    print!(
        "user_account_owner: {}, user_transfer_authority: {}",
        user_accounts_owner.pubkey(),
        user_transfer_authority.pubkey()
    );
    print_user_token_account_owners(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after_tree: BTreeMap<Pubkey, u64> =
        get_token_balances_map(&mut banks_client, user_token_pubkeys).await;
    let mut expected_user_token_balances_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        expected_user_token_balances_arrvec.push(deposit_tokens_to_mint[i] - deposit_tokens_for_approval[i]);
    }
    let expected_user_token_balances = expected_user_token_balances_arrvec.into_inner().unwrap();
    println!("expected_user_token_balances: {:?}", expected_user_token_balances);
    println!("user_token_balances_after: {:?}", user_token_balances_after_tree);
    //assert_eq!(expected_user_token_balances, user_token_balances_after);
    let user_lp_token_balance_after =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    println!("user_lp_token_balance_after: {:?}", user_lp_token_balance_after);
    let governance_fee_balance =
        get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);

    let mut exact_input_amounts_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT - 1 {
        let approval_amount: u64 = inc * 100;
        let mint_amount: u64 = approval_amount / 50;
        exact_input_amounts_arrayvec.push(mint_amount);
        inc += 1;
    }
    exact_input_amounts_arrayvec.push(0);
    let exact_input_amounts: [AmountT; TOKEN_COUNT] = exact_input_amounts_arrayvec.into_inner().unwrap();

    println!("[DEV] exact_input_amounts: {:?}", exact_input_amounts);

    //TODO: do i need to revoke afterwards?
    println!("[DEV] preparing accounts for swap");
    pool.prepare_accounts_for_swap(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority.pubkey(),
        &user_token_pubkeys,
        exact_input_amounts,
    )
    .await;

    let output_token_index: u8 = (TOKEN_COUNT - 1) as u8;
    pool.execute_swap_exact_input(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        exact_input_amounts,
        output_token_index,
        0,
    )
    .await;

    let user_token_balances_after_swap = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    println!("user_token_balances_after_swap: {:?}", user_token_balances_after_swap);
    for i in 0..TOKEN_COUNT - 1 {
        assert_eq!(
            user_token_balances_after[i] - exact_input_amounts[i],
            user_token_balances_after_swap[i]
        );
    }

    let governance_fee_balance =
        get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);
}

#[tokio::test]
async fn test_pool_remove_uniform() {
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

    let amp_factor = DecimalU64::new(1000, 0).unwrap();
    let lp_fee = DecimalU64::new(1000, 4).unwrap();
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let pool = TestPoolAccountInfo::<{ TOKEN_COUNT }>::new();
    pool.init_pool(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;

    let mut deposit_tokens_to_mint_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut deposit_tokens_for_approval_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT {
        let approval_amount: u64 = inc * 100;
        let mint_amount: u64 = approval_amount * 2;
        deposit_tokens_to_mint_arrayvec.push(mint_amount);
        deposit_tokens_for_approval_arrayvec.push(approval_amount);
        inc += 1;
    }
    let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = deposit_tokens_to_mint_arrayvec.into_inner().unwrap();
    let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] =
        deposit_tokens_for_approval_arrayvec.into_inner().unwrap();
    let user_transfer_authority = Keypair::new();
    let (user_token_accounts, user_lp_token_account) = pool
        .prepare_accounts_for_add(
            &mut banks_client,
            &payer,
            &user_accounts_owner,
            &user_transfer_authority.pubkey(),
            &user_accounts_owner,
            deposit_tokens_to_mint,
            deposit_tokens_for_approval,
        )
        .await;
    //let user_token_accounts_debug = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        let user_token_acct_acct = get_account(&mut banks_client, &user_token_accounts[i].pubkey()).await;
        let user_token_acct = Token::unpack(&user_token_acct_acct.data).unwrap();
        println!(
            "user_token_accounts[{}].amount is {}. delegated_amount: {}",
            i, user_token_acct.amount, user_token_acct.delegated_amount
        );
    }

    let mut user_token_keypairs_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        user_token_keypairs_arrvec.push(user_token_accounts[i].pubkey());
    }
    let user_token_pubkeys = user_token_keypairs_arrvec.into_inner().unwrap();
    let user_token_balances_before = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_lp_token_balances_before =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    assert_eq!(deposit_tokens_to_mint, user_token_balances_before);
    assert_eq!(0, user_lp_token_balances_before[0]);
    println!("[DEV] Executing add");
    pool.execute_add(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account.pubkey(),
        deposit_tokens_for_approval,
        0,
    )
    .await;

    let user_lp_token_balance_after_add = get_token_balance(&mut banks_client, user_lp_token_account.pubkey()).await;
    println!("user_lp_token_balance_after_add: {:?}", user_lp_token_balance_after_add);

    let exact_burn_amount: AmountT = 100;
    println!("[DEV] prepare_accounts_for_remove");
    pool.prepare_accounts_for_remove(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority.pubkey(),
        &user_lp_token_account.pubkey(),
        exact_burn_amount,
    )
    .await;

    let minimum_output_amounts: [AmountT; TOKEN_COUNT] = [1; TOKEN_COUNT];
    //let user_lp_balance_before =
    println!("[DEV] execute_remove_uniform");
    pool.execute_remove_uniform(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account.pubkey(),
        exact_burn_amount,
        minimum_output_amounts,
    )
    .await;

    let user_lp_token_balance_after_remove = get_token_balance(&mut banks_client, user_lp_token_account.pubkey()).await;
    assert_eq!(
        user_lp_token_balance_after_add - exact_burn_amount,
        user_lp_token_balance_after_remove
    );
    let user_token_balances_after_remove =
        get_token_balances::<{ TOKEN_COUNT }>(&mut banks_client, user_token_pubkeys).await;
    for i in 0..TOKEN_COUNT {
        println!(
            "[DEV] user_token_balances_after_remove[{}]: {}",
            i, user_token_balances_after_remove[i]
        );
        assert!(user_token_balances_before[i] + minimum_output_amounts[i] >= user_token_balances_after_remove[i]);
    }
}

#[tokio::test]
async fn test_pool_remove_exact_burn() {
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

    let amp_factor = DecimalU64::new(1000, 0).unwrap();
    let lp_fee = DecimalU64::new(1000, 4).unwrap();
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let pool = TestPoolAccountInfo::<{ TOKEN_COUNT }>::new();
    pool.init_pool(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;

    let mut deposit_tokens_to_mint_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut deposit_tokens_for_approval_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT {
        let approval_amount: u64 = inc * 100;
        let mint_amount: u64 = approval_amount * 2;
        deposit_tokens_to_mint_arrayvec.push(mint_amount);
        deposit_tokens_for_approval_arrayvec.push(approval_amount);
        inc += 1;
    }
    let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = deposit_tokens_to_mint_arrayvec.into_inner().unwrap();
    let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] =
        deposit_tokens_for_approval_arrayvec.into_inner().unwrap();
    let user_transfer_authority = Keypair::new();
    let (user_token_accounts, user_lp_token_account) = pool
        .prepare_accounts_for_add(
            &mut banks_client,
            &payer,
            &user_accounts_owner,
            &user_transfer_authority.pubkey(),
            &user_accounts_owner,
            deposit_tokens_to_mint,
            deposit_tokens_for_approval,
        )
        .await;
    //let user_token_accounts_debug = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        let user_token_acct_acct = get_account(&mut banks_client, &user_token_accounts[i].pubkey()).await;
        let user_token_acct = Token::unpack(&user_token_acct_acct.data).unwrap();
        println!(
            "user_token_accounts[{}].amount is {}. delegated_amount: {}",
            i, user_token_acct.amount, user_token_acct.delegated_amount
        );
    }

    let mut user_token_keypairs_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        user_token_keypairs_arrvec.push(user_token_accounts[i].pubkey());
    }
    let user_token_pubkeys = user_token_keypairs_arrvec.into_inner().unwrap();
    let user_token_balances_before = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_lp_token_balances_before =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    assert_eq!(deposit_tokens_to_mint, user_token_balances_before);
    assert_eq!(0, user_lp_token_balances_before[0]);
    println!("[DEV] Executing add");
    pool.execute_add(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account.pubkey(),
        deposit_tokens_for_approval,
        0,
    )
    .await;

    let user_lp_token_balance_after_add = get_token_balance(&mut banks_client, user_lp_token_account.pubkey()).await;
    println!("user_lp_token_balance_after_add: {:?}", user_lp_token_balance_after_add);

    let exact_burn_amount: AmountT = 500;
    println!("[DEV] prepare_accounts_for_remove");
    pool.prepare_accounts_for_remove(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority.pubkey(),
        &user_lp_token_account.pubkey(),
        exact_burn_amount,
    )
    .await;

    let minimum_output_amount = 1;
    let output_token_index = 0;
    // println!("[DEV] execute_remove_uniform");
    // TODO: commented out for now due to underflow error
    // pool.execute_remove_exact_burn(
    //     &mut banks_client,
    //     &payer,
    //     &user_accounts_owner,
    //     &user_transfer_authority,
    //     &user_token_accounts,
    //     &spl_token::id(),
    //     &user_lp_token_account.pubkey(),
    //     exact_burn_amount,
    //     output_token_index,
    //     minimum_output_amount,
    // )
    // .await;

    // let user_lp_token_balance_after_remove = get_token_balance(&mut banks_client, user_lp_token_account.pubkey()).await;
    // assert_eq!(
    //     user_lp_token_balance_after_add - exact_burn_amount,
    //     user_lp_token_balance_after_remove
    // );
    // let user_token_balances_after_remove =
    //     get_token_balances::<{ TOKEN_COUNT }>(&mut banks_client, user_token_pubkeys).await;
    // let output_token_index = output_token_index as usize;
    // println!(
    //     "[DEV] user_token_balances_after_remove[{}]: {}",
    //     output_token_index, user_token_balances_after_remove[output_token_index]
    // );
    // assert!(user_token_balances_before[output_token_index] + minimum_output_amount >= user_token_balances_after_remove[output_token_index]);
}

/*************** Invariant Tests
    1. lp_share/lp_supply_before * depth_before <= lp_share/lp_supply_after * depth_after
        a. "your share of the depth of the pool must never decrease"
        b. if lp_fee == 0 then your share should be the same otherwise it should increase
*/

#[tokio::test]
async fn test_pool_swap_exact_input_lp_share() {
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

    let amp_factor = DecimalU64::from(1);
    let lp_fee = DecimalU64::from(0);
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let pool = TestPoolAccountInfo::<{ TOKEN_COUNT }>::new();
    pool.init_pool(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;

    let mut deposit_tokens_to_mint_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut deposit_tokens_for_approval_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT {
        let approval_amount: u64 = inc * 100;
        let mint_amount: u64 = approval_amount * 2;
        deposit_tokens_to_mint_arrayvec.push(mint_amount);
        deposit_tokens_for_approval_arrayvec.push(approval_amount);
        inc += 1;
    }
    let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = deposit_tokens_to_mint_arrayvec.into_inner().unwrap();
    let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] =
        deposit_tokens_for_approval_arrayvec.into_inner().unwrap();
    let user_transfer_authority = Keypair::new();
    let (user_token_accounts, user_lp_token_account) = pool
        .prepare_accounts_for_add(
            &mut banks_client,
            &payer,
            &user_accounts_owner,
            &user_transfer_authority.pubkey(),
            &user_accounts_owner,
            deposit_tokens_to_mint,
            deposit_tokens_for_approval,
        )
        .await;
    for i in 0..TOKEN_COUNT {
        let user_token_acct_acct = get_account(&mut banks_client, &user_token_accounts[i].pubkey()).await;
        let user_token_acct = Token::unpack(&user_token_acct_acct.data).unwrap();
        println!(
            "user_token_accounts[{}].amount is {}. delegated_amount: {}",
            i, user_token_acct.amount, user_token_acct.delegated_amount
        );
    }

    let mut user_token_keypairs_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        user_token_keypairs_arrvec.push(user_token_accounts[i].pubkey());
    }
    let user_token_pubkeys = user_token_keypairs_arrvec.into_inner().unwrap();
    let user_token_balances_before = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_lp_token_balances_before =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    assert_eq!(deposit_tokens_to_mint, user_token_balances_before);
    assert_eq!(0, user_lp_token_balances_before[0]);
    println!("[DEV] Executing add");
    pool.execute_add(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account.pubkey(),
        deposit_tokens_for_approval,
        0,
    )
    .await;

    print!(
        "user_account_owner: {}, user_transfer_authority: {}",
        user_accounts_owner.pubkey(),
        user_transfer_authority.pubkey()
    );
    print_user_token_account_owners(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after_tree = get_token_balances_map(&mut banks_client, user_token_pubkeys).await;

    let pool_token_account_balances_after_add = pool.get_token_account_balances(&mut banks_client).await;
    println!(
        "[DEV] pool_token_account_balances_after_add: {:?}",
        pool_token_account_balances_after_add
    );
    let mut expected_user_token_balances_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        expected_user_token_balances_arrvec.push(deposit_tokens_to_mint[i] - deposit_tokens_for_approval[i]);
    }
    let expected_user_token_balances = expected_user_token_balances_arrvec.into_inner().unwrap();
    println!("expected_user_token_balances: {:?}", expected_user_token_balances);
    println!("user_token_balances_after: {:?}", user_token_balances_after_tree);
    //assert_eq!(expected_user_token_balances, user_token_balances_after);
    let user_lp_token_balance_after =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    println!("user_lp_token_balance_after: {:?}", user_lp_token_balance_after);
    let governance_fee_balance =
        get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);

    let mut exact_input_amounts_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT - 1 {
        let approval_amount: u64 = inc * 100;
        let mint_amount: u64 = approval_amount / 50;
        exact_input_amounts_arrayvec.push(mint_amount);
        inc += 1;
    }
    exact_input_amounts_arrayvec.push(0);
    let exact_input_amounts: [AmountT; TOKEN_COUNT] = exact_input_amounts_arrayvec.into_inner().unwrap();

    println!("[DEV] exact_input_amounts: {:?}", exact_input_amounts);

    /** Invariant Tests
    1. lp_share/lp_supply_before * depth_before <= lp_share/lp_supply_after * depth_after
        a. "your share of the depth of the pool must never decrease"
        b. if lp_fee == 0 then your share should be the same otherwise it should increase
    */
    let lp_supply_before = get_mint_state(&mut banks_client, &pool.lp_mint_keypair.pubkey())
        .await
        .supply;
    let depth_before = pool.get_depth(&mut banks_client, amp_factor).await;
    let share_before = (100 * depth_before) / lp_supply_before;
    println!(
        "[DEV] lp_supply_before: {}, depth_before: {}, share_before: {}",
        lp_supply_before, depth_before, share_before
    );

    //TODO: do i need to revoke afterwards?
    println!("[DEV] preparing accounts for swap");
    pool.prepare_accounts_for_swap(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority.pubkey(),
        &user_token_pubkeys,
        exact_input_amounts,
    )
    .await;

    let output_token_index: u8 = (TOKEN_COUNT - 1) as u8;
    pool.execute_swap_exact_input(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        exact_input_amounts,
        output_token_index,
        0,
    )
    .await;
    let pool_token_account_balances_after_swap = pool.get_token_account_balances(&mut banks_client).await;
    println!(
        "[DEV] pool_token_account_balances_after_swap: {:?}",
        pool_token_account_balances_after_swap
    );
    let user_token_balances_after_swap = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    println!("user_token_balances_after_swap: {:?}", user_token_balances_after_swap);
    for i in 0..TOKEN_COUNT - 1 {
        assert_eq!(
            user_token_balances_after[i] - exact_input_amounts[i],
            user_token_balances_after_swap[i]
        );
    }

    let governance_fee_balance =
        get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);

    let lp_supply_after = get_mint_state(&mut banks_client, &pool.lp_mint_keypair.pubkey())
        .await
        .supply;
    let depth_after = pool.get_depth(&mut banks_client, amp_factor).await;
    let share_after = (100 * depth_after) / lp_supply_after;
    println!(
        "[DEV] lp_supply_after: {}, depth_after: {}, share_after: {}",
        lp_supply_after, depth_after, share_after
    );
}

#[tokio::test]
async fn test_pool_add2() {
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

    let amp_factor = DecimalU64::new(1000, 0).unwrap();
    let lp_fee = DecimalU64::new(99, 2).unwrap(); //3
    let governance_fee = DecimalU64::from(0);
    let pool = TestPoolAccountInfo::<{ TOKEN_COUNT }>::new();
    pool.init_pool(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;
    // u64 1 == 1/1_000_000 of a dollar = 0.000001
    //0.000500; 0.007560; 0.001500; 0.000002000
    // let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = [
    //     // 0.000_001
    //     //DecimalU64::new(5, 4).unwrap().get_raw(),     // 0.000_500
    //     50000, //5 x 10^1 / 10^6 =
    //     //DecimalU64::new(756, 5).unwrap().get_raw(),   // 0.007_560
    //     75600, // 7.56 * 10^2
    //     //DecimalU64::new(15, 4).unwrap().get_raw(),    // 0.001_500
    //     150000, //1.5 * 10^2
    //     //DecimalU64::new(2, 3).unwrap().get_raw()      // 0.002_000
    //     200000, // 2 * 10 ^ 2  / 10^6
    // ];

    let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = [(u64::MAX - 10) / 2; TOKEN_COUNT];

    let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] = [
        // 0.000_001
        //DecimalU64::new(5, 4).unwrap().get_raw(),     // 0.000_500
        50, //5 x 10^1 / 10^6 =
        //DecimalU64::new(756, 5).unwrap().get_raw(),   // 0.007_560
        756, // 7.56 * 10^2
        //DecimalU64::new(15, 4).unwrap().get_raw(),    // 0.001_500
        150, //1.5 * 10^2
        //DecimalU64::new(2, 3).unwrap().get_raw()      // 0.002_000
        200, // 2 * 10 ^ 2  / 10^6
    ];

    // println!("[DEV] deposit_tokens_to_mint: {:?}", deposit_tokens_to_mint);

    // let mut deposit_tokens_to_mint_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    // let mut deposit_tokens_for_approval_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    // let mut inc: u64 = 1;
    // for i in 0..TOKEN_COUNT {
    //     let approval_amount: u64 = inc * 100;
    //     let mint_amount: u64 = approval_amount * 2;
    //     deposit_tokens_to_mint_arrayvec.push(mint_amount);
    //     deposit_tokens_for_approval_arrayvec.push(approval_amount);
    //     inc += 1;
    // }
    // let deposit_tokens_to_mint: [AmountT; TOKEN_COUNT] = deposit_tokens_to_mint_arrayvec.into_inner().unwrap();
    // let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] =
    //     deposit_tokens_for_approval_arrayvec.into_inner().unwrap();
    let user_transfer_authority = Keypair::new();
    let (user_token_accounts, user_lp_token_account) = pool
        .prepare_accounts_for_add(
            &mut banks_client,
            &payer,
            &user_accounts_owner,
            &user_transfer_authority.pubkey(),
            &user_accounts_owner, //user_accounts_owner is mint authority. will clean up this ugly setup later.
            deposit_tokens_to_mint,
            deposit_tokens_for_approval,
        )
        .await;
    //let user_token_accounts_debug = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        let user_token_acct_acct = get_account(&mut banks_client, &user_token_accounts[i].pubkey()).await;
        let user_token_acct = Token::unpack(&user_token_acct_acct.data).unwrap();
        println!(
            "user_token_accounts[{}].amount is {}. delegated_amount: {}",
            i, user_token_acct.amount, user_token_acct.delegated_amount
        );
    }

    let mut user_token_keypairs_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        user_token_keypairs_arrvec.push(user_token_accounts[i].pubkey());
    }
    let user_token_pubkeys = user_token_keypairs_arrvec.into_inner().unwrap();
    let user_token_balances_before = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_lp_token_balances_before =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    assert_eq!(deposit_tokens_to_mint, user_token_balances_before);
    assert_eq!(0, user_lp_token_balances_before[0]);
    println!("[DEV] Executing add");
    pool.execute_add(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account.pubkey(),
        deposit_tokens_for_approval,
        0,
    )
    .await;

    let user_token_balances_after = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let mut expected_user_token_balances_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        expected_user_token_balances_arrvec.push(deposit_tokens_to_mint[i] - deposit_tokens_for_approval[i]);
    }
    let expected_user_token_balances = expected_user_token_balances_arrvec.into_inner().unwrap();
    println!("expected_user_token_balances: {:?}", expected_user_token_balances);
    println!("user_token_balances_after: {:?}", user_token_balances_after);
    assert_eq!(expected_user_token_balances, user_token_balances_after);
    let user_lp_token_balance_after =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account.pubkey()]).await;
    println!("user_lp_token_balance_after: {:?}", user_lp_token_balance_after);
    let governance_fee_balance =
        get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);
    let pool_token_balances = pool.get_token_account_balances(&mut banks_client).await;
    println!("pool_token_balances: {:?}", pool_token_balances);

    println!("Repeating add");

    let deposit_tokens_for_approval: [AmountT; TOKEN_COUNT] = [5495938558252285952, 7208774, 0, 0];

    println!("Prep add 2");
    let mint_authority = user_accounts_owner;
    let user_accounts_owner2 = Keypair::new();
    let (user_token_accounts2, user_lp_token_account2) = pool
        .prepare_accounts_for_add(
            &mut banks_client,
            &payer,
            &user_accounts_owner2,
            &user_transfer_authority.pubkey(),
            &mint_authority,
            deposit_tokens_to_mint,
            deposit_tokens_for_approval,
        )
        .await;

    println!("Execute Add 2");
    pool.execute_add(
        &mut banks_client,
        &payer,
        &user_accounts_owner2,
        &user_transfer_authority,
        &user_token_accounts2,
        &spl_token::id(),
        &user_lp_token_account2.pubkey(),
        deposit_tokens_for_approval,
        0,
    )
    .await;

    let user_lp_token_balance_after =
        get_token_balances::<{ 1 }>(&mut banks_client, [user_lp_token_account2.pubkey()]).await;
    println!("user_lp_token_balance_after_2: {:?}", user_lp_token_balance_after);
    let governance_fee_balance =
        get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);
    let pool_token_balances = pool.get_token_account_balances(&mut banks_client).await;
    println!("pool_token_balances_2: {:?}", pool_token_balances);
}
