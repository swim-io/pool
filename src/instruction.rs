use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    pubkey::Pubkey,
    clock::UnixTimestamp,
};
use crate::decimal::DecimalU64;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PoolInstruction<const TOKEN_COUNT: usize> {
    Init {
        nonce: u8,
        amp_factor: DecimalU64,
        lp_fee: DecimalU64,
        governance_fee: DecimalU64,
    },
    Add {
        input_amounts: [u64; TOKEN_COUNT],
        minimum_lp_amount: u64,
    },
    RemoveOneExact {
        exact_burn_amount: u64,
        output_token_index: u8,
        minimum_output_amount: u64,
    },
    RemoveAllExact {
        exact_burn_amount: u64,
        minimum_output_amounts: [u64; TOKEN_COUNT],
    },
    RemoveBounded {
        maximum_burn_amount: u64,
        output_amounts: [u64; TOKEN_COUNT],
    },
    Swap {
        input_amounts: [u64; TOKEN_COUNT],
        output_token_index: u8,
        minimum_output_amount: u64,
    },
    PrepareFeeChange {
        lp_fee: DecimalU64,
        governance_fee: DecimalU64,
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
        target_value: DecimalU64,
    },
    HaltAmpFactorAdjustment {},
    SetPaused {
        paused: bool,
    },
}
