// This module is essentially a poor man's decimal (using base 10), unsigned only, floating point
// implementation with a limit on the decimal point being within at most MAX_DECIMALS
// places away from 0 (e.g. DecimalU8 can represent values from 0.00 to 2.55 in 0.01 increments,
// from 0.0 to 25.5 in 0.1 increments, or the default u8 range of 0 to 255)
//
// The efficiency of this module depends on the efficiency of unsigned.leading_zeros(). On an x86 cpu
// this ought to be a single instruction, but this might not be the case for other architectures.
//
// All math in this module is implemented in such a way that all operations that *aren't*
// don't explicitly use checked_* (i.e. all the inline ops like +,-,*,/,%, etc.) should never
// be able to fail and could hence be replaced by unsafe_* calls to reduce strain on compute budget.

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use std::{
    cmp,
    cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd},
    convert::TryFrom,
    fmt,
    fmt::{Display, Formatter},
    io,
    iter::{Product, Sum},
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign},
};
use thiserror::Error;

use uint::construct_uint;
construct_uint! {
    #[derive(BorshSerialize, BorshDeserialize, BorshSchema)]
    pub struct U128(2);
}

impl U128 {
    pub const fn const_from(value: u128) -> Self {
        let mut ret = [0; 2];
        ret[0] = value as u64;
        ret[1] = (value >> 64) as u64;
        Self(ret)
    }

    pub const fn ten_to_the(exp: u8) -> Self {
        Self::const_from(TEN_TO_THE[exp as usize])
    }
}

construct_uint! {
    pub struct U256(4);
}

#[derive(Error, Debug)]
pub enum DecimalError {
    #[error("Maximum decimals exceeded")]
    MaxDecimalsExceeded,
    #[error("Conversion error")]
    ConversionError,
}

