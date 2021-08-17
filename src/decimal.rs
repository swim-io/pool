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

// KEEP_MAX_DECIMALS = true ensures that after all operations it holds that ...
//  ... result.decimals == max(operand1.decimals, operand2.decimals)
//  KEEP_MAX_DECIMALS = false otoh means result.decimals will float freely to give the best result
//  Notice that KEEP_MAX_DECIMALS means potentially lower precision in case of multiplication/division!
const KEEP_MAX_DECIMALS_DEFAULT: bool = false;

// all math in this module is implemented in such a way that all operations that *aren't* ...
//  ... explicitly checked_* (i.e. all the inline ops like +,-,*,/,%, etc.) should never ...
//  ... be able to fail and could hence be replaced by unsafe_* calls to reduce strain on compute budget

const fn ten_to_the(exp: u8) -> u128 {
    TEN_TO_THE[exp as usize]
}
const U128_MAX_DECIMALS: usize = 39;
const TEN_TO_THE: [u128; U128_MAX_DECIMALS] = create_ten_to_the();
const fn create_ten_to_the() -> [u128; U128_MAX_DECIMALS] {
    let mut ttt = [1 as u128; U128_MAX_DECIMALS];
    let mut i = 1;
    loop { //const functions can't use for loops
        if i == U128_MAX_DECIMALS {
            break;
        }
        ttt[i] = 10*ttt[i-1];
        i += 1;
        
    }
    ttt
}

const U128_BITS: usize = 128; //u128::BITS is still an unstable feature
const BOUND_UNUSED_DECIMALS: [u8; U128_BITS + 2] = create_bound_unused_decimals();
const fn create_bound_unused_decimals() -> [u8; U128_BITS + 2] {
    let mut bud = [0; U128_BITS + 2];
    let mut pot: u128 = 10;
    let mut i = 1 as usize; //we start with the second iteration
    loop { //const functions can't use for loops
        let jump = ((1 << i as u128) / pot) as u8;
        if jump == 1 {
            pot = match pot.checked_mul(10) {
                Some(v) => v,
                None => {
                    bud[i] = bud[i-1] + 1;
                    loop {
                        i += 1;
                        if i >= U128_BITS + 2 {
                            return bud;
                        }
                        bud[i] = bud[i-1];
                    }
                }
            }
        }
        bud[i] = bud[i-1] + jump;
        i += 1;
    }
}

