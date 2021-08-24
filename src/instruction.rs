use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    pubkey::Pubkey,
    clock::UnixTimestamp,
};
use crate::decimal::DecimalU64;

type AmountT = u64;
type DecT = DecimalU64;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PoolInstruction<const TOKEN_COUNT: usize> {
    Init {
        nonce: u8,
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
    },
    DeFiInstruction(DeFiInstruction::<TOKEN_COUNT>),
    GovernanceInstruction(GovernanceInstruction::<TOKEN_COUNT>)
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum DeFiInstruction<const TOKEN_COUNT: usize> {
    Add {
        input_amounts: [AmountT; TOKEN_COUNT],
        minimum_mint_amount: AmountT,
    },
    SwapExactInput {
        exact_input_amounts: [AmountT; TOKEN_COUNT],
        output_token_index: u8,
        minimum_output_amount: AmountT,
    },
    SwapExactOutput {
        maximum_input_amount: AmountT,
        input_token_index: u8,
        exact_output_amounts: [AmountT; TOKEN_COUNT],
    },
    RemoveUniform {
        exact_burn_amount: AmountT,
        minimum_output_amounts: [AmountT; TOKEN_COUNT],
    },
    RemoveExactBurn {
        exact_burn_amount: AmountT,
        output_token_index: u8,
        minimum_output_amount: AmountT,
    },
    RemoveExactOutput {
        maximum_burn_amount: AmountT,
        exact_output_amounts: [AmountT; TOKEN_COUNT],
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum GovernanceInstruction<const TOKEN_COUNT: usize> {
    PrepareFeeChange {
        lp_fee: DecT,
        governance_fee: DecT,
    },
    EnactFeeChange {},
    PrepareGovernanceTransition {
        upcoming_governance_key: Pubkey,
    },
    EnactGovernanceTransition {},
    ChangeGovernanceFeeAccount {
        governance_fee_key: Pubkey,
    },
    AdjustAmpFactor {
        target_ts: UnixTimestamp,
        target_value: DecT,
    },
    HaltAmpFactorAdjustment {},
    SetPaused {
        paused: bool,
    }
}