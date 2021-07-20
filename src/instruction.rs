use borsh::{BorshDeserialize, BorshSerialize};

use crate::pool_fees::PoolFees;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PoolInstruction<const TOKEN_COUNT: usize> {
    Init {
        nonce: u8,
        amp_factor: u64,
        pool_fees: PoolFees,
    },
    Add {

    },
    Remove {

    },
    Swap {

    },
    PrepareFeeChange {

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

    },
    HaltAmpFactorAdjustment {
        
    },
    SetPaused {
        paused: bool,
    },
}
