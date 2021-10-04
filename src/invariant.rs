// see doc/swap_invariants.ipynb for an explanation on the math in here

//TODO it should be possible to get rid of the duplicated code and the ugly encoding of the type in the
//     function name by properly using generics... but I couldn't figure out how in a reasonable amount
//     of time (the num_traits crate only got me so far...)

use crate::{decimal::DecimalU64, error::PoolError};

use std::{
    ops::{Add, Sub},
    vec::Vec,
};

use arrayvec::ArrayVec;
use uint::construct_uint;
construct_uint! {
    pub struct U256(4);
}

type InvariantResult<T> = Result<T, PoolError>;

type AmountT = u64;
type AmpT = u64;
type FeeT = u64;
type DecT = DecimalU64;

const AMP_PRECISION: u8 = 3;
const FEE_PRECISION: u8 = 6;
const AMP_MULTIPLIER: u64 = ten_to_the(AMP_PRECISION);
const FEE_MULTIPLIER: u64 = ten_to_the(FEE_PRECISION);

const fn ten_to_the(precision: u8) -> u64 {
    10u64.pow(precision as u32)
}

impl U256 {
    const SHIFT_MULTIPLIER: Self = Self([0, 1, 0, 0]);

    fn shift_down(&self) -> Self {
        Self([self.0[1], self.0[2], self.0[3], 0])
    }

    fn shift_up(&self) -> Self {
        debug_assert!(self.0[3] == 0, "overflow in U256 while trying to upshift {}", self);
        Self([0, self.0[0], self.0[1], self.0[2]])
    }

    fn rounded_div_u64(&self, denominator: u64) -> Self {
        (self + denominator / 2) / denominator
    }

    fn rounded_div(&self, denominator: Self) -> Self {
        (self + denominator / 2) / denominator
    }

    fn rounded_mul_div(amount: AmountT, numerator: Self, denominator: Self) -> u64 {
        (numerator * amount).rounded_div(denominator).as_u64()
    }

    fn sub_given_order(keep_order: bool, v1: Self, v2: Self) -> Self {
        if keep_order {
            v1 - v2
        } else {
            v2 - v1
        }
    }
}

fn rounded_mul_div(amount: AmountT, numerator: u64, denominator: u64) -> u64 {
    (U256::from(amount) * numerator).rounded_div_u64(denominator).as_u64()
}

fn from_decimal_with_precision(decimal: DecT, precision: u8) -> u64 {
    let rounded = decimal.round(precision);
    rounded.get_raw() * ten_to_the(precision - rounded.get_decimals())
}

fn abs_difference(v1: U256, v2: U256) -> U256 {
    if v1 > v2 {
        v1 - v2
    } else {
        v2 - v1
    }
}

fn sub_given_order(keep_order: bool, v1: AmountT, v2: AmountT) -> AmountT {
    if keep_order {
        v1 - v2
    } else {
        v2 - v1
    }
}

fn exclude_index<const TOKEN_COUNT: usize>(index: usize, array: &[AmountT; TOKEN_COUNT]) -> Vec<AmountT> {
    array
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, v)| *v)
        .collect::<Vec<AmountT>>()
}

fn sum_balances<const TOKEN_COUNT: usize>(balances: &[AmountT; TOKEN_COUNT]) -> U256 {
    balances.iter().fold(U256::zero(), |acc, &balance| acc + balance)
}

fn binary_op_balances<const TOKEN_COUNT: usize>(
    op: impl Fn(AmountT, AmountT) -> AmountT,
    balances1: &[AmountT; TOKEN_COUNT],
    balances2: &[AmountT; TOKEN_COUNT],
) -> [AmountT; TOKEN_COUNT] {
    balances1
        .iter()
        .zip(balances2.iter())
        .map(|(&v1, &v2)| op(v1, v2))
        .collect::<ArrayVec<_, TOKEN_COUNT>>()
        .into_inner()
        .unwrap()
}

fn unary_op_balances<const TOKEN_COUNT: usize>(
    op: impl Fn(AmountT) -> AmountT,
    balances: &[AmountT; TOKEN_COUNT],
) -> [AmountT; TOKEN_COUNT] {
    balances
        .iter()
        .map(|&v| op(v))
        .collect::<ArrayVec<_, TOKEN_COUNT>>()
        .into_inner()
        .unwrap()
}

