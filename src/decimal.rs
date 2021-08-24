// This module is essentially a poor man's decimal (using base 10), unsigned only, floating point
// implementation with a limit on the decimal point being within at most MAX_DECIMALS
// places away from 0 (e.g. DecimalU8 can represent values from 0.00 to 2.55 in 0.01 increments,
// from 0.0 to 25.5 in 0.1 increments, or the default u8 range of 0 to 255)
//
// The idea of this module is to perform all operations with built-in unsigned types and its
// efficiency depends on the efficiency of unsigned.leading_zeros(). On an x86 cpu this ought to
// be a single instruction, but this might not be the case for other architectures.
//
// All math in this module is implemented in such a way that all operations that *aren't*
// don't explicitly use checked_* (i.e. all the inline ops like +,-,*,/,%, etc.) should never
// be able to fail and could hence be replaced by unsafe_* calls to reduce strain on compute budget.

use std::{
    io,
    cmp, cmp::{PartialEq, Eq, PartialOrd, Ord, Ordering},
    ops::{Add, AddAssign, Sub, SubAssign, Mul, MulAssign, Div, DivAssign},
    convert::TryFrom,
    fmt, fmt::{Display, Formatter},
    iter::{Sum, Product},
};
use borsh::{BorshDeserialize, BorshSerialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DecimalError {
    #[error("Maximum decimals exceeded")]
    MaxDecimalsExceeded,
    #[error("Conversion error")]
    ConversionError
}

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

// The following code uses unsigned.leading_zeros() to approximate log10 of a given number as well
// as to find the number of unused decimal places that are still available.
//
//e.g.:
// multiplying by 10 = 8 + 2 = 0b1010 -> x*10 = x*8 + x*2 = x<<3 + x<<1
// an unsized integer with 4 leading zeros can always be multiplied by 10 without risk of overflow
// => it definitely has one "unused decimal"
// an unsized integer with 2 or fewer leading zeros can never be safely multiplied by 10
// => it definitely has no "unused decimal"
// an unsized integer with 3 leading zeros might or might not be safely multiplied by 10:
// 0b0001_0000 * 10 = 0b1010_0000 -> safe to multiply by 10
// 0b0001_1100 * 10 = 0b1110_0000 + 0b0011_1000 -> overflow
// => with 3 leading zeros in binary representation, therefore the lower bound is 0, the upper bound is 1
// 
// multiplying by 10^2 = 64 + 32 + 4 = 0b1100100 -> x*100 = x*64 + x*32 + x*4 = x<<6 + x<<5 + x<<2
// so again 6 leading zeros might or might not be enough to multiply by 100 while ...
// ... 5 definitely isn't enough and 7 certainly is
// so 6 has a lower bound of 1 and an upper bound of 2
// 
// same for multiplying by 10^3 = 0b1111101000 -> x*1000 = x<<9 + ... so 9 might or might not be enough
// so 9 has a lower bound of 2 and an upper bound of 3
//
// however multiplying by 10^4 = 0b10011100010000 -> x*10000 = x<<13 + ... so 13 not 12!
// so 12 has both a lower and an upper bound of 3 and
// while 13 has has 3 and 4 respectively
//
// so to find the bounds for a given number i of leading zeros, we can calculate the lower bound of i
// and the upper bound of i will be the same as the lower bound of i+1
//
// example:
// get_order_of_magnitude(255u8) = 3 and get_unused_decimals(255u8) = 0
// but also!:
// get_order_of_magnitude(26u8) = 2 but still get_unused_decimals(255u8) = 0
// while:
// get_order_of_magnitude(25u8) = 2 however get_unused_decimals(255u8) = 1
const BIT_TO_DEC_SIZE: usize = 128+2; //u128::BITS is still an unstable feature
const BIT_TO_DEC_ARRAY: [u8;BIT_TO_DEC_SIZE] = create_bit_to_dec_array();
const fn create_bit_to_dec_array() -> [u8; BIT_TO_DEC_SIZE] {
    let mut btd = [0; BIT_TO_DEC_SIZE];
    let mut pot: u128 = 10;
    let mut i = 1 as usize; //we start with the second iteration
    loop { //const functions can't use for loops
        let jump = ((1 << i as u128) / pot) as u8;
        if jump == 1 {
            pot = match pot.checked_mul(10) {
                Some(v) => v,
                None => {
                    btd[i] = btd[i-1] + 1;
                    loop {
                        i += 1;
                        if i >= BIT_TO_DEC_SIZE {
                            return btd;
                        }
                        btd[i] = btd[i-1];
                    }
                }
            }
        }
        btd[i] = btd[i-1] + jump;
        i += 1;
    }
}

macro_rules! impl_unused_decimal_bounds {(
    $type:ty,
    $bits:expr //<$type>::BITS is still unstable
) => {
    fn get_unused_decimals(value: $type) -> u8 {
        let leading_zeros = value.leading_zeros() as usize;
        let lower_bound = BIT_TO_DEC_ARRAY[leading_zeros];
        let upper_bound = BIT_TO_DEC_ARRAY[leading_zeros + 1];
        if lower_bound == upper_bound || value.checked_mul(ten_to_the(upper_bound)).is_none() {
            lower_bound
        }
        else {
            upper_bound
        }
    }

    fn get_order_of_magnitude(value: $type) -> u8 {
        debug_assert!(value != 0);
        let used_bits = $bits as usize - value.leading_zeros() as usize;
        let lower_bound = BIT_TO_DEC_ARRAY[used_bits-1];
        let upper_bound = BIT_TO_DEC_ARRAY[used_bits];
        if lower_bound == upper_bound || value/ten_to_the(lower_bound) < 10 {
            lower_bound
        }
        else {
            upper_bound
        }
    }
}}

macro_rules! unsigned_decimal {(
    $name:ident,
    $value_type:ty,
    $bits:expr, //<$value_type>::BITS is still unstable
    $max_decimals:expr //floor(log_10(2^bits-1))
) => {
    #[derive(BorshSerialize, Debug, Clone, Copy, Default)]
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

        pub const fn new(value: $value_type, decimals: u8) -> Result<Self, DecimalError> {
            if decimals > Self::MAX_DECIMALS {
                return Err(DecimalError::MaxDecimalsExceeded)
            }
            Ok(Self{value, decimals})
        }

        //workaround because From trait's from function isn't const...
        pub const fn const_from(value: $value_type) -> Self {
            Self{value, decimals: 0}
        }

        pub const fn get_raw(&self) -> $value_type {
            self.value
        }

        pub const fn get_decimals(&self) -> u8 {
            self.decimals
        }

        pub fn trunc(&self) -> $value_type {
            self.value / Self::ten_to_the(self.decimals)
        }

        pub fn fract(&self) -> $value_type {
            self.value % Self::ten_to_the(self.decimals)
        }

        pub fn ceil(&self, decimals: u8) -> Self {
            let mut ret = self.clone();
            if decimals < ret.decimals {
                let pot = Self::ten_to_the(ret.decimals - decimals);
                let up = if (ret.value % pot > 0) {1} else {0};
                ret.value /= pot;
                ret.value += up;
                ret.decimals = decimals;
            }
            ret
        }

        //TODO banker's rounding?
        pub fn round(&self, decimals: u8) -> Self {
            let mut ret = self.clone();
            if decimals < ret.decimals {
                let pot = Self::ten_to_the(ret.decimals - decimals);
                let up = if (ret.value % pot) / (pot/10) >= 5 {1} else {0};
                ret.value /= pot;
                ret.value += up;
                ret.decimals = decimals;
            }
            ret
        }

        pub fn floor(&self, decimals: u8) -> Self {
            let mut ret = self.clone();
            if decimals < ret.decimals {
                ret.value /= Self::ten_to_the(ret.decimals - decimals);
                ret.decimals = decimals;
            }
            ret
        }

        //reduce decimals as to eliminate all trailing decimal zeros
        pub fn normalize(&self) -> Self {
            if self.decimals == 0 {
                return self.clone();
            }
            //binary search
            let mut shift = 0;
            let mut dec = self.decimals;
            loop {
                dec = (dec + 1)/2;
                if self.value % Self::ten_to_the(shift + dec) == 0 {
                    shift += dec;
                }
                if dec == 1 {
                    break;
                }
            }
            Self{value: self.value / Self::ten_to_the(shift), decimals: self.decimals - shift}
        }
    }

    impl From<$value_type> for $name {
        fn from(value: $value_type) -> Self {
            Self{value, decimals: 0}
        }
    }

    impl Display for $name {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            let normalized = self.normalize();
            let fract = normalized.fract();
            if fract == 0 {
                write!(f, "{}", self.trunc())
            }
            else {
                write!(f, "{}.{:0decimals$}", self.trunc(), fract, decimals = normalized.decimals as usize)
            }
        }
    }

    impl BorshDeserialize for $name {
        fn deserialize(buf: &mut &[u8]) -> Result<Self, io::Error> {
            let value = <$value_type>::deserialize(buf)?;
            let decimals = <u8>::deserialize(buf)?;
            if decimals > Self::MAX_DECIMALS {
                Err(io::Error::new(io::ErrorKind::InvalidData, "decimals value out of bounds"))
            }
            else {
                Ok(Self{value, decimals})
            }
        }
    }

    impl PartialEq for $name {
        fn eq(&self, other: &Self) -> bool {
            self.trunc() == other.trunc() &&
            match self.decimals.cmp(&other.decimals) {
                Ordering::Equal => {self.fract() == other.fract()},
                Ordering::Less => {self.fract()*Self::ten_to_the(other.decimals-self.decimals) == other.fract()},
                Ordering::Greater => {self.fract() == other.fract()*Self::ten_to_the(self.decimals-other.decimals)},
            }
        }
    }

    impl PartialEq<$value_type> for $name {
        fn eq(&self, other: &$value_type) -> bool {
            self.trunc() == *other && self.fract() == 0
        }
    }

    impl PartialEq<$name> for $value_type {
        fn eq(&self, other: &$name) -> bool {
            other.eq(self)
        }
    }

    impl Eq for $name {}
    
    impl PartialOrd for $name {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl PartialOrd<$value_type> for $name {
        fn partial_cmp(&self, other: &$value_type) -> Option<Ordering> {
            Some(self.cmp(&Self{value: *other, decimals: 0}))
        }
    }

    impl PartialOrd<$name> for $value_type {
        fn partial_cmp(&self, other: &$name) -> Option<Ordering> {
            Some($name{value: *self, decimals: 0}.cmp(other))
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
            self.checked_add(other)
                .unwrap_or_else(|| panic!("Overflow while adding {:?} {:?}", self, other))
        }
    }

    impl Add<$value_type> for $name {
        type Output = Self;

        fn add(self, other: $value_type) -> Self::Output {
            self + Self{value: other, decimals: 0}
        }
    }

    impl Add<$name> for $value_type {
        type Output = $name;

        fn add(self, other: $name) -> Self::Output {
            other + self
        }
    }

    impl AddAssign for $name {
        fn add_assign(&mut self, other: Self) {
            *self = *self + other;
        }
    }

    impl AddAssign<$value_type> for $name {
        fn add_assign(&mut self, other: $value_type) {
            *self = *self + other;
        }
    }

    impl Sum for $name {
        fn sum<I>(iter: I) -> Self
            where I: Iterator<Item = Self> {
            iter.fold(Self::from(0), |accumulator, it| accumulator + it)
        }
    }

    impl Sub for $name {
        type Output = Self;

        fn sub(self, other: Self) -> Self::Output {
            self.checked_sub(other)
                .unwrap_or_else(|| panic!("Underflow while subtracting {:?} {:?}", self, other))
        }
    }

    impl Sub<$value_type> for $name {
        type Output = Self;

        fn sub(self, other: $value_type) -> Self::Output {
            self - Self{value: other, decimals: 0}
        }
    }

    impl Sub<$name> for $value_type {
        type Output = $name;

        fn sub(self, other: $name) -> Self::Output {
            $name{value: self, decimals: 0} - other
        }
    }

    impl SubAssign for $name {
        fn sub_assign(&mut self, other: Self) {
            *self = *self - other;
        }
    }

    impl SubAssign<$value_type> for $name {
        fn sub_assign(&mut self, other: $value_type) {
            *self = *self - other;
        }
    }

    impl Mul for $name {
        type Output = Self;

        fn mul(self, other: Self) -> Self::Output {
            self.checked_mul(other)
                .unwrap_or_else(|| panic!("Overflow while multiplying {:?} {:?}", self, other))
        }
    }

    impl Mul<$value_type> for $name {
        type Output = Self;

        fn mul(self, other: $value_type) -> Self::Output {
            self * Self{value: other, decimals: 0}
        }
    }

    impl Mul<$name> for $value_type {
        type Output = $name;

        fn mul(self, other: $name) -> Self::Output {
            other * self
        }
    }

    impl MulAssign for $name {
        fn mul_assign(&mut self, other: Self) {
            *self = *self * other;
        }
    }

    impl MulAssign<$value_type> for $name {
        fn mul_assign(&mut self, other: $value_type) {
            *self = *self * other;
        }
    }

    impl Product for $name {
        fn product<I>(iter: I) -> Self
            where I: Iterator<Item = Self> {
            iter.fold(Self::from(1), |accumulator, it| accumulator * it)
        }
    }

    impl Div for $name {
        type Output = Self;

        fn div(self, other: Self) -> Self::Output {
            self.checked_div(other)
                .unwrap_or_else(|| panic!("Division by zero while dividing {:?} {:?}", self, other))
        }
    }

    impl Div<$value_type> for $name {
        type Output = Self;

        fn div(self, other: $value_type) -> Self::Output {
            self / Self{value: other, decimals: 0}
        }
    }

    impl Div<$name> for $value_type {
        type Output = $name;

        fn div(self, other: $name) -> Self::Output {
            $name{value: self, decimals: 0} / other
        }
    }

    impl DivAssign for $name {
        fn div_assign(&mut self, other: Self) {
            *self = *self / other;
        }
    }

    impl DivAssign<$value_type> for $name {
        fn div_assign(&mut self, other: $value_type) {
            *self = *self / other;
        }
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

        //shifts the passed value so it fits within Self
        fn shift_to_fit(mut value: $larger_type, mut decimals: u8) -> Option<Self> {
            //space exceeded ensures that value/ten_to_the(excess_decimals) fits within $value_type
            let space_exceeded = if value > <$value_type>::MAX as $larger_type {
                1 + Self::get_order_of_magnitude(value>>Self::BITS)
            } else {
                0
            };
            //decimals_exceeded ensures that resulting decimals aren't larger than MAX_DECIMALS
            let decimals_exceeded = decimals.checked_sub(Self::MAX_DECIMALS).unwrap_or(0);

            let down_shift_decimals = cmp::max(space_exceeded, decimals_exceeded);
            if down_shift_decimals != 0 {
                if decimals < down_shift_decimals {
                    return None;
                }
                value /= ten_to_the(down_shift_decimals) as $larger_type;
                decimals -= down_shift_decimals;
                
                if value == 0 {
                    decimals = 0;
                }
            }
            Some(Self{value: value as $value_type, decimals})
        }

        pub fn checked_add(self, other: Self) -> Option<Self> {
            let (v1, v2, decimals) = Self::equalize_decimals(self, other);
            Self::shift_to_fit(v1 + v2, decimals)
        }

        pub fn checked_sub(self, other: Self) -> Option<Self> {
            let (v1, v2, decimals) = Self::equalize_decimals(self, other);
            if v1 < v2 {
                None
            }
            else {
                Self::shift_to_fit(v1 - v2, decimals)
            }
        }

        pub fn checked_mul(self, other: Self) -> Option<Self> {
            let value = self.value as $larger_type * other.value as $larger_type;
            let decimals = self.decimals + other.decimals;
            Self::shift_to_fit(value, decimals)
        }

        pub fn checked_div(self, other: Self) -> Option<Self> {
            if other.value == 0 {
                return None;
            }

            let numerator = self.value as $larger_type;
            let denominator = other.value as $larger_type;
            let upshift = Self::get_unused_decimals(numerator);
            let upshifted_num = numerator * ten_to_the(upshift);
            let quotient = upshifted_num / denominator;
            let decimals = self.decimals + upshift - other.decimals;
            Self::shift_to_fit(quotient, decimals)
        }
    }
}}

