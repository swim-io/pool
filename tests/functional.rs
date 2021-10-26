//#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use pool::{common::*, instruction::*, TOKEN_COUNT};
use solana_program_test::*;

struct Parameters {
    amp_factor: DecT,
    lp_fee: DecT,
    governance_fee: DecT,
    lp_decimals: u8,
    stable_decimals: [u8; TOKEN_COUNT],
    pool_balances: [AmountT; TOKEN_COUNT],
    user_funds: [AmountT; TOKEN_COUNT],
}

fn default_params() -> Parameters {
    Parameters {
        amp_factor: DecT::new(1000, 0).unwrap(),
        lp_fee: DecT::new(1000, 4).unwrap(),
        governance_fee: DecT::new(1000, 5).unwrap(),
        lp_decimals: 6,
        stable_decimals: [6; TOKEN_COUNT],
        pool_balances: [(10 as AmountT).pow(9); TOKEN_COUNT],
        user_funds: [(10 as AmountT).pow(8); TOKEN_COUNT],
    }
}

struct User {
    lp: TokenAccount,
    stables: [TokenAccount; TOKEN_COUNT],
}

impl User {
    fn new(
        funds: &[AmountT; TOKEN_COUNT],
        stable_mints: &[MintAccount; TOKEN_COUNT],
        pool_test: &DeployedPool,
        solnode: &mut SolanaNode,
    ) -> User {
        let lp = pool_test.create_lp_account(solnode);
        let stables: [_; TOKEN_COUNT] = create_array(|i| TokenAccount::new(&stable_mints[i], solnode));
        for i in 0..TOKEN_COUNT {
            stable_mints[i].mint_to(&stables[i], funds[i], solnode);
        }
        Self { lp, stables }
    }

    fn stable_approve(&self, amounts: &[AmountT; TOKEN_COUNT], solnode: &mut SolanaNode) {
        for i in 0..TOKEN_COUNT {
            self.stables[i].approve(amounts[i], solnode);
        }
    }

    async fn stable_balances(&self, solnode: &mut SolanaNode) -> [AmountT; TOKEN_COUNT] {
        let mut balances = [0; TOKEN_COUNT];
        for i in 0..TOKEN_COUNT {
            balances[i] = self.stables[i].balance(solnode).await;
        }
        balances
    }
}

async fn setup_standard_testcase(params: &Parameters) -> (SolanaNode, DeployedPool, User, User) {
    let mut solnode = SolanaNode::new().await;
    let stable_mints: [_; TOKEN_COUNT] = create_array(|i| MintAccount::new(params.stable_decimals[i], &mut solnode));
    let pool = DeployedPool::new(
        params.lp_decimals,
        &stable_mints,
        params.amp_factor,
        params.lp_fee,
        params.governance_fee,
        &mut solnode,
    )
    .await
    .unwrap();
    let user = User::new(&params.user_funds, &stable_mints, &pool, &mut solnode);
    let lp_collective = User::new(&params.pool_balances, &stable_mints, &pool, &mut solnode);
    lp_collective.stable_approve(&params.pool_balances, &mut solnode);
    let defi_ix = DeFiInstruction::<TOKEN_COUNT>::Add {
        input_amounts: params.pool_balances,
        minimum_mint_amount: 0 as AmountT,
    };
    pool.execute_defi_instruction(defi_ix, &lp_collective.stables, Some(&lp_collective.lp), &mut solnode)
        .await
        .unwrap();

    (solnode, pool, user, lp_collective)
}

#[tokio::test]
async fn test_pool_init() {
    let mut solnode = SolanaNode::new().await;

    let params = default_params();
    let stable_mints: [_; TOKEN_COUNT] = create_array(|i| MintAccount::new(params.stable_decimals[i], &mut solnode));

    let pool = DeployedPool::new(
        params.lp_decimals,
        &stable_mints,
        params.amp_factor,
        params.lp_fee,
        params.governance_fee,
        &mut solnode,
    )
    .await
    .unwrap();

    assert_eq!(pool.balances(&mut solnode).await, [0; TOKEN_COUNT]);
}

