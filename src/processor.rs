use arrayvec::ArrayVec;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
    program_option::COption,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    sysvar::{clock::Clock, rent::Rent, Sysvar},
};
use std::fmt;

use spl_token::{error::TokenError, state::Account as TokenState, state::Mint as MintState};

use crate::{
    amp_factor::AmpFactor,
    error::PoolError,
    instruction::PoolInstruction,
    pool_fee::{FeeRepr, PoolFee},
    state::PoolState,
};
use borsh::{BorshDeserialize, BorshSerialize};

const FEE_CHANGE_DELAY: i64 = 3 * 86400;

pub struct Processor<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Processor<TOKEN_COUNT> {
    fn get_account_array<R>(
        closure: impl FnMut(usize) -> Result<R, ProgramError>,
    ) -> Result<[R; TOKEN_COUNT], ProgramError>
    where
        R: fmt::Debug,
    {
        Ok((0..TOKEN_COUNT)
            .into_iter()
            .map(closure)
            .collect::<Result<ArrayVec<_, TOKEN_COUNT>, _>>()?
            .into_inner()
            .unwrap())
    }

    fn check_program_owner_and_unpack<T: Pack + IsInitialized>(
        packed_acc_info: &AccountInfo,
    ) -> Result<T, ProgramError> {
        spl_token::check_program_account(packed_acc_info.owner)?;
        T::unpack(&packed_acc_info.data.borrow()).or(Err(ProgramError::InvalidAccountData))
    }

    fn check_and_deserialize_pool_state(
        pool_account: &AccountInfo,
        program_id: &Pubkey,
    ) -> Result<PoolState<TOKEN_COUNT>, ProgramError> {
        if pool_account.owner != program_id {
            return Err(ProgramError::IllegalOwner);
        }

        let pool_state = PoolState::<TOKEN_COUNT>::deserialize(
            &mut &**pool_account.data.try_borrow_mut().unwrap(),
        )?;

        if !pool_state.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        Ok(pool_state)
    }

    fn serialize_pool(
        pool_state: &PoolState<TOKEN_COUNT>,
        pool_account: &AccountInfo,
    ) -> ProgramResult {
        pool_state
            .serialize(&mut *pool_account.data.try_borrow_mut().unwrap())
            .or(Err(ProgramError::AccountDataTooSmall))
    }

    fn verify_governance_signature(
        governance_account: &AccountInfo,
        pool_state: &PoolState<TOKEN_COUNT>,
    ) -> ProgramResult {
        if *governance_account.key != pool_state.governance_key {
            return Err(PoolError::InvalidGovernanceAccount.into());
        }

        if !governance_account.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        Ok(())
    }

    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = PoolInstruction::<TOKEN_COUNT>::try_from_slice(instruction_data)?;

