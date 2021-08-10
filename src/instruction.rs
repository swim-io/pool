use borsh::{BorshDeserialize, BorshSerialize};

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

    },
    Remove {

    },
    Swap {

    },
    PrepareFeeChange {
        lp_fee: FeeRepr,
        governance_fee: FeeRepr,
    },
    EnactFeeChange {

    },
    PrepareGovernanceTransition {

    },
    EnactGovernanceTransition {

    },
    ChangeGovernanceFeeAccounts {

    },
    AdjustAmpFactor {
        target_ts: u64,
        target_value: u32,
    },
    HaltAmpFactorAdjustment {
        
    },
    SetPaused {
        paused: bool,
    },
}