#[tokio::test]
async fn test_pool_add() {
    let params = default_params();
    let (mut solnode, pool, user, lp_collective) = setup_standard_testcase(&params).await;

    let input_amounts: [_; TOKEN_COUNT] = create_array(|i| (i + 1) as u64 * params.user_funds[i] / 10);
    user.stable_approve(&input_amounts, &mut solnode);
    let defi_ix = DeFiInstruction::Add {
        input_amounts,
        minimum_mint_amount: 0 as AmountT,
    };
    pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
        .await
        .unwrap();

    println!("pool balances: {:?}", pool.balances(&mut solnode).await);
    println!(
        "lp_collective lp balance: {}",
        lp_collective.lp.balance(&mut solnode).await
    );
    println!(
        "lp_collective stable balance: {:?}",
        lp_collective.stable_balances(&mut solnode).await
    );
    println!("user lp balance: {}", user.lp.balance(&mut solnode).await);
    println!("user stable balance: {:?}", user.stable_balances(&mut solnode).await);
}

#[tokio::test]
async fn test_pool_swap_exact_input() {
    let params = default_params();
    let (mut solnode, pool, user, _) = setup_standard_testcase(&params).await;
    let exact_input_amounts = create_array(|i| i as u64 * params.user_funds[i] / 10);

    user.stable_approve(&exact_input_amounts, &mut solnode);
    let defi_ix = DeFiInstruction::SwapExactInput {
        exact_input_amounts,
        output_token_index: 0,
        minimum_output_amount: 0 as AmountT,
    };

    let lp_supply_before = pool.lp_total_supply(&mut solnode).await;
    let depth_before = pool.state(&mut solnode).await.previous_depth;
    println!("> user balance before: {:?}", user.stable_balances(&mut solnode).await);
    pool.execute_defi_instruction(defi_ix, &user.stables, None, &mut solnode)
        .await
        .unwrap();

    let depth_after = pool.state(&mut solnode).await.previous_depth;
    let lp_supply_after = pool.lp_total_supply(&mut solnode).await;
    // lp_share/lp_supply_before * depth_before <= lp_share/lp_supply_after * depth_after
    //  a. "your share of the depth of the pool must never decrease"
    //  b. if lp_fee == 0 then your share should be the same otherwise it should increase
    if params.lp_fee + params.governance_fee == 0 {
        assert_eq!(
            depth_before,
            (depth_after * lp_supply_before as u128) / lp_supply_after as u128
        );
    } else {
        assert!(depth_before <= (depth_after * lp_supply_before as u128) / lp_supply_after as u128);
    }

    println!(">  user balance after: {:?}", user.stable_balances(&mut solnode).await);
}

#[tokio::test]
async fn test_pool_remove_uniform() {
    let mut params = default_params();
    params.user_funds = [0; TOKEN_COUNT];
    let (mut solnode, pool, _, lp_collective) = setup_standard_testcase(&params).await;

    let lp_total_supply = lp_collective.lp.balance(&mut solnode).await;
    lp_collective.lp.approve(lp_total_supply, &mut solnode);
    let defi_ix = DeFiInstruction::RemoveUniform {
        exact_burn_amount: lp_total_supply,
        minimum_output_amounts: params.pool_balances,
    };
    pool.execute_defi_instruction(defi_ix, &lp_collective.stables, Some(&lp_collective.lp), &mut solnode)
        .await
        .unwrap();

    assert_eq!(lp_collective.stable_balances(&mut solnode).await, params.pool_balances);
    assert_eq!(lp_collective.lp.balance(&mut solnode).await, 0);
    assert_eq!(pool.balances(&mut solnode).await, [0; TOKEN_COUNT]);
}

#[tokio::test]
async fn test_expensive_add() {
    let initial_balances: [AmountT; TOKEN_COUNT] =
        [5_590_413, 6_341_331, 4_947_048, 3_226_825, 2_560_56724, 3_339_50641];

    let user_add: [AmountT; TOKEN_COUNT] = [
        10_000_000,
        9_000_000,
        11_000_000,
        12_000_000,
        13_000_00000,
        12_000_00000,
    ];

    let params = Parameters {
        amp_factor: DecT::new(1000, 0).unwrap(),
        lp_fee: DecT::new(3, 6).unwrap(),
        governance_fee: DecT::new(1, 6).unwrap(),
        lp_decimals: 6,
        stable_decimals: create_array(|i| if i < 4 { 6 } else { 8 }),
        pool_balances: create_array(|i| initial_balances[i]),
        user_funds: create_array(|i| user_add[i]),
    };

    let (mut solnode, pool, user, _) = setup_standard_testcase(&params).await;

    user.stable_approve(&params.user_funds, &mut solnode);
    let defi_ix = DeFiInstruction::Add {
        input_amounts: params.user_funds,
        minimum_mint_amount: 0 as AmountT,
    };
    pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
        .await
        .unwrap();
}

