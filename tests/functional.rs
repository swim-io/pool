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
use std::{collections::BTreeMap, str::FromStr};

type AmountT = u64;
type DecT = DecimalU64;

async fn get_account(banks_client: &mut BanksClient, pubkey: &Pubkey) -> Account {
    banks_client
        .get_account(*pubkey)
        .await
        .expect("account not found")
        .expect("account empty")
}

async fn get_token_balances<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    token_accounts: [Pubkey; TOKEN_COUNT],
) -> [u64; TOKEN_COUNT] {
    let mut token_accounts_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        let token_account = get_account(banks_client, &token_accounts[i]).await;
        let account_info = Token::unpack_from_slice(token_account.data.as_slice()).unwrap();
        token_accounts_arrvec.push(account_info.amount);
    }
    token_accounts_arrvec.into_inner().unwrap()
}

async fn get_token_balances2<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    token_accounts: [Pubkey; TOKEN_COUNT],
) -> BTreeMap<Pubkey, u64> {
    let mut btree = BTreeMap::<Pubkey, u64>::new();
    for i in 0..TOKEN_COUNT {
        let token_account = get_account(banks_client, &token_accounts[i]).await;
        let account_info = Token::unpack_from_slice(token_account.data.as_slice()).unwrap();
        btree.insert(token_accounts[i], account_info.amount);
    }
    btree
}

async fn print_user_token_account_owners<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    token_accounts: [Pubkey; TOKEN_COUNT],
) {
    for i in 0..TOKEN_COUNT {
        let token_account = get_account(banks_client, &token_accounts[i]).await;
        let spl_token_account_info = Token::unpack_from_slice(token_account.data.as_slice()).unwrap();
        println!("token_account.key: {} token_account.owner: {} spl_token_account_info.owner: {}", &token_accounts[i], token_account.owner, spl_token_account_info.owner);
    }
}

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

    const RESERVE_AMOUNT: u64 = 42;

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

    let amp_factor = DecimalU64::from(1000);
    let lp_fee = DecimalU64::from(0);
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

    print!("user_account_owner: {}, user_transfer_authority: {}", user_accounts_owner.pubkey(), user_transfer_authority.pubkey());
    print_user_token_account_owners(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after_tree = get_token_balances2(&mut banks_client, user_token_pubkeys).await;
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
    
    println!("[DEV] preparing accounts for swap");
    pool.prepare_accounts_for_swap_exact_input(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority.pubkey(),
        &user_token_pubkeys,
        exact_input_amounts,
    ).await;

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
    ).await;

    

    let user_token_balances_after_swap = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    println!("user_token_balances_after_swap: {:?}", user_token_balances_after_swap);
    for i in 0..TOKEN_COUNT - 1 {
        assert_eq!(user_token_balances_after[i] - exact_input_amounts[i], user_token_balances_after_swap[i]);
    }

    let governance_fee_balance = get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);


}

#[tokio::test]
async fn test_pool_swap_exact_output() {
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

    print!("user_account_owner: {}, user_transfer_authority: {}", user_accounts_owner.pubkey(), user_transfer_authority.pubkey());
    print_user_token_account_owners(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    let user_token_balances_after_tree = get_token_balances2(&mut banks_client, user_token_pubkeys).await;
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
    let mut exact_output_amounts_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
    let mut inc: u64 = 1;
    for i in 0..TOKEN_COUNT - 1 {
        let mint_amount: u64 = inc;
        exact_output_amounts_arrayvec.push(mint_amount);
        inc += 1;
    }
    exact_output_amounts_arrayvec.push(0);
    let exact_output_amounts: [AmountT; TOKEN_COUNT] = exact_output_amounts_arrayvec.into_inner().unwrap();
    println!("[DEV] exact_output_amounts: {:?}", exact_output_amounts);
    let input_token_index = 3;
    let maximum_input_amount = 10;
    //TODO: do i need to revoke afterwards?
    println!("[DEV] preparing accounts for swap");
    pool.prepare_accounts_for_swap_exact_output(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority.pubkey(),
        &user_token_pubkeys,
        maximum_input_amount,
        input_token_index,
        
    ).await;

    pool.execute_swap_exact_output(
        &mut banks_client,
        &payer,
        &user_accounts_owner,
        &user_transfer_authority,
        &user_token_accounts,
        &spl_token::id(),
        maximum_input_amount,
        input_token_index,
        exact_output_amounts,
    ).await;

    

    let user_token_balances_after_swap = get_token_balances(&mut banks_client, user_token_pubkeys).await;
    println!("user_token_balances_after_swap: {:?}", user_token_balances_after_swap);
    // for i in 0..TOKEN_COUNT - 1 {
    //     assert_eq!(user_token_balances_after[i] - exact_input_amounts[i], user_token_balances_after_swap[i]);
    // }

    let governance_fee_balance = get_token_balances::<{ 1 }>(&mut banks_client, [pool.governance_fee_keypair.pubkey()]).await;
    println!("governance_fee_balance: {:?}", governance_fee_balance);


}
