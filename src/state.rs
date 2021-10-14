use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use solana_program::{clock::UnixTimestamp, pubkey::Pubkey};

use crate::{amp_factor::AmpFactor, pool_fee::PoolFee};

//TODO arguably, various fields should be Options (e.g. all the prepared_* fields)
//     the advantage of taking a special value approach is that serialized data
//     always has the same size (otherwise we'll have to figure out the maximum
//     size of a serialized PoolState in order to ensure that the pool's state
//     account has space and sol to be rent exempt in all cases)
#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Debug)]
pub struct PoolState<const TOKEN_COUNT: usize> {
    pub nonce: u8,
    pub is_paused: bool,
    pub amp_factor: AmpFactor,
    pub lp_fee: PoolFee,
    pub governance_fee: PoolFee,

    pub lp_mint_key: Pubkey,
    pub lp_decimal_equalizer: u8,

    pub token_mint_keys: [Pubkey; TOKEN_COUNT],
    pub token_decimal_equalizers: [u8; TOKEN_COUNT],
    pub token_keys: [Pubkey; TOKEN_COUNT],

    pub governance_key: Pubkey,
    pub governance_fee_key: Pubkey,
    pub prepared_governance_key: Pubkey,
    pub governance_transition_ts: UnixTimestamp,
    pub prepared_lp_fee: PoolFee,
    pub prepared_governance_fee: PoolFee,
    pub fee_transition_ts: UnixTimestamp,
    pub previous_depth: u128,
}

impl<const TOKEN_COUNT: usize> PoolState<TOKEN_COUNT> {
    // pub const LEN: usize = 8 + 8 + (TOKEN_COUNT * 2 * 64) +
    pub fn is_initialized(&self) -> bool {
        self.lp_mint_key != Pubkey::default()
    }
}
