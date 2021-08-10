use arrayvec::ArrayVec;
use solana_program::{
//    msg,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    // program::{
    //     invoke,
    //     invoke_signed
    // },
    account_info::{
        next_account_info,
        AccountInfo
    },
    program_pack::{
        Pack,
        IsInitialized,
    },
    program_option::COption,
    pubkey::Pubkey,
    sysvar::{
        rent::Rent,
		clock::Clock,
        Sysvar
    },
};

use spl_token::{
    state::Account as TokenAccount,
    state::Mint,
    error::TokenError,
};

use crate::{
	instruction::PoolInstruction,
	pool_fee::{
        FeeRepr,
        PoolFee,
    },
    amp_factor::AmpFactor,
	state::Pool,
    error::PoolError,
};
use borsh::{BorshDeserialize, BorshSerialize};

const FEE_CHANGE_DELAY: i64 = 3 * 86400;

pub struct Processor<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Processor<TOKEN_COUNT> {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = PoolInstruction::<TOKEN_COUNT>::try_from_slice(instruction_data)?;

        match instruction { //all this boiler-plate could probably be replaced by implementing a procedural macro on PoolInstruction
            PoolInstruction::Init{nonce, amp_factor, lp_fee, governance_fee} => {
                Self::process_init(nonce, amp_factor, lp_fee, governance_fee, program_id, accounts)?;
            },
            // PoolInstruction::Add {..} => {
            // },
            // PoolInstruction::Remove {..} => {
            // },
            // PoolInstruction::Swap {..} => {
            // },
            PoolInstruction::PrepareFeeChange{lp_fee, governance_fee} => {
                Self::process_prepare_fee_change(lp_fee, governance_fee, program_id, accounts)?;
            },
            PoolInstruction::EnactFeeChange{} => {
                Self::process_enact_fee_change(program_id, accounts)?;
            },
            PoolInstruction::AdjustAmpFactor{target_ts, target_value} => {
                Self::process_adjust_amp_factor(target_ts, target_value, program_id, accounts)?;
            },
            PoolInstruction::HaltAmpFactorAdjustment{} => {
                Self::process_halt_amp_factor_adjustment(program_id, accounts)?;
            },
            _ => {todo!();}
        }
        Ok(())
    }

    fn check_program_owner_and_unpack<T: Pack+IsInitialized>(packed_acc_info : &AccountInfo) -> Result<T, ProgramError> {
        spl_token::check_program_account(packed_acc_info.owner)?;
        T::unpack(&packed_acc_info.data.borrow()).or(Err(ProgramError::InvalidAccountData))
    }

    fn process_init(
        nonce: u8,
        amp_factor: u32,
        lp_fee: FeeRepr,
        governance_fee: FeeRepr,
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {

        let mut check_duplicate_and_get_next = {
            let mut keys: Vec<Pubkey> = vec![];
            let mut account_info_iter = accounts.iter();
            move || -> Result<&AccountInfo, ProgramError> {
                let acc = next_account_info(&mut account_info_iter)?;
                if *acc.key != Pubkey::default() {
                    if  keys.contains(acc.key) {
                        return Err(PoolError::DuplicateAccount.into());
                    }
                    keys.push(acc.key.clone());
                }
                Ok(acc)
            }
        };

        let pool_state_account = check_duplicate_and_get_next()?;
        if !Rent::get().unwrap().is_exempt(pool_state_account.lamports(), pool_state_account.data_len()) {
            return Err(ProgramError::AccountNotRentExempt);
        }
        if pool_state_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }
        if Pool::<TOKEN_COUNT>::deserialize(&mut &**pool_state_account.data.try_borrow_mut().unwrap())?.is_initialized() {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        let authority = Pubkey::create_program_address(&[&pool_state_account.key.to_bytes(), &[nonce]], program_id)
			.or(Err(ProgramError::IncorrectProgramId))?;
        
        let lp_mint_account = check_duplicate_and_get_next()?;
        let lp_mint = Self::check_program_owner_and_unpack::<Mint>(lp_mint_account)?;
        if lp_mint.supply != 0 {
            return Err(PoolError::MintHasBalance.into());
        }
        if COption::Some(authority) != lp_mint.mint_authority {
            return Err(PoolError::InvalidMintAuthority.into());
        }
        if lp_mint.freeze_authority.is_some() {
            return Err(PoolError::MintHasFreezeAuthority.into());
        }
        
        let mut get_account_array = || -> Result<[&AccountInfo; TOKEN_COUNT], ProgramError> {
            Ok((0..TOKEN_COUNT).into_iter()
                .map(|_| check_duplicate_and_get_next())
                .collect::<Result<ArrayVec<_,TOKEN_COUNT>,_>>()?
                .into_inner()
                .unwrap()
            )
        };
        let token_mint_accounts = get_account_array()?;
        let token_accounts = get_account_array()?;

        for i in 0..TOKEN_COUNT {
            let token_mint_account = token_mint_accounts[i];
            let token_account = token_mint_accounts[i];
            let mint = Self::check_program_owner_and_unpack::<Mint>(token_mint_account)?;
            let token = Self::check_program_owner_and_unpack::<TokenAccount>(token_account)?;

            if mint.decimals != lp_mint.decimals {
                return Err(TokenError::MintDecimalsMismatch.into());
            }
            if token.mint != *token_mint_account.key {
                return Err(TokenError::MintMismatch.into());
            }
            if token.owner != authority {
                return Err(TokenError::OwnerMismatch.into());
            }
            if token.amount != 0 {
                return Err(PoolError::TokenAccountHasBalance.into());
            }
            if token.delegate.is_some() {
                return Err(PoolError::TokenAccountHasDelegate.into());
            }
            if token.close_authority.is_some() {
                return Err(PoolError::TokenAccountHasCloseAuthority.into());
            }
        }

        let governance_account = check_duplicate_and_get_next()?;
        let governance_fee_account =  check_duplicate_and_get_next()?;
        if (governance_fee.value != 0 || *governance_fee_account.key != Pubkey::default()) &&
            Self::check_program_owner_and_unpack::<TokenAccount>(governance_fee_account)?.mint != *lp_mint_account.key {
            return Err(TokenError::MintMismatch.into());
        }

        let to_key_array = |account_array: &[&AccountInfo; TOKEN_COUNT]| -> [Pubkey; TOKEN_COUNT] {
            account_array
                .iter()
                .map(|account| account.key.clone())
                .collect::<ArrayVec<_,TOKEN_COUNT>>()
                .into_inner()
                .unwrap()
        };

		Pool{
            nonce,
            is_paused: false,
            amp_factor: AmpFactor::new(amp_factor)?,
            lp_fee: PoolFee::new(lp_fee)?,
            governance_fee: PoolFee::new(governance_fee)?,
            token_mint_keys: to_key_array(&token_mint_accounts),
            token_keys: to_key_array(&token_accounts),
            lp_mint_key: lp_mint_account.key.clone(),
            governance_key: governance_account.key.clone(),
            governance_fee_key: governance_fee_account.key.clone(),
            prepared_governance_key: Pubkey::default(),
            governance_action_deadline: 0,
            future_lp_fee: PoolFee::default(),
            future_governance_fee: PoolFee::default(),
        }
            .serialize(&mut *pool_state_account.data.try_borrow_mut().unwrap())
            .or(Err(ProgramError::AccountDataTooSmall))
    }


    // ---------------

    fn process_prepare_fee_change(
        lp_fee: FeeRepr,
        governance_fee: FeeRepr,
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_state_account = next_account_info(account_info_iter)?;

        if pool_state_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let mut pool = Pool::<TOKEN_COUNT>::deserialize(&mut &**pool_state_account.data.try_borrow_mut().unwrap())?;

        if !pool.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        let governance_account = next_account_info(account_info_iter)?;
        if *governance_account.key != pool.governance_key {
            return Err(PoolError::InvalidGovernanceAccount.into())
        }

        let current_ts: i64 = Clock::get().unwrap().unix_timestamp;
        
        pool.future_lp_fee = PoolFee::new(lp_fee)?;
        pool.future_governance_fee = PoolFee::new(governance_fee)?;
        pool.governance_action_deadline = current_ts + FEE_CHANGE_DELAY;

        pool.serialize(&mut *pool_state_account.data.try_borrow_mut().unwrap()).or(Err(ProgramError::AccountDataTooSmall))
    }

    fn process_enact_fee_change(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_state_account = next_account_info(account_info_iter)?;

        if pool_state_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let mut pool = Pool::<TOKEN_COUNT>::deserialize(&mut &**pool_state_account.data.try_borrow_mut().unwrap())?;

        if !pool.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        let governance_account = next_account_info(account_info_iter)?;
        if *governance_account.key != pool.governance_key {
            return Err(PoolError::InvalidGovernanceAccount.into())
        }

        let current_ts: i64 = Clock::get().unwrap().unix_timestamp;

        if pool.governance_action_deadline == 0 {
            return Err(PoolError::InvalidEnact.into());
        }
        if current_ts < pool.governance_action_deadline {
            return Err(PoolError::InsufficientDelay.into());
        }

        pool.lp_fee = pool.future_lp_fee;
        pool.governance_fee = pool.future_governance_fee;
        pool.governance_action_deadline = 0;
        pool.future_lp_fee = PoolFee::default();
        pool.future_governance_fee = PoolFee::default();
        
        pool.serialize(&mut *pool_state_account.data.try_borrow_mut().unwrap()).or(Err(ProgramError::AccountDataTooSmall))
    }

    fn process_adjust_amp_factor(
        target_ts: u64,
        target_value: u32,
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_state_account = next_account_info(account_info_iter)?;

        if pool_state_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let mut pool = Pool::<TOKEN_COUNT>::deserialize(&mut &**pool_state_account.data.try_borrow_mut().unwrap())?;

        if !pool.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        let governance_account = next_account_info(account_info_iter)?;
        if *governance_account.key != pool.governance_key {
            return Err(PoolError::InvalidGovernanceAccount.into())
        }

        let current_ts: i64 = Clock::get().unwrap().unix_timestamp;

        pool.amp_factor.set_target(current_ts as u64, target_value, target_ts).unwrap();
        
        pool.serialize(&mut *pool_state_account.data.try_borrow_mut().unwrap()).unwrap();

        Ok(())
    }

    fn process_halt_amp_factor_adjustment(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_state_account = next_account_info(account_info_iter)?;

        if pool_state_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let mut pool = Pool::<TOKEN_COUNT>::deserialize(&mut &**pool_state_account.data.try_borrow_mut().unwrap())?;

        if !pool.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        let governance_account = next_account_info(account_info_iter)?;
        if *governance_account.key != pool.governance_key {
            return Err(PoolError::InvalidGovernanceAccount.into())
        }

        let current_ts: i64 = Clock::get().unwrap().unix_timestamp;

        pool.amp_factor.stop_adjustment(current_ts as u64);

        pool.serialize(&mut *pool_state_account.data.try_borrow_mut().unwrap()).unwrap();

        Ok(())
    }
}
