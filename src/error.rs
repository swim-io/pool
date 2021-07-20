use thiserror::Error;
use solana_program::program_error::ProgramError;
use spl_token::error::TokenError;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

//OFFSET is used to deal with technical debt imposed on us by SPL::ProgramError
// ProgramError uses the Custom variant to store other errors (such as spl token TokenError but also our PoolError)
// so to distinguish TokenErrors from PoolErrors, we're offsetting PoolErrors by 100 while TokenErrors start at 0
const OFFSET: isize = 100;

#[derive(Error, Debug, FromPrimitive)]
pub enum PoolError {
	#[error("Invalid amp factor input")]
	InvalidAmpInput = OFFSET,
	#[error("Invalid fee input")]
	InvalidFeeInput,
}

impl From<PoolError> for ProgramError {
	fn from(e: PoolError) -> Self {
		ProgramError::Custom(e as u32)
	}
}

pub fn to_error_msg(error: &ProgramError) -> String {
	match error {
		ProgramError::Custom(ec)  if *ec < OFFSET as u32 => TokenError::from_u32(*ec).unwrap().to_string(),
		ProgramError::Custom(ec) => PoolError::from_u32(*ec).unwrap().to_string(),
		e => e.to_string(),
	}
}
