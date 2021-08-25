use borsh::{BorshDeserialize, BorshSerialize};

use crate::pool_fee::FeeRepr;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PoolInstruction<const TOKEN_COUNT: usize> {
    /// Initializes a new pool 
    ///
    /// Accounts expected by this instruction:
    ///     0. `[writable]` The pool state account to initalize
    ///     1. `[writable]` LP Token Mint. Must be empty, owned by authority 
    ///             authority isn't passed in but programatically derived
    ///     2. ..2 + TOKEN_COUNT  `[]` Token mint accounts
    ///     3. ..2 + (2 * TOKEN_COUNT) `[]` Token accounts. Must be empty
    ///     4. ..3 + (2 * TOKEN_COUNT) `[]` Governance account
    ///     5. ..4 + (2 * TOKEN_COUNT) `[]` Governance Fee account. 
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
