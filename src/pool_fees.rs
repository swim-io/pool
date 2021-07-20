//naming: pool_fee to distinguish from other fees (such as Solana's fee sysvar)

use borsh::{BorshDeserialize, BorshSerialize};
use crate::error::PoolError;
//use std::mem::variant_count; //TODO not stabilized yet so hardcoding variant_count for now

pub enum Type {
    Trade = 0, //= 0 to indicated that the integer values serve as indices into an array
    Governance,
}

//used to abstract away the decimals that Fees uses to store rates internally
pub struct Repr {
    pub value: u32,
    pub decimals: u8,
}

//fees are stored with a resolution of one hundredth of a basis point, i.e. 10^-6
const DECIMALS: u8 = 6;
const DECIMALS_DENOMINATOR: u32 = 10u32.pow(DECIMALS as u32);

#[derive(BorshSerialize, BorshDeserialize, Debug)]
//TODO replace hardcoded 2 with variant_count::<FeeType>() once it's stabilized
pub struct PoolFees([u32; 2]);

impl PoolFees {
    pub fn new() -> Self {
        Self([0u32; 2])
    }

    pub fn set_fee(& mut self, fee_type: Type, fee_rate: Repr) -> Result<(), PoolError> {
        self.0[fee_type as usize] = if fee_rate.value > 0 {
            if fee_rate.value / 10u32.pow(fee_rate.decimals as u32) > 0 {
                //fee has to be less than 100 %
                return Err(PoolError::InvalidFeeInput);
            }
    
            if fee_rate.decimals > DECIMALS {
                //if the passed in decimals are larger than what we can represent internally
                // then those digits better not matter (i.e. be zero)
                let denominator = 10u32.pow((fee_rate.decimals - DECIMALS) as u32);
                if fee_rate.value % denominator != 0 {
                    return Err(PoolError::InvalidFeeInput);
                }
                fee_rate.value / denominator
            }
            else {
                fee_rate.value * 10u32.pow((DECIMALS - fee_rate.decimals) as u32)
            }
        }
        else {
            0
        };
        Ok(())
    }

    pub fn get_fee(&self, fee_type: Type) -> Repr {
        Repr{value: self.0[fee_type as usize], decimals: DECIMALS}
    }

    pub fn apply_fee(&self, fee: Type, amount: u64) -> u64 {
        ((amount as u128 * self.0[fee as usize] as u128) / DECIMALS_DENOMINATOR as u128) as u64
    }
}
