use arrayvec::ArrayVec;

use solana_program::{
    msg,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    program::{
        invoke,
        invoke_signed
    },
    account_info::{
        next_account_info,
        AccountInfo
    },
    pubkey::Pubkey,
    sysvar::{
        rent::Rent,
		clock::Clock,
        Sysvar
    },
};

use crate::{
	instruction::PoolInstruction,
	pool_fees::PoolFees,
	state::Pool,
};
use borsh::{BorshDeserialize, BorshSerialize};

pub struct Processor<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Processor<TOKEN_COUNT> {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = PoolInstruction::<TOKEN_COUNT>::try_from_slice(instruction_data)?;

        match instruction { //all this boiler-plate could probably be replaced by implementing a procedural macro on PoolInstruction
            PoolInstruction::Init{nonce, amp_factor, pool_fees} => {
                Self::process_init(nonce, amp_factor, pool_fees, program_id, accounts)?;
            },
            // PoolInstruction::Add {..} => {
            // },
            // PoolInstruction::Remove {..} => {
            // },
            // PoolInstruction::Swap {..} => {
            // },
            _ => {todo!();}
        }
        Ok(())
    }

    fn process_init(
        nonce: u8,
        amp_factor: u64,
        pool_fees: PoolFees,
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {

		let rent = Rent::get().unwrap();

        let account_info_iter = &mut accounts.iter();
		let pool_state_account = next_account_info(account_info_iter)?;
		//TODO check that state_account.data_len() is equal to borsh serialized data length
		if !rent.is_exempt(pool_state_account.lamports(), pool_state_account.data_len()) {
			 return Err(ProgramError::AccountNotRentExempt);
		} 
		
		let authority = Pubkey::create_program_address(&[&pool_state_account.key.to_bytes(),&[nonce]], program_id)
				.or(Err(ProgramError::IncorrectProgramId))?;

		// let mut pool;

		// pool.amp_factor.init(amp_factor, Clock::get().unwrap().unix_timestamp);
		// pool.nonce = nonce;
		// pool.pool_fees = pool_fees;

		// pool.governance_key = *next_account_info(account_info_iter)?.key;
		// pool.governance_fee_key = *next_account_info(account_info_iter)?.key;

        // let mut get_key_array = || (0..TOKEN_COUNT).into_iter()
        //     .map(|_| next_account_info(account_info_iter).map(|ai| ai.key.clone()))
        //     .collect::<Result<ArrayVec<_,TOKEN_COUNT>,_>>()?
        //     .into_inner()
		// 	.or(Err(ProgramError::NotEnoughAccountKeys));

        // pool.token_mint_keys = get_key_array()?;
        // pool.token_keys = get_key_array()?;
		
		// let lp_mint_account = next_account_info(account_info_iter)?;

        Ok(())
    }
}