macro_rules! impl_interop{(
    $name:ident,
    $larger_name:ident,
    $larger_type:ty
) => {
impl $name {
    pub fn upcast_mul(self, other: Self) -> $larger_name {
        $larger_name::new(self.value as $larger_type * other.value as $larger_type, self.decimals + other.decimals).unwrap()
    }
}

impl From<$name> for $larger_name {
    fn from(v: $name) -> Self {
        Self{value: v.get_raw() as $larger_type, decimals: v.get_decimals()}
    }
}

impl TryFrom<$larger_name> for $name {
    type Error = DecimalError;

    fn try_from(v: $larger_name) -> Result<Self, Self::Error> {
        Self::shift_to_fit(v.get_raw(), v.get_decimals()).ok_or(DecimalError::ConversionError)
    }
}

}}

unsigned_decimal! {DecimalU128, u128, 128, 38}
impl DecimalU128 {
    impl_unused_decimal_bounds!(u128, 128);

    fn equalize_decimals(v1: Self, v2: Self) -> (u128, u128, u8) {
        if v1.decimals == v2.decimals {
            //special handling to optimize and simplify typical case
            (v1.value, v2.value, v1.decimals)
        }
        else {
            let v1_has_fewer_decimals = v1.decimals < v2.decimals;
            let (fewer_dec, more_dec) = if v1_has_fewer_decimals {(&v1, &v2)} else {(&v2, &v1)};
            let dec_diff = more_dec.decimals - fewer_dec.decimals;
            let shift = cmp::min(Self::get_unused_decimals(fewer_dec.value), dec_diff);
            let shifted_fewer_value = fewer_dec.value * ten_to_the(shift);
            let shifted_more_value = more_dec.value / ten_to_the(dec_diff-shift);

            if v1_has_fewer_decimals {
                (shifted_fewer_value, shifted_more_value, fewer_dec.decimals + shift)
            }
            else {
                (shifted_more_value, shifted_fewer_value, fewer_dec.decimals + shift)
            }
        }
    }

