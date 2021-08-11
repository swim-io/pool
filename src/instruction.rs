use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

use crate::pool_fee::FeeRepr;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PoolInstruction<const TOKEN_COUNT: usize> {
    Init {
        nonce: u8,
        amp_factor: u32,
        lp_fee: FeeRepr,
        governance_fee: FeeRepr,
    },
    Add {
        deposit_amounts: [u64; TOKEN_COUNT],
        minimum_mint_amount: u64,
    },
    Remove {},
    Swap {},
    PrepareFeeChange {
        lp_fee: FeeRepr,
        governance_fee: FeeRepr,
    },
    EnactFeeChange {},
    PrepareGovernanceTransition {
        upcoming_governance_key: Pubkey,
    },
    ChangeGovernanceFeeAccounts {},
    AdjustAmpFactor {
        target_ts: u64,
        target_value: u32,
    },
    HaltAmpFactorAdjustment {},
    SetPaused {
        paused: bool,
    },
}
