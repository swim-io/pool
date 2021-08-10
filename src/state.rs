use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

use crate::{amp_factor::AmpFactor, pool_fee::PoolFee};

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct PoolState<const TOKEN_COUNT: usize> {
    pub nonce: u8,
    pub is_paused: bool,
    pub amp_factor: AmpFactor,
    pub lp_fee: PoolFee,
    pub governance_fee: PoolFee,

    pub lp_mint_key: Pubkey,

    pub token_mint_keys: [Pubkey; TOKEN_COUNT],
    pub token_keys: [Pubkey; TOKEN_COUNT],

    pub governance_key: Pubkey,
    pub governance_fee_key: Pubkey, //are fees minted as LP tokens?
    //pub governance_fee_keys: [Pubkey; TOKEN_COUNT], //or individually?
    pub prepared_governance_key: Pubkey,
    pub governance_action_deadline: solana_program::clock::UnixTimestamp,
    pub prepared_lp_fee: PoolFee,
    pub prepared_governance_fee: PoolFee,
}

impl<const TOKEN_COUNT: usize> PoolState<TOKEN_COUNT> {
    pub fn is_initialized(&self) -> bool {
        self.lp_mint_key != Pubkey::default()
    }
}
