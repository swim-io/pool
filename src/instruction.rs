use crate::decimal::DecimalU64;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    clock::UnixTimestamp,
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};

type AmountT = u64;
type DecT = DecimalU64;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PoolInstruction<const TOKEN_COUNT: usize> {
    /// Initializes a new pool
    ///
    /// Accounts expected by this instruction:
    ///     0. `[w]` The pool state account to initalize
    ///     1. `[]` LP Token Mint. Must be empty, owned by authority
    ///             authority isn't passed in but programatically derived
    ///     2. ..2 + TOKEN_COUNT  `[]` Token mint accounts
    ///     3. ..2 + (2 * TOKEN_COUNT) `[]` Token accounts. Must be empty
    ///     4. ..3 + (2 * TOKEN_COUNT) `[]` Governance account
    ///     5. ..4 + (2 * TOKEN_COUNT) `[]` Governance Fee account.
    Init {
        nonce: u8,
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
    },
    DeFiInstruction(DeFiInstruction<TOKEN_COUNT>),
    GovernanceInstruction(GovernanceInstruction<TOKEN_COUNT>),
}

/// Creates an `Init` instruction
pub fn create_init_ix<const TOKEN_COUNT: usize>(
    program_id: &Pubkey,
    pool: &Pubkey,
    lp_mint: &Pubkey,
    token_mints: [Pubkey; TOKEN_COUNT],
    token_accounts: [Pubkey; TOKEN_COUNT],
    governance_account: &Pubkey,
    governance_fee_account: &Pubkey,
    nonce: u8,
    amp_factor: DecT,
    lp_fee: DecT,
    governance_fee: DecT,
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new(*pool, false),
        AccountMeta::new_readonly(*lp_mint, false),
    ];
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new_readonly(token_mints[i], false));
    }
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new_readonly(token_accounts[i], false));
    }
    accounts.push(AccountMeta::new_readonly(*governance_account, false));
    accounts.push(AccountMeta::new_readonly(*governance_fee_account, false));
    let data = PoolInstruction::<TOKEN_COUNT>::Init {
        nonce,
        amp_factor,
        lp_fee,
        governance_fee,
    }
    .try_to_vec()?;

    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data,
    })
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum DeFiInstruction<const TOKEN_COUNT: usize> {
    /// Adds/Deposits the specified input_amounts and mints
    /// at least `minimum_mint_amount` LP tokens
    ///
    /// Accounts expected by this instruction:
    ///     0. `[]` The pool state account
    ///     1. `[]` pool authority
    ///     2. ..2 + TOKEN_COUNT `[w]` pool's token accounts
    ///     3. ..3 + TOKEN_COUNT `[w]` LP Token Mint
    ///     4. ..4 + TOKEN_COUNT `[w]` governance_fee_account
    ///     5. ..5 + TOKEN_COUNT `[s]` user transfer authority account
    ///     6. ..6 + TOKEN_COUNT `[w]` user token accounts
    ///     7. ..6 + (2 * TOKEN_COUNT) `[]` SPL token program account
    ///     8. ..7 + (2 * TOKEN_COUNT) `[w]` user LP token account   
    Add {
        input_amounts: [AmountT; TOKEN_COUNT],
        minimum_mint_amount: AmountT,
    },
    /// Swaps in the exact specified amounts for
    /// at least `minimum_out_amount` of the output_token specified
    /// by output_token_index
    ///
    /// Accounts expected by this instruction:
    ///     0. `[]` The pool state account
    ///     1. `[]` pool authority
    ///     2. ..2 + TOKEN_COUNT `[w]` pool's token accounts
    ///     3. ..3 + TOKEN_COUNT `[w]` LP Token Mint
    ///     4. ..4 + TOKEN_COUNT `[w]` governance_fee_account
    ///     5. ..5 + TOKEN_COUNT `[s]` user transfer authority account
    ///     6. ..6 + TOKEN_COUNT `[w]` user token accounts
    ///     7. ..6 + (2 * TOKEN_COUNT) `[]` SPL token program account
    SwapExactInput {
        exact_input_amounts: [AmountT; TOKEN_COUNT],
        output_token_index: u8,
        minimum_output_amount: AmountT,
    },
    /// Swaps in at most `maximum_input_amount` of the input token specified by
    /// `input_token_index` for the exact_output_amounts
    ///
    /// Accounts expected by this instruction:
    ///     0. `[]` The pool state account
    ///     1. `[]` pool authority
    ///     2. ..2 + TOKEN_COUNT `[w]` pool's token accounts
    ///     3. ..3 + TOKEN_COUNT `[w]` LP Token Mint
    ///     4. ..4 + TOKEN_COUNT `[w]` governance_fee_account
    ///     5. ..5 + TOKEN_COUNT `[s]` user transfer authority account
    ///     6. ..6 + TOKEN_COUNT `[w]` user token accounts
    ///     7. ..6 + (2 * TOKEN_COUNT) `[]` SPL token program account
    SwapExactOutput {
        maximum_input_amount: AmountT,
        input_token_index: u8,
        exact_output_amounts: [AmountT; TOKEN_COUNT],
    },

    /// Withdraw at least the number of tokens specified by `minimum_output_amounts` by
    /// burning `exact_burn_amount` of LP tokens
    /// Final withdrawal amounts are based on current deposit ratios
    ///
    ///
    /// Accounts expected by this instruction:
    ///     0. `[]` The pool state account
    ///     1. `[]` pool authority
    ///     2. ..2 + TOKEN_COUNT `[w]` pool's token accounts
    ///     3. ..3 + TOKEN_COUNT `[w]` LP Token Mint
    ///     4. ..4 + TOKEN_COUNT `[w]` governance_fee_account
    ///     5. ..5 + TOKEN_COUNT `[s]` user transfer authority account
    ///     6. ..6 + TOKEN_COUNT `[w]` user token accounts
    ///     7. ..6 + (2 * TOKEN_COUNT) `[]` SPL token program account
    ///     8. ..7 + (2 * TOKEN_COUNT) `[w]` user LP token account to withdraw/burn from  
    RemoveUniform {
        exact_burn_amount: AmountT,
        minimum_output_amounts: [AmountT; TOKEN_COUNT],
    },
    /// Withdraw at least `minimum_output_amount` of output token specified by `output_token_index` by
    /// burning `exact_burn_amount` of LP tokens
    ///
    ///
    /// Accounts expected by this instruction:
    ///     0. `[]` The pool state account
    ///     1. `[]` pool authority
    ///     2. ..2 + TOKEN_COUNT `[w]` pool's token accounts
    ///     3. ..3 + TOKEN_COUNT `[w]` LP Token Mint
    ///     4. ..4 + TOKEN_COUNT `[w]` governance_fee_account
    ///     5. ..5 + TOKEN_COUNT `[s]` user transfer authority account
    ///     6. ..6 + TOKEN_COUNT `[w]` user token accounts
    ///     7. ..6 + (2 * TOKEN_COUNT) `[]` SPL token program account
    ///     8. ..7 + (2 * TOKEN_COUNT) `[w]` user LP token account to withdraw/burn from  
    RemoveExactBurn {
        exact_burn_amount: AmountT,
        output_token_index: u8,
        minimum_output_amount: AmountT,
    },
    /// Withdraw exactly the number of output tokens specified by `exact_output_amount`
    /// by burning at most `maximum_burn_amounts` of LP tokens
    ///
    /// Accounts expected by this instruction:
    ///     0. `[]` The pool state account
    ///     1. `[]` pool authority
    ///     2. ..2 + TOKEN_COUNT `[w]` pool's token accounts
    ///     3. ..3 + TOKEN_COUNT `[w]` LP Token Mint
    ///     4. ..4 + TOKEN_COUNT `[w]` governance_fee_account
    ///     5. ..5 + TOKEN_COUNT `[s]` user transfer authority account
    ///     6. ..6 + TOKEN_COUNT `[w]` user token accounts
    ///     7. ..6 + (2 * TOKEN_COUNT) `[]` SPL token program account
    ///     8. ..7 + (2 * TOKEN_COUNT) `[w]` user LP token account to withdraw/burn from  
    RemoveExactOutput {
        maximum_burn_amount: AmountT,
        exact_output_amounts: [AmountT; TOKEN_COUNT],
    },
}

