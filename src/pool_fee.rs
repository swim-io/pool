//naming: pool_fee to distinguish from other fees (such as Solana's fee sysvar)

use borsh::{BorshDeserialize, BorshSerialize};
use muldiv::MulDiv;
use crate::error::PoolError;

//fees are stored with a resolution of one hundredth of a basis point, i.e. 10^-6
const DECIMALS: u8 = 6;
//10^(DECIMALS+2) has to fit into ValueT
pub type ValueT = u32;
const DECIMALS_DENOMINATOR: ValueT = (10 as ValueT).pow(DECIMALS as u32);

#[derive(BorshSerialize, BorshDeserialize, Debug, Default)]
//used to abstract away the decimals that Fees uses to store rates internally
pub struct FeeRepr {
    pub value: ValueT,
    pub decimals: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Default)]
pub struct PoolFee(ValueT);

impl PartialEq for PoolFee {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PoolFee {
    pub fn new(fee_repr: FeeRepr) -> Result<Self, PoolError> {
        let mut ret = Self::default();
        ret.set(fee_repr)?;
        Ok(ret)
    }

    pub fn set(& mut self, fee_repr: FeeRepr) -> Result<(), PoolError> {
        self.0 = if fee_repr.value > 0 {
            if fee_repr.value / (10 as ValueT).checked_pow(fee_repr.decimals as u32).ok_or(PoolError::InvalidFeeInput)? > 0 {
                //fee has to be less than 100 %
                return Err(PoolError::InvalidFeeInput);
            }
    
            if fee_repr.decimals > DECIMALS {
                //if the passed in decimals are larger than what we can represent internally
                // then those digits better be zero
                let denominator = (10 as ValueT).pow((fee_repr.decimals - DECIMALS) as u32);
                if fee_repr.value % denominator != 0 {
                    return Err(PoolError::InvalidFeeInput);
                }
                fee_repr.value / denominator
            }
            else {
                fee_repr.value * (10 as ValueT).pow((DECIMALS - fee_repr.decimals) as u32)
            }
        }
        else {
            0
        };
        Ok(())
    }

    pub fn get(&self) -> FeeRepr {
        FeeRepr{value: self.0, decimals: DECIMALS}
    }

    pub fn apply(&self, amount: u64) -> u64 {
        amount.mul_div_round(self.0 as u64, DECIMALS_DENOMINATOR as u64).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pool_fee() {
        // 50% fee
        let _fee = PoolFee::new(FeeRepr{value: 500000, decimals:6}).unwrap();
    }

    #[test]
    #[should_panic]
    fn invalid_set_pool_fee() {
        // 5% fee
        let mut fee = PoolFee::new(FeeRepr{value: 500000, decimals:7}).unwrap();
        fee.set(FeeRepr{value:1000000, decimals:6}).unwrap();
    }

    #[test]
    fn apply_fee() {
        // 0.5% fee
        let fee = PoolFee::new(FeeRepr{value: 500000, decimals:8}).unwrap();
        let amount: u64 = 1000000;
        assert_eq!(5000, fee.apply(amount));
    }
    
    #[test]
    #[should_panic]
    fn overflow_value() {
        let _fee = PoolFee::new(FeeRepr{value: 123456789, decimals:11}).unwrap();
    }
}