    fn fix_fractional_decimal_excess(mut value: u128, mut decimals: u8) -> Self {
        if let Some(shift) = decimals.checked_sub(Self::MAX_DECIMALS) {
            //check required to prevent index overrun in ten_to_the
            if shift > Self::MAX_DECIMALS {
                return Self::from(0);
            }
            value /= ten_to_the(shift);
            decimals = Self::MAX_DECIMALS;
        }
        
        if value == 0 {
            decimals = 0;
        }

        Self{value, decimals}
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        
        let (val_1, val_2, decimals) = Self::equalize_decimals(self, other);
        match val_1.checked_add(val_2) {
            Some(value) => Some(Self{value, decimals}),
            None => {
                let value = (val_1/10 + val_2/10) + (val_1%10 + val_2%10)/10;
                Some(Self{value, decimals: decimals-1})
            }
        }
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        let (val_1, val_2, decimals) = Self::equalize_decimals(self, other);
        if val_1 < val_2 {
            Some(Self{value: val_1 - val_2, decimals})
        }
        else {
            None
        }
    }

    //TODO only 64 bit precision!
    pub fn checked_mul(self, other: Self) -> Option<Self> {
        //extra handling to prevent retrieving oom for 0 value
        if self.value == 0 || other.value == 0 {
            return Some(Self::from(0));
        }
        let v1 = self.normalize();
        let v2 = other.normalize();

        let v1_oom = Self::get_order_of_magnitude(v1.value);
        let v2_oom = Self::get_order_of_magnitude(v2.value);

        let full_precision = v1_oom + v2_oom;
        let full_decimals = v1.decimals + v2.decimals;

        if full_precision <= Self::MAX_DECIMALS {
            //guaranteed fit
            return Some(Self::fix_fractional_decimal_excess(v1.value * v2.value, full_decimals));
        }

        if full_precision > Self::MAX_DECIMALS + full_decimals + 2 {
            //guaranteed overflow
            return None;
        }

        let excess_oom = full_precision - (Self::MAX_DECIMALS + 1);
        let (fewer_oom, mut fewer_val,more_oom, mut more_val) =
            if v1_oom < v2_oom {(v1_oom, self.value, v2_oom, other.value)}
            else {(v2_oom, other.value, v1_oom, self.value)};
        
        let oom_diff = more_oom - fewer_oom;
        if excess_oom <= oom_diff {
            more_val /= ten_to_the(excess_oom);
        }
        else {
            let split_excess = excess_oom - oom_diff;
            fewer_val /= ten_to_the((split_excess+1)/2);
            more_val /= ten_to_the(oom_diff + split_excess/2);
        }

        let mut decimals = full_decimals - excess_oom;
        let product = match more_val.checked_mul(fewer_val) {
            Some(value) => value,
            None => {
                if decimals == 0 {
                    return None;
                }
                decimals -= 1;
                match (more_val/10).checked_mul(fewer_val){
                    Some(value) => value,
                    None => {
                        if decimals == 0 {
                            return None;
                        }
                        decimals -= 1;
                        (more_val/100) * fewer_val
                    }
                }
            }
        };
        Some(Self::fix_fractional_decimal_excess(product, decimals))
    }

