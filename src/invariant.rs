//see doc/swap_invariants.ipynb for an explanation on the math in here

use crate::{decimal::{DecimalU64, DecimalU128}};

type ValueT = u64;
type ValDecT = DecimalU64;

pub struct Invariant<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Invariant<TOKEN_COUNT> {
    pub fn calculate_depth(
        pool_balances: [ValueT; TOKEN_COUNT],
        amp_factor: DecimalU64,
    ) -> ValueT {
        let n = TOKEN_COUNT as ValueT;
        let balances_sum = pool_balances.iter().map(|token_balance| ValDecT::from(*token_balance)).sum();
        let amp_n_to_the_n = amp_factor * (n.pow(n as u32)) as ValueT;
        let amp_times_sum = amp_n_to_the_n.upcast_mul(balances_sum);
        
        let mut depth_previous_iteration = ValDecT::from(0);
        let mut depth = balances_sum;
        while std::cmp::max(depth, depth_previous_iteration) - std::cmp::min(depth, depth_previous_iteration) > 1 {
            depth_previous_iteration = depth;

            let reciprocal_decay: DecimalU64 = pool_balances.iter() //why is rustc incorrectly inferring u64 for reciprocal_decay?!?
                .map(|token_balance| depth/(n * ValDecT::from(*token_balance))).product();
            let n_times_depth_times_decay = depth.upcast_mul(reciprocal_decay * n);
            let numerator = amp_times_sum + n_times_depth_times_decay;
            let denominator = amp_n_to_the_n - 1 + (n + 1) * reciprocal_decay;

            depth = ((numerator / DecimalU128::from(denominator)).round(0).trunc() as u64).into();
        }
        
        depth.round(0).trunc()
    }

    // pub fn calculate_bought_amount(
    //     bought_index: usize,
    //     sold_amounts: [ValueT; TOKEN_COUNT],
    //     pool_balances: [ValueT; TOKEN_COUNT],
    //     depth: ValueT,
    // ) -> ValueT {
    //     0
    // }

    //pub fn calculate_
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clac_depth_trivial() {
        const TOKEN_COUNT: usize = 6;
        let amp_factor = DecimalU64::one();
        let billion = 10u64.pow(6+11);
        //assert_eq!(Invariant::<TOKEN_COUNT>::calculate_depth([100, 100], amp_factor), 200 as u64);
        assert_eq!(Invariant::<TOKEN_COUNT>::calculate_depth([20*billion, 10*billion, 20*billion, 5*billion, 2*billion, billion], amp_factor), 5797595776747225262u64);
    }
}