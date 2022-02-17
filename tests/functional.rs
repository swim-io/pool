#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
// use helpers::mod_v2::*;
use pool::{common::*, instruction::*, TOKEN_COUNT};
use solana_program_test::*;
use solana_sdk::signature::{Keypair, Signer};
use std::time::{SystemTime, UNIX_EPOCH};

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

    fn stable_balances(&self, solnode: &mut SolanaNode) -> [AmountT; TOKEN_COUNT] {
        let mut balances = [0; TOKEN_COUNT];
        for i in 0..TOKEN_COUNT {
            balances[i] = self.stables[i].balance(solnode);
        }
        balances
    }
}
fn setup_standard_testcase(params: &Parameters) -> (SolanaNode, DeployedPool, User, User) {
    let mut solnode = SolanaNode::new();
    let stable_mints: [_; TOKEN_COUNT] = create_array(|i| MintAccount::new(params.stable_decimals[i], &mut solnode));
    solnode.execute_transaction().expect("transaction failed unexpectedly");

    let pool = DeployedPool::new(
        params.lp_decimals,
        &stable_mints,
        params.amp_factor,
        params.lp_fee,
        params.governance_fee,
        &mut solnode,
    )
    .unwrap();
    let user = User::new(&params.user_funds, &stable_mints, &pool, &mut solnode);
    let lp_collective = User::new(&params.pool_balances, &stable_mints, &pool, &mut solnode);
    lp_collective.stable_approve(&params.pool_balances, &mut solnode);
    let defi_ix = DeFiInstruction::<TOKEN_COUNT>::Add {
        input_amounts: params.pool_balances,
        minimum_mint_amount: 0 as AmountT,
    };
    pool.execute_defi_instruction(defi_ix, &lp_collective.stables, Some(&lp_collective.lp), &mut solnode)
        .unwrap();

    (solnode, pool, user, lp_collective)
}

#[cfg(test)]
mod test {
    use {
        super::*,
        // assert_matches::*,
        solana_program::instruction::{AccountMeta, Instruction},
        solana_program_test::*,
        solana_sdk::{signature::Signer, transaction::Transaction},
    };

    #[test]
    fn test_pool_init() {
        let mut solnode = SolanaNode::new();

        let params = default_params();
        let stable_mints: [_; TOKEN_COUNT] =
            create_array(|i| MintAccount::new(params.stable_decimals[i], &mut solnode));

        let pool = DeployedPool::new(
            params.lp_decimals,
            &stable_mints,
            params.amp_factor,
            params.lp_fee,
            params.governance_fee,
            &mut solnode,
        )
        .unwrap();

        assert_eq!(pool.balances(&mut solnode), [0; TOKEN_COUNT]);
    }

    #[test]
    fn test_pool_add() {
        let params = default_params();
        let (mut solnode, pool, user, lp_collective) = setup_standard_testcase(&params);

        let input_amounts: [_; TOKEN_COUNT] = create_array(|i| (i + 1) as u64 * params.user_funds[i] / 10);
        user.stable_approve(&input_amounts, &mut solnode);
        let defi_ix = DeFiInstruction::Add {
            input_amounts,
            minimum_mint_amount: 0 as AmountT,
        };
        pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
            .unwrap();

        println!("pool balances: {:?}", pool.balances(&mut solnode));
        println!("lp_collective lp balance: {}", lp_collective.lp.balance(&mut solnode));
        println!(
            "lp_collective stable balance: {:?}",
            lp_collective.stable_balances(&mut solnode)
        );
        println!("user lp balance: {}", user.lp.balance(&mut solnode));
        println!("user stable balance: {:?}", user.stable_balances(&mut solnode));
    }

