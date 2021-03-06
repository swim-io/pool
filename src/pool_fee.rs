//naming: pool_fee to distinguish from other fees (such as Solana's fee sysvar)

use crate::{decimal::DecimalU64, error::PoolError};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

//fees are stored with a resolution of one hundredth of a basis point, i.e. 10^-6
const DECIMALS: u8 = 6;
//10^(DECIMALS+2) has to fit into ValueT
pub type ValueT = u32;
type DecT = DecimalU64;

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Debug, Default)]
pub struct PoolFee(ValueT);

impl PoolFee {
    pub fn new(fee: DecT) -> Result<Self, PoolError> {
        let mut ret = Self::default();
        ret.set(fee)?;
        Ok(ret)
    }

    pub fn set(&mut self, fee: DecT) -> Result<(), PoolError> {
        let floored_fee = fee.floor(DECIMALS);
        if fee >= DecT::from(1) || floored_fee != fee {
            //fee has to be less than 100 % and decimals have to fit
            return Err(PoolError::InvalidFeeInput);
        }

        self.0 = (floored_fee.get_raw() * 10u64.pow((DECIMALS - floored_fee.get_decimals()) as u32)) as u32;

        Ok(())
    }

    pub fn get(&self) -> DecT {
        DecT::new(self.0 as u64, DECIMALS).unwrap()
    }
}

#[cfg(all(test, not(feature = "test-bpf")))]
mod tests {
    use super::*;

    fn new_u64(value: u64, decimals: u8) -> DecT {
        DecT::new(value, decimals).unwrap()
    }

    #[test]
    fn new_pool_fee() {
        // 50% fee
        let _fee = PoolFee::new(new_u64(500000, 6)).unwrap();
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
        let fee_value = new_u64(500000, 8);
        let fee = PoolFee::new(fee_value).unwrap();
        assert_eq!(fee.get(), fee_value.floor(DECIMALS));
    }

    #[test]
    #[should_panic]
    fn overflow_value() {
        let _fee = PoolFee::new(new_u64(123456789, 11)).unwrap();
    }
}
