use std::{
    cmp,
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

const KEEP_MAX_DECIMALS_DEFAULT: bool = true;

// all math in here is implemented in such a way that all operations that *aren't* ...
//  ... explicitly checked_* (i.e. all the inline ops like +,-,*,/,%, etc.) should never ...
//  ... be able to fail and could hence be replaced by unsafe_* calls to reduce strain on compute budget
//
// KEEP_MAX_DECIMALS = true ensures that after all operations it holds that ...
//  ... result.decimals == max(operand1.decimals, operand2.decimals)
//  KEEP_MAX_DECIMALS = false otoh means result.decimals can be different from either operators
//  Notice that KEEP_MAX_DECIMALS means potentially lower precision in case of division!
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
            self.value % Self::ten_to_the(self.decimals)
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

        fn equalize_decimals<const KEEP_MAX_DECIMALS: bool>(v1: Self, v2: Self) -> Option<($value_type, $value_type, u8)> {
            if v1.decimals == v2.decimals {
                //special handling to optimize and simplify typical case
                Some((v1.value, v2.value, v1.decimals))
            }
            else {
                let v1_has_fewer_decimals = v1.decimals < v2.decimals;
                let (fewer_dec, more_dec) = if v1_has_fewer_decimals {(&v1, &v2)} else {(&v2, &v1)};
                let dec_diff = more_dec.decimals - fewer_dec.decimals;
                let (_, ubud) = Self::get_bound_unused_decimals(fewer_dec.value);
                let mut shift = cmp::min(ubud, dec_diff);
                if KEEP_MAX_DECIMALS && shift != dec_diff {
                    return None;
                }

                let shifted_fewer_value = match fewer_dec.value.checked_mul(Self::ten_to_the(shift)) {
                    Some(value) => value,
                    None => {
                        if KEEP_MAX_DECIMALS {
                            return None;
                        }
                        shift -= 1;
                        fewer_dec.value * Self::ten_to_the(shift)
                    }
                };
                let shifted_more_value = more_dec.value/Self::ten_to_the(dec_diff-shift);

                Some(
                    if v1_has_fewer_decimals {
                        (shifted_fewer_value, shifted_more_value, fewer_dec.decimals + shift)
                    }
                    else {
                        (shifted_more_value, shifted_fewer_value, fewer_dec.decimals + shift)
                    }
                )
            }
        }

        pub fn checked_add<const KEEP_MAX_DECIMALS: bool>(self, other: Self) -> Option<Self> {
            match Self::equalize_decimals::<KEEP_MAX_DECIMALS>(self, other) {
                Some((val_1, val_2, decimals)) => {
                    match val_1.checked_add(val_2) {
                        Some(value) => Some(Self{value, decimals}),
                        None => {
                            if KEEP_MAX_DECIMALS || decimals == 0 {
                                return None;
                            }
                            let value = (val_1/10 + val_2/10) + (val_1%10 + val_2%10)/10;
                            Some(Self{value, decimals: decimals-1})
                        }
                    }
                },
                None => None
            }
        }

        pub fn checked_sub<const KEEP_MAX_DECIMALS: bool>(self, other: Self) -> Option<Self> {
            match Self::equalize_decimals::<KEEP_MAX_DECIMALS>(self, other) {
                Some((val_1, val_2, decimals)) => {
                    if val_1 < val_2 {
                        Some(Self{value: val_1 - val_2, decimals})
                    }
                    else {
                        None
                    }
                },
                None => None
            }
        }

        pub fn checked_mul<const KEEP_MAX_DECIMALS: bool>(self, _other: Self) -> Option<Self> {
            todo!()
        }

        pub fn checked_div<const KEEP_MAX_DECIMALS: bool>(self, _other: Self) -> Option<Self> {
            todo!()
        }

        pub const fn ten_to_the(exp: u8) -> $value_type {
            Self::TEN_TO_THE[exp as usize]
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
        
        //determines the number of decimal places that are definitely unused given the number of leading
        // zeros in the binary representation
        //
        //e.g.:
        // multiplying by 10 = 8 + 2 = 0b1010 -> x*10 = x*8 + x*2 = x<<3 + x<<1
        // an unsized integer with at least 4 leading zeros can always be multiplied by 10 without risk of overflow
        // an unsized integer with 2 or fewer leading zeros can never be safely multiplied by 10
        // an unsized integer with 3 leading zeros might or might not be safely multiplied by 10:
        // 0b0001_0000 * 10 = 0b1010_0000 -> safe to multiply by 10
        // 0b0001_1100 * 10 = 0b1110_0000 + 0b0011_1000 -> overflow
        // 
        // multiplying by 10^2 = 64 + 32 + 4 = 0b1100100 -> x*100 = x*64 + x*32 + x*4 = x<<6 + x<<5 + x<<2
        // so again 6 leading zeros might or might not be enough to multiply by 100 while ...
        // ... 5 definitely isn't enough and 7 certainly is
        // 
        // same for multiplying by 10^3 = 0b1111101000 -> x*1000 = x<<9 + ... so 9 might or might not be enough
        //
        // however multiplying by 10^4 = 0b10011100010000 -> x*10000 = x<<13 + ... so 13 not 12!
        const fn get_bound_unused_decimals(value: $value_type) -> (u8,u8) {
            let zeros = value.leading_zeros() as usize;
            (Self::BOUND_UNUSED_DECIMALS[zeros], Self::BOUND_UNUSED_DECIMALS[zeros+1])
        }
        const BOUND_UNUSED_DECIMALS: [u8; (Self::BITS + 2) as usize] = Self::create_bound_unused_decimals();
        const fn create_bound_unused_decimals() -> [u8; (Self::BITS + 2) as usize] {
            let mut bud = [Self::MAX_DECIMALS; (Self::BITS + 2) as usize];
            bud[0] = 0;
            let mut pot: $value_type = 10;
            let mut val: $value_type = 1 << 1; //essentially we start with the second iteration
            let mut i = 1;
            loop { //const functions can't use for loops
                let jump = (val / pot) as u8;
                if jump == 1 {
                    if pot == Self::ten_to_the(Self::MAX_DECIMALS) {
                        break;
                    }
                    pot *= 10;
                }
                bud[i] = bud[i-1] + jump;
                val <<= 1;
                i += 1;
            }
            bud
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
            self.checked_add::<KEEP_MAX_DECIMALS_DEFAULT>(other)
                .unwrap_or_else(|| panic!("Overflow while adding {:?} {:?}", self, other))
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
            self.checked_sub::<KEEP_MAX_DECIMALS_DEFAULT>(other)
                .unwrap_or_else(|| panic!("Underflow while subtracting {:?} {:?}", self, other))
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
            self.checked_mul::<KEEP_MAX_DECIMALS_DEFAULT>(other)
                .unwrap_or_else(|| panic!("Overflow while multiplying {:?} {:?}", self, other))
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
            self.checked_div::<KEEP_MAX_DECIMALS_DEFAULT>(other)
                .unwrap_or_else(|| panic!("Division by zero while dividing {:?} {:?}", self, other))
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

unsigned_decimal! {
    DecimalU32,
    u32,
    32,
    9,
}

unsigned_decimal! {
    DecimalU16,
    u16,
    16,
    4,
}

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
        assert_eq!(new(128,2).checked_add::<false>(new(128,2)), Some(new(25,1)));
        assert!(new(128,2).checked_add::<true>(new(128,2)).is_none());
        assert!(new(128,0).checked_add::<false>(new(128,0)).is_none());
    }

    // #[test]
    // fn print_bounds() {
    //     print!("Bound:");
    //     for i in 0..=33 {
    //         print!("{}", DecimalU32::BOUND_UNUSED_DECIMALS[i]);
    //     }
    // }
}