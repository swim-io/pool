use arrayvec::ArrayVec;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::UnixTimestamp,
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
    instruction::{burn, mint_to, transfer},
    state::Account as TokenState,
    state::Mint as MintState,
};

use crate::{
    amp_factor::AmpFactor,
    decimal::DecimalU64,
    error::PoolError,
    instruction::{DeFiInstruction, GovernanceInstruction, PoolInstruction, PoolInstruction::*},
    invariant::Invariant,
    pool_fee::PoolFee,
    state::PoolState,
};
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::msg;
//Note - using this b/c of not all bytes read error. found from using this - https://brson.github.io/2021/06/08/rust-on-solana
// use solana_program::borsh::try_from_slice_unchecked;
const ENACT_DELAY: UnixTimestamp = 3 * 86400;

type AmountT = u64;

pub struct Processor<const TOKEN_COUNT: usize>;
impl<const TOKEN_COUNT: usize> Processor<TOKEN_COUNT> {
    pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
        msg!("[DEV] process - TOKEN_COUNT: {}", TOKEN_COUNT);
        match PoolInstruction::<TOKEN_COUNT>::try_from_slice(instruction_data)? {
            //all this boiler-plate could probably be replaced by implementing a procedural macro on PoolInstruction
            PoolInstruction::Init {
                nonce,
                amp_factor,
                lp_fee,
                governance_fee,
            } => {
                msg!("[DEV] process_init");
                Self::process_init(nonce, amp_factor, lp_fee, governance_fee, program_id, accounts)
            }

            PoolInstruction::DeFiInstruction(defi_instruction) => {
                msg!("[DEV] Processing Defi ix");
                Self::process_defi_instruction(defi_instruction, program_id, accounts)
            }
            PoolInstruction::GovernanceInstruction(governance_instruction) => {
                Self::process_governance_instruction(governance_instruction, program_id, accounts)
            }
        }
    }

    fn process_init(
        nonce: u8,
        amp_factor: DecimalU64,
        lp_fee: DecimalU64,
        governance_fee: DecimalU64,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        if lp_fee + governance_fee >= DecimalU64::from(1) {
            return Err(PoolError::InvalidFeeInput.into());
        }

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
        msg!("[DEV] TOKEN_COUNT: {}", TOKEN_COUNT);
        msg!("[DEV] checking if pool is large enought to be rent exempt");
        if !Rent::get()?.is_exempt(pool_account.lamports(), pool_account.data_len()) {
            return Err(ProgramError::AccountNotRentExempt);
        }
        msg!("[DEV] pool passed rent exmption check");
        msg!("[DEV] check_and_deserialize_pool_state");

        match Self::check_and_deserialize_pool_state(&pool_account, &program_id) {
            Err(ProgramError::UninitializedAccount) => (),
            Err(e) => return Err(e),
            Ok(_) => return Err(ProgramError::AccountAlreadyInitialized),
        }
        msg!("[DEV] passed check_and_deserialize_pool_state");

        msg!("[DEV] checking get_authority_account");
        let pool_authority_account = Self::get_pool_authority(pool_account.key, nonce, program_id)?;
        msg!("[DEV] passed get_authority_account");

        msg!("[DEV] checking lp_mint_account");
        let lp_mint_account = check_duplicate_and_get_next()?;
        let lp_mint_state = Self::check_program_owner_and_unpack::<MintState>(lp_mint_account)?;
        if lp_mint_state.supply != 0 {
            return Err(PoolError::MintHasBalance.into());
        }
        if COption::Some(pool_authority_account) != lp_mint_state.mint_authority {
            return Err(PoolError::InvalidMintAuthority.into());
        }
        if lp_mint_state.freeze_authority.is_some() {
            return Err(PoolError::MintHasFreezeAuthority.into());
        }
        msg!("[DEV] passed lp_mint_account checks");

        let token_mint_accounts = Self::get_array(|_| check_duplicate_and_get_next())?;
        msg!("[DEV] token_mint_accounts.len: {}", token_mint_accounts.len());
        let token_accounts = Self::get_array(|_| check_duplicate_and_get_next())?;
        msg!("[DEV] token_accounts.len: {}", token_accounts.len());

        for i in 0..TOKEN_COUNT {
            msg!("[DEV] checking token_mint_account & token_account [{}]", i);
            let token_mint_account = token_mint_accounts[i];
            let token_account = token_accounts[i];
            msg!("[DEV] checking mint_state[{}]. Pubkey: {}", i, token_mint_account.key);
            let mint_state = Self::check_program_owner_and_unpack::<MintState>(token_mint_account)?;
            msg!("[DEV] checking token_state[{}]. Pubkey: {}", i, token_account.key);
            //let token_state = Self::check_program_owner_and_unpack::<TokenState>(token_account)?;
            let token_state = TokenState::unpack(&token_account.data.borrow())?;

            msg!("[DEV] passed token_state[{}]", i);
            //for now we enforce the same decimals across all tokens though in the future this should become more flexible
            if mint_state.decimals != lp_mint_state.decimals {
                return Err(TokenError::MintDecimalsMismatch.into());
            }
            if token_state.mint != *token_mint_account.key {
                return Err(TokenError::MintMismatch.into());
            }
            if token_state.owner != pool_authority_account {
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
            msg!("[DEV] finished checking mint_state & token_state[{}]", i);
        }

        msg!("[DEV] checking governance & governance_fee accounts");
        let governance_account = check_duplicate_and_get_next()?;
        let governance_fee_account = check_duplicate_and_get_next()?;
        if (governance_fee != DecimalU64::from(0) || *governance_fee_account.key != Pubkey::default())
            && Self::check_program_owner_and_unpack::<TokenState>(governance_fee_account)?.mint != *lp_mint_account.key
        {
            return Err(TokenError::MintMismatch.into());
        }
        msg!("[DEV] passed checking governance & governance_fee accounts");

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
        );
        msg!("[DEV] Serialized pool");
        Ok(())
    }

    fn process_defi_instruction(
        defi_instruction: DeFiInstruction<TOKEN_COUNT>,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        msg!("[DEV] processing defi ix");
        let mut account_info_iter = accounts.iter();
        let pool_account = next_account_info(&mut account_info_iter)?;
        let pool_state = Self::check_and_deserialize_pool_state(pool_account, &program_id)?;
        msg!("[DEV] checked & deserialized pool_state");

        if pool_state.is_paused && !matches!(defi_instruction, DeFiInstruction::RemoveUniform { .. }) {
            return Err(PoolError::PoolIsPaused.into());
        }

        let pool_authority_account = next_account_info(&mut account_info_iter)?;
        if *pool_authority_account.key != Self::get_pool_authority(pool_account.key, pool_state.nonce, program_id)? {
            return Err(PoolError::InvalidPoolAuthorityAccount.into());
        }
        msg!("[DEV] checked pool authority");
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
        msg!("[DEV] checked pool token accounts");

        let pool_balances = Self::get_array(|i| {
            Ok(Self::check_program_owner_and_unpack::<TokenState>(pool_token_accounts[i])?.amount)
        })?;

        msg!("[DEV] Checked pool balances");
        let lp_mint_account = next_account_info(&mut account_info_iter)?;
        if *lp_mint_account.key != pool_state.lp_mint_key {
            return Err(PoolError::InvalidMintAccount.into());
        }
        msg!("[DEV] checked lp_mint_account");
        let lp_total_supply = Self::check_program_owner_and_unpack::<MintState>(lp_mint_account)?.supply;
        let governance_fee_account = next_account_info(&mut account_info_iter)?;
        if *governance_fee_account.key != pool_state.governance_fee_key {
            return Err(PoolError::InvalidGovernanceFeeAccout.into());
        }
        msg!("[DEV] checked governacen_fee_account");

        let user_authority_account = next_account_info(&mut account_info_iter)?;
        msg!("[DEV] checked user_authority_account");
        let user_token_accounts = Self::get_array(|_| Ok(next_account_info(&mut account_info_iter)?))?;
        msg!("[DEV] checked user_token_accounts");
        let token_program_account = next_account_info(&mut account_info_iter)?;

        msg!("[DEV] checked token_program_account");
        let governance_mint_amount = match defi_instruction {
            DeFiInstruction::Add {
                input_amounts,
                minimum_mint_amount,
            } => {
                msg!("[DEV] Processing Add ix");
                if input_amounts.iter().all(|amount| *amount == 0) {
                    return Err(ProgramError::InvalidInstructionData);
                }

                //check if the pool is currently empty
                if lp_total_supply == 0 && input_amounts.iter().any(|amount| *amount == 0) {
                    return Err(PoolError::AddRequiresAllTokens.into());
                }

                let user_lp_token_account = next_account_info(&mut account_info_iter)?;

                let (mint_amount, governance_mint_amount) = Invariant::<TOKEN_COUNT>::add(
                    &input_amounts,
                    &pool_balances,
                    pool_state.amp_factor.get(Self::get_current_ts()?),
                    pool_state.lp_fee.get(),
                    pool_state.governance_fee.get(),
                    lp_total_supply,
                );

                if mint_amount < minimum_mint_amount {
                    return Err(PoolError::OutsideSpecifiedLimits.into());
                }

                for i in 0..TOKEN_COUNT {
                    if input_amounts[i] > 0 {
                        msg!("[DEV] transferring {} for i = {}", input_amounts[i], i);
                        Self::transfer_token(
                            user_token_accounts[i],
                            pool_token_accounts[i],
                            input_amounts[i],
                            user_authority_account,
                            token_program_account,
                        )?;
                    }
                }

                Self::mint_token(
                    lp_mint_account,
                    user_lp_token_account,
                    mint_amount,
                    pool_authority_account,
                    token_program_account,
                    pool_account,
                    pool_state.nonce,
                )?;

                governance_mint_amount
            }

            DeFiInstruction::RemoveUniform {
                exact_burn_amount,
                minimum_output_amounts,
            } => {
                if exact_burn_amount == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let user_lp_token_account = next_account_info(&mut account_info_iter)?;
                let user_share = DecimalU64::from(exact_burn_amount) / lp_total_supply;

                for i in 0..TOKEN_COUNT {
                    let output_amount = (pool_balances[i] * user_share).trunc();
                    if output_amount < minimum_output_amounts[i] {
                        return Err(PoolError::OutsideSpecifiedLimits.into());
                    }
                    Self::transfer_pool_token(
                        pool_token_accounts[i],
                        user_token_accounts[i],
                        output_amount,
                        pool_authority_account,
                        token_program_account,
                        pool_account,
                        pool_state.nonce,
                    )?;
                }

                Self::burn_token(
                    user_lp_token_account,
                    lp_mint_account,
                    exact_burn_amount,
                    user_authority_account,
                    token_program_account,
                )?;

                0
            }

            DeFiInstruction::SwapExactInput {
                exact_input_amounts,
                output_token_index,
                minimum_output_amount,
            } => {
                let output_token_index = output_token_index as usize;
                if exact_input_amounts.iter().all(|amount| *amount == 0)
                    || output_token_index >= TOKEN_COUNT
                    || exact_input_amounts[output_token_index] != 0
                {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let (output_amount, governance_mint_amount) = Invariant::<TOKEN_COUNT>::swap_exact_input(
                    &exact_input_amounts,
                    output_token_index,
                    &pool_balances,
                    pool_state.amp_factor.get(Self::get_current_ts()?),
                    pool_state.lp_fee.get(),
                    pool_state.governance_fee.get(),
                    lp_total_supply,
                );

                if output_amount < minimum_output_amount {
                    return Err(PoolError::OutsideSpecifiedLimits.into());
                }

                for i in 0..TOKEN_COUNT {
                    if exact_input_amounts[i] > 0 {
                        Self::transfer_token(
                            user_token_accounts[i],
                            pool_token_accounts[i],
                            exact_input_amounts[i],
                            user_authority_account,
                            token_program_account,
                        )?;
                    }
                }

                Self::transfer_pool_token(
                    pool_token_accounts[output_token_index],
                    user_token_accounts[output_token_index],
                    output_amount,
                    pool_authority_account,
                    token_program_account,
                    pool_account,
                    pool_state.nonce,
                )?;

                governance_mint_amount
            }

            DeFiInstruction::SwapExactOutput {
                maximum_input_amount,
                input_token_index,
                exact_output_amounts,
            } => {
                let input_token_index = input_token_index as usize;

                if exact_output_amounts.iter().all(|amount| *amount == 0)
                    || input_token_index >= TOKEN_COUNT
                    || exact_output_amounts[input_token_index] != 0
                    || exact_output_amounts
                        .iter()
                        .zip(pool_balances.iter())
                        .any(|(output_amount, pool_balance)| *output_amount >= *pool_balance)
                {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let (input_amount, governance_mint_amount) = Invariant::<TOKEN_COUNT>::swap_exact_output(
                    input_token_index,
                    &exact_output_amounts,
                    &pool_balances,
                    pool_state.amp_factor.get(Self::get_current_ts()?),
                    pool_state.lp_fee.get(),
                    pool_state.governance_fee.get(),
                    lp_total_supply,
                );

                if input_amount > maximum_input_amount {
                    return Err(PoolError::OutsideSpecifiedLimits.into());
                }

                Self::transfer_token(
                    user_token_accounts[input_token_index],
                    pool_token_accounts[input_token_index],
                    input_amount,
                    user_authority_account,
                    token_program_account,
                )?;

                for i in 0..TOKEN_COUNT {
                    if exact_output_amounts[i] > 0 {
                        Self::transfer_pool_token(
                            pool_token_accounts[i],
                            user_token_accounts[i],
                            exact_output_amounts[i],
                            pool_authority_account,
                            token_program_account,
                            pool_account,
                            pool_state.nonce,
                        )?;
                    }
                }

                governance_mint_amount
            }

            DeFiInstruction::RemoveExactBurn {
                exact_burn_amount,
                output_token_index,
                minimum_output_amount,
            } => {
                let output_token_index = output_token_index as usize;
                if output_token_index >= TOKEN_COUNT || exact_burn_amount == 0 {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let user_lp_token_account = next_account_info(&mut account_info_iter)?;

                let (output_amount, governance_mint_amount) = Invariant::<TOKEN_COUNT>::remove_exact_burn(
                    exact_burn_amount,
                    output_token_index,
                    &pool_balances,
                    pool_state.amp_factor.get(Self::get_current_ts()?),
                    pool_state.lp_fee.get(),
                    pool_state.governance_fee.get(),
                    lp_total_supply,
                );

                if output_amount < minimum_output_amount {
                    return Err(PoolError::OutsideSpecifiedLimits.into());
                }

                Self::burn_token(
                    user_lp_token_account,
                    lp_mint_account,
                    exact_burn_amount,
                    user_authority_account,
                    token_program_account,
                )?;

                Self::transfer_pool_token(
                    pool_token_accounts[output_token_index],
                    user_token_accounts[output_token_index],
                    output_amount,
                    pool_authority_account,
                    token_program_account,
                    pool_account,
                    pool_state.nonce,
                )?;

                governance_mint_amount
            }

            DeFiInstruction::RemoveExactOutput {
                maximum_burn_amount,
                exact_output_amounts,
            } => {
                if exact_output_amounts.iter().all(|amount| *amount == 0)
                    || maximum_burn_amount == 0
                    || exact_output_amounts
                        .iter()
                        .zip(pool_balances.iter())
                        .any(|(output_amount, pool_balance)| *output_amount >= *pool_balance)
                {
                    return Err(ProgramError::InvalidInstructionData);
                }

                let user_lp_token_account = next_account_info(&mut account_info_iter)?;

                let (burn_amount, governance_mint_amount) = Invariant::<TOKEN_COUNT>::remove_exact_output(
                    &exact_output_amounts,
                    &pool_balances,
                    pool_state.amp_factor.get(Self::get_current_ts()?),
                    pool_state.lp_fee.get(),
                    pool_state.governance_fee.get(),
                    lp_total_supply,
                );

                if burn_amount > maximum_burn_amount {
                    return Err(PoolError::OutsideSpecifiedLimits.into());
                }

                Self::burn_token(
                    user_lp_token_account,
                    lp_mint_account,
                    burn_amount,
                    user_authority_account,
                    token_program_account,
                )?;

                for i in 0..TOKEN_COUNT {
                    if exact_output_amounts[i] > 0 {
                        Self::transfer_pool_token(
                            pool_token_accounts[i],
                            user_token_accounts[i],
                            exact_output_amounts[i],
                            pool_authority_account,
                            token_program_account,
                            pool_account,
                            pool_state.nonce,
                        )?;
                    }
                }

                governance_mint_amount
            }
        };

        if governance_mint_amount > 0 {
          msg!("[DEV] transferring {} as governance_fee", governance_mint_amount);
            Self::mint_token(
                lp_mint_account,
                governance_fee_account,
                governance_mint_amount,
                pool_authority_account,
                token_program_account,
                pool_account,
                pool_state.nonce,
            )?;
        }

        Ok(())
    }

    fn process_governance_instruction(
        governance_instruction: GovernanceInstruction<TOKEN_COUNT>,
        program_id: &Pubkey,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let pool_account = next_account_info(account_info_iter)?;
        let mut pool_state = Self::check_and_deserialize_pool_state(&pool_account, &program_id)?;

        Self::verify_governance_signature(next_account_info(account_info_iter)?, &pool_state)?;

        match governance_instruction {
            GovernanceInstruction::PrepareFeeChange { lp_fee, governance_fee } => {
                if lp_fee + governance_fee >= DecimalU64::from(1) {
                    return Err(PoolError::InvalidFeeInput.into());
                }

                pool_state.prepared_lp_fee = PoolFee::new(lp_fee)?;
                pool_state.prepared_governance_fee = PoolFee::new(governance_fee)?;
                pool_state.fee_transition_ts = Self::get_current_ts()? + ENACT_DELAY;
            }

            GovernanceInstruction::EnactFeeChange {} => {
                if pool_state.fee_transition_ts == 0 {
                    return Err(PoolError::InvalidEnact.into());
                }

                if pool_state.fee_transition_ts > Self::get_current_ts()? {
                    return Err(PoolError::InsufficientDelay.into());
                }

                pool_state.lp_fee = pool_state.prepared_lp_fee;
                pool_state.governance_fee = pool_state.prepared_governance_fee;
                pool_state.prepared_lp_fee = PoolFee::default();
                pool_state.prepared_governance_fee = PoolFee::default();
                pool_state.fee_transition_ts = 0;
            }

            GovernanceInstruction::PrepareGovernanceTransition {
                upcoming_governance_key,
            } => {
                pool_state.prepared_governance_key = upcoming_governance_key;
                pool_state.governance_transition_ts = Self::get_current_ts()? + ENACT_DELAY;
            }

            GovernanceInstruction::EnactGovernanceTransition {} => {
                if pool_state.governance_transition_ts == 0 {
                    return Err(PoolError::InvalidEnact.into());
                }

                if pool_state.governance_transition_ts > Self::get_current_ts()? {
                    return Err(PoolError::InsufficientDelay.into());
                }

                pool_state.governance_key = pool_state.prepared_governance_key;
                pool_state.prepared_governance_key = Pubkey::default();
                pool_state.governance_transition_ts = 0;
            }

            GovernanceInstruction::ChangeGovernanceFeeAccount { governance_fee_key } => {
                if governance_fee_key != Pubkey::default() {
                    let governance_fee_account = next_account_info(account_info_iter)?;
                    if *governance_fee_account.key != governance_fee_key {
                        return Err(PoolError::InvalidGovernanceFeeAccout.into());
                    }

                    let governance_fee_state =
                        Self::check_program_owner_and_unpack::<TokenState>(governance_fee_account)?;
                    if governance_fee_state.mint != pool_state.lp_mint_key {
                        return Err(TokenError::MintMismatch.into());
                    }
                } else if pool_state.governance_fee.get() == DecimalU64::from(0) {
                    return Err(PoolError::InvalidGovernanceFeeAccout.into());
                }

                pool_state.governance_fee_key = governance_fee_key;
            }

            GovernanceInstruction::AdjustAmpFactor {
                target_ts,
                target_value,
            } => {
                pool_state
                    .amp_factor
                    .set_target(Self::get_current_ts()?, target_value, target_ts)?;
            }

            GovernanceInstruction::SetPaused { paused } => {
                pool_state.is_paused = paused;
            }
        }

        Self::serialize_pool(&pool_state, pool_account)
    }

    // -------------------------------- Helper Functions --------------------------------

    fn get_pool_authority(pool_key: &Pubkey, nonce: u8, program_id: &Pubkey) -> Result<Pubkey, ProgramError> {
        Pubkey::create_program_address(&[&pool_key.to_bytes(), &[nonce]], program_id)
            .or(Err(ProgramError::IncorrectProgramId))
    }

    fn check_program_owner_and_unpack<T: Pack + IsInitialized>(account: &AccountInfo) -> Result<T, ProgramError> {
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

        let pool_state = PoolState::<TOKEN_COUNT>::deserialize(&mut &**pool_account.data.try_borrow_mut().unwrap())?;

        if !pool_state.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        Ok(pool_state)
    }

    fn serialize_pool(pool_state: &PoolState<TOKEN_COUNT>, pool_account: &AccountInfo) -> ProgramResult {
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

    fn transfer_token<'a>(
        sender_account: &AccountInfo<'a>,
        recipient_account: &AccountInfo<'a>,
        amount: AmountT,
        authority_account: &AccountInfo<'a>,
        token_program_account: &AccountInfo<'a>,
    ) -> ProgramResult {
        let transfer_ix = transfer(
            token_program_account.key,
            &sender_account.key,
            &recipient_account.key,
            &authority_account.key,
            &[],
            amount,
        )?;

        invoke(
            &transfer_ix,
            &[
                sender_account.clone(),
                recipient_account.clone(),
                authority_account.clone(),
                token_program_account.clone(),
            ],
        )
    }

    fn transfer_pool_token<'a>(
        pool_token_account: &AccountInfo<'a>,
        recipient_account: &AccountInfo<'a>,
        amount: AmountT,
        pool_authority_account: &AccountInfo<'a>,
        token_program_account: &AccountInfo<'a>,
        pool_account: &AccountInfo,
        nonce: u8,
    ) -> ProgramResult {
        let transfer_ix = transfer(
            token_program_account.key,
            &pool_token_account.key,
            &recipient_account.key,
            &pool_authority_account.key,
            &[],
            amount,
        )?;

        invoke_signed(
            &transfer_ix,
            &[
                pool_token_account.clone(),
                recipient_account.clone(),
                pool_authority_account.clone(),
                token_program_account.clone(),
            ],
            &[&[&pool_account.key.to_bytes()[..32], &[nonce]][..]],
        )
    }

    fn mint_token<'a>(
        lp_mint_account: &AccountInfo<'a>,
        recipient_account: &AccountInfo<'a>,
        mint_amount: AmountT,
        pool_authority_account: &AccountInfo<'a>,
        token_program_account: &AccountInfo<'a>,
        pool_account: &AccountInfo,
        nonce: u8,
    ) -> ProgramResult {
        let mint_ix = mint_to(
            token_program_account.key,
            lp_mint_account.key,
            recipient_account.key,
            pool_authority_account.key,
            &[],
            mint_amount,
        )?;

        invoke_signed(
            &mint_ix,
            &[
                lp_mint_account.clone(),
                recipient_account.clone(),
                pool_authority_account.clone(),
                token_program_account.clone(),
            ],
            &[&[&pool_account.key.to_bytes()[..32], &[nonce]][..]],
        )
    }

    pub fn burn_token<'a>(
        lp_account: &AccountInfo<'a>,
        lp_mint_account: &AccountInfo<'a>,
        burn_amount: AmountT,
        lp_authority: &AccountInfo<'a>,
        token_program_account: &AccountInfo<'a>,
    ) -> Result<(), ProgramError> {
        let burn_ix = burn(
            token_program_account.key,
            lp_account.key,
            lp_mint_account.key,
            lp_authority.key,
            &[],
            burn_amount,
        )?;

        invoke(
            &burn_ix,
            &[
                lp_account.clone(),
                lp_mint_account.clone(),
                lp_authority.clone(),
                token_program_account.clone(),
            ],
        )
    }

    fn get_current_ts() -> Result<UnixTimestamp, ProgramError> {
        let current_ts = Clock::get()?.unix_timestamp;
        assert!(current_ts > 0);
        Ok(current_ts)
    }

    fn get_array<R>(closure: impl FnMut(usize) -> Result<R, ProgramError>) -> Result<[R; TOKEN_COUNT], ProgramError>
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
}