pub struct Invariant<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Invariant<TOKEN_COUNT> {
    pub fn add(
        input_amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        let amp_factor = from_decimal_with_precision(amp_factor, AMP_PRECISION);
        if lp_total_supply == 0 {
            Ok((Self::internal_calculate_depth(&input_amounts, amp_factor)?.as_u64(), 0))
        } else {
            let lp_fee = from_decimal_with_precision(lp_fee, FEE_PRECISION);
            let governance_fee = from_decimal_with_precision(governance_fee, FEE_PRECISION);
            let total_fee = lp_fee + governance_fee;
            Self::add_remove(
                true,
                &input_amounts,
                &pool_balances,
                amp_factor,
                total_fee,
                governance_fee,
                lp_total_supply,
            )
        }
    }

    pub fn swap_exact_input(
        input_amounts: &[AmountT; TOKEN_COUNT],
        output_index: usize,
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        let amp_factor = from_decimal_with_precision(amp_factor, AMP_PRECISION);
        let lp_fee = from_decimal_with_precision(lp_fee, FEE_PRECISION);
        let governance_fee = from_decimal_with_precision(governance_fee, FEE_PRECISION);
        let total_fee = lp_fee + governance_fee;
        Self::swap(
            true,
            &input_amounts,
            output_index,
            &pool_balances,
            amp_factor,
            total_fee,
            governance_fee,
            lp_total_supply,
        )
    }

    pub fn swap_exact_output(
        input_index: usize,
        output_amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        let amp_factor = from_decimal_with_precision(amp_factor, AMP_PRECISION);
        let lp_fee = from_decimal_with_precision(lp_fee, FEE_PRECISION);
        let governance_fee = from_decimal_with_precision(governance_fee, FEE_PRECISION);
        let total_fee = lp_fee + governance_fee;
        Self::swap(
            false,
            &output_amounts,
            input_index,
            &pool_balances,
            amp_factor,
            total_fee,
            governance_fee,
            lp_total_supply,
        )
    }

    pub fn remove_exact_burn(
        burn_amount: AmountT,
        output_index: usize,
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        let amp_factor = from_decimal_with_precision(amp_factor, AMP_PRECISION);
        let lp_fee = from_decimal_with_precision(lp_fee, FEE_PRECISION);
        let governance_fee = from_decimal_with_precision(governance_fee, FEE_PRECISION);
        let total_fee = lp_fee + governance_fee;
        Self::internal_remove_exact_burn(
            burn_amount,
            output_index,
            &pool_balances,
            amp_factor,
            total_fee,
            governance_fee,
            lp_total_supply,
        )
    }

    pub fn remove_exact_output(
        output_amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        let amp_factor = from_decimal_with_precision(amp_factor, AMP_PRECISION);
        let lp_fee = from_decimal_with_precision(lp_fee, FEE_PRECISION);
        let governance_fee = from_decimal_with_precision(governance_fee, FEE_PRECISION);
        let total_fee = lp_fee + governance_fee;
        Self::add_remove(
            false,
            &output_amounts,
            &pool_balances,
            amp_factor,
            total_fee,
            governance_fee,
            lp_total_supply,
        )
    }

    pub fn calculate_depth(pool_balances: &[AmountT; TOKEN_COUNT], amp_factor: DecT) -> AmountT {
        let amp_factor = from_decimal_with_precision(amp_factor, AMP_PRECISION);
        Self::internal_calculate_depth(&pool_balances, amp_factor)
            .unwrap()
            .as_u64()
    }

