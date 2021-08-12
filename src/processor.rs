use arrayvec::ArrayVec;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_option::COption,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    sysvar::{clock::Clock, rent::Rent, Sysvar},
};
use std::fmt;

use spl_token::{
    error::TokenError,
    instruction::{mint_to, transfer},
    state::Account as TokenState,
    state::Mint as MintState,
};

use crate::{
    amp_factor::AmpFactor,
    error::PoolError,
    instruction::PoolInstruction,
    pool_fee::{FeeRepr, PoolFee},
    state::PoolState,
};
use borsh::{BorshDeserialize, BorshSerialize};

const ENACT_DELAY: i64 = 3 * 86400;

pub struct Processor<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Processor<TOKEN_COUNT> {
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
            } => Self::process_init(nonce, amp_factor, lp_fee, governance_fee, program_id, accounts),

            PoolInstruction::Add {
                input_amounts,
                minimum_lp_amount,
            } => Self::process_add(input_amounts, minimum_lp_amount, program_id, accounts),

            PoolInstruction::RemoveOneExact {
                exact_burn_amount,
                output_token_index,
                minimum_output_amount,
            } => Self::process_remove_one_exact(exact_burn_amount, output_token_index, minimum_output_amount, program_id, accounts),

            PoolInstruction::RemoveAllExact {
                exact_burn_amount,
                minimum_output_amounts,
            } => Self::process_remove_all_exact(exact_burn_amount, minimum_output_amounts, program_id, accounts),

            PoolInstruction::RemoveBounded {
                maximum_burn_amount,
                output_amounts,
            } => Self::process_remove_bounded(maximum_burn_amount, output_amounts, program_id, accounts),

            PoolInstruction::Swap {
                input_amounts,
                output_token_index,
                minimum_output_amount,
            } => Self::process_swap(input_amounts, output_token_index, minimum_output_amount, program_id, accounts),

            PoolInstruction::PrepareFeeChange {
                lp_fee,
                governance_fee,
            } => Self::process_prepare_fee_change(&lp_fee, &governance_fee, program_id, accounts),

            PoolInstruction::EnactFeeChange {
            } => Self::process_enact_fee_change(program_id, accounts),
            
            PoolInstruction::PrepareGovernanceTransition {
                upcoming_governance_key,
            } => Self::process_prepare_governance_transition(&upcoming_governance_key, program_id, accounts),

            PoolInstruction::EnactGovernanceTransition {
            } => Self::process_enact_governance_transition(program_id, accounts),

            PoolInstruction::ChangeGovernanceFeeAccount {
                governance_fee_key
            } => Self::process_change_governance_fee_account(&governance_fee_key, program_id, accounts),

            PoolInstruction::AdjustAmpFactor {
                target_ts,
                target_value,
            } => Self::process_adjust_amp_factor(target_ts, target_value, program_id, accounts),

            PoolInstruction::HaltAmpFactorAdjustment {
            } => Self::process_halt_amp_factor_adjustment(program_id, accounts),

            PoolInstruction::SetPaused {
                paused
            } => Self::process_set_paused(paused, program_id, accounts),
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
        if !Rent::get()?.is_exempt(pool_account.lamports(), pool_account.data_len()) {
            return Err(ProgramError::AccountNotRentExempt);
        }

        match Self::check_and_deserialize_pool_state(&pool_account, &program_id) {
            Err(ProgramError::UninitializedAccount) => (),
            Err(e) => return Err(e),
            Ok(_) => return Err(ProgramError::AccountAlreadyInitialized),
        }

        let authority = Self::get_pool_authority(pool_account.key, nonce, program_id)?;

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

        let token_mint_accounts = Self::get_array(|_| check_duplicate_and_get_next())?;
        let token_accounts = Self::get_array(|_| check_duplicate_and_get_next())?;

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
                governance_transition_ts: 0,
                prepared_lp_fee: PoolFee::default(),
                prepared_governance_fee: PoolFee::default(),
                fee_transition_ts: 0,
            },
            &pool_account,
        )
    }

    fn process_add(
        input_amounts: [u64; TOKEN_COUNT],
        minimum_lp_amount: u64,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        if input_amounts.iter().all(|amount| *amount == 0) {
            return Err(ProgramError::InvalidInstructionData); //TODO better error message?
        }

        let mut account_info_iter = accounts.iter();
        let pool_account = next_account_info(&mut account_info_iter)?;
        let pool_state = Self::check_and_deserialize_pool_state(pool_account, &program_id)?;

        if pool_state.is_paused {
            return Err(PoolError::PoolIsPaused.into());
        }

        let user_authority_account = next_account_info(&mut account_info_iter)?;
        let user_lp_token_account = next_account_info(&mut account_info_iter)?;
        let user_token_accounts =
            Self::get_array(|_| Ok(next_account_info(&mut account_info_iter)?))?;
        let pool_token_accounts = {
            let check_pool_token_account = |i| {
                // -> Result<&AccountInfo, ProgramError>
                let pool_token_account = next_account_info(&mut account_info_iter)?;
                if *pool_token_account.key != pool_state.token_keys[i] {
                    return Err(PoolError::PoolTokenAccountExpected.into());
                }
                Ok(pool_token_account)
            };
            Self::get_array(check_pool_token_account)?
        };

        let pool_authority = next_account_info(&mut account_info_iter)?;
        if *pool_authority.key
            != Self::get_pool_authority(pool_account.key, pool_state.nonce, program_id)?
        {
            return Err(PoolError::InvalidPoolAuthorityAccount.into());
        }

        let lp_mint_account = next_account_info(&mut account_info_iter)?;
        if *lp_mint_account.key != pool_state.lp_mint_key {
            return Err(PoolError::InvalidMintAccount.into());
        }
        let token_program_account = next_account_info(&mut account_info_iter)?;
        let pool_token_states = Self::get_array(|i| {
            Self::check_program_owner_and_unpack::<TokenState>(pool_token_accounts[i])
        })?;

        //check if the pool is currently empty (if one token balance is 0, all token balances must be zero)
        if pool_token_states[0].amount == 0 && input_amounts.iter().any(|amount| *amount == 0) {
            return Err(PoolError::AddRequiresAllTokens.into());
        }

        let mint_amount = 0u64; //TODO

        if mint_amount < minimum_lp_amount {
            return Err(PoolError::SlippageExceeded.into());
        }

        for i in 0..TOKEN_COUNT {
            let transfer_ix = transfer(
                token_program_account.key,
                &user_token_accounts[i].key,
                &pool_token_accounts[i].key,
                &user_authority_account.key,
                &[],
                input_amounts[i],
            )?;

            invoke(
                &transfer_ix,
                &[
                    user_token_accounts[i].clone(),
                    pool_token_accounts[i].clone(),
                    user_authority_account.clone(),
                    token_program_account.clone(),
                ],
            )?;
        }

        let mint_ix = mint_to(
            token_program_account.key,
            lp_mint_account.key,
            user_lp_token_account.key,
            pool_authority.key,
            &[],
            mint_amount,
        )?;

        invoke_signed(
            &mint_ix,
            &[
                lp_mint_account.clone(),
                user_lp_token_account.clone(),
                pool_authority.clone(),
                token_program_account.clone(),
            ],
            &[&[&pool_account.key.to_bytes()[..32], &[pool_state.nonce]][..]],
        )
    }

    fn process_remove_one_exact(
        exact_burn_amount: u64,
        output_token_index: u8,
        minimum_output_amount: u64,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        //TODO
        Ok(())
    }

    fn process_remove_all_exact(
        exact_burn_amount: u64,
        minimum_output_amounts: [u64; TOKEN_COUNT],
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        //TODO
        Ok(())
    }

    fn process_remove_bounded(
        maximum_burn_amount: u64,
        output_amounts: [u64; TOKEN_COUNT],
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        //TODO
        Ok(())
    }

    fn process_swap(
        input_amounts: [u64; TOKEN_COUNT],
        output_token_index: u8,
        minimum_output_amount: u64,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        //TODO
        Ok(())
    }

    fn process_prepare_fee_change(
        lp_fee: &FeeRepr,
        governance_fee: &FeeRepr,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        pool_state.prepared_lp_fee = PoolFee::new(*lp_fee)?;
        pool_state.prepared_governance_fee = PoolFee::new(*governance_fee)?;
        pool_state.fee_transition_ts = Clock::get()?.unix_timestamp + ENACT_DELAY;

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn process_enact_fee_change(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        if pool_state.fee_transition_ts == 0 {
            return Err(PoolError::InvalidEnact.into());
        }

        if pool_state.fee_transition_ts > Clock::get()?.unix_timestamp {
            return Err(PoolError::InsufficientDelay.into());
        }

        pool_state.lp_fee = pool_state.prepared_lp_fee;
        pool_state.governance_fee = pool_state.prepared_governance_fee;
        pool_state.prepared_lp_fee = PoolFee::default();
        pool_state.prepared_governance_fee = PoolFee::default();
        pool_state.fee_transition_ts = 0;

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn process_prepare_governance_transition(
        upcoming_governance_key: &Pubkey,
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        pool_state.prepared_governance_key = *upcoming_governance_key;
        pool_state.governance_transition_ts = Clock::get()?.unix_timestamp + ENACT_DELAY;

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn process_enact_governance_transition(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        if pool_state.governance_transition_ts == 0 {
            return Err(PoolError::InvalidEnact.into());
        }

        if pool_state.governance_transition_ts > Clock::get()?.unix_timestamp {
            return Err(PoolError::InsufficientDelay.into());
        }

        pool_state.governance_key = pool_state.prepared_governance_key;
        pool_state.prepared_governance_key = Pubkey::default();
        pool_state.governance_transition_ts = 0;

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn process_change_governance_fee_account(
        governance_fee_key: &Pubkey,
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        if *governance_fee_key != Pubkey::default() {
            let governance_fee_account = next_account_info(account_info_iter)?;
            if *governance_fee_account.key != *governance_fee_key {
                return Err(PoolError::InvalidGovernanceFeeAccout.into())
            }

            let governance_fee_state = Self::check_program_owner_and_unpack::<TokenState>(governance_fee_account)?;
            if governance_fee_state.mint != pool_state.lp_mint_key {
                return Err(TokenError::MintMismatch.into());
            }
        }
        else if pool_state.governance_fee.get().value == 0 {
            return Err(PoolError::InvalidGovernanceFeeAccout.into())
        }

        pool_state.governance_fee_key = *governance_fee_key;

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

    fn process_set_paused(
        paused: bool,
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        pool_state.is_paused = paused;

        Self::serialize_pool(&pool_state, pool_account)
    }

    fn get_array<R>(
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
            .unwrap()) //we can unwrap because we know that there is enough capacity
    }

    fn get_pool_authority(
        pool_key: &Pubkey,
        nonce: u8,
        program_id: &Pubkey,
    ) -> Result<Pubkey, ProgramError> {
        Pubkey::create_program_address(&[&pool_key.to_bytes(), &[nonce]], program_id)
            .or(Err(ProgramError::IncorrectProgramId))
    }

    fn check_program_owner_and_unpack<T: Pack + IsInitialized>(
        account: &AccountInfo,
    ) -> Result<T, ProgramError> {
        spl_token::check_program_account(account.owner)?;
        T::unpack(&account.data.borrow()).or(Err(ProgramError::InvalidAccountData))
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
}
