//naming: pool_fee to distinguish from other fees (such as Solana's fee sysvar)

use borsh::{BorshDeserialize, BorshSerialize};
use muldiv::MulDiv;
use crate::error::PoolError;

//fees are stored with a resolution of one hundredth of a basis point, i.e. 10^-6
const DECIMALS: u8 = 6;
//10^(DECIMALS+2) has to fit into ValueT
pub type ValueT = u32;
const DECIMALS_DENOMINATOR: ValueT = (10 as ValueT).pow(DECIMALS as u32);

pub enum FeeType {
    Trade = 0, //= 0 to indicated that the integer values serve as indices into an array
    Governance,
}

//used to abstract away the decimals that Fees uses to store rates internally
pub struct FeeRepr {
    pub value: ValueT,
    pub decimals: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
//TODO replace hardcoded 2 with std::mem::variant_count::<FeeType>() once it's stabilized
pub struct PoolFees([ValueT; 2]);

impl PoolFees {
    pub fn new() -> Self {
        Self([0; 2])
    }

    pub fn set_fee(& mut self, fee_type: FeeType, fee_rate: FeeRepr) -> Result<(), PoolError> {
        self.0[fee_type as usize] = if fee_rate.value > 0 {
            if fee_rate.value / (10 as ValueT).checked_pow(fee_rate.decimals as u32).ok_or(PoolError::InvalidFeeInput)? > 0 {
                //fee has to be less than 100 %
                return Err(PoolError::InvalidFeeInput);
            }
    
            if fee_rate.decimals > DECIMALS {
                //if the passed in decimals are larger than what we can represent internally
                // then those digits better be zero
                let denominator = (10 as ValueT).pow((fee_rate.decimals - DECIMALS) as u32);
                if fee_rate.value % denominator != 0 {
                    return Err(PoolError::InvalidFeeInput);
                }
                fee_rate.value / denominator
            }
            else {
                fee_rate.value * (10 as ValueT).pow((DECIMALS - fee_rate.decimals) as u32)
            }
        }
        else {
            0
        };
        Ok(())
    }

    pub fn get_fee(&self, fee_type: FeeType) -> FeeRepr {
        FeeRepr{value: self.0[fee_type as usize], decimals: DECIMALS}
    }

    pub fn apply_fee(&self, fee_type: FeeType, amount: u64) -> u64 {
        amount.mul_div_round(self.0[fee_type as usize] as u64, DECIMALS_DENOMINATOR as u64).unwrap()
    }
}
