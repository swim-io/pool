use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use solana_program::program_error::ProgramError;
use spl_token::error::TokenError;
use thiserror::Error;

//OFFSET is used to deal with technical debt imposed on us by SPL::ProgramError
// ProgramError uses the Custom variant to store other errors (such as spl token TokenError but also our PoolError)
// so to distinguish TokenErrors from PoolErrors, we're offsetting PoolErrors by 100 while TokenErrors start at 0
const OFFSET: isize = 100;

#[derive(Error, Debug, FromPrimitive)]
pub enum PoolError {
    #[error("Specified amp factor is out of bounds")]
    InvalidAmpFactorValue = OFFSET,
    #[error("Amp factor adjustment window is too short")]
    InvalidAmpFactorTimestamp,
    #[error("Given fee is invalid")]
    InvalidFeeInput,
    #[error("Can't pass the same account twice here")]
    DuplicateAccount,
    #[error("Lp token mint has a positive balance")]
    MintHasBalance,

    // 105
    #[error("Pool does not have mint authority of lp token mint")]
    InvalidMintAuthority,
    #[error("Lp token mint's freeze authority is set")]
    MintHasFreezeAuthority,
    #[error("Token account has a positive balance")]
    TokenAccountHasBalance,
    #[error("Token account's delegate is set")]
    TokenAccountHasDelegate,
    #[error("Token account's close authority is set")]
    TokenAccountHasCloseAuthority,

    // 110
    #[error("Invalid governance account")]
    InvalidGovernanceAccount,
    #[error("Invalid governance fee account")]
    InvalidGovernanceFeeAccount,
    #[error("Invalid pool authority account")]
    InvalidPoolAuthorityAccount,
    #[error("Invalid mint account")]
    InvalidMintAccount,
    #[error("Not enough time has passed since prepare instruction")]
    InsufficientDelay,

    //115
    #[error("Nothing to enact")]
    InvalidEnact,
    #[error("Pool is paused")]
    PoolIsPaused,
    #[error("Expected a token account that belongs to the pool")]
    PoolTokenAccountExpected,
    #[error("The instruction could not be completed within the specified limits")]
    OutsideSpecifiedLimits,
    #[error("Initial add to pool must include all tokens")]
    AddRequiresAllTokens,

    //120
    #[error("Remove can't completed due to the approximative nature of fee math implementation")]
    ImpossibleRemove,
    #[error("The maximum difference in decimals between tokens in the pool has been exceeded")]
    MaxDecimalDifferenceExceeded,
}

impl From<PoolError> for ProgramError {
    fn from(e: PoolError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

pub fn to_error_msg(error: &ProgramError) -> String {
    match error {
        ProgramError::Custom(ec) if *ec < OFFSET as u32 => TokenError::from_u32(*ec).unwrap().to_string(),
        ProgramError::Custom(ec) => PoolError::from_u32(*ec).unwrap().to_string(),
        e => e.to_string(),
    }
}