macro_rules! unsigned_decimal {(
    $name:ident,
    $value_type:ty,
    $bits:expr, //<$value_type>::BITS is still unstable
    $max_decimals:expr //floor(log_10(2^bits-1))
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

        fn ten_to_the(exp: u8) -> $value_type {
            debug_assert!(exp <= Self::MAX_DECIMALS, "exp={} exceeded MAX_DECIMALS={}", exp, Self::MAX_DECIMALS);
            ten_to_the(exp) as $value_type
        }

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

macro_rules! impl_unused_decimal_bounds {(
    $type:ty,
    $bits:expr //<$type>::BITS is still unstable
) => {
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
    #[allow(dead_code)] //DecimalU128 is incomplete and hence does not use this function yet
    const fn get_lower_bound_unused_decimals(value: $type) -> u8 {
        BOUND_UNUSED_DECIMALS[value.leading_zeros() as usize + 1]
    }

    const fn get_upper_bound_unused_decimals(value: $type) -> u8 {       
        BOUND_UNUSED_DECIMALS[value.leading_zeros() as usize]
    }
}}

macro_rules! impl_checked_math {(
    $name:ident,
    $value_type:ty,
    $larger_type:ty,
    $larger_max_decimals:expr
) => {
    impl $name {
        impl_unused_decimal_bounds!($larger_type, 2*Self::BITS);

        fn equalize_decimals(v1: Self, v2: Self) -> ($larger_type, $larger_type, u8) {
            let v1_val = v1.value as $larger_type;
            let v2_val = v2.value as $larger_type;
            match v1.decimals.cmp(&v2.decimals) {
                Ordering::Equal   => (v1_val, v2_val, v1.decimals),
                Ordering::Less    => (v1_val*ten_to_the(v2.decimals-v1.decimals) as $larger_type, v2_val, v2.decimals),
                Ordering::Greater => (v1_val, v2_val*ten_to_the(v1.decimals-v2.decimals) as $larger_type, v1.decimals),
            }
        }

        fn shift_to_fit<const KEEP_MAX_DECIMALS: bool>(mut value: $larger_type, mut decimals: u8) -> Option<Self> {
            if value > <$value_type>::MAX.into() {
                if KEEP_MAX_DECIMALS {
                    return None;
                }
                else {
                    let lbud = Self::get_lower_bound_unused_decimals(value);
                    let down_shift_decimals = $larger_max_decimals - lbud - Self::MAX_DECIMALS;
                    if decimals < down_shift_decimals {
                        return None;
                    }
                    value /= ten_to_the(down_shift_decimals) as $larger_type;
                    decimals -= down_shift_decimals;
                    if value > <$value_type>::MAX.into() {
                        if decimals == 0 {
                            return None;
                        }
                        value /= 10;
                        decimals -= 1;   
                    }
                }
            }
            
            Some(Self{value: value as $value_type, decimals})
        }

        pub fn checked_add<const KEEP_MAX_DECIMALS: bool>(self, other: Self) -> Option<Self> {
            let (v1, v2, decimals) = Self::equalize_decimals(self, other);
            Self::shift_to_fit::<KEEP_MAX_DECIMALS>(v1 + v2, decimals)
        }

        pub fn checked_sub<const KEEP_MAX_DECIMALS: bool>(self, other: Self) -> Option<Self> {
            let (v1, v2, decimals) = Self::equalize_decimals(self, other);
            if v1 < v2 {
                None
            }
            else {
                Self::shift_to_fit::<KEEP_MAX_DECIMALS>(v1 - v2, decimals)
            }
        }

        pub fn checked_mul<const KEEP_MAX_DECIMALS: bool>(self, other: Self) -> Option<Self> {
            let mut value = self.value as $larger_type * other.value as $larger_type;
            let mut decimals = self.decimals + other.decimals;
            if KEEP_MAX_DECIMALS {
                value /= ten_to_the(cmp::min(self.decimals, other.decimals)) as $larger_type;
                decimals = cmp::max(self.decimals, other.decimals);
            }
            Self::shift_to_fit::<KEEP_MAX_DECIMALS>(value, decimals)
        }

        pub fn checked_div<const KEEP_MAX_DECIMALS: bool>(self, other: Self) -> Option<Self> {
            if other.value == 0 {
                return None;
            }

            //other.normalize(); //is this ever helpful? (we'd have to save max_decimals first)

            let val1 = self.value as $larger_type;
            let val2 = other.value as $larger_type;
            let mut shift = Self::get_upper_bound_unused_decimals(val1);

            let val1_shifted = match val1.checked_mul(ten_to_the(shift) as $larger_type) {
                Some(v) => v,
                None => {
                    shift -= 1;
                    val1 * ten_to_the(shift) as $larger_type
                }
            };
            let mut value = val1_shifted / val2;
            let mut decimals = self.decimals + shift - other.decimals;
            if KEEP_MAX_DECIMALS {
                let max_decimals = cmp::max(self.decimals, other.decimals);
                match decimals.cmp(&max_decimals) {
                    Ordering::Less => {
                        value = match value.checked_mul(ten_to_the(max_decimals - decimals) as $larger_type) {
                            Some(v) => v,
                            None => {return None;}
                        }
                    },
                    Ordering::Greater => {
                        value /= ten_to_the(decimals - max_decimals) as $larger_type;
                        decimals = max_decimals;
                    }
                    _ => ()
                }
            }
            Self::shift_to_fit::<KEEP_MAX_DECIMALS>(value, decimals)
        }
    }
}}

macro_rules! impl_ops {(
    $name: ident
) => {
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
                Ordering::Equal => {
                    match self.decimals.cmp(&other.decimals) {
                        Ordering::Equal => self.fract().cmp(&other.fract()),
                        Ordering::Less => (self.fract()*Self::ten_to_the(other.decimals-self.decimals)).cmp(&other.fract()),
                        Ordering::Greater => self.fract().cmp(&(other.fract()*Self::ten_to_the(self.decimals-other.decimals))),
                    }
                },
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
        fn add_assign(&mut self, other: Self) {
            *self = *self + other;
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
        fn sub_assign(&mut self, other: Self) {
            *self = *self - other;
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
        fn mul_assign(&mut self, other: Self) {
            *self = *self * other;
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
        fn div_assign(&mut self, other: Self) {
            *self = *self / other;
        }
    }
}}

unsigned_decimal! {DecimalU8, u8, 8, 2}
impl_checked_math!{DecimalU8, u8, u16, 4}
impl_ops!{DecimalU8}
// unsigned_decimal! {DecimalU16, u16, 16, 4}
// impl_checked_math!{DecimalU16, u16, u32, 9}
// impl_ops!{DecimalU16}
// unsigned_decimal! {DecimalU32, u32, 32, 9}
// impl_checked_math!{DecimalU32, u32, u64, 19}
// impl_ops!{DecimalU32}
unsigned_decimal! {DecimalU64, u64, 64, 19}
impl_checked_math!{DecimalU64, u64, u128, 38}
impl_ops!{DecimalU64}

unsigned_decimal! {DecimalU128, u128, 128, 38}
impl DecimalU128 {
    impl_unused_decimal_bounds!(u128, 128);

    fn equalize_decimals<const KEEP_MAX_DECIMALS: bool>(v1: Self, v2: Self) -> Option<(u128, u128, u8)> {
        if v1.decimals == v2.decimals {
            //special handling to optimize and simplify typical case
            Some((v1.value, v2.value, v1.decimals))
        }
        else {
            let v1_has_fewer_decimals = v1.decimals < v2.decimals;
            let (fewer_dec, more_dec) = if v1_has_fewer_decimals {(&v1, &v2)} else {(&v2, &v1)};
            let dec_diff = more_dec.decimals - fewer_dec.decimals;
            let ubud = Self::get_upper_bound_unused_decimals(fewer_dec.value);
            let mut shift = cmp::min(ubud, dec_diff);
            if KEEP_MAX_DECIMALS && shift != dec_diff {
                return None;
            }

            let shifted_fewer_value = match fewer_dec.value.checked_mul(ten_to_the(shift)) {
                Some(value) => value,
                None => {
                    if KEEP_MAX_DECIMALS {
                        return None;
                    }
                    shift -= 1;
                    fewer_dec.value * ten_to_the(shift)
                }
            };
            let shifted_more_value = more_dec.value/ten_to_the(dec_diff-shift);

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
        todo!();
    }

    pub fn checked_div<const KEEP_MAX_DECIMALS: bool>(self, _other: Self) -> Option<Self> {
        todo!();
    }
}
impl_ops!{DecimalU128}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_test() {
        let new_u8 = |value, decimals| DecimalU8::new(value, decimals).unwrap();
        assert_eq!(new_u8(111,2) + new_u8(11,1), new_u8(221,2));
        assert_eq!(new_u8(127,0) + new_u8(128,0), new_u8(255,0));
        assert_eq!(new_u8(127,2) + new_u8(128,2), new_u8(255,2));
        assert_eq!(new_u8(1,1) * new_u8(1,1), new_u8(1,2));
        assert_eq!(new_u8(1,0) / new_u8(3,0), new_u8(33,2));
        //assert_eq!(new_u8(128,2).checked_add::<false>(new_u8(128,2)), Some(new_u8(25,1)));
        //assert!(new_u8(128,2).checked_add::<true>(new_u8(128,2)).is_none());
        //assert!(new_u8(128,0).checked_add::<false>(new_u8(128,0)).is_none());

    }

    // #[test]
    // fn print_bounds() {
    //     print!("BOUND_UNUSED_DECIMALS:");
    //     for i in 0..130 {
    //         print!("({},{})", i, BOUND_UNUSED_DECIMALS[i]);
    //     }
    // }
}