    #[test]
    fn test_pool_swap_exact_input() {
        let params = default_params();
        let (mut solnode, pool, user, _) = setup_standard_testcase(&params);
        let exact_input_amounts = create_array(|i| i as u64 * params.user_funds[i] / 10);

        user.stable_approve(&exact_input_amounts, &mut solnode);
        let defi_ix = DeFiInstruction::SwapExactInput {
            exact_input_amounts,
            output_token_index: 0,
            minimum_output_amount: 0 as AmountT,
        };

        let lp_supply_before = pool.lp_total_supply(&mut solnode);
        let depth_before = pool.state(&mut solnode).previous_depth;
        println!("> user balance before: {:?}", user.stable_balances(&mut solnode));
        pool.execute_defi_instruction(defi_ix, &user.stables, None, &mut solnode)
            .unwrap();

        let depth_after = pool.state(&mut solnode).previous_depth;
        let lp_supply_after = pool.lp_total_supply(&mut solnode);
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

        println!(">  user balance after: {:?}", user.stable_balances(&mut solnode));
    }

    #[test]
    fn test_pool_swap_exact_output() {
        let params = default_params();
        let (mut solnode, pool, user, _) = setup_standard_testcase(&params);
        let mut exact_output_amounts = create_array(|i| i as u64 * params.user_funds[i] / 100);
        exact_output_amounts[0] = 0;

        let approve_amounts = create_array(|i| if i == 0 { u64::MAX } else { 0 });

        user.stable_approve(&approve_amounts, &mut solnode);
        let defi_ix = DeFiInstruction::SwapExactOutput {
            maximum_input_amount: u64::MAX as AmountT,
            input_token_index: 0,
            exact_output_amounts,
        };

        let lp_supply_before = pool.lp_total_supply(&mut solnode);
        let depth_before = pool.state(&mut solnode).previous_depth;
        println!("> user balance before: {:?}", user.stable_balances(&mut solnode));
        pool.execute_defi_instruction(defi_ix, &user.stables, None, &mut solnode)
            .unwrap();

        let depth_after = pool.state(&mut solnode).previous_depth;
        let lp_supply_after = pool.lp_total_supply(&mut solnode);
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

        println!(">  user balance after: {:?}", user.stable_balances(&mut solnode));
    }

    #[test]
    fn test_pool_remove_uniform() {
        let mut params = default_params();
        params.user_funds = [0; TOKEN_COUNT];
        let (mut solnode, pool, _, lp_collective) = setup_standard_testcase(&params);

        let lp_total_supply = lp_collective.lp.balance(&mut solnode);
        let original_depth = (pool.state(&mut solnode)).previous_depth;
        let original_balances = pool.balances(&mut solnode);

        {
            println!("> removeUniform(one quarter of lp supply)");
            lp_collective.lp.approve(lp_total_supply / 4, &mut solnode);
            let defi_ix = DeFiInstruction::RemoveUniform {
                exact_burn_amount: lp_total_supply / 4,
                minimum_output_amounts: create_array(|i| params.pool_balances[i] / 4),
            };
            pool.execute_defi_instruction(defi_ix, &lp_collective.stables, Some(&lp_collective.lp), &mut solnode)
                .unwrap();

            assert_eq!(
                lp_collective.stable_balances(&mut solnode),
                create_array(|i| params.pool_balances[i] / 4)
            );
            assert_eq!(lp_collective.lp.balance(&mut solnode), (lp_total_supply / 4) * 3);
            assert_eq!(
                pool.balances(&mut solnode),
                create_array(|i| (original_balances[i] / 4) * 3)
            );
            assert_eq!((pool.state(&mut solnode)).previous_depth, (original_depth / 4) * 3);
        }

        {
            println!("> setting pool to paused (subsequent removeUniform has to work regardless!)");
            let gov_ix = GovernanceInstruction::SetPaused { paused: true };
            pool.execute_governance_instruction(gov_ix, None, &mut solnode).unwrap();

            assert!(pool.state(&mut solnode).is_paused);
        }

        {
            println!("> removeUniform(remaining three quarters of lp supply)");
            lp_collective.lp.approve((lp_total_supply / 4) * 3, &mut solnode);
            let defi_ix = DeFiInstruction::RemoveUniform {
                exact_burn_amount: (lp_total_supply / 4) * 3,
                minimum_output_amounts: create_array(|i| (params.pool_balances[i] / 4) * 3),
            };
            pool.execute_defi_instruction(defi_ix, &lp_collective.stables, Some(&lp_collective.lp), &mut solnode)
                .unwrap();

            assert_eq!(lp_collective.stable_balances(&mut solnode), original_balances);
            assert_eq!(lp_collective.lp.balance(&mut solnode), 0);
            assert_eq!(pool.balances(&mut solnode), [0; TOKEN_COUNT]);
            assert_eq!(pool.state(&mut solnode).previous_depth, 0u128);
        }
    }