/// Creates an `Add` DefiInstruction
pub fn create_add_ix<const TOKEN_COUNT: usize>(
    program_id: &Pubkey,
    pool: &Pubkey,
    authority: &Pubkey,
    pool_token_accounts: [Pubkey; TOKEN_COUNT],
    lp_mint: &Pubkey,
    governance_fee_account: &Pubkey,
    user_transfer_authority: &Pubkey,
    user_token_accounts: [Pubkey; TOKEN_COUNT],
    token_program_account: &Pubkey,
    user_lp_token_account: &Pubkey,
    input_amounts: [AmountT; TOKEN_COUNT],
    minimum_mint_amount: AmountT,
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new_readonly(*pool, false),
        AccountMeta::new_readonly(*authority, false),
    ];
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(pool_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new(*lp_mint, false));
    accounts.push(AccountMeta::new(*governance_fee_account, false));

    // used from SPL binary-oracle-pair. not actually necessary since the implementation only supports
    //  that using a separate keypair
    accounts.push(AccountMeta::new_readonly(
        *user_transfer_authority,
        authority != user_transfer_authority,
    ));
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(user_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new_readonly(*token_program_account, false));
    accounts.push(AccountMeta::new(*user_lp_token_account, false));

    let d = DeFiInstruction::<TOKEN_COUNT>::Add {
        input_amounts,
        minimum_mint_amount,
    };
    let data = PoolInstruction::<TOKEN_COUNT>::DeFiInstruction(d).try_to_vec()?;

    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data,
    })
}

