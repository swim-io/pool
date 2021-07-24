use borsh::{BorshDeserialize, BorshSerialize};
use muldiv::MulDiv;

use crate::error::PoolError;

pub type TimestampT = u64;
pub type ValueT = u32;

//we don't want to use a minimum amp factor that's too low because it would make adjustment steps
// too discontinuous (in the most extreme case, going from 1 to 2 would constitute a doubling)
pub const MIN_AMP_VALUE: ValueT = 10;
pub const MAX_AMP_VALUE: ValueT = (10 as ValueT).pow(6);

pub const MIN_ADJUSTMENT_WINDOW: TimestampT = 60*60*24;
pub const MAX_RELATIVE_ADJUSTMENT: ValueT = 10;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct AmpFactor {
    //invariants:
    // inital_ts <= target_ts
    // MIN_AMP_VALUE <= initial_value <= MAX_AMP_VALUE
    // MIN_AMP_VALUE <= target_value <= MAX_AMP_VALUE
    initial_value: ValueT,
    initial_ts: TimestampT,
    target_value: ValueT,
    target_ts: TimestampT,
}

impl Default for AmpFactor {
    fn default() -> Self {
        AmpFactor::new(MIN_AMP_VALUE).unwrap()
    }
}

impl AmpFactor {
    pub fn new(amp_factor: ValueT) -> Result<AmpFactor, PoolError> {
        if !(MIN_AMP_VALUE..MAX_AMP_VALUE).contains(&amp_factor) {
            Err(PoolError::InvalidAmpFactorValue)
        }
        else {
            Ok(AmpFactor{
                initial_value: MIN_AMP_VALUE,
                initial_ts: 0,
                target_value: amp_factor,
                target_ts: 0,
            })
        }
    }

    pub fn get(&self, current_ts: TimestampT) -> ValueT {
        if current_ts >= self.target_ts { //check if we are inside an adjustment window
            //not in an adjustment window
            self.target_value
        }
        else {
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
            // the maximum change to a factor of 10
            
            let value_diff = self.target_value as i64 - self.initial_value as i64;
            let time_since_initial = (current_ts - self.initial_ts) as i64;
            let total_adjustment_time = (self.target_ts - self.initial_ts) as i64;

            let delta = value_diff.mul_div_round(time_since_initial,total_adjustment_time).unwrap();

            (self.initial_value as i64 + delta) as _
        }
    }

    pub fn set_target(&mut self, current_ts: TimestampT, target_value: ValueT, target_ts: TimestampT) -> Result<(), PoolError> {
        if !(MIN_AMP_VALUE..MAX_AMP_VALUE).contains(&target_value) {
            return Err(PoolError::InvalidAmpFactorValue);
        }

        if target_ts < current_ts + MIN_ADJUSTMENT_WINDOW {
            return Err(PoolError::InvalidAmpFactorTimestamp);
        }
        
        let initial_value = self.get(current_ts);
        if (initial_value < target_value && initial_value*MAX_RELATIVE_ADJUSTMENT < target_value) ||
           (initial_value > target_value && initial_value > target_value*MAX_RELATIVE_ADJUSTMENT) {
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
}
