//this entire module needs refactoring using a viable fixed int implementation that properly handles decimals
// and provides functionality like mul_div

use borsh::{BorshDeserialize, BorshSerialize};
use muldiv::MulDiv;

use crate::error::PoolError;

pub type TimestampT = u64;
pub type PublicValueT = u32;

pub const MIN_AMP_VALUE: PublicValueT = 1;
pub const MAX_AMP_VALUE: PublicValueT = (10 as PublicValueT).pow(6);

pub const MIN_ADJUSTMENT_WINDOW: TimestampT = 60 * 60 * 24;
pub const MAX_RELATIVE_ADJUSTMENT: PublicValueT = 10;

type PrivateValueT = u64;
const DECIMALS: u8 = 6;
const DECIMALS_DENOMINATOR: PrivateValueT = (10 as PrivateValueT).pow(DECIMALS as u32);

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct AmpFactor {
    //invariants:
    // inital_ts <= target_ts
    // MIN_AMP_VALUE <= initial_value <= MAX_AMP_VALUE
    // MIN_AMP_VALUE <= target_value <= MAX_AMP_VALUE
    initial_value: PrivateValueT,
    initial_ts: TimestampT,
    target_value: PrivateValueT,
    target_ts: TimestampT,
}

impl Default for AmpFactor {
    fn default() -> Self {
        AmpFactor::new(MIN_AMP_VALUE).unwrap()
    }
}

impl AmpFactor {
    fn get(&self, current_ts: TimestampT) -> PrivateValueT {
        if current_ts >= self.target_ts {
            //check if we are inside an adjustment window
            //not in an adjustment window
            self.target_value
        } else {
            assert!(current_ts >= self.initial_ts);

            //we are within an adjustment window and hence need to interpolate the amp factor
            //
            //mathematically speaking we ought to use exponential interpolation
            // to see why, assume an amp factor adjustment from 1 to 4:
            // going from 1 to 2 constitutes a doubling, as does going from 2 to 4
            // hence we should use the first half of the alotted time to go from 1 to 2 and
            // the second half to go from 2 to 4
            //
            //ultimately however, it's only important that the adjustment happens gradually
            // to prevent exploitation (see: https://medium.com/@peter_4205/curve-vulnerability-report-a1d7630140ec)
            // and so for simplicity's sake we use linear interpolation and restrict
            // the maximum _relative_ change to a factor of 10 (i.e. amp_factor at most do
            // a 10x over a day (not +10, but potentially much more))

            let value_diff = self.target_value as i64 - self.initial_value as i64;
            let time_since_initial = (current_ts - self.initial_ts) as i64;
            let total_adjustment_time = (self.target_ts - self.initial_ts) as i64;

            let delta = value_diff
                .mul_div_round(time_since_initial, total_adjustment_time)
                .unwrap();

            (self.initial_value as i64 + delta) as _
        }
    }

    pub fn new(amp_factor: u32) -> Result<AmpFactor, PoolError> {
        if !(MIN_AMP_VALUE..=MAX_AMP_VALUE).contains(&amp_factor) {
            Err(PoolError::InvalidAmpFactorValue)
        } else {
            Ok(AmpFactor {
                initial_value: MIN_AMP_VALUE as PrivateValueT * DECIMALS_DENOMINATOR,
                initial_ts: 0,
                target_value: amp_factor as PrivateValueT * DECIMALS_DENOMINATOR,
                target_ts: 0,
            })
        }
    }

    pub fn set_target(
        &mut self,
        current_ts: TimestampT,
        target_value: PublicValueT,
        target_ts: TimestampT,
    ) -> Result<(), PoolError> {
        if !(MIN_AMP_VALUE..=MAX_AMP_VALUE).contains(&target_value) {
            return Err(PoolError::InvalidAmpFactorValue);
        }

        let target_value = (target_value as PrivateValueT) * DECIMALS_DENOMINATOR;

        if target_ts < current_ts + MIN_ADJUSTMENT_WINDOW {
            return Err(PoolError::InvalidAmpFactorTimestamp);
        }

        let initial_value = self.get(current_ts);
        if (initial_value < target_value && initial_value * (MAX_RELATIVE_ADJUSTMENT as PrivateValueT) < target_value)
            || (initial_value > target_value
                && initial_value > target_value * MAX_RELATIVE_ADJUSTMENT as PrivateValueT)
        {
            return Err(PoolError::InvalidAmpFactorValue);
        }

        self.initial_value = initial_value;
        self.initial_ts = current_ts;
        self.target_value = target_value;
        self.target_ts = target_ts;

        Ok(())
    }

    pub fn stop_adjustment(&mut self, current_ts: TimestampT) {
        self.target_value = self.get(current_ts);
        self.target_ts = current_ts;
    }

    pub fn apply(&self, current_ts: TimestampT, val: u64) -> u64 {
        val
            .mul_div_round(self.get(current_ts) as u64, DECIMALS_DENOMINATOR as u64)
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_amp_factor() {
        assert!(AmpFactor::new(0).is_err());
        assert!(AmpFactor::new(MIN_AMP_VALUE-1).is_err());
        assert!(AmpFactor::new(MAX_AMP_VALUE+1).is_err());

        assert!(AmpFactor::new(MIN_AMP_VALUE).is_ok());
        assert!(AmpFactor::new(MIN_AMP_VALUE+1).is_ok());
        assert!(AmpFactor::new((MIN_AMP_VALUE+MAX_AMP_VALUE)/2).is_ok());
        assert!(AmpFactor::new(MAX_AMP_VALUE-1).is_ok());
        assert!(AmpFactor::new(MAX_AMP_VALUE).is_ok());
    }

    #[test]
    fn valid_set_target() {
        let mut amp = AmpFactor::new(10000).unwrap();
        assert_eq!(amp.apply(1, 1), 10000);

        amp.set_target(20000, 20000, 106400).unwrap();

        assert_eq!(amp.apply(20000,1), 10000);
        assert_eq!(amp.apply(30000,1), 11157);
        assert_eq!(amp.apply(50000,1), 13472);
        assert_eq!(amp.apply(70000,1), 15787);
        assert_eq!(amp.apply(90000,1), 18102);
        assert_eq!(amp.apply(106400,1), 20000);
    }

    #[test]
    #[should_panic]
    fn invalid_set_target() {
        //Target value set to 20x initial value
        let mut amp = AmpFactor::new(1000).unwrap();
        amp.set_target(20000, 20000, 106400).unwrap();
    }

    #[test]
    #[should_panic]
    fn invalid_adjustment_window() {
        let mut amp = AmpFactor::new(10000).unwrap();
        amp.set_target(20000, 20000, 50000).unwrap();
    }
}
