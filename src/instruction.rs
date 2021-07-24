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