/// Creates a `SwapExactInput` DefiInstruction
pub fn create_swap_exact_input_ix<const TOKEN_COUNT: usize>(
    program_id: &Pubkey,
    pool: &Pubkey,
    authority: &Pubkey,
    pool_token_accounts: [Pubkey; TOKEN_COUNT],
    lp_mint: &Pubkey,
    governance_fee_account: &Pubkey,
    user_transfer_authority: &Pubkey,
    user_token_accounts: [Pubkey; TOKEN_COUNT],
    token_program_account: &Pubkey,
    exact_input_amounts: [AmountT; TOKEN_COUNT],
    output_token_index: u8,
    minimum_output_amount: AmountT,
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new_readonly(*pool, false),
        AccountMeta::new_readonly(*authority, false),
    ];
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(pool_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new(*lp_mint, false));
    accounts.push(AccountMeta::new(*governance_fee_account, false));

    // used from SPL binary-oracle-pair. not actually necessary since the implementation only supports
    //  that using a separate keypair
    accounts.push(AccountMeta::new_readonly(
        *user_transfer_authority,
        authority != user_transfer_authority,
    ));
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(user_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new_readonly(*token_program_account, false));

    let d = DeFiInstruction::<TOKEN_COUNT>::SwapExactInput {
        exact_input_amounts,
        output_token_index,
        minimum_output_amount,
    };

    let data = PoolInstruction::<TOKEN_COUNT>::DeFiInstruction(d).try_to_vec()?;
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data,
    })
}