pub const fn ten_to_the(exp: u8) -> u128 {
    TEN_TO_THE[exp as usize]
}
const U128_MAX_DECIMALS: usize = 39;
const TEN_TO_THE: [u128; U128_MAX_DECIMALS] = create_ten_to_the();
const fn create_ten_to_the() -> [u128; U128_MAX_DECIMALS] {
    let mut ttt = [1 as u128; U128_MAX_DECIMALS];
    let mut i = 1;
    loop {
        //const functions can't use for loops
        if i == U128_MAX_DECIMALS {
            break;
        }
        ttt[i] = 10 * ttt[i - 1];
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
const BIT_TO_DEC_SIZE: usize = 128 + 2; //u128::BITS is still an unstable feature
const BIT_TO_DEC_ARRAY: [u8; BIT_TO_DEC_SIZE] = create_bit_to_dec_array();
const fn create_bit_to_dec_array() -> [u8; BIT_TO_DEC_SIZE] {
    let mut btd = [0; BIT_TO_DEC_SIZE];
    let mut pot: u128 = 10;
    let mut i = 1 as usize; //we start with the second iteration
    loop {
        //const functions can't use for loops
        let jump = ((1 << i as u128) / pot) as u8;
        if jump == 1 {
            pot = match pot.checked_mul(10) {
                Some(v) => v,
                None => {
                    btd[i] = btd[i - 1] + 1;
                    loop {
                        i += 1;
                        if i >= BIT_TO_DEC_SIZE {
                            return btd;
                        }
                        btd[i] = btd[i - 1];
                    }
                }
            }
        }
        btd[i] = btd[i - 1] + jump;
        i += 1;
    }
}

macro_rules! unsigned_decimal {
    (
    $name:ident,
    $convert:ident,
    $upcast:ident,
    $downcast:ident,
    $value_type:ty,
    $larger_type:ty,
    $bits:expr, //<$value_type>::BITS is still unstable
    $max_decimals:expr $(,)? //floor(log_10(2^bits-1))
) => {
        #[derive(BorshSerialize, BorshSchema, Debug, Clone, Copy, Default)]
        pub struct $name {
            value: $value_type,
            decimals: u8,
        }

        impl $name {
            pub const BITS: u32 = $bits;
            pub const MAX_DECIMALS: u8 = $max_decimals;

            fn ten_to_the_value_type(exp: u8) -> $value_type {
                $convert!(ten_to_the(exp), $value_type)
            }

            //TODO this is unnecessarily wasteful
            fn ten_to_the_larger_type(exp: u8) -> $larger_type {
                if exp <= Self::MAX_DECIMALS {
                    $upcast!(Self::ten_to_the_value_type(exp), $larger_type)
                } else {
                    $upcast!(Self::ten_to_the_value_type(Self::MAX_DECIMALS), $larger_type)
                        * $upcast!(
                            Self::ten_to_the_value_type(exp - Self::MAX_DECIMALS),
                            $larger_type
                        )
                }
            }

            fn get_unused_decimals(value: $value_type) -> u8 {
                let leading_zeros = value.leading_zeros() as usize;
                let lower_bound = BIT_TO_DEC_ARRAY[leading_zeros];
                let upper_bound = BIT_TO_DEC_ARRAY[leading_zeros + 1];
                if lower_bound == upper_bound
                    || value
                        .checked_mul(Self::ten_to_the_value_type(upper_bound))
                        .is_none()
                {
                    lower_bound
                } else {
                    upper_bound
                }
            }

            fn get_order_of_magnitude(value: $value_type) -> u8 {
                debug_assert!(value != $convert!(0, $value_type));
                let used_bits = $bits as usize - value.leading_zeros() as usize;
                let lower_bound = BIT_TO_DEC_ARRAY[used_bits - 1];
                let upper_bound = BIT_TO_DEC_ARRAY[used_bits];
                if lower_bound == upper_bound
                    || value / Self::ten_to_the_value_type(lower_bound) < $convert!(10, $value_type)
                {
                    lower_bound
                } else {
                    upper_bound
                }
            }

            pub const fn new(value: $value_type, decimals: u8) -> Result<Self, DecimalError> {
                if decimals > Self::MAX_DECIMALS {
                    return Err(DecimalError::MaxDecimalsExceeded);
                }
                Ok(Self { value, decimals })
            }

            //workaround because From trait's from function isn't const...
            pub const fn const_from(value: $value_type) -> Self {
                Self { value, decimals: 0 }
            }

            pub const fn get_raw(&self) -> $value_type {
                self.value
            }

            pub const fn get_decimals(&self) -> u8 {
                self.decimals
            }

            pub fn trunc(&self) -> $value_type {
                self.value / Self::ten_to_the_value_type(self.decimals)
            }

            pub fn fract(&self) -> $value_type {
                self.value % Self::ten_to_the_value_type(self.decimals)
            }

            pub fn ceil(&self, decimals: u8) -> Self {
                let mut ret = self.clone();
                if decimals < ret.decimals {
                    let pot = Self::ten_to_the_value_type(ret.decimals - decimals);
                    let up = $convert!(
                        if (ret.value % pot > $convert!(0, $value_type)) {
                            1
                        } else {
                            0
                        },
                        $value_type
                    );
                    ret.value /= pot;
                    ret.value += up;
                    ret.decimals = decimals;
                }
                ret
            }

            pub fn round(&self, decimals: u8) -> Self {
                let mut ret = self.clone();
                if decimals < ret.decimals {
                    let pot = Self::ten_to_the_value_type(ret.decimals - decimals);
                    let up = $convert!(
                        if (ret.value % pot) / (pot / 10) >= $convert!(5, $value_type) {
                            1
                        } else {
                            0
                        },
                        $value_type
                    );
                    ret.value /= pot;
                    ret.value += up;
                    ret.decimals = decimals;
                }
                ret
            }

            pub fn floor(&self, decimals: u8) -> Self {
                let mut ret = self.clone();
                if decimals < ret.decimals {
                    ret.value /= Self::ten_to_the_value_type(ret.decimals - decimals);
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
                    let next_dec = (dec + 1) / 2;
                    if self.value % Self::ten_to_the_value_type(shift + dec) == $convert!(0, $value_type) {
                        shift += next_dec;
                        dec -= next_dec;
                    } else {
                        dec = next_dec;
                    }
                    if dec <= 1 {
                        break;
                    }
                }
                Self {
                    value: self.value / Self::ten_to_the_value_type(shift),
                    decimals: self.decimals - shift,
                }
            }

            fn equalize_decimals(v1: Self, v2: Self) -> ($larger_type, $larger_type, u8) {
                let v1_val = $upcast!(v1.value, $larger_type);
                let v2_val = $upcast!(v2.value, $larger_type);
                match v1.decimals.cmp(&v2.decimals) {
                    Ordering::Equal => (v1_val, v2_val, v1.decimals),
                    Ordering::Less => (
                        v1_val * Self::ten_to_the_larger_type(v2.decimals - v1.decimals),
                        v2_val,
                        v2.decimals,
                    ),
                    Ordering::Greater => (
                        v1_val,
                        v2_val * Self::ten_to_the_larger_type(v1.decimals - v2.decimals),
                        v1.decimals,
                    ),
                }
            }

            //shifts the passed value so it fits within Self
            fn shift_to_fit(mut value: $larger_type, mut decimals: u8) -> Option<Self> {
                //space exceeded ensures that value/ten_to_the(excess_decimals) fits within $value_type
                let space_exceeded = if value > $upcast!(<$value_type>::MAX, $larger_type) {
                    1 + Self::get_order_of_magnitude($downcast!(value >> Self::BITS, $value_type))
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
                    value /= Self::ten_to_the_larger_type(down_shift_decimals);
                    decimals -= down_shift_decimals;

                    if $downcast!(value, $value_type) == $convert!(0, $value_type) {
                        decimals = 0;
                    }
                }
                Some(Self {
                    value: $downcast!(value, $value_type),
                    decimals,
                })
            }

            pub fn checked_add(self, other: Self) -> Option<Self> {
                let (v1, v2, decimals) = Self::equalize_decimals(self, other);
                Self::shift_to_fit(v1 + v2, decimals)
            }

            pub fn checked_sub(self, other: Self) -> Option<Self> {
                let (v1, v2, decimals) = Self::equalize_decimals(self, other);
                if v1 < v2 {
                    None
                } else {
                    Self::shift_to_fit(v1 - v2, decimals)
                }
            }

            pub fn checked_mul(self, other: Self) -> Option<Self> {
                let value = $upcast!(self.value, $larger_type) * $upcast!(other.value, $larger_type);
                let decimals = self.decimals + other.decimals;
                Self::shift_to_fit(value, decimals)
            }

            pub fn checked_div(self, other: Self) -> Option<Self> {
                if other.value == $convert!(0, $value_type) {
                    return None;
                }

                let numerator = $upcast!(self.value, $larger_type);
                let denominator = $upcast!(other.value, $larger_type);
                //upshift might not shift all the way to the top but it's not a problem:
                //for example: u32 has 9 max decimals while u64 has 19
                //in this case, upshift can be at most 18 (decimals)
                //however while we are not using the full range, since we only ultimately
                //need 9 decimals the "wasted space" is not an issue
                let upshift = Self::get_unused_decimals(self.value) + Self::MAX_DECIMALS;
                let upshifted_num = numerator * Self::ten_to_the_larger_type(upshift);
                let quotient = upshifted_num / denominator;
                let decimals = self.decimals + upshift - other.decimals;
                Self::shift_to_fit(quotient, decimals)
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                let normalized = self.normalize();
                let fract = normalized.fract();
                if fract == $convert!(0, $value_type) {
                    write!(f, "{}", self.trunc())
                } else {
                    write!(
                        f,
                        "{}.{:0decimals$}",
                        self.trunc(),
                        fract,
                        decimals = normalized.decimals as usize
                    )
                }
            }
        }

        impl BorshDeserialize for $name {
            fn deserialize(buf: &mut &[u8]) -> Result<Self, io::Error> {
                let value = <$value_type>::deserialize(buf)?;
                let decimals = <u8>::deserialize(buf)?;
                if decimals > Self::MAX_DECIMALS {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("decimals value out of bounds: {}", decimals),
                    ))
                } else {
                    Ok(Self { value, decimals })
                }
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.trunc() == other.trunc()
                    && match self.decimals.cmp(&other.decimals) {
                        Ordering::Equal => self.fract() == other.fract(),
                        Ordering::Less => {
                            self.fract() * Self::ten_to_the_value_type(other.decimals - self.decimals) == other.fract()
                        }
                        Ordering::Greater => {
                            self.fract() == other.fract() * Self::ten_to_the_value_type(self.decimals - other.decimals)
                        }
                    }
            }
        }

        impl PartialEq<$value_type> for $name {
            fn eq(&self, other: &$value_type) -> bool {
                self.trunc() == *other && self.fract() == $convert!(0, $value_type)
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
                Some(self.cmp(&Self {
                    value: *other,
                    decimals: 0,
                }))
            }
        }

        impl PartialOrd<$name> for $value_type {
            fn partial_cmp(&self, other: &$name) -> Option<Ordering> {
                Some(
                    $name {
                        value: *self,
                        decimals: 0,
                    }
                    .cmp(other),
                )
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> Ordering {
                let cmp = self.trunc().cmp(&other.trunc());
                match cmp {
                    Ordering::Equal => match self.decimals.cmp(&other.decimals) {
                        Ordering::Equal => self.fract().cmp(&other.fract()),
                        Ordering::Less => (self.fract() * Self::ten_to_the_value_type(other.decimals - self.decimals))
                            .cmp(&other.fract()),
                        Ordering::Greater => self
                            .fract()
                            .cmp(&(other.fract() * Self::ten_to_the_value_type(self.decimals - other.decimals))),
                    },
                    _ => cmp,
                }
            }
        }

        impl From<$value_type> for $name {
            fn from(value: $value_type) -> Self {
                Self { value, decimals: 0 }
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
                self + Self {
                    value: other,
                    decimals: 0,
                }
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
            where
                I: Iterator<Item = Self>,
            {
                iter.fold(
                    Self {
                        value: $convert!(0, $value_type),
                        decimals: 0,
                    },
                    |accumulator, it| accumulator + it,
                )
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
                self - Self {
                    value: other,
                    decimals: 0,
                }
            }
        }

        impl Sub<$name> for $value_type {
            type Output = $name;

            fn sub(self, other: $name) -> Self::Output {
                $name {
                    value: self,
                    decimals: 0,
                } - other
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
                self * Self {
                    value: other,
                    decimals: 0,
                }
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
            where
                I: Iterator<Item = Self>,
            {
                iter.fold(
                    Self {
                        value: $convert!(1, $value_type),
                        decimals: 0,
                    },
                    |accumulator, it| accumulator * it,
                )
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
                self / Self {
                    value: other,
                    decimals: 0,
                }
            }
        }

        impl Div<$name> for $value_type {
            type Output = $name;

            fn div(self, other: $name) -> Self::Output {
                $name {
                    value: self,
                    decimals: 0,
                } / other
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
    };
}

impl From<u128> for DecimalU128 {
    fn from(value: u128) -> Self {
        Self {
            value: U128::from(value),
            decimals: 0,
        }
    }
}

macro_rules! to_uint128 {
    (
    $value:expr,
    $_:ty $(,)?
) => {
        U128::from($value)
    };
}

macro_rules! from_uint128 {
    (
    $value:expr,
    $_:ty $(,)?
) => {
        $value.as_u64()
    };
}

macro_rules! normal_cast {
    (
    $value:expr,
    $builtin_type:ty $(,)?
) => {
        $value as $builtin_type
    };
}

macro_rules! uint_cross_cast {
    (
    $value:expr,
    $uint_type:ty $(,)?
) => {
        <$uint_type>::from($value.as_u128())
    };
}

unsigned_decimal! {DecimalU128, to_uint128, uint_cross_cast, uint_cross_cast, U128, U256, 128, 38}
unsigned_decimal! {DecimalU64, normal_cast, to_uint128, from_uint128, u64, U128, 64, 19}
//impl_interop!{DecimalU64, DecimalU128, u128}
// unsigned_decimal! {DecimalU32, u32, 32, 9}
// impl_checked_math!{DecimalU32, u32, DecimalU64, u64, 19}
// unsigned_decimal! {DecimalU16, u16, 16, 4}
// impl_checked_math!{DecimalU16, u16, DecimalU32, u32, 9}
// unsigned_decimal! {DecimalU8, u8, 8, 2}
// impl_checked_math!{DecimalU8, u8, DecimalU16, u16, 4}

macro_rules! impl_interop {
    (
    $name:ident,
    $larger_name:ident,
    $upcast:ident,
    $larger_type:ty $(,)?
) => {
        impl $name {
            pub fn upcast_mul(self, other: Self) -> $larger_name {
                $larger_name::new(
                    $upcast!(self.value, $larger_type) * $upcast!(other.value, $larger_type),
                    self.decimals + other.decimals,
                )
                .unwrap()
            }
        }

        impl From<$name> for $larger_name {
            fn from(v: $name) -> Self {
                Self {
                    value: $upcast!(v.get_raw(), $larger_type),
                    decimals: v.get_decimals(),
                }
            }
        }

        impl TryFrom<$larger_name> for $name {
            type Error = DecimalError;

            fn try_from(v: $larger_name) -> Result<Self, Self::Error> {
                Self::shift_to_fit(v.get_raw(), v.get_decimals()).ok_or(DecimalError::ConversionError)
            }
        }
    };
}

impl_interop! {DecimalU64, DecimalU128, to_uint128, U128}

#[cfg(all(test, not(feature = "test-bpf")))]
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
        let pi = new_u64(31415, 4);
        assert_eq!(pi.ceil(0), new_u64(4, 0));
        assert_eq!(pi.ceil(1), new_u64(32, 1));
        assert_eq!(pi.ceil(2), new_u64(315, 2));
        assert_eq!(pi.ceil(3), new_u64(3142, 3));
        assert_eq!(pi.ceil(4), pi);
        assert_eq!(pi.round(0), new_u64(3, 0));
        assert_eq!(pi.round(1), new_u64(31, 1));
        assert_eq!(pi.round(2), new_u64(314, 2));
        assert_eq!(pi.round(3), new_u64(3142, 3));
        assert_eq!(pi.round(4), pi);
        assert_eq!(pi.floor(0), new_u64(3, 0));
        assert_eq!(pi.floor(1), new_u64(31, 1));
        assert_eq!(pi.floor(2), new_u64(314, 2));
        assert_eq!(pi.floor(3), new_u64(3141, 3));
        assert_eq!(pi.floor(4), pi);
    }

    #[test]
    fn u128_mul() {
        let new_u128 = |value, decimals| DecimalU128::new(U128::from(value), decimals).unwrap();
        assert_eq!(DecimalU128::from(1) * DecimalU128::from(1), DecimalU128::from(1));
        assert_eq!(new_u128(u128::MAX, 0) * new_u128(1, 1), new_u128(u128::MAX, 1));
        assert_eq!(new_u128(u128::MAX, 0).checked_mul(new_u128(10, 0)), None);
        assert_eq!(
            new_u128(u128::MAX, 38) * new_u128(10u128.pow(10), 0),
            new_u128(u128::MAX, 28)
        );
        assert_eq!(
            new_u128(10u128.pow(10), 0) * new_u128(10u128.pow(10), 0),
            new_u128(10u128.pow(20), 0)
        );
    }

    #[test]
    fn u128_div() {
        let new_u128 = |value, decimals| DecimalU128::new(U128::from(value), decimals).unwrap();
        assert_eq!(
            DecimalU128::from(u128::MAX) / DecimalU128::from(u128::MAX),
            DecimalU128::from(1)
        );
        assert_eq!(
            new_u128(u128::MAX, 0) / new_u128(u128::MAX, 38),
            new_u128(10u128.pow(38), 0)
        );
        assert_eq!(new_u128(u128::MAX, 38) / new_u128(u128::MAX, 0), new_u128(1, 38));
        assert_eq!(new_u128(u128::MAX, 38) / new_u128(u128::MAX, 38), DecimalU128::from(1));
        assert_eq!(new_u128(1, 38) / new_u128(u128::MAX, 0), DecimalU128::from(0));
        assert!(new_u128(u128::MAX, 0).checked_div(new_u128(1, 1)).is_none());
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
            let calc_oom = DecimalU128::get_order_of_magnitude(U128::from(i));
            assert_eq!(
                calc_oom, test_oom,
                "for {} got order of magnitude of {} instead of {}",
                i, calc_oom, test_oom
            );
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
