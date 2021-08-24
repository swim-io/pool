//naming: pool_fee to distinguish from other fees (such as Solana's fee sysvar)

use crate::{
    error::PoolError,
    decimal::DecimalU64,
};
use borsh::{BorshDeserialize, BorshSerialize};

//fees are stored with a resolution of one hundredth of a basis point, i.e. 10^-6
const DECIMALS: u8 = 6;
//10^(DECIMALS+2) has to fit into ValueT
pub type ValueT = u32;

#[derive(BorshSerialize, BorshDeserialize, Debug, Default)]
pub struct PoolFee(ValueT);

impl PoolFee {
    pub fn new(fee: DecimalU64) -> Result<Self, PoolError> {
        let mut ret = Self::default();
        ret.set(fee)?;
        Ok(ret)
    }

    pub fn set(&mut self, fee: DecimalU64) -> Result<(), PoolError> {
        let floored_fee = fee.floor(DECIMALS);
        if fee >= DecimalU64::from(1) || floored_fee != fee {
            //fee has to be less than 100 % and decimals have to fit
            return Err(PoolError::InvalidFeeInput);
        }
        
        self.0 = floored_fee.get_raw() as u32;

        Ok(())
    }

    pub fn get(&self) -> DecimalU64 {
        DecimalU64::new(self.0 as u64, DECIMALS).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_u64(value: u64, decimals: u8) -> DecimalU64 {
        DecimalU64::new(value, decimals).unwrap()
    }

    #[test]
    fn new_pool_fee() {
        // 50% fee
        let _fee = PoolFee::new( new_u64(500000, 6)).unwrap();
    }

    #[test]
    #[should_panic]
    fn invalid_set_pool_fee() {
        let mut fee = PoolFee::default();
        fee.set(new_u64(1000000, 6)).unwrap();
    }

    #[test]
    fn get_fee() {
        // 0.5% fee
        let fee_value = new_u64( 500000, 8);
        let fee = PoolFee::new(fee_value).unwrap();
        assert_eq!(fee.get(), fee_value.floor(DECIMALS));
    }

    #[test]
    #[should_panic]
    fn overflow_value() {
        let _fee = PoolFee::new(new_u64( 123456789, 11)).unwrap();
    }
}
