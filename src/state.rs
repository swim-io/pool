use solana_program::pubkey::Pubkey;
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    pool_fee::PoolFee,
    amp_factor::AmpFactor,
};

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Pool<const TOKEN_COUNT: usize> {
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
    pub prepared_governenace_key: Pubkey,
    pub governance_action_cooldown: solana_program::clock::UnixTimestamp,
}

impl<const TOKEN_COUNT: usize> Pool<TOKEN_COUNT> {
    pub fn is_initialized(&self) -> bool {
        self.lp_mint_key != Pubkey::default()
    }
}