    fn swap(
        is_exact_input: bool, //false => exact output
        amounts: &[AmountT; TOKEN_COUNT],
        index: usize,
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: AmpT,
        total_fee: FeeT,
        governance_fee: FeeT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        debug_assert!(amounts[index] == 0);
        let initial_depth = Self::internal_calculate_depth(pool_balances, amp_factor)?;
        //println!("initial_depth: {}", initial_depth);
        let mut updated_balances = binary_op_balances(
            if is_exact_input { AmountT::add } else { AmountT::sub },
            &pool_balances,
            &amounts,
        );
        //println!("updated balances: {:?}", updated_balances);
        let swap_base_balances = &(if is_exact_input && total_fee > 0 {
            let input_fee_amounts = unary_op_balances(|v| rounded_mul_div(v, total_fee, FEE_MULTIPLIER), amounts);
            binary_op_balances(AmountT::sub, &updated_balances, &input_fee_amounts)
        } else {
            updated_balances
        });

        //println!("swap_base_balances: {:?}", swap_base_balances);
        let known_balances = exclude_index(index, swap_base_balances);
        //println!("known_balances: {:?}", known_balances);
        let unknown_balance = Self::calculate_unknown_balance(&known_balances, initial_depth, amp_factor)?;
        //println!("unknown_balance: {}", unknown_balance);
        let intermediate_amount = sub_given_order(is_exact_input, pool_balances[index], unknown_balance);
        let final_amount = if !is_exact_input && total_fee > 0 {
            rounded_mul_div(intermediate_amount, FEE_MULTIPLIER, FEE_MULTIPLIER - total_fee)
        } else {
            intermediate_amount
        };

        updated_balances[index] =
            if is_exact_input { AmountT::sub } else { AmountT::add }(updated_balances[index], final_amount);

        let governance_mint_amount = if total_fee > 0 {
            let final_depth = Self::internal_calculate_depth(&updated_balances, amp_factor)?;
            let total_fee_depth = final_depth - initial_depth;
            let governance_depth_shifted = (total_fee_depth * governance_fee).shift_up().rounded_div_u64(total_fee);

            (governance_depth_shifted * lp_total_supply)
                .rounded_div(initial_depth + total_fee_depth - governance_depth_shifted.shift_down())
                .shift_down()
                .as_u64()
        } else {
            0
        };
        Ok((final_amount, governance_mint_amount))
    }

    fn add_remove(
        is_add: bool, //false => remove
        amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: AmpT,
        total_fee: FeeT,
        governance_fee: FeeT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        let initial_depth = Self::internal_calculate_depth(pool_balances, amp_factor)?;
        let updated_balances = binary_op_balances(
            if is_add { AmountT::add } else { AmountT::sub },
            &pool_balances,
            &amounts,
        );
        let updated_depth = Self::internal_calculate_depth(&updated_balances, amp_factor)?;
        if total_fee > 0 {
            let sum_updated_balances = sum_balances(&updated_balances);
            let sum_pool_balances = sum_balances(pool_balances);
            let scaled_balances = unary_op_balances(
                |balance| U256::rounded_mul_div(balance, sum_updated_balances, sum_pool_balances),
                pool_balances,
            );
            let taxbase = binary_op_balances(
                |updated, scaled| match (is_add, updated > scaled) {
                    (true, true) => updated - scaled,
                    (false, false) => scaled - updated,
                    _ => 0,
                },
                &updated_balances,
                &scaled_balances,
            );

            let fee_shifted = if is_add {
                U256::from(total_fee).shift_up().rounded_div_u64(FEE_MULTIPLIER)
            } else {
                U256::from(FEE_MULTIPLIER)
                    .shift_up()
                    .rounded_div_u64(FEE_MULTIPLIER - total_fee)
                    - U256::from(1).shift_up()
            };
            let fee_amounts = unary_op_balances(|balance| (fee_shifted * balance).shift_down().as_u64(), &taxbase);
            if updated_balances
                .iter()
                .zip(fee_amounts.iter())
                .any(|(&updated_balance, &fee_amount)| updated_balance <= fee_amount)
            {
                //This error is an artifact of the approximative, simplified way in which fees are calculated.
                //Fees are calculated using amounts rather than depth. This should be fine in real world situations
                //but (like all linear approximations) it leads to impossible demands in extreme situations:
                //The fee math implementation assumes that e.g. a fee of 25 % on inputs can be offset by charging an
                //extra 33 % to the token balance that is being withdrawn. This is only marginally true however
                //because the marginal price of each additional token withdrawn increases (tending towards infinity
                //as the balance of that particular token approaches zero), while the marginal price of each additional
                //token added decreases (tending towards zero as its token balance approaches infinity).
                //
                //Another easy intuition pump to see the issue with this approach is:
                //When withdrawing essentially the entire balance of one token, there is no way to withdraw an
                //additional (say) 10 % in fees of that token, since those extra 10 % simply don't exist in the pool.
                //
                //Overall, this issue should be of little practical concern however since any remove that would run
                //into it is economically trumped by a proportional remove that avoids fees altogether and would
                //essentially withdraw all token balances, including the requested one.
                return Err(PoolError::ImpossibleRemove);
            }
            let fee_adjusted_balances = binary_op_balances(AmountT::sub, &updated_balances, &fee_amounts);
            let fee_adjusted_depth = Self::internal_calculate_depth(&fee_adjusted_balances, amp_factor)?;
            let total_fee_depth = updated_depth - fee_adjusted_depth;
            let user_depth = U256::sub_given_order(is_add, fee_adjusted_depth, initial_depth);
            let lp_amount = (user_depth * lp_total_supply).rounded_div(initial_depth).as_u64();
            let governance_depth_shifted = (total_fee_depth * governance_fee).shift_up().rounded_div_u64(total_fee);
            let governance_mint_amount = (governance_depth_shifted
                * if is_add { AmountT::add } else { AmountT::sub }(lp_total_supply, lp_amount))
            .rounded_div(
                if is_add { updated_depth } else { fee_adjusted_depth } - governance_depth_shifted.shift_down(),
            )
            .shift_down()
            .as_u64();
            Ok((lp_amount, governance_mint_amount))
        } else {
            let lp_amount = (U256::sub_given_order(is_add, updated_depth, initial_depth) * lp_total_supply)
                .rounded_div(initial_depth)
                .as_u64();
            Ok((lp_amount, 0))
        }
    }