    //TODO only 64 bit precision! implement a fast division method ( https://en.wikipedia.org/wiki/Division_algorithm#Fast_division_methods )
    pub fn checked_div(self, other: Self) -> Option<Self> {
        if other.value == 0 {
            return None;
        }

        let other = other.normalize();

        //shift denominator so it uses half the available space in u128 to get maximum precision
        //this ensures the maximum precision of the final result
        let denom_downshift = Self::get_order_of_magnitude(other.value).checked_sub(Self::MAX_DECIMALS/2).unwrap_or(0);

        //shift numerator up so that it uses all available space in u128
        //this ensures the greatest possible precision after the division
        let num_upshift = Self::get_unused_decimals(self.value);

        let decimals = (self.decimals + num_upshift + denom_downshift).checked_sub(other.decimals)?;

        let numerator = self.value * ten_to_the(num_upshift);
        let denominator = other.value / ten_to_the(denom_downshift);
        let value = numerator / denominator;

        //TODO by more intelligently calculating denom_downshift and num_upshift,
        //     one can get rid of this final adjustment
        Some(Self::fix_fractional_decimal_excess(value, decimals))
    }
}

unsigned_decimal! {DecimalU64, u64, 64, 19}
impl_checked_math!{DecimalU64, u64, u128, 38}
impl_interop!{DecimalU64, DecimalU128, u128}
// unsigned_decimal! {DecimalU32, u32, 32, 9}
// impl_checked_math!{DecimalU32, u32, DecimalU64, u64, 19}
// unsigned_decimal! {DecimalU16, u16, 16, 4}
// impl_checked_math!{DecimalU16, u16, DecimalU32, u32, 9}
// unsigned_decimal! {DecimalU8, u8, 8, 2}
// impl_checked_math!{DecimalU8, u8, DecimalU16, u16, 4}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_test() {
        // let new_u8 = |value, decimals| DecimalU8::new(value, decimals).unwrap();
        // assert_eq!(new_u8(111,2) + new_u8(11,1), new_u8(221,2));
        // assert_eq!(new_u8(127,0) + new_u8(128,0), new_u8(255,0));
        // assert_eq!(new_u8(127,2) + new_u8(128,2), new_u8(255,2));
        // assert_eq!(new_u8(1,1) * new_u8(1,1), new_u8(1,2));
        // assert_eq!(new_u8(1,0) / new_u8(3,0), new_u8(33,2));
        let new_u64 = |value, decimals| DecimalU64::new(value, decimals).unwrap();
        let pi = new_u64(31415,4);
        assert_eq!(pi.ceil(0), new_u64(4,0));
        assert_eq!(pi.ceil(1), new_u64(32,1));
        assert_eq!(pi.ceil(2), new_u64(315,2));
        assert_eq!(pi.ceil(3), new_u64(3142,3));
        assert_eq!(pi.ceil(4), pi);
        assert_eq!(pi.round(0), new_u64(3,0));
        assert_eq!(pi.round(1), new_u64(31,1));
        assert_eq!(pi.round(2), new_u64(314,2));
        assert_eq!(pi.round(3), new_u64(3142,3));
        assert_eq!(pi.round(4), pi);
        assert_eq!(pi.floor(0), new_u64(3,0));
        assert_eq!(pi.floor(1), new_u64(31,1));
        assert_eq!(pi.floor(2), new_u64(314,2));
        assert_eq!(pi.floor(3), new_u64(3141,3));
        assert_eq!(pi.floor(4), pi);
    }

    #[test]
    fn u128_mul() {
        let new_u128 = |value, decimals| DecimalU128::new(value, decimals).unwrap();
        assert_eq!(DecimalU128::from(1)            * DecimalU128::from(1)       , DecimalU128::from(1));
        assert_eq!(new_u128(u128::MAX,0)           * new_u128(1,1)              , new_u128(u128::MAX,1));
        assert_eq!(new_u128(u128::MAX,0).checked_mul(new_u128(10,0))            , None);
        // assert_eq!(new_u128(u128::MAX,38)          * new_u128(10u128.pow(10)+1,0) , new_u128(u128::MAX,28));
        // assert_eq!(new_u128(10u128.pow(10),0)      * new_u128(10u128.pow(10),0) , new_u128(10u128.pow(20),0));
    }

    #[test]
    fn u128_div() {
        let new_u128 = |value, decimals| DecimalU128::new(value, decimals).unwrap();
        assert_eq!(DecimalU128::from(u128::MAX) / DecimalU128::from(u128::MAX), DecimalU128::from(1));
        assert!(new_u128(u128::MAX,0).checked_div(new_u128(u128::MAX,38)).is_none());
        assert_eq!(new_u128(u128::MAX,38) / new_u128(u128::MAX,0), new_u128(1, 38));
        assert_eq!(new_u128(u128::MAX,38) / new_u128(u128::MAX,38), DecimalU128::from(1));
        assert_eq!(new_u128(1,38) / new_u128(u128::MAX,0), DecimalU128::from(0));
        assert!(new_u128(u128::MAX,0).checked_div(new_u128(1,1)).is_none());
    }

    #[test]
    fn get_order_of_magnitude() {
        let mut test_oom = 0u8;
        let mut pot = 10u128;
        for i in 1..2000u128 {
            if i / pot != 0 {
                test_oom += 1;
                pot *= 10;
            }
            let calc_oom = DecimalU128::get_order_of_magnitude(i);
            assert_eq!(calc_oom, test_oom, "for {} got order of magnitude of {} instead of {}", i, calc_oom, test_oom);
        }
    }

    // #[test]
    // fn print_bounds() {
    //     print!("BIT_TO_DEC_ARRAY:");
    //     for i in 0..BIT_TO_DEC_SIZE {
    //         print!("{},", BIT_TO_DEC_ARRAY[i]);
    //     }
    // }
}