// see doc/swap_invariants.ipynb for an explanation on the math in here

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
type DecT = DecimalU64;

const AMP_PRECISION: u8 = 3;
//const FEE_PRECISION : u8 = 6;
const AMP_MULTI: u64 = ten_to_the_precision(AMP_PRECISION);
//const FEE_MULTI : u64 = ten_to_the_precision(FEE_PRECISION);
const OFFSET_SHIFT: u32 = 64;

const fn ten_to_the_precision(precision: u8) -> u64 {
    10u64.pow(precision as u32)
}

fn with_precision(decimal: DecT, precision: u8) -> U256 {
    U256::from(decimal.round(precision).get_raw()) * ten_to_the_precision(precision)
}

fn difference(v1: &U256, v2: &U256) -> U256 {
    if v1 > v2 {
        v1 - v2
    } else {
        v2 - v1
    }
}

fn sub_given_order(keep_order: bool, v1: DecT, v2: DecT) -> DecT {
    if keep_order {
        v1 - v2
    } else {
        v2 - v1
    }
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
        Ok(if lp_total_supply == 0 {
            (
                Self::internal_calculate_depth(&Self::to_dec_array(input_amounts), amp_factor)?.trunc(),
                0,
            )
        } else {
            Self::trunc(&Self::add_remove(
                true,
                &Self::to_dec_array(input_amounts),
                &Self::to_dec_array(pool_balances),
                amp_factor,
                lp_fee,
                governance_fee,
                DecT::from(lp_total_supply),
            )?)
        })
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
        Ok(Self::trunc(&Self::swap(
            true,
            &Self::to_dec_array(input_amounts),
            output_index,
            &Self::to_dec_array(pool_balances),
            amp_factor,
            lp_fee,
            governance_fee,
            DecT::from(lp_total_supply),
        )?))
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
        Ok(Self::trunc(&Self::swap(
            false,
            &Self::to_dec_array(output_amounts),
            input_index,
            &Self::to_dec_array(pool_balances),
            amp_factor,
            lp_fee,
            governance_fee,
            DecT::from(lp_total_supply),
        )?))
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
        Ok(Self::trunc(&Self::internal_remove_exact_burn(
            DecT::from(burn_amount),
            output_index,
            &Self::to_dec_array(pool_balances),
            amp_factor,
            lp_fee,
            governance_fee,
            DecT::from(lp_total_supply),
        )?))
    }

    pub fn remove_exact_output(
        output_amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> InvariantResult<(AmountT, AmountT)> {
        Ok(Self::trunc(&Self::add_remove(
            false,
            &Self::to_dec_array(output_amounts),
            &Self::to_dec_array(pool_balances),
            amp_factor,
            lp_fee,
            governance_fee,
            DecT::from(lp_total_supply),
        )?))
    }

    fn swap(
        is_exact_input: bool, //false => exact output
        amounts: &[DecT; TOKEN_COUNT],
        index: usize,
        pool_balances: &[DecT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: DecT,
    ) -> InvariantResult<(DecT, DecT)> {
        debug_assert!(amounts[index] == 0);
        let initial_depth = Self::internal_calculate_depth(pool_balances, amp_factor)?;
        //println!("initial_depth: {}", initial_depth);
        let mut updated_balances = if is_exact_input {
            Self::add_balances
        } else {
            Self::sub_balances
        }(&pool_balances, &amounts);
        //println!("updated balances: {:?}", updated_balances);
        let total_fee = lp_fee + governance_fee;
        let swap_base_balances = &(if is_exact_input && total_fee > 0 {
            let input_fee_amounts = Self::scale_balances(total_fee, amounts);
            Self::sub_balances(&updated_balances, &input_fee_amounts)
        } else {
            updated_balances
        });

        //println!("swap_base_balances: {:?}", swap_base_balances);
        let known_balances = Self::exclude_index(index, swap_base_balances);
        //println!("known_balances: {:?}", known_balances);
        let unknown_balance = Self::calculate_unknown_balance(&known_balances, initial_depth, amp_factor)?;
        //println!("unknown_balance: {}", unknown_balance);
        let intermediate_amount = sub_given_order(is_exact_input, pool_balances[index], unknown_balance);
        let final_amount = if !is_exact_input && total_fee > 0 {
            intermediate_amount / (1 - total_fee)
        } else {
            intermediate_amount
        };

        updated_balances[index] =
            if is_exact_input { DecT::sub } else { DecT::add }(updated_balances[index], final_amount);
        let governance_mint_amount = if total_fee > 0 {
            let final_depth = Self::internal_calculate_depth(&updated_balances, amp_factor)?;
            let total_fee_depth = final_depth - initial_depth;
            let governance_depth = total_fee_depth * (governance_fee / total_fee);
            governance_depth / (initial_depth + total_fee_depth - governance_depth) * lp_total_supply
        } else {
            DecT::from(0)
        };
        Ok((final_amount, governance_mint_amount))
    }

    fn add_remove(
        is_add: bool, //false => remove
        amounts: &[DecT; TOKEN_COUNT],
        pool_balances: &[DecT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: DecT,
    ) -> InvariantResult<(DecT, DecT)> {
        let initial_depth = Self::internal_calculate_depth(pool_balances, amp_factor)?;
        let updated_balances = if is_add { Self::add_balances } else { Self::sub_balances }(&pool_balances, &amounts);
        let updated_depth = Self::internal_calculate_depth(&updated_balances, amp_factor)?;
        let total_fee = lp_fee + governance_fee;
        if total_fee > 0 {
            let fee = if is_add { total_fee } else { (1 / (1 - total_fee)) - 1 };
            let sum_updated_balances = updated_balances.iter().map(|v| *v).sum::<DecT>();
            let sum_pool_balances = pool_balances.iter().map(|v| *v).sum::<DecT>();
            let scale_factor = sum_updated_balances / sum_pool_balances;
            let scaled_balances = Self::scale_balances(scale_factor, pool_balances);
            let taxbase = updated_balances
                .iter()
                .zip(scaled_balances.iter())
                .map(|(&updated, &scaled)| match (is_add, updated > scaled) {
                    (true, true) => updated - scaled,
                    (false, false) => scaled - updated,
                    _ => DecT::from(0),
                })
                .collect::<ArrayVec<_, TOKEN_COUNT>>()
                .into_inner()
                .unwrap();

            let fee_amounts = Self::scale_balances(fee, &taxbase);
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
            let fee_adjusted_balances = Self::sub_balances(&updated_balances, &fee_amounts);
            let fee_adjusted_depth = Self::internal_calculate_depth(&fee_adjusted_balances, amp_factor)?;
            let total_fee_depth = updated_depth - fee_adjusted_depth;
            let user_depth = sub_given_order(is_add, fee_adjusted_depth, initial_depth);
            let lp_amount = lp_total_supply * (user_depth / initial_depth);
            let governance_depth = total_fee_depth * (governance_fee / total_fee);
            let governance_mint_amount =
                governance_depth / (initial_depth + total_fee_depth - governance_depth) * lp_total_supply;
            Ok((lp_amount, governance_mint_amount))
        } else {
            let lp_amount = (sub_given_order(is_add, updated_depth, initial_depth) / initial_depth) * lp_total_supply;
            Ok((lp_amount, DecT::from(0)))
        }
    }

    fn internal_remove_exact_burn(
        burn_amount: DecT,
        output_index: usize,
        pool_balances: &[DecT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: DecT,
    ) -> InvariantResult<(DecT, DecT)> {
        debug_assert!(burn_amount > 0);
        let initial_depth = Self::internal_calculate_depth(&pool_balances, amp_factor)?;
        let updated_depth = (lp_total_supply - burn_amount) / lp_total_supply * initial_depth;
        debug_assert!(initial_depth > updated_depth);
        let known_balances = Self::exclude_index(output_index, &pool_balances);
        let unknown_balance = Self::calculate_unknown_balance(&known_balances, updated_depth, amp_factor)?;
        let base_amount = pool_balances[output_index] - unknown_balance;
        let total_fee = lp_fee + governance_fee;
        if total_fee > 0 {
            let sum_pool_balances = pool_balances.iter().map(|v| *v).sum::<DecT>();
            let taxable_percentage = 1 - pool_balances[output_index] / sum_pool_balances;
            let fee = (1 / (1 - total_fee)) - 1;
            let input_equivalent_amount = base_amount / (1 + taxable_percentage * fee);
            let taxbase = input_equivalent_amount * taxable_percentage;
            let fee_amount = fee * taxbase;
            let output_amount = base_amount - fee_amount;
            let mut updated_balances = *pool_balances;
            updated_balances[output_index] -= output_amount;
            let total_fee_depth = Self::internal_calculate_depth(&updated_balances, amp_factor)? - updated_depth;
            let governance_depth = total_fee_depth * (governance_fee / total_fee);
            let governance_mint_amount =
                governance_depth / (initial_depth + total_fee_depth - governance_depth) * lp_total_supply;
            Ok((output_amount, governance_mint_amount))
        } else {
            Ok((base_amount, DecT::from(0)))
        }
    }

    pub fn calculate_depth(pool_balances: &[AmountT; TOKEN_COUNT], amp_factor: DecT) -> DecT {
        Self::internal_calculate_depth(&Self::to_dec_array(&pool_balances), amp_factor).unwrap()
    }

    fn internal_calculate_depth(pool_balances: &[DecT; TOKEN_COUNT], amp_factor: DecT) -> InvariantResult<DecT> {
        let OFFSET_MULTI = U256::from(1) << OFFSET_SHIFT;
        let n = TOKEN_COUNT as AmountT;
        let amp_factor = with_precision(amp_factor, AMP_PRECISION);
        let pool_balances = Self::to_U256_array(&pool_balances);
        let pool_balances_times_n = pool_balances
            .iter()
            .map(|&b| b * n)
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap();
        let balances_sum = pool_balances
            .iter()
            .fold(U256::from(0), |acc, &pool_balance| acc + pool_balance);
        let amp_times_sum = (amp_factor * balances_sum) << OFFSET_SHIFT; //less than 192 bits
        let denominator_fixed = (amp_factor - 1 * AMP_MULTI) << OFFSET_SHIFT; //less than 128 bits

        let mut previous_depth = U256::from(0);
        let mut depth = balances_sum;
        while difference(&depth, &previous_depth) > U256::from(1) {
            previous_depth = depth;

            let reciprocal_decay = pool_balances_times_n
                .iter()
                .fold(OFFSET_MULTI * AMP_MULTI, |acc, &pool_balance_times_n| {
                    (acc * depth) / pool_balance_times_n
                });
            let n_times_depth_times_decay = depth * reciprocal_decay * n;
            let numerator = amp_times_sum + n_times_depth_times_decay;
            let denominator = denominator_fixed + reciprocal_decay * (n + 1);

            depth = numerator / denominator;
        }

        Ok(DecT::from(depth.as_u64()))
    }

    fn calculate_unknown_balance(
        // this should have type &[AmountT; TOKEN_COUNT-1] but Rust currently does not support const operations
        // on const generics and hence TOKEN_COUNT-1 is illegal
        known_balances: &Vec<DecT>,
        depth: DecT,
        amp_factor: DecT,
    ) -> InvariantResult<DecT> {
        let OFFSET_MULTI = U256::from(1) << OFFSET_SHIFT;
        let n = TOKEN_COUNT as AmountT;
        let amp_factor = with_precision(amp_factor, AMP_PRECISION);
        let depth = depth.trunc();
        debug_assert!(n == TOKEN_COUNT as AmountT);
        let known_balance_sum = known_balances
            .iter()
            .fold(U256::from(0), |acc, &known_balance| acc + known_balance.trunc());
        //println!("known_balance_sum: {}", known_balance_sum);
        let reciprocal_decay = known_balances
            .iter()
            .fold(OFFSET_MULTI * AMP_MULTI, |acc, &known_balance| {
                (acc * depth) / (known_balance.trunc() * n)
            });
        //println!("reciprocal_decay: {}", reciprocal_decay);

        let numerator_fixed = ((((reciprocal_decay * depth) / n) * depth) / amp_factor) >> OFFSET_SHIFT;
        //println!("numerator_fixed: {}", numerator_fixed);
        //can't sub depth from denominator_fixed because overall result could turn negative
        let denominator_fixed = known_balance_sum + U256::from(depth * AMP_MULTI) / amp_factor;
        //println!("denominator_fixed: {}", denominator_fixed);

        let mut previous_unknown_balance = U256::from(0);
        let mut unknown_balance = U256::from(depth);
        while difference(&unknown_balance, &previous_unknown_balance) > U256::from(1) {
            previous_unknown_balance = unknown_balance;
            let numerator = numerator_fixed + unknown_balance * unknown_balance;
            let denominator = (denominator_fixed + unknown_balance * 2) - depth;
            //println!("numerator: {}", numerator);
            //println!("denominator: {}", denominator);

            unknown_balance = numerator / denominator;
            //println!("unknown_balance: {}", unknown_balance);
        }

        Ok(DecT::from(unknown_balance.as_u64()))
    }

    pub fn to_U256_array(arr: &[DecT; TOKEN_COUNT]) -> [U256; TOKEN_COUNT] {
        arr.iter()
            .map(|&amount| U256::from(amount.trunc()))
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap()
    }

    fn trunc(result: &(DecT, DecT)) -> (AmountT, AmountT) {
        (result.0.trunc(), result.1.trunc())
    }

    pub fn to_dec_array(arr: &[AmountT; TOKEN_COUNT]) -> [DecT; TOKEN_COUNT] {
        arr.iter()
            .map(|amount| DecT::from(*amount))
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap()
    }

    fn exclude_index(index: usize, array: &[DecT; TOKEN_COUNT]) -> Vec<DecT> {
        array
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, v)| *v)
            .collect::<Vec<DecT>>()
    }

    fn add_balances(v1: &[DecT; TOKEN_COUNT], v2: &[DecT; TOKEN_COUNT]) -> [DecT; TOKEN_COUNT] {
        Self::op_balances(DecT::add, v1, v2)
    }

    fn sub_balances(v1: &[DecT; TOKEN_COUNT], v2: &[DecT; TOKEN_COUNT]) -> [DecT; TOKEN_COUNT] {
        Self::op_balances(DecT::sub, v1, v2)
    }

    fn op_balances(
        op: impl Fn(DecT, DecT) -> DecT,
        v1: &[DecT; TOKEN_COUNT],
        v2: &[DecT; TOKEN_COUNT],
    ) -> [DecT; TOKEN_COUNT] {
        let mut ret = [DecT::from(0); TOKEN_COUNT];
        for i in 0..TOKEN_COUNT {
            ret[i] = op(v1[i], v2[i]);
        }
        ret
    }

    fn scale_balances(scalar: DecT, array: &[DecT; TOKEN_COUNT]) -> [DecT; TOKEN_COUNT] {
        let mut ret = [DecT::from(0); TOKEN_COUNT];
        for i in 0..TOKEN_COUNT {
            ret[i] = scalar * array[i];
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::DecimalU128;
    use std::convert::TryFrom;

    fn assert_close_enough(v1: DecT, v2: DecT, max_diff: AmountT) {
        let diff = if v1 > v2 { v1 - v2 } else { v2 - v1 };
        assert!(
            diff <= DecT::from(max_diff),
            "not sufficiently close: {} {}, max_diff: {}",
            v1,
            v2,
            max_diff
        );
    }

    #[test]
    //#[ignore]
    fn basic_invariant_tests() {
        const TOKEN_COUNT: usize = 6;
        //grouped to signify that exact_depth depends on balances and amp_factor
        let (balances, amp_factor, exact_depth) = (
            Invariant::<TOKEN_COUNT>::to_dec_array(&[20, 10, 20, 5, 2, 1]),
            DecT::from(1),
            //DecimalU128::new(5797595776747225261683921277u128.into(), 26).unwrap()
            DecimalU128::new(3770007484983239375907243892u128.into(), 26).unwrap(),
        );

        let exponent = 6 + 4;
        let large_amount = DecT::from(10u64.pow(exponent));
        let balances = Invariant::<TOKEN_COUNT>::scale_balances(large_amount, &balances);
        let shifted_depth =
            DecimalU128::new(exact_depth.get_raw(), exact_depth.get_decimals() - exponent as u8).unwrap();
        let expected_depth = DecT::try_from(shifted_depth).unwrap();

        let depth = Invariant::<TOKEN_COUNT>::internal_calculate_depth(&balances, amp_factor).unwrap();
        assert_close_enough(depth, expected_depth, 1);
        for i in 0..TOKEN_COUNT {
            let input_balances = balances
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, v)| *v)
                .collect::<Vec<DecT>>();
            // println!("balances: {:?}", balances);
            // println!("input_balances: {:?}", input_balances);
            let unknown_balance =
                Invariant::<TOKEN_COUNT>::calculate_unknown_balance(&input_balances, depth, amp_factor).unwrap();
            assert_close_enough(unknown_balance, balances[i], 1);
        }
    }

    #[test]
    #[ignore]
    fn whatever() {
        const TOKEN_COUNT: usize = 4;
        let balances = [
            DecT::from(10000),
            DecT::from(20000),
            DecT::from(30000),
            DecT::from(40000),
        ];
        let lp_total_supply = DecT::from(99999);
        let amp_factor = DecimalU64::new(1, 0).unwrap();
        let lp_fee = DecimalU64::new(10, 2).unwrap();
        let governance_fee = DecimalU64::new(1, 2).unwrap();
        let input = [DecT::from(2000), DecT::from(4000), DecT::from(6000), DecT::from(0)];
        let (output_amount, governance_mint_amount) = Invariant::<TOKEN_COUNT>::swap(
            true,
            &input,
            3,
            &balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
        .unwrap();
        println!("output_amount: {}", output_amount);
        println!("governance_mint_amount: {}", governance_mint_amount);
    }
}