        match instruction {
            //all this boiler-plate could probably be replaced by implementing a procedural macro on PoolInstruction
            PoolInstruction::Init {
                nonce,
                amp_factor,
                lp_fee,
                governance_fee,
            } => Self::process_init(
                nonce,
                amp_factor,
                lp_fee,
                governance_fee,
                program_id,
                accounts,
            ),
            PoolInstruction::Add {
                deposit_amounts,
                minimum_mint_amount,
            } => Self::process_add(deposit_amounts, minimum_mint_amount, program_id, accounts),
            // PoolInstruction::Remove {..} => {
            // },
            // PoolInstruction::Swap {..} => {
            // },
            PoolInstruction::PrepareFeeChange {
                lp_fee,
                governance_fee,
            } => Self::process_prepare_fee_change(lp_fee, governance_fee, program_id, accounts),
            PoolInstruction::EnactFeeChange {} => {
                Self::process_enact_fee_change(program_id, accounts)
            }
            PoolInstruction::AdjustAmpFactor {
                target_ts,
                target_value,
            } => Self::process_adjust_amp_factor(target_ts, target_value, program_id, accounts),
            PoolInstruction::HaltAmpFactorAdjustment {} => {
                Self::process_halt_amp_factor_adjustment(program_id, accounts)
            }
            _ => {
                todo!()
            }
        }
    }

    fn process_init(
        nonce: u8,
        amp_factor: u32,
        lp_fee: FeeRepr,
        governance_fee: FeeRepr,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let mut check_duplicate_and_get_next = {
            let mut keys: Vec<&Pubkey> = vec![];
            let mut account_info_iter = accounts.iter();
            move || -> Result<&AccountInfo, ProgramError> {
                let acc = next_account_info(&mut account_info_iter)?;
                if *acc.key != Pubkey::default() {
                    if keys.contains(&acc.key) {
                        return Err(PoolError::DuplicateAccount.into());
                    }
                    keys.push(acc.key);
                }
                Ok(acc)
            }
        };

        let pool_account = check_duplicate_and_get_next()?;
        if !Rent::get()
            .unwrap()
            .is_exempt(pool_account.lamports(), pool_account.data_len())
        {
            return Err(ProgramError::AccountNotRentExempt);
        }
        
        match Self::check_and_deserialize_pool_state(&pool_account, &program_id) {
            Err(ProgramError::UninitializedAccount) => (),
            Err(e) => return Err(e),
            Ok(_) => return Err(ProgramError::AccountAlreadyInitialized),
        }

        let authority =
            Pubkey::create_program_address(&[&pool_account.key.to_bytes(), &[nonce]], program_id)
                .or(Err(ProgramError::IncorrectProgramId))?;

        let lp_mint_account = check_duplicate_and_get_next()?;
        let lp_mint_state = Self::check_program_owner_and_unpack::<MintState>(lp_mint_account)?;
        if lp_mint_state.supply != 0 {
            return Err(PoolError::MintHasBalance.into());
        }
        if COption::Some(authority) != lp_mint_state.mint_authority {
            return Err(PoolError::InvalidMintAuthority.into());
        }
        if lp_mint_state.freeze_authority.is_some() {
            return Err(PoolError::MintHasFreezeAuthority.into());
        }

        let token_mint_accounts = Self::get_account_array(|_| check_duplicate_and_get_next())?;
        let token_accounts = Self::get_account_array(|_| check_duplicate_and_get_next())?;

        for i in 0..TOKEN_COUNT {
            let token_mint_account = token_mint_accounts[i];
            let token_account = token_mint_accounts[i];
            let mint_state = Self::check_program_owner_and_unpack::<MintState>(token_mint_account)?;
            let token_state = Self::check_program_owner_and_unpack::<TokenState>(token_account)?;

            //for now we enforce the same decimals across all tokens though in the future this should become more flexible
            if mint_state.decimals != lp_mint_state.decimals {
                return Err(TokenError::MintDecimalsMismatch.into());
            }
            if token_state.mint != *token_mint_account.key {
                return Err(TokenError::MintMismatch.into());
            }
            if token_state.owner != authority {
                return Err(TokenError::OwnerMismatch.into());
            }
            if token_state.amount != 0 {
                return Err(PoolError::TokenAccountHasBalance.into());
            }
            if token_state.delegate.is_some() {
                return Err(PoolError::TokenAccountHasDelegate.into());
            }
            if token_state.close_authority.is_some() {
                return Err(PoolError::TokenAccountHasCloseAuthority.into());
            }
        }

        let governance_account = check_duplicate_and_get_next()?;
        let governance_fee_account = check_duplicate_and_get_next()?;
        if (governance_fee.value != 0 || *governance_fee_account.key != Pubkey::default())
            && Self::check_program_owner_and_unpack::<TokenState>(governance_fee_account)?.mint
                != *lp_mint_account.key
        {
            return Err(TokenError::MintMismatch.into());
        }

        let to_key_array = |account_array: &[&AccountInfo; TOKEN_COUNT]| -> [Pubkey; TOKEN_COUNT] {
            account_array
                .iter()
                .map(|account| account.key.clone())
                .collect::<ArrayVec<_, TOKEN_COUNT>>()
                .into_inner()
                .unwrap()
        };

        Self::serialize_pool(
            &PoolState {
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
                prepared_lp_fee: PoolFee::default(),
                prepared_governance_fee: PoolFee::default(),
            },
            &pool_account,
        )
    }

    fn process_add(
        deposit_amounts: [u32; TOKEN_COUNT],
        minimum_mint_amount: u32,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        if deposit_amounts.iter().all(|amount| *amount == 0) {
            return Err(ProgramError::InvalidInstructionData); //TODO better error message?
        }

        let mut account_info_iter = accounts.iter();
        let pool_account = next_account_info(&mut account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        if pool_state.is_paused {
            return Err(PoolError::PoolIsPaused.into());
        }

        let pool_token_accounts = {
            let check_pool_token_account = |i| -> Result<&AccountInfo, ProgramError> {
                let pool_token_account = next_account_info(&mut account_info_iter)?;
                if *pool_token_account.key != pool_state.token_keys[i] {
                    return Err(PoolError::PoolTokenAccountExpected.into());
                }
                Ok(pool_token_account)
            };
            Self::get_account_array(check_pool_token_account)?
        };

        let user_token_accounts =
            Self::get_account_array(|_| Ok(next_account_info(&mut account_info_iter)?))?;
        
        todo!();

        Self::serialize_pool(&pool_state, &pool_account)
    }

    fn process_prepare_fee_change(
        lp_fee: FeeRepr,
        governance_fee: FeeRepr,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;
        
        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        let current_ts: i64 = Clock::get().unwrap().unix_timestamp;

        pool_state.prepared_lp_fee = PoolFee::new(lp_fee)?;
        pool_state.prepared_governance_fee = PoolFee::new(governance_fee)?;
        pool_state.governance_action_deadline = current_ts + FEE_CHANGE_DELAY;

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn process_enact_fee_change(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;
        
        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        let current_ts: i64 = Clock::get().unwrap().unix_timestamp;

        if pool_state.governance_action_deadline == 0 {
            return Err(PoolError::InvalidEnact.into());
        }
        if current_ts < pool_state.governance_action_deadline {
            return Err(PoolError::InsufficientDelay.into());
        }

        pool_state.lp_fee = pool_state.prepared_lp_fee;
        pool_state.governance_fee = pool_state.prepared_governance_fee;
        pool_state.governance_action_deadline = 0;
        pool_state.prepared_lp_fee = PoolFee::default();
        pool_state.prepared_governance_fee = PoolFee::default();

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn process_adjust_amp_factor(
        target_ts: u64,
        target_value: u32,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;
        
        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        let current_ts: i64 = Clock::get()?.unix_timestamp;

        pool_state
            .amp_factor
            .set_target(current_ts as u64, target_value, target_ts)?;

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn process_halt_amp_factor_adjustment(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;
        
        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        let current_ts: i64 = Clock::get()?.unix_timestamp;

        pool_state.amp_factor.stop_adjustment(current_ts as u64);

        Self::serialize_pool(&pool_state, pool_account)
    }
}