    fn internal_remove_exact_burn(
        burn_amount: AmountT,
        output_index: usize,
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: AmpT,
        total_fee: FeeT,
        governance_fee: FeeT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        debug_assert!(burn_amount > 0);
        let initial_depth = Self::internal_calculate_depth(&pool_balances, amp_factor)?;
        let updated_depth = (initial_depth * (lp_total_supply - burn_amount)).rounded_div_u64(lp_total_supply);
        debug_assert!(initial_depth > updated_depth);
        let known_balances = exclude_index(output_index, &pool_balances);
        let unknown_balance = Self::calculate_unknown_balance(&known_balances, updated_depth, amp_factor)?;
        let base_amount = pool_balances[output_index] - unknown_balance;
        if total_fee > 0 {
            let sum_pool_balances = sum_balances(&pool_balances);
            let taxable_percentage_shifted = (sum_pool_balances - pool_balances[output_index])
                .shift_up()
                .rounded_div(sum_pool_balances);
            let fee_shifted = U256::from(FEE_MULTIPLIER)
                .shift_up()
                .rounded_div_u64(FEE_MULTIPLIER - total_fee)
                - U256::from(1).shift_up();
            let taxbase = (taxable_percentage_shifted * base_amount)
                .rounded_div(U256::from(1).shift_up() + (taxable_percentage_shifted * fee_shifted).shift_down());
            let fee_amount = (fee_shifted * taxbase).shift_down().as_u64();
            let output_amount = base_amount - fee_amount;
            let mut updated_balances = *pool_balances;
            updated_balances[output_index] -= output_amount;
            let total_fee_depth = Self::internal_calculate_depth(&updated_balances, amp_factor)? - updated_depth;
            let governance_depth_shifted = (total_fee_depth * governance_fee).shift_up().rounded_div_u64(total_fee);
            let governance_mint_amount = (governance_depth_shifted * (lp_total_supply - burn_amount))
                .rounded_div(updated_depth - governance_depth_shifted.shift_down())
                .shift_down()
                .as_u64();
            Ok((output_amount, governance_mint_amount))
        } else {
            Ok((base_amount, 0))
        }
    }

    fn internal_calculate_depth(pool_balances: &[AmountT; TOKEN_COUNT], amp_factor: AmpT) -> InvariantResult<U256> {
        let n = TOKEN_COUNT as AmountT;
        let pool_balances_times_n = pool_balances
            .iter()
            .map(|&pool_balance| U256::from(pool_balance) * n)
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap();
        let pool_balances_sum = pool_balances
            .iter()
            .fold(U256::from(0), |acc, &pool_balance| acc + pool_balance);
        let amp_times_sum = (pool_balances_sum * amp_factor).shift_up(); //less than 192 bits
        let denominator_fixed = U256::from(amp_factor - 1 * AMP_MULTIPLIER).shift_up(); //less than 128 bits

        let mut previous_depth = U256::from(0);
        let mut depth = pool_balances_sum;
        while abs_difference(depth, previous_depth) > U256::from(1) {
            previous_depth = depth;

            let reciprocal_decay = pool_balances_times_n
                .iter()
                .fold(U256::SHIFT_MULTIPLIER * AMP_MULTIPLIER, |acc, &pool_balance_times_n| {
                    (acc * depth).rounded_div(pool_balance_times_n)
                });
            let n_times_depth_times_decay = depth * reciprocal_decay * n;
            let numerator = amp_times_sum + n_times_depth_times_decay;
            let denominator = denominator_fixed + reciprocal_decay * (n + 1);

            depth = numerator.rounded_div(denominator);
        }

        Ok(depth)
    }