// #[tokio::test]
// async fn test_expensive_add2() {
//     let initial_balances: [AmountT; TOKEN_COUNT] = [
//         28_799_968_080,
//         28_799_968_080,
//         8_861_528_640,
//         8_492_298_280,
//         6_646_146_480,
//         19_569_209_080,
//     ];

//     let user_add: [AmountT; TOKEN_COUNT] = [
//         2_879_996_964,
//         664_614_684,
//         1_956_921_014,
//         3_507_688_610,
//         664_614_684,
//         2_879_996_964,
//     ];

//     let params = Parameters {
//         amp_factor: DecT::from(1000),
//         lp_fee: DecT::new(2000, 5).unwrap(),
//         governance_fee: DecT::new(1000, 5).unwrap(),
//         lp_decimals: 6,
//         // stable_decimals: create_array(|i| if i < 4 { 6 } else { 8 }),
//         stable_decimals: create_array(|_| 6),
//         pool_balances: create_array(|i| initial_balances[i]),
//         user_funds: create_array(|i| user_add[i]),
//     };

//     let (mut solnode, pool, user, _) = setup_standard_testcase(&params).await;

//     user.stable_approve(&params.user_funds, &mut solnode);
//     let defi_ix = DeFiInstruction::Add {
//         input_amounts: params.user_funds,
//         minimum_mint_amount: 3_507_688_610 as AmountT,
//     };
//     pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
//         .await
//         .unwrap();
// }

// #[tokio::test]
// async fn test_expensive_add3() {
//     let initial_balances: [AmountT; TOKEN_COUNT] = [
//         289_625_991_284,
//         469_587_772_276,
//         289_625_991_284,
//         469_587_772_276,
//         303_685_505_424,
//         165_902_266_852,
//     ];

//     let user_add: [AmountT; TOKEN_COUNT] = [
//         31_809_650_787,
//         23_373_942_291,
//         33_742_833_984,
//         33_742_833_984,
//         33_742_833_984,
//         33_742_833_984,
//     ];

//     let params = Parameters {
//         amp_factor: DecT::from(1000),
//         lp_fee: DecT::new(2000, 5).unwrap(),
//         governance_fee: DecT::new(1000, 5).unwrap(),
//         lp_decimals: 6,
//         stable_decimals: create_array(|_| 6),
//         pool_balances: create_array(|i| initial_balances[i]),
//         user_funds: create_array(|i| user_add[i]),
//     };

//     let (mut solnode, pool, user, _) = setup_standard_testcase(&params).await;

//     user.stable_approve(&params.user_funds, &mut solnode);
//     let defi_ix = DeFiInstruction::Add {
//         input_amounts: params.user_funds,
//         minimum_mint_amount: 31_809_650_787 as AmountT,
//     };
//     pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
//         .await
//         .unwrap();
// }

// #[tokio::test]
// async fn test_expensive_add4() {
//     let initial_balances: [AmountT; TOKEN_COUNT] = [
//         289_625_991_284,
//         469_587_772_276,
//         289_625_991_284,
//         469_587_772_276,
//         303_685_505_424,
//         165_902_266_852,
//     ];

//     let user_add: [AmountT; TOKEN_COUNT] = [
//         28_962_599_046,
//         69_453_999_654,
//         53_988_534_144,
//         50_895_441_042,
//         50_895_441_042,
//         37_398_307_506,
//     ];

//     let params = Parameters {
//         amp_factor: DecT::from(1000),
//         lp_fee: DecT::new(2000, 5).unwrap(),
//         governance_fee: DecT::new(1000, 5).unwrap(),
//         lp_decimals: 6,
//         stable_decimals: create_array(|_| 6),
//         pool_balances: create_array(|i| initial_balances[i]),
//         user_funds: create_array(|i| user_add[i]),
//     };

//     let (mut solnode, pool, user, _) = setup_standard_testcase(&params).await;

//     user.stable_approve(&params.user_funds, &mut solnode);
//     let defi_ix = DeFiInstruction::Add {
//         input_amounts: params.user_funds,
//         minimum_mint_amount: 3_507_688_610 as AmountT,
//     };
//     pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
//         .await
//         .unwrap();
// }
