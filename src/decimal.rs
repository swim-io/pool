use std::{
    cmp::{PartialEq, Eq, PartialOrd, Ord, Ordering},
    ops::{Add, AddAssign, Sub, SubAssign, Mul, MulAssign, Div, DivAssign},
    fmt,
    fmt::{Display, Formatter},
};
use borsh::{BorshDeserialize, BorshSerialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DecimalError {
    #[error("Maximum decimals exceeded")]
    MaxDecimalsExceeded,
}

//all math in here is implemented in such a way that all operations that *aren't*
// explicitly checked_* (i.e. all the inline ops like +,-,*,/,%, etc.) should never
// be able to fail and could hence be replaced by unsafe_* calls to reduce strain on compute budget
macro_rules! unsigned_decimal {(
    $name:ident,
    $value_type:ty,
    $bits:expr, //<$value_type>::BITS; is still unstable
    $max_decimals:expr, //floor(log_10(2^bits-1))
    // $overflow_policy:ty,
    // $rounding_policy:ty,
) => {
    #[derive(BorshSerialize, Debug, Clone, Copy)]
    pub struct $name {
        value: $value_type,
        decimals: u8
    }

    impl $name {
        pub const BITS: u32 = $bits;
        pub const MAX_DECIMALS: u8 = $max_decimals;

        pub fn new(value: $value_type, decimals: u8) -> Result<Self, DecimalError> {
            if decimals > Self::MAX_DECIMALS {
                return Err(DecimalError::MaxDecimalsExceeded)
            }
            Ok(Self{value, decimals})
        }

        pub fn get_raw(&self) -> $value_type {
            self.value
        }

        pub fn get_decimals(&self) -> u8 {
            self.decimals
        }

        pub fn trunc(&self) -> $value_type {
            self.value / Self::ten_to_the(self.decimals)
        }

        pub fn fract(&self) -> $value_type {
            self.decimals % Self::ten_to_the(self.decimals)
        }

        //reduce decimals as to eliminate all trailing zeros
        pub fn normalize(&mut self) {
            //binary search
            let mut decimals = self.decimals;
            while decimals != 0 {
                let dec_half = (decimals + 1)/2;
                if self.value % Self::ten_to_the(dec_half) == 0 {
                    self.value /= Self::ten_to_the(dec_half);
                    self.decimals -= dec_half;
                }
                decimals /= 2;
            }
        }

        pub fn checked_add(self, other: Self) -> Option<Self> {
            if self.decimals == other.decimals {
                match self.value.checked_add(other.value) {
                    Some(value) => Some(Self{value: value, decimals: self.decimals}),
                    None => {
                        if self.decimals == 0 {
                            return None;
                        }
                        let trunc = (self.trunc() + other.trunc());
                        let fract = (self.fract() + other.fract());
                        Some(Self{value: trunc*Self::ten_to_the(self.decimals-1) + fract/10, decimals: self.decimals-1})
                    }
                }
            }
            else {
                let (fewer_dec, more_dec) = if self.decimals < other.decimals {(&self, &other)} else {(&other, &self)};

                let (fract, decimals) = {
                    let dec_diff = more_dec.decimals - fewer_dec.decimals;
                    let fewer_frac = fewer_dec.fract();
                    let more_frac = more_dec.fract();
                    //following checked_add can only possibly fail if decimails == MAX_DECIMALS for self or other
                    match (fewer_frac*Self::ten_to_the(dec_diff)).checked_add(more_frac) {
                        Some(value) => (value, more_dec.decimals),
                        None => (fewer_frac*Self::ten_to_the(dec_diff-1) + more_frac/10, more_dec.decimals-1)
                    }
                };

                let fewer_trunc = fewer_dec.trunc();
                let more_trunc_with_carry = {
                    let carry_over = fract/Self::ten_to_the(decimals); //either 0 or 1
                    more_dec.trunc() + carry_over //always safe since more_dec.decimals is >= 1
                };
                let fract = fract % Self::ten_to_the(decimals); //eliminate carry from fract if it exists

                //following checked_add can only possibly fail if decimals == 0 for self or other
                let trunc = match fewer_trunc.checked_add(more_trunc_with_carry) {
                    Some(value) => value,
                    None => {return None;}
                };
                
                let lbud = Self::get_lower_bound_unused_decimals(trunc);
                let trunc_with_all_dec = trunc.checked_mul(Self::ten_to_the(decimals));
                if lbud >= decimals || (lbud+1 == decimals && trunc_with_all_dec.is_some()) {
                    //block: we can fit all decimals into the result
                    Some(Self{value: trunc_with_all_dec.unwrap() + fract, decimals})
                }
                else {
                    //block: we have to truncate some decimals
                    debug_assert!(trunc != 0);
                    debug_assert!((lbud as u32) < Self::BITS); //should be a logical consequence of the previous line
                    let unused_decimals = lbud + if trunc.checked_mul(Self::ten_to_the(lbud+1)).is_some() {1} else {0};
                    let value = trunc*Self::ten_to_the(unused_decimals) +
                                fract/Self::ten_to_the(decimals-unused_decimals);
                    Some(Self{value, decimals: unused_decimals})
                }
            }
        }

        pub fn checked_sub(self, _other: Self) -> Option<Self> {
            todo!()
        }

        pub fn checked_mul(self, _other: Self) -> Option<Self> {
            todo!()
        }

        pub fn checked_div(self, _other: Self) -> Option<Self> {
            todo!()
        }

        const fn create_ten_to_the() -> [$value_type; (Self::MAX_DECIMALS+1) as usize] {
            let mut ttt = [1 as $value_type; (Self::MAX_DECIMALS+1) as usize];
            let mut i = 1;
            loop { //const functions can't use for loops
                if i > Self::MAX_DECIMALS as usize {
                    break;
                }
                ttt[i] = 10*ttt[i-1];
                i += 1;
                
            }
            ttt
        }

        const TEN_TO_THE: [$value_type; (Self::MAX_DECIMALS+1) as usize] = Self::create_ten_to_the();

        fn ten_to_the(exp: u8) -> $value_type {
            Self::TEN_TO_THE[exp as usize]
        }
        
        //determines the number of decimal places that are definitely unused given the number of leading
        // zeros in the binary representation
        //
        //e.g.:
        // an unsized integer with at least 4 leading zeros can always be multiplied by 10 without risk of overflow
        // an unsized integer with 2 or fewer leading zeros can never be safely multiplied by 10
        // an unsized integer with 3 leading zeros might or might not be safely multiplied by 10:
        // b*10 = b*8 + b*2 = b<<3 + b<<1
        // 0b0001_0000 * 10 = 0b1010_0000 -> safe to multiply by 10
        // 0b0001_1100 * 10 = 0b1110_0000 + 0b0011_1000 -> overflow
        const fn create_loader_bound_unused_decimals() -> [u8; (Self::BITS + 1) as usize] {
            let mut tzbl = [Self::MAX_DECIMALS; (Self::BITS + 1) as usize];
            tzbl[0] = 0;
            let mut pot: $value_type = 10;
            let mut val: $value_type = 1;
            let mut i = 1;
            loop { //const functions can't use for loops
                if i == Self::BITS as usize {
                    break;
                }
                tzbl[i] = tzbl[i-1] + if val / pot != 0 {pot *= 10; 1} else {0};
                val <<= 1;
                i += 1;
            }
            tzbl
        }

        const LOWER_BOUND_UNUSED_DECIMALS: [u8; (Self::BITS + 1) as usize] = Self::create_loader_bound_unused_decimals();

        fn get_lower_bound_unused_decimals(value: $value_type) -> u8 {
            Self::LOWER_BOUND_UNUSED_DECIMALS[value.leading_zeros() as usize]
        }
    }

    impl BorshDeserialize for $name {
        fn deserialize(buf: &mut &[u8]) -> Result<Self, std::io::Error> {
            let value = <$value_type>::deserialize(buf)?;
            let decimals = <u8>::deserialize(buf)?;
            if decimals > Self::MAX_DECIMALS {
                todo!();
            }
            else {
                Ok(Self{value, decimals})
            }
        }
    }

    impl PartialEq for $name {
        fn eq(&self, other: &Self) -> bool {
            self.trunc() == other.trunc() && self.fract() == other.fract()
        }
    }
    
    impl Eq for $name {}
    
    impl PartialOrd for $name {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    
    impl Ord for $name {
        fn cmp(&self, other: &Self) -> Ordering {
            let cmp = self.trunc().cmp(&other.trunc());
            match cmp {
                //TODO BUGGY! can't compare fracts - need to multiply to correct decimals!
                Ordering::Equal => self.fract().cmp(&other.fract()),
                _ => cmp
            }
        }
    }

    impl Add for $name {
        type Output = Self;

        fn add(self, other: Self) -> Self::Output {
            self.checked_add(other).unwrap_or_else(|| panic!("Overflow while adding decimals {:?} {:?}", self, other))
        }
    }

    impl AddAssign for $name {
        fn add_assign(&mut self, _other: Self) {
            todo!()
        }
    }

    impl Sub for $name {
        type Output = Self;

        fn sub(self, other: Self) -> Self::Output {
            self.checked_sub(other).unwrap_or_else(|| panic!("Underflow while subtracting decimals {:?} {:?}", self, other))
        }
    }

    impl SubAssign for $name {
        fn sub_assign(&mut self, _other: Self) {
            todo!()
        }
    }

    impl Mul for $name {
        type Output = Self;

        fn mul(self, other: Self) -> Self::Output {
            self.checked_mul(other).unwrap_or_else(|| panic!("Overflow while multiplying decimals {:?} {:?}", self, other))
        }
    }

    impl MulAssign for $name {
        fn mul_assign(&mut self, _other: Self) {
            todo!()
        }
    }

    impl Div for $name {
        type Output = Self;

        fn div(self, other: Self) -> Self::Output {
            self.checked_div(other).unwrap_or_else(|| panic!("Division by zero while dividing decimals {:?} {:?}", self, other))
        }
    }

    impl DivAssign for $name {
        fn div_assign(&mut self, _other: Self) {
            todo!()
        }
    }
    
    impl Display for $name {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            let fract = self.fract();
            if fract == 0 {
                write!(f, "{}", self.trunc())
            }
            else {
                write!(f, "{}.{}", self.trunc(), fract)
            }
        }
    }
}}

// unsigned_decimal! {
//     DecimalU128,
//     u128,
//     128,
//     38,
// }

// unsigned_decimal! {
//     DecimalU64,
//     u64,
//     64,
//     19,
// }

// unsigned_decimal! {
//     DecimalU32,
//     u32,
//     32,
//     9,
// }

unsigned_decimal! {
    DecimalU8,
    u8,
    8,
    2,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_test() {
        let new = |value, decimals| DecimalU8::new(value, decimals).unwrap();
        assert_eq!(new(111,2)+new(11,1), new(221,2));
        assert_eq!(new(127,0)+new(128,0), new(255,0));
        assert_eq!(new(127,2)+new(128,2), new(255,2));
        assert_eq!(new(128,2)+new(128,2), new(25,1));
        assert!(new(128,0).checked_add(new(128,0)).is_none());
    }
}