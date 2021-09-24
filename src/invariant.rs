// see doc/swap_invariants.ipynb for an explanation on the math in here

// TODO document variable sizing considerations u64 having 19 decimal places
//      tokens having 6 decimal places by default, which gives an upper bound of
//      2^64 = 1.8 * 10^(13+6) = 18T full tokens
//      to work with
use crate::decimal::{DecimalU128, DecimalU64};

use std::{
    cmp,
    convert::TryFrom,
    ops::{Add, Div, Mul, Sub},
    vec::Vec,
};

use solana_program::msg; //used for debugging

use arrayvec::ArrayVec;

type AmountT = u64;
type DecT = DecimalU64;
type LargerDecT = DecimalU128;

pub struct Invariant<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Invariant<TOKEN_COUNT> {
    pub fn add(
        input_amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> (AmountT, AmountT) {
        if lp_total_supply == 0 {
            (Self::calculate_depth(input_amounts, amp_factor).trunc(), 0)
        } else {
            Self::add_remove(
                true,
                input_amounts,
                pool_balances,
                amp_factor,
                lp_fee,
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
    ) -> (AmountT, AmountT) {
        Self::swap(
            true,
            input_amounts,
            output_index,
            pool_balances,
            amp_factor,
            lp_fee,
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
    ) -> (AmountT, AmountT) {
        Self::swap(
            false,
            output_amounts,
            input_index,
            pool_balances,
            amp_factor,
            lp_fee,
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
    ) -> (AmountT, AmountT) {
        // msg!("[DEV]
        // remove_exact_burn(
        //     burn_amount:{},
        //     output_index: {},
        //     pool_balances: {:?},
        //     amp_factor: {:?},
        //     lp_fee: {:?},
        //     governanace_fee: {:?},
        //     lp_total_supply: {}
        // )", burn_amount, output_index, pool_balances, amp_factor, lp_fee, governance_fee, lp_total_supply);
        let governance_mint_amount = burn_amount * governance_fee;
        let initial_depth = Self::calculate_depth(pool_balances, amp_factor);
        let total_fee = lp_fee + governance_fee;
        // msg!("[DEV] governance_mint_amount: {:?}, initial_depth: {:?}, total_fee: {:?}", governance_mint_amount, initial_depth, total_fee);

        //divided by two because it's only half a swap!
        let fee_adjusted_burn_amount = burn_amount * (1 - total_fee / 2);
        let updated_depth = DecT::from(fee_adjusted_burn_amount) / lp_total_supply * initial_depth;
        // msg!("[DEV] fee_adjusted_burn_amount: {:?}, updated_depth: {:?}", fee_adjusted_burn_amount, updated_depth);

        let known_balances = Self::exclude_index(output_index, pool_balances);
        // msg!("[DEV] known_balances: {:?}", known_balances);

        let unknown_balance = Self::calculate_unknown_balance(&known_balances, updated_depth, amp_factor);
        // msg!("[DEV] output_amount =
        //             unknown_balance: {:?}
        //             - pool_balances[{}]: {:?}", unknown_balance, output_index, pool_balances[output_index]);
        let output_amount = unknown_balance - pool_balances[output_index];

        (output_amount.trunc(), governance_mint_amount.trunc())
    }

    pub fn remove_exact_output(
        output_amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> (AmountT, AmountT) {
        Self::add_remove(
            false,
            output_amounts,
            pool_balances,
            amp_factor,
            lp_fee,
            governance_fee,
            lp_total_supply,
        )
    }

    fn swap(
        exact_input: bool, //false => exact_output
        amounts: &[AmountT; TOKEN_COUNT],
        index: usize,
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> (AmountT, AmountT) {
        debug_assert!(amounts[index] == 0);
        let initial_depth = Self::calculate_depth(pool_balances, amp_factor);
        let mut updated_balances = if exact_input {
            Self::add_balances
        } else {
            Self::sub_balances
        }(&pool_balances, &amounts);
        let balances = Self::exclude_index(index, &updated_balances);
        let unknown_balance = Self::calculate_unknown_balance(&balances, initial_depth, amp_factor);
        let swapped_amount = Self::difference(unknown_balance, pool_balances[index].into());
        let total_fee = lp_fee + governance_fee;
        let fee_adjusted_amount =
            if exact_input { DecT::mul } else { DecT::div }(swapped_amount, DecT::from(1) - total_fee).trunc();
        //for some reason rustc isn't smart enough to allow me to use sub_assign/add_assign here:
        updated_balances[index] =
            if exact_input { AmountT::sub } else { AmountT::add }(updated_balances[index], fee_adjusted_amount);
        let updated_depth = Self::calculate_depth(&updated_balances, amp_factor);
        let governance_mint_amount =
            lp_total_supply * (governance_fee / total_fee) * (updated_depth - initial_depth) / initial_depth;

        (fee_adjusted_amount, governance_mint_amount.trunc())
    }

    fn add_remove(
        is_add: bool, //false => remove
        amounts: &[AmountT; TOKEN_COUNT],
        pool_balances: &[AmountT; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        lp_total_supply: AmountT,
    ) -> (AmountT, AmountT) {
        let initial_depth = Self::calculate_depth(pool_balances, amp_factor);
        let updated_balances = if is_add { Self::add_balances } else { Self::sub_balances }(&pool_balances, &amounts);
        let updated_depth = Self::calculate_depth(&updated_balances, amp_factor);
        let uniform_balances = Self::scale_balances(updated_depth / initial_depth, pool_balances);
        let total_fee = lp_fee + governance_fee;
        let fee_frac = (total_fee * TOKEN_COUNT as AmountT) / (4 * (TOKEN_COUNT - 1) as AmountT);
        let fee_adjusted_balances = updated_balances
            .iter()
            .zip(uniform_balances.iter())
            .map(|(updated, uniform)| (*updated - fee_frac * Self::difference(DecT::from(*updated), *uniform)).trunc())
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap();

        //TODO this should be unncessary, I think we should be able to calculate with_fee_depth from the ratios directly
        let fee_adjusted_depth = Self::calculate_depth(&fee_adjusted_balances, amp_factor);
        let lp_amount = lp_total_supply * Self::difference(initial_depth, fee_adjusted_depth) / initial_depth;
        let governance_mint_amount =
            lp_total_supply * (governance_fee / total_fee) * Self::difference(updated_depth, fee_adjusted_depth)
                / initial_depth;
        (lp_amount.trunc(), governance_mint_amount.trunc())
    }

    fn calculate_depth(pool_balances: &[AmountT; TOKEN_COUNT], amp_factor: DecT) -> DecT {
        let n = TOKEN_COUNT as AmountT;
        let balances_sum: DecT = pool_balances.iter().sum::<AmountT>().into(); //TODO more instances below: why is the AmountT type annotation here necessary?
        let amp_n_to_the_n = amp_factor * (n.pow(n as u32)) as AmountT;
        let amp_times_sum = amp_n_to_the_n.upcast_mul(balances_sum);

        let mut previous_depth = DecT::from(0);
        let mut depth = balances_sum;
        while Self::difference(depth, previous_depth) > 1 {
            previous_depth = depth;

            let reciprocal_decay: DecT = pool_balances //TODO why is rustc incorrectly inferring u64 for reciprocal_decay without the DecT type declaration?!?
                .iter()
                .map(|pool_balance| depth / (n * DecT::from(*pool_balance)))
                .product();
            let n_times_depth_times_decay = depth.upcast_mul(reciprocal_decay * n);
            let numerator = amp_times_sum + n_times_depth_times_decay;
            let denominator = amp_n_to_the_n - 1 + (n + 1) * reciprocal_decay;

            depth = DecT::try_from(numerator / LargerDecT::from(denominator)).unwrap();
        }

        depth
    }

    fn calculate_unknown_balance(
        // this should have type &[AmountT; TOKEN_COUNT-1] but Rust currently does not support const operations
        // on const generics and hence TOKEN_COUNT-1 is illegal
        known_balances: &Vec<AmountT>,
        depth: DecT,
        amp_factor: DecT,
    ) -> DecT {
        // msg!("[DEV] calculate_unknown_balance(
        //     known_balances: {:?},
        //     depth: {:?},
        //     amp_factor: {:?},
        // ),", known_balances, depth, amp_factor);
        let n: AmountT = (known_balances.len() + 1) as AmountT;
        debug_assert!(n == TOKEN_COUNT as AmountT);
        let input_sum: DecT = known_balances
            .iter()
            .sum::<AmountT>() //TODO same as above: why is the AmountT type annotation here necessary?
            .into();
        // msg!("[DEV] n: {}, input_sum: {:?}", n, input_sum);
        let amp_n_to_the_n = amp_factor * (n.pow(n as u32)) as AmountT; //Ann
        let depth_div_amp_nn = depth / amp_n_to_the_n; // D / Ann
        let recip_decay: DecT = known_balances
            .iter()
            .map(|input_balance| depth / (n * DecT::from(*input_balance)))
            .product();

        //msg!("[DEV] amp_n_to_the_n: {:?}, depth_div_amp_nn: {:?}, recip_decay: {:?}", amp_n_to_the_n, depth_div_amp_nn, recip_decay);

        let numerator_fixed = (depth / n).upcast_mul(depth_div_amp_nn * recip_decay);
        //can't sub depth from denominator_fixed because overall result could turn negative
        let denominator_fixed = input_sum + depth_div_amp_nn; //b = S_ + D / Ann
                                                              //msg!("[DEV] numerator_fixed: {:?}, denominator_fixed: {:?}", numerator_fixed, denominator_fixed);
                                                              // println!("            depth: {}", depth);
                                                              // println!("          depth/n: {}", depth/n);
                                                              // println!(" depth_div_amp_nn: {}", depth_div_amp_nn);
                                                              // println!("      recip_decay: {}", recip_decay);
                                                              // println!("  multiply prev 2: {}", depth_div_amp_nn*recip_decay);
                                                              // println!("  numerator_fixed: {}", numerator_fixed);
                                                              // println!("denominator_fixed: {}", denominator_fixed);

        let mut previous_unknown_balance = DecT::from(0);
        let mut unknown_balance = depth;
        //msg!("[DEV] previous_unknwon_balance: {:?}, unknown_balance: {:?}", previous_unknown_balance, unknown_balance);
        while Self::difference(unknown_balance, previous_unknown_balance) > 1 {
            previous_unknown_balance = unknown_balance;
            //msg!("[DEV] previous_unknwon_balance: {:?}", previous_unknown_balance);
            let numerator = unknown_balance.upcast_mul(unknown_balance) + numerator_fixed;
            let denominator = (2 * unknown_balance + denominator_fixed) - depth;
            //msg!("[DEV] num: {:?}, denom: {:?}", numerator, denominator);
            unknown_balance = DecT::try_from(numerator / LargerDecT::from(denominator)).unwrap();
            //msg!("[DEV] unknown_balance: {:?}", unknown_balance);

            // println!("  num: {}", numerator);
            // println!("denom: {}", denominator);
            // println!(" quot: {}", unknown_balance);
        }

        // msg!("[DEV] returning unknown_balance: {}", unknown_balance);
        unknown_balance
    }

    fn exclude_index(index: usize, array: &[AmountT; TOKEN_COUNT]) -> Vec<AmountT> {
        array
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, v)| *v)
            .collect::<Vec<AmountT>>()
    }

    fn difference(v1: DecT, v2: DecT) -> DecT {
        cmp::max(v1, v2) - cmp::min(v1, v2)
    }

    fn add_balances(v1: &[AmountT; TOKEN_COUNT], v2: &[AmountT; TOKEN_COUNT]) -> [AmountT; TOKEN_COUNT] {
        Self::op_balances(AmountT::add, v1, v2)
    }

    fn sub_balances(v1: &[AmountT; TOKEN_COUNT], v2: &[AmountT; TOKEN_COUNT]) -> [AmountT; TOKEN_COUNT] {
        Self::op_balances(AmountT::sub, v1, v2)
    }

    fn op_balances(
        op: impl Fn(AmountT, AmountT) -> AmountT,
        v1: &[AmountT; TOKEN_COUNT],
        v2: &[AmountT; TOKEN_COUNT],
    ) -> [AmountT; TOKEN_COUNT] {
        let mut ret = [0; TOKEN_COUNT];
        for i in 0..TOKEN_COUNT {
            ret[i] = op(v1[i], v2[i]);
        }
        ret
    }

    fn scale_balances(scalar: DecimalU64, array: &[AmountT; TOKEN_COUNT]) -> [DecT; TOKEN_COUNT] {
        let mut ret = [DecimalU64::from(0); TOKEN_COUNT];
        for i in 0..TOKEN_COUNT {
            ret[i] = scalar * array[i];
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array_map<const TOKEN_COUNT: usize>(func: impl Fn(&u64) -> u64, arr: &[u64; TOKEN_COUNT]) -> [u64; TOKEN_COUNT] {
        arr.iter()
            .map(func)
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap()
    }

    #[test]
    fn basic_invariant_tests() {
        const TOKEN_COUNT: usize = 6;
        let mul_by = |factor| move |val: &u64| (*val) * factor;

        //grouped to signify that exact_depth depends on balances and amp_factor
        let (balances, amp_factor, exact_depth) = (
            [20, 10, 20, 5, 2, 1],
            DecT::from(1),
            DecimalU128::new(5797595776747225261683921277u128.into(), 26).unwrap(),
        );

        let exponent = 6 + 10;
        let large_amount = 10u64.pow(exponent);
        let balances = array_map(mul_by(large_amount), &balances);
        let shifted_depth =
            DecimalU128::new(exact_depth.get_raw(), exact_depth.get_decimals() - exponent as u8).unwrap();
        let expected_depth = DecimalU64::try_from(shifted_depth).unwrap();

        let depth = Invariant::<TOKEN_COUNT>::calculate_depth(&balances, amp_factor);
        assert_eq!(depth.trunc() / 10, expected_depth.trunc() / 10);
        for i in 0..TOKEN_COUNT {
            let input_balances = balances
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, v)| *v)
                .collect::<Vec<AmountT>>();
            // println!("balances: {:?}", balances);
            // println!("input_balances: {:?}", input_balances);
            let unknown_balance =
                Invariant::<TOKEN_COUNT>::calculate_unknown_balance(&input_balances, depth, amp_factor).trunc();
            assert_eq!(unknown_balance, balances[i]);
        }
    }
}