    fn calculate_unknown_balance(
        // this should have type &[AmountT; TOKEN_COUNT-1] but Rust currently does not support const operations
        // on const generics and hence TOKEN_COUNT-1 is illegal and so it has to be a Vec instead...
        known_balances: &Vec<AmountT>,
        depth: U256,
        amp_factor: AmpT,
    ) -> InvariantResult<AmountT> {
        let n = TOKEN_COUNT as AmountT;
        debug_assert!(n == TOKEN_COUNT as AmountT);
        let known_balance_sum = known_balances
            .iter()
            .fold(U256::from(0), |acc, &known_balance| acc + known_balance);
        let reciprocal_decay = known_balances
            .iter()
            .fold(U256::SHIFT_MULTIPLIER * AMP_MULTIPLIER, |acc, &known_balance| {
                (acc * depth).rounded_div_u64(known_balance * n)
            });
        let numerator_fixed = ((reciprocal_decay * depth).rounded_div_u64(n) * depth)
            .rounded_div_u64(amp_factor)
            .shift_down();
        //can't sub depth from denominator_fixed because overall result could turn negative
        let denominator_fixed = known_balance_sum + depth * AMP_MULTIPLIER / amp_factor;

        let mut previous_unknown_balance = U256::from(0);
        let mut unknown_balance = depth;
        while abs_difference(unknown_balance, previous_unknown_balance) > U256::from(1) {
            previous_unknown_balance = unknown_balance;
            let numerator = numerator_fixed + unknown_balance * unknown_balance;
            let denominator = (denominator_fixed + unknown_balance * 2) - depth;

            unknown_balance = numerator.rounded_div(denominator);
        }

        Ok(unknown_balance.as_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::DecimalU128;
    use std::convert::TryFrom;

    const BASE: AmountT = 10u64.pow(10);

    fn assert_close_enough(v1: AmountT, v2: AmountT, max_diff: AmountT) {
        let diff = if v1 > v2 { v1 - v2 } else { v2 - v1 };
        assert!(
            diff <= max_diff,
            "not sufficiently close: {} {}, max_diff: {}",
            v1,
            v2,
            max_diff
        );
    }

    #[test]
    fn basic_invariant_tests() {
        const TOKEN_COUNT: usize = 6;
        //grouped to signify that exact_depth depends on balances and amp_factor
        let (balances, amp_factor, exact_depth) = (
            [20, 10, 20, 5, 2, 1],
            1 * AMP_MULTIPLIER,
            //DecimalU128::new(5797595776747225261683921277u128.into(), 26).unwrap()
            DecimalU128::new(3770007484983239375907243892u128.into(), 26).unwrap(),
        );

        let exponent = 6 + 4;
        let large_amount = 10u64.pow(exponent);
        let balances = unary_op_balances(|balance| balance * large_amount, &balances);
        let shifted_depth =
            DecimalU128::new(exact_depth.get_raw(), exact_depth.get_decimals() - exponent as u8).unwrap();
        let expected_depth = DecT::try_from(shifted_depth).unwrap();

        let depth = Invariant::<TOKEN_COUNT>::internal_calculate_depth(&balances, amp_factor).unwrap();
        assert_close_enough(depth.as_u64(), expected_depth.trunc(), 1);
        // println!(">>>        balances: {:?}", balances);
        for i in 0..TOKEN_COUNT {
            let input_balances = exclude_index(i, &balances);
            // println!(">>>  input_balances: {:?}", input_balances);
            // println!(">>> --------------------------");
            let unknown_balance = Invariant::<TOKEN_COUNT>::calculate_unknown_balance(
                &input_balances,
                U256::from(expected_depth.trunc()),
                amp_factor,
            )
            .unwrap();
            // println!(">>> unknown_balance: {}", unknown_balance);
            assert_close_enough(unknown_balance, balances[i], 1);
        }
    }

    #[test]
    fn swap_in_vs_out() {
        // println!(">>>\n");
        const TOKEN_COUNT: usize = 3;
        let lp_total_supply = BASE * TOKEN_COUNT as AmountT;
        let amp_factor = DecT::new(1313, 3).unwrap();
        let lp_fee = DecT::new(10, 2).unwrap();
        // let lp_fee = DecT::from(0);
        let governance_fee = DecT::new(40, 2).unwrap();

        let balances = [BASE; TOKEN_COUNT];
        let mut amounts = [0; TOKEN_COUNT];
        let original_input = balances[0] / 2;
        amounts[0] = original_input;

        let (yielded_output, government_mint_in) = Invariant::<TOKEN_COUNT>::swap_exact_input(
            &amounts,
            1,
            &balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
        .unwrap();
        // println!(">>> swap_exact_input:\n>>> input: {}\n>>> output: {}\n>>> govfee: {}", original_input, yielded_output, government_mint_in);

        amounts[0] = yielded_output;

        let (required_input, government_mint_out) = Invariant::<TOKEN_COUNT>::swap_exact_output(
            1,
            &amounts,
            &balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
        .unwrap();
        // println!(">>> swap_exact_input:\n>>> output: {}\n>>>  input: {}\n>>> govfee: {}", yielded_output, required_input, government_mint_out);

        assert_close_enough(required_input, original_input, 1);
        assert_close_enough(government_mint_in, government_mint_out, 1);
    }

    #[test]
    fn remove_consistency() {
        // println!(">>>\n");
        const TOKEN_COUNT: usize = 3;
        let lp_total_supply = BASE * TOKEN_COUNT as AmountT;
        let amp_factor = DecT::from(1);
        let lp_fee = DecT::new(10, 2).unwrap();
        // let lp_fee = DecT::from(0);
        let governance_fee = DecT::new(40, 2).unwrap();

        let balances = [BASE; TOKEN_COUNT];
        let mut output = [0; TOKEN_COUNT];
        output[0] = balances[0] / 2;

        let (lp_required, gov_fee_token_remove) = Invariant::<TOKEN_COUNT>::remove_exact_output(
            &output,
            &balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
        .unwrap();

        let (amount_received, gov_fee_lp_burn) = Invariant::<TOKEN_COUNT>::remove_exact_burn(
            lp_required,
            0,
            &balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
        .unwrap();
        // println!(">>> removing {} coins (of one type) requires {} lp tokens", output[0], lp_required);
        // println!(">>> burning {} lp tokens netted {} coins (of one type)", lp_required, amount_received);

        assert_close_enough(output[0], amount_received, 1);
        assert_close_enough(gov_fee_token_remove, gov_fee_lp_burn, 1);
    }

    #[test]
    fn uniform_and_imbalanced_vs_together_add() {
        uniform_and_imbalanced_vs_together(true);
    }

    #[test]
    fn uniform_and_imbalanced_vs_together_remove() {
        uniform_and_imbalanced_vs_together(false);
    }

    fn uniform_and_imbalanced_vs_together(is_add: bool) {
        // println!(">>>\n");
        const TOKEN_COUNT: usize = 3;
        let lp_total_supply = BASE * TOKEN_COUNT as AmountT;
        let amp_factor = DecT::new(1313, 3).unwrap();
        let lp_fee = DecT::new(10, 2).unwrap();
        let governance_fee = DecT::new(20, 2).unwrap();
        let balanced_divisor = 2;

        let balances = [BASE; TOKEN_COUNT];
        let balanced_amounts = [balances[0] / balanced_divisor; TOKEN_COUNT];

        let pool_op = if is_add {
            Invariant::<TOKEN_COUNT>::add
        } else {
            Invariant::<TOKEN_COUNT>::remove_exact_output
        };

        let (split_first_lp, nothing) = pool_op(
            &balanced_amounts,
            &balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
        .unwrap();
        assert_eq!(nothing, 0);
        // println!(">>>          split_first_lp: {}", split_first_lp);

        let mut imbalanced_amounts = [0; TOKEN_COUNT];
        imbalanced_amounts[0] = balances[0] / balanced_divisor / 2;

        let (split_second_lp, split_governance_fee) = pool_op(
            &imbalanced_amounts,
            &binary_op_balances(
                if is_add { AmountT::add } else { AmountT::sub },
                &balances,
                &balanced_amounts,
            ),
            amp_factor,
            lp_fee,
            governance_fee,
            if is_add { AmountT::add } else { AmountT::sub }(lp_total_supply, lp_total_supply / balanced_divisor),
        )
        .unwrap();
        // println!(">>>         split_second_lp: {}", split_second_lp);
        // println!(">>>    split_governance_fee: {}", split_governance_fee);

        let (together_lp, together_governance_fee) = pool_op(
            &binary_op_balances(AmountT::add, &balanced_amounts, &imbalanced_amounts),
            &balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
        .unwrap();
        // println!(">>>             together_lp: {}", together_lp);
        // println!(">>> together_governance_fee: {}", together_governance_fee);

        assert_close_enough(together_lp, split_first_lp + split_second_lp, 1);
        assert_close_enough(together_governance_fee, split_governance_fee, 1);
    }
}