    #[test]
    fn test_expensive_add() {
        let scale_factor = (10 as AmountT).pow(9);
        let initial_balances: [AmountT; TOKEN_COUNT] =
            [5_590_413, 6_341_331, 4_947_048, 3_226_825, 2_560_56724, 3_339_50641];

        let initial_balances: [_; TOKEN_COUNT] = create_array(|i| initial_balances[i] * scale_factor);

        let user_add: [AmountT; TOKEN_COUNT] = [
            10_000_000,
            9_000_000,
            11_000_000,
            12_000_000,
            13_000_00000,
            12_000_00000,
        ];

        let user_add: [_; TOKEN_COUNT] = create_array(|i| user_add[i] * scale_factor);

        let params = Parameters {
            amp_factor: DecT::new(1000, 0).unwrap(),
            lp_fee: DecT::new(3, 6).unwrap(),
            governance_fee: DecT::new(1, 6).unwrap(),
            lp_decimals: 6,
            stable_decimals: create_array(|i| if i < 4 { 6 } else { 8 }),
            pool_balances: create_array(|i| initial_balances[i]),
            user_funds: create_array(|i| user_add[i]),
        };

        let (mut solnode, pool, user, _) = setup_standard_testcase(&params);

        user.stable_approve(&params.user_funds, &mut solnode);
        let defi_ix = DeFiInstruction::Add {
            input_amounts: params.user_funds,
            minimum_mint_amount: 0 as AmountT,
        };
        println!("> user balance before: {:?}", user.stable_balances(&mut solnode));
        pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
            .unwrap();
        println!(">       user lp after: {:?}", user.lp.balance(&mut solnode));
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
    // // about 264_000 compute budget used for the remove
    // async fn test_expensive_remove() {
    //     let initial_balances: [AmountT; TOKEN_COUNT] = [
    //         195_269_254_200,
    //         68_344_238_970,
    //         165_978_866_070,
    //         11_933_121_090,
    //         11_933_121_090,
    //         195_269_254_200
    //     ];
    //
    //     let user_add: [AmountT; TOKEN_COUNT] = [
    //         10_000_000,
    //         9_000_000,
    //         11_000_000,
    //         12_000_000,
    //         13_000_00000,
    //         12_000_00000,
    //     ];

    //     let exact_output_amounts: [AmountT; TOKEN_COUNT] =     [
    //         4_271_514_975,
    //         745_820_075,
    //         10_373_679_225,
    //         10_373_679_225,
    //         4_271_514_975,
    //         12_204_328_500
    //     ];

    //     let maximum_burn_amount = u64::MAX; //12_204_328_500;

    //     let params = Parameters {
    //         amp_factor: DecT::new(1000, 0).unwrap(),
    //         lp_fee: DecT::new(3, 6).unwrap(),
    //         governance_fee: DecT::new(1, 6).unwrap(),
    //         lp_decimals: 6,
    //         stable_decimals: create_array(|i| if i < 4 { 6 } else { 8 }),
    //         pool_balances: create_array(|i| initial_balances[i]),
    //         user_funds: create_array(|i| user_add[i]),
    //     };

    //     let (mut solnode, pool, _, lp_collective) = setup_standard_testcase(&params).await;

    //     lp_collective.lp.approve(maximum_burn_amount, &mut solnode);

    //     let defi_ix = DeFiInstruction::RemoveExactOutput {
    //         maximum_burn_amount,
    //         exact_output_amounts,
    //     };

    //     pool.execute_defi_instruction(defi_ix,  &lp_collective.stables, Some(&lp_collective.lp), &mut solnode)
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

    /* Governance Ix tests */
    #[test]
    fn test_prepare_fee_change() {
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

        let (mut solnode, pool, ..) = setup_standard_testcase(&params);

        let new_lp_fee = DecT::new(6, 6).unwrap();
        let new_governance_fee = DecT::new(2, 6).unwrap();
        let gov_ix = GovernanceInstruction::PrepareFeeChange {
            lp_fee: new_lp_fee,
            governance_fee: new_governance_fee,
        };
        pool.execute_governance_instruction(gov_ix, None, &mut solnode).unwrap();

        let updated_state = pool.state(&mut solnode);
        assert_eq!(updated_state.prepared_lp_fee.get(), new_lp_fee);
        assert_eq!(updated_state.prepared_governance_fee.get(), new_governance_fee);
    }

    #[test]
    fn test_prepare_governance_transition() {
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

        let (mut solnode, pool, ..) = setup_standard_testcase(&params);

        let new_gov_key = Keypair::new();
        let gov_ix = GovernanceInstruction::PrepareGovernanceTransition {
            upcoming_governance_key: new_gov_key.pubkey(),
        };
        pool.execute_governance_instruction(gov_ix, None, &mut solnode).unwrap();

        let updated_state = pool.state(&mut solnode);
        assert_eq!(updated_state.prepared_governance_key, new_gov_key.pubkey());
    }

    #[test]
    fn test_change_governance_fee_account() {
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

        let (mut solnode, pool, ..) = setup_standard_testcase(&params);

        let new_gov_fee_token_account = pool.create_lp_account(&mut solnode);

        let gov_ix = GovernanceInstruction::ChangeGovernanceFeeAccount {
            governance_fee_key: *new_gov_fee_token_account.pubkey(),
        };
        pool.execute_governance_instruction(gov_ix, Some(new_gov_fee_token_account.pubkey()), &mut solnode)
            .unwrap();

        let updated_state = pool.state(&mut solnode);
        assert_eq!(updated_state.governance_fee_key, *new_gov_fee_token_account.pubkey());
    }

    #[test]
    fn test_adjust_amp_factor() {
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

        let (mut solnode, pool, ..) = setup_standard_testcase(&params);

        let curr_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let target_ts = curr_ts + (10 * pool::amp_factor::MIN_ADJUSTMENT_WINDOW);
        let target_value = DecT::new(1010, 0).unwrap();
        let gov_ix = GovernanceInstruction::AdjustAmpFactor {
            target_ts,
            target_value,
        };
        pool.execute_governance_instruction(gov_ix, None, &mut solnode).unwrap();

        let updated_state = pool.state(&mut solnode);
        assert_eq!(updated_state.amp_factor.get(target_ts + 100), target_value);
    }

    #[test]
    fn test_pause() {
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

        let (mut solnode, pool, user, _) = setup_standard_testcase(&params);

        let gov_ix = GovernanceInstruction::SetPaused { paused: true };
        pool.execute_governance_instruction(gov_ix, None, &mut solnode).unwrap();
        assert!(pool.state(&mut solnode).is_paused);

        user.stable_approve(&params.user_funds, &mut solnode);
        let defi_ix = DeFiInstruction::Add {
            input_amounts: params.user_funds,
            minimum_mint_amount: 0 as AmountT,
        };
        //TODO: check this. after changing pool, this shouldn't be passing since i'm not throwing an error anymore?
        // println!("\n\nSHOULD FAIL THIS EXECUTE_DEFI_IX\n\n");
        pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
            .expect_err("Should not be able to execute defi_ix when paused");

        let gov_ix = GovernanceInstruction::SetPaused { paused: false };
        pool.execute_governance_instruction(gov_ix, None, &mut solnode).unwrap();

        assert!(!pool.state(&mut solnode).is_paused);

        user.stable_approve(&params.user_funds, &mut solnode);
        let defi_ix = DeFiInstruction::Add {
            input_amounts: params.user_funds,
            minimum_mint_amount: 0 as AmountT,
        };
        pool.execute_defi_instruction(defi_ix, &user.stables, Some(&user.lp), &mut solnode)
            .unwrap();
    }
}