/// Creates a `RemoveUniform` DefiInstruction
pub fn create_remove_uniform_ix<const TOKEN_COUNT: usize>(
    program_id: &Pubkey,
    pool: &Pubkey,
    authority: &Pubkey,
    pool_token_accounts: [Pubkey; TOKEN_COUNT],
    lp_mint: &Pubkey,
    governance_fee_account: &Pubkey,
    user_transfer_authority: &Pubkey,
    user_token_accounts: [Pubkey; TOKEN_COUNT],
    token_program_account: &Pubkey,
    user_lp_token_account: &Pubkey,
    exact_burn_amount: AmountT,
    minimum_output_amounts: [AmountT; TOKEN_COUNT],
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new_readonly(*pool, false),
        AccountMeta::new_readonly(*authority, false),
    ];
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(pool_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new(*lp_mint, false));
    accounts.push(AccountMeta::new(*governance_fee_account, false));

    // used from SPL binary-oracle-pair. not actually necessary since the implementation only supports
    //  that using a separate keypair
    accounts.push(AccountMeta::new_readonly(
        *user_transfer_authority,
        authority != user_transfer_authority,
    ));
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(user_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new_readonly(*token_program_account, false));
    accounts.push(AccountMeta::new(*user_lp_token_account, false));

    let d = DeFiInstruction::<TOKEN_COUNT>::RemoveUniform {
        exact_burn_amount,
        minimum_output_amounts,
    };

    let data = PoolInstruction::<TOKEN_COUNT>::DeFiInstruction(d).try_to_vec()?;
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data,
    })
}

/// Creates a `RemoveExactBurn` DefiInstruction
/// Accounts expected by this instruction:
///     0. `[]` The pool state account
///     1. `[]` pool authority
///     2. ..2 + TOKEN_COUNT `[w]` pool's token accounts
///     3. ..3 + TOKEN_COUNT `[w]` LP Token Mint
///     4. ..4 + TOKEN_COUNT `[w]` governance_fee_account
///     5. ..5 + TOKEN_COUNT `[s]` user transfer authority account
///     6. ..6 + TOKEN_COUNT `[w]` user token accounts
///     7. ..6 + (2 * TOKEN_COUNT) `[]` SPL token program account
///     8. ..7 + (2 * TOKEN_COUNT) `[w]` user LP token account to withdraw/burn from  
pub fn create_remove_exact_burn_ix<const TOKEN_COUNT: usize>(
    program_id: &Pubkey,
    pool: &Pubkey,
    authority: &Pubkey,
    pool_token_accounts: [Pubkey; TOKEN_COUNT],
    lp_mint: &Pubkey,
    governance_fee_account: &Pubkey,
    user_transfer_authority: &Pubkey,
    user_token_accounts: [Pubkey; TOKEN_COUNT],
    token_program_account: &Pubkey,
    user_lp_token_account: &Pubkey,
    exact_burn_amount: AmountT,
    output_token_index: u8,
    minimum_output_amount: AmountT,
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new_readonly(*pool, false),
        AccountMeta::new_readonly(*authority, false),
    ];
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(pool_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new(*lp_mint, false));
    accounts.push(AccountMeta::new(*governance_fee_account, false));

    // used from SPL binary-oracle-pair. not actually necessary since the implementation only supports
    //  that using a separate keypair
    accounts.push(AccountMeta::new_readonly(
        *user_transfer_authority,
        authority != user_transfer_authority,
    ));
    for i in 0..TOKEN_COUNT {
        accounts.push(AccountMeta::new(user_token_accounts[i], false));
    }
    accounts.push(AccountMeta::new_readonly(*token_program_account, false));
    accounts.push(AccountMeta::new(*user_lp_token_account, false));

    let d = DeFiInstruction::<TOKEN_COUNT>::RemoveExactBurn {
        exact_burn_amount,
        output_token_index,
        minimum_output_amount,
    };

    let data = PoolInstruction::<TOKEN_COUNT>::DeFiInstruction(d).try_to_vec()?;
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data,
    })
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum GovernanceInstruction<const TOKEN_COUNT: usize> {
    PrepareFeeChange {
        lp_fee: DecT,
        governance_fee: DecT,
    },
    EnactFeeChange {},
    PrepareGovernanceTransition {
        upcoming_governance_key: Pubkey,
    },
    EnactGovernanceTransition {},
    ChangeGovernanceFeeAccount {
        governance_fee_key: Pubkey,
    },
    AdjustAmpFactor {
        target_ts: UnixTimestamp,
        target_value: DecT,
    },
    SetPaused {
        paused: bool,
    },
}
