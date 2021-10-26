use std::{collections::HashMap, convert::TryInto, str::FromStr};

use borsh::{BorshDeserialize, BorshSerialize};
use pool::error::*;
use pool::instruction::*;
use pool::TOKEN_COUNT;
use pool::{decimal::*, instruction::*, invariant::*, processor::Processor, state::*};
use solana_program::{
    account_info::AccountInfo,
    instruction::{Instruction, InstructionError},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::{self},
};
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    hash::Hash,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction::create_account,
    transaction::{Transaction, TransactionError},
    transport::TransportError,
};
use {
    arbitrary::{Arbitrary, Result as ArbResult, Unstructured},
    honggfuzz::fuzz,
};

use arrayvec::ArrayVec;
use spl_token::{
    instruction::approve,
    state::{Account as Token, AccountState, Mint},
};
use std::collections::BTreeMap;

use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::instruction::{initialize_mint, mint_to};

use rand::prelude::*;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Use u8 as an account id to simplify the address space and re-use accounts
/// more often.
type AccountId = u8;

type AmountT = u64;
type DecT = DecimalU64;

pub struct PoolInfo<const TOKEN_COUNT: usize> {
    pub pool_keypair: Keypair,
    pub nonce: u8,
    pub authority: Pubkey,
    pub lp_mint_keypair: Keypair,
    pub token_mint_keypairs: [Keypair; TOKEN_COUNT],
    pub token_account_keypairs: [Keypair; TOKEN_COUNT],
    pub governance_keypair: Keypair,
    pub governance_fee_keypair: Keypair,
}

impl<const TOKEN_COUNT: usize> PoolInfo<TOKEN_COUNT> {
    pub fn new() -> Self {
        let pool_keypair = Keypair::new();
        let lp_mint_keypair = Keypair::new();
        let (authority, nonce) = Pubkey::find_program_address(&[&pool_keypair.pubkey().to_bytes()[..32]], &pool::id());
        let mut token_mint_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
        let mut token_account_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
        for _i in 0..TOKEN_COUNT {
            token_mint_arrayvec.push(Keypair::new());
            token_account_arrayvec.push(Keypair::new());
        }
        let token_mint_keypairs: [Keypair; TOKEN_COUNT] = token_mint_arrayvec.into_inner().unwrap();
        let token_account_keypairs: [Keypair; TOKEN_COUNT] = token_account_arrayvec.into_inner().unwrap();
        let governance_keypair = Keypair::new();
        let governance_fee_keypair = Keypair::new();

        Self {
            pool_keypair,
            nonce,
            authority,
            lp_mint_keypair,
            token_mint_keypairs,
            token_account_keypairs,
            governance_keypair,
            governance_fee_keypair,
        }
    }

    pub async fn get_pool_state(&self, banks_client: &mut BanksClient) -> PoolState<TOKEN_COUNT> {
        let pool_account = get_account(banks_client, &self.pool_keypair.pubkey()).await;
        let pool_state = PoolState::<TOKEN_COUNT>::deserialize(&mut pool_account.data.as_slice()).unwrap();
        pool_state
    }

    pub fn get_token_mint_pubkeys(&self) -> [Pubkey; TOKEN_COUNT] {
        Self::to_key_array(&self.token_mint_keypairs)
    }

    pub fn get_token_account_pubkeys(&self) -> [Pubkey; TOKEN_COUNT] {
        Self::to_key_array(&self.token_account_keypairs)
    }

    pub async fn get_token_account_balances(&self, banks_client: &mut BanksClient) -> [AmountT; TOKEN_COUNT] {
        let token_account_pubkeys = self.get_token_account_pubkeys();
        get_token_balances(banks_client, token_account_pubkeys).await
    }

    fn to_key_array(account_slice: &[Keypair; TOKEN_COUNT]) -> [Pubkey; TOKEN_COUNT] {
        account_slice
            .iter()
            .map(|account| account.pubkey())
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap()
    }

    /// Creates pool's token mint accounts and token accounts
    /// for all tokens and LP token
    pub async fn init_pool(
        &self,
        banks_client: &mut BanksClient,
        payer: &Keypair,
        user_accounts_owner: &Keypair,
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
    ) {
        let rent = banks_client.get_rent().await.unwrap();

        let token_mint_pubkeys = *(&self.get_token_mint_pubkeys());
        let token_account_pubkeys = *(&self.get_token_account_pubkeys());

        let pool_len = solana_program::borsh::get_packed_len::<pool::state::PoolState<TOKEN_COUNT>>();

        // create pool keypair, lp mint & lp token account
        let mut ixs_vec = vec![
            create_account(
                &payer.pubkey(),
                &self.pool_keypair.pubkey(),
                rent.minimum_balance(pool_len),
                pool_len as u64,
                &pool::id(),
            ),
            // Create LP Mint account
            create_account(
                &payer.pubkey(),
                &self.lp_mint_keypair.pubkey(),
                rent.minimum_balance(Mint::LEN),
                Mint::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::id(),
                &self.lp_mint_keypair.pubkey(),
                &self.authority,
                None,
                6,
            )
            .unwrap(),
        ];
        // create token mints and Token accounts
        for i in 0..TOKEN_COUNT {
            println!("adding create_account & initialize_mint ix for {}", i);
            ixs_vec.push(create_account(
                &payer.pubkey(),
                &token_mint_pubkeys[i],
                //&token_mint_keypairs[i],
                rent.minimum_balance(Mint::LEN),
                Mint::LEN as u64,
                &spl_token::id(),
            ));
            ixs_vec.push(
                spl_token::instruction::initialize_mint(
                    &spl_token::id(),
                    &token_mint_pubkeys[i],
                    &user_accounts_owner.pubkey(),
                    None,
                    6,
                )
                .unwrap(),
            );
        }
        for i in 0..TOKEN_COUNT {
            println!("adding create_account & initialize_account ix for {}", i);
            ixs_vec.push(create_account(
                &payer.pubkey(),
                &token_account_pubkeys[i],
                //&token_account_keypairs[i],
                rent.minimum_balance(Token::LEN),
                Token::LEN as u64,
                &spl_token::id(),
            ));
            ixs_vec.push(
                spl_token::instruction::initialize_account(
                    &spl_token::id(),
                    &token_account_pubkeys[i],
                    &token_mint_pubkeys[i],
                    &self.authority,
                )
                .unwrap(),
            );
        }

        // create governance keypair & governacne_fee token account
        println!("creating governance & governanace_fee token account");
        ixs_vec.push(create_account(
            &payer.pubkey(),
            &self.governance_keypair.pubkey(),
            rent.minimum_balance(Token::LEN), //TODO: not sure what the len of this should be? data would just be empty?
            Token::LEN as u64,
            &user_accounts_owner.pubkey(), //TODO: randomly assigned owner to the user account owner
        ));
        ixs_vec.push(create_account(
            &payer.pubkey(),
            &self.governance_fee_keypair.pubkey(),
            rent.minimum_balance(Token::LEN),
            Token::LEN as u64,
            &spl_token::id(),
        ));
        ixs_vec.push(
            spl_token::instruction::initialize_account(
                &spl_token::id(),
                &self.governance_fee_keypair.pubkey(),
                &self.lp_mint_keypair.pubkey(),
                &user_accounts_owner.pubkey(), //TODO: randomly assigned governance_fee token account owner to the user account owner,
            )
            .unwrap(),
        );
        ixs_vec.push(
            create_init_ix::<TOKEN_COUNT>(
                &pool::id(),
                &self.pool_keypair.pubkey(),
                &self.lp_mint_keypair.pubkey(),
                &token_mint_pubkeys,
                &token_account_pubkeys,
                &self.governance_keypair.pubkey(),
                &self.governance_fee_keypair.pubkey(),
                self.nonce,
                amp_factor,
                lp_fee,
                governance_fee,
            )
            .unwrap(),
        );

        let mut transaction = Transaction::new_with_payer(&ixs_vec, Some(&payer.pubkey()));
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        let mut signatures = vec![
            payer,
            &self.pool_keypair,
            //user_accounts_owner,
            &self.lp_mint_keypair,
        ];

        for i in 0..TOKEN_COUNT {
            signatures.push(&self.token_mint_keypairs[i]);
        }
        for i in 0..TOKEN_COUNT {
            signatures.push(&self.token_account_keypairs[i]);
        }

        signatures.push(&self.governance_keypair);
        signatures.push(&self.governance_fee_keypair);

        transaction.sign(&signatures, recent_blockhash);

        banks_client.process_transaction(transaction).await.unwrap();
    }

    pub async fn execute_add(
        &self,
        banks_client: &mut BanksClient,
        payer: &Keypair,
        user_accounts_owner: &Keypair,
        user_transfer_authority: &Keypair,
        user_token_accounts: [Pubkey; TOKEN_COUNT],
        token_program_account: &Pubkey,
        user_lp_token_account: &Pubkey,
        deposit_amounts: [AmountT; TOKEN_COUNT],
        minimum_amount: AmountT,
    ) {
        let add_ix = DeFiInstruction::<TOKEN_COUNT>::Add {
            input_amounts: deposit_amounts,
            minimum_mint_amount: minimum_amount,
        };

        let mut transaction = Transaction::new_with_payer(
            &[create_defi_ix(
                add_ix,
                &pool::id(),
                &self.pool_keypair.pubkey(),
                &self.authority,
                &self.get_token_account_pubkeys(),
                &self.lp_mint_keypair.pubkey(),
                &self.governance_fee_keypair.pubkey(),
                &user_transfer_authority.pubkey(),
                &user_token_accounts,
                &spl_token::id(),
                Some(&user_lp_token_account),
            )
            .unwrap()],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[payer, user_transfer_authority], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();
    }
}

#[derive(Debug, Arbitrary)]
pub struct FuzzInstruction<const TOKEN_COUNT: usize> {
    instruction: DeFiInstruction<TOKEN_COUNT>,
    user_acct_id: AccountId,
}
#[derive(Debug)]
pub struct FuzzData<const TOKEN_COUNT: usize> {
    fuzz_instructions: Vec<FuzzInstruction<TOKEN_COUNT>>,
    magnitude_seed: u64,
    user_bases: [u8; TOKEN_COUNT],
    user_magnitude: u32,
}

impl<'a, const TOKEN_COUNT: usize> Arbitrary<'a> for FuzzData<TOKEN_COUNT> {
    fn arbitrary(u: &mut Unstructured<'a>) -> ArbResult<Self> {
        let mut fuzz_instructions = <Vec<FuzzInstruction<TOKEN_COUNT>> as Arbitrary<'a>>::arbitrary(u)?;

        let magnitude_seed = <u64 as Arbitrary<'a>>::arbitrary(u)?;
        let mut magnitude_rng = rand_chacha::ChaCha8Rng::seed_from_u64(magnitude_seed);

        let user_magnitude = {
            let mut base_mag = <u32 as Arbitrary<'a>>::arbitrary(u)?;
            while base_mag == 0 {
                base_mag = magnitude_rng.gen::<u32>();
            }
            base_mag
        };
        let max_ix_magnitude = ((user_magnitude >> 4) + 1) as u64;

        let mut ix_bases = [0 as u8; TOKEN_COUNT];
        magnitude_rng.fill(&mut ix_bases[..]);

        let mut user_bases: [u8; TOKEN_COUNT] = <[u8; TOKEN_COUNT] as Arbitrary<'a>>::arbitrary(u)?;

        for i in 0..TOKEN_COUNT {
            while user_bases[i] == 0 {
                user_bases[i] = magnitude_rng.gen::<u8>();
            }
        }

        for i in 0..fuzz_instructions.len() {
            //TODO: Regenerate ix_bases each ix?
            let bounded_index = (0..TOKEN_COUNT).choose(&mut magnitude_rng).unwrap();
            let defi_ix = &mut fuzz_instructions[i].instruction;
            // println!("[DEV] original defi_ix: {:?}", defi_ix);
            match defi_ix {
                pool::instruction::DeFiInstruction::Add {
                    input_amounts,
                    minimum_mint_amount,
                } => {
                    let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                    *minimum_mint_amount = max_ix_magnitude * base;
                    for input_idx in 0..TOKEN_COUNT {
                        let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                        let input_amount = max_ix_magnitude * base;
                        input_amounts[input_idx] = input_amount;
                    }
                }
                pool::instruction::DeFiInstruction::SwapExactInput {
                    exact_input_amounts,
                    output_token_index,
                    minimum_output_amount,
                } => {
                    let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                    *minimum_output_amount = max_ix_magnitude * base;
                    for tkn_idx in 0..TOKEN_COUNT {
                        let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                        let input_amount = max_ix_magnitude * base;
                        exact_input_amounts[tkn_idx] = input_amount;
                    }
                    exact_input_amounts[bounded_index] = 0;
                    *output_token_index = bounded_index as u8;
                }
                pool::instruction::DeFiInstruction::SwapExactOutput {
                    maximum_input_amount,
                    input_token_index,
                    exact_output_amounts,
                } => {
                    let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                    *maximum_input_amount = max_ix_magnitude * base;
                    for tkn_idx in 0..TOKEN_COUNT {
                        let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                        let output_amount = max_ix_magnitude * base;
                        exact_output_amounts[tkn_idx] = output_amount;
                    }
                    exact_output_amounts[bounded_index] = 0;
                    *input_token_index = bounded_index as u8;
                }
                pool::instruction::DeFiInstruction::RemoveUniform {
                    exact_burn_amount,
                    minimum_output_amounts,
                } => {
                    let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                    *exact_burn_amount = base * max_ix_magnitude;
                    for tkn_idx in 0..TOKEN_COUNT {
                        let base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                        let minimum_output_amount = max_ix_magnitude * base;
                        minimum_output_amounts[tkn_idx] = minimum_output_amount;
                    }
                }
                pool::instruction::DeFiInstruction::RemoveExactBurn {
                    exact_burn_amount,
                    output_token_index,
                    minimum_output_amount,
                } => {
                    let mut base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                    *exact_burn_amount = base * max_ix_magnitude;
                    *output_token_index = bounded_index as u8;
                    base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                    *minimum_output_amount = base * max_ix_magnitude;
                }
                pool::instruction::DeFiInstruction::RemoveExactOutput {
                    maximum_burn_amount,
                    exact_output_amounts,
                } => {
                    let mut base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                    *maximum_burn_amount = base * max_ix_magnitude;
                    for tkn_idx in 0..TOKEN_COUNT {
                        base = *ix_bases.choose(&mut magnitude_rng).unwrap() as u64;
                        let exact_output_amount = base * max_ix_magnitude;
                        exact_output_amounts[tkn_idx] = exact_output_amount;
                    }
                }
            };
        }

        Ok(FuzzData {
            fuzz_instructions,
            magnitude_seed,
            user_bases,
            user_magnitude,
        })
    }
}

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    loop {
        fuzz!(|fuzz_data: FuzzData<TOKEN_COUNT>| {
            let mut program_test =
                ProgramTest::new("pool", pool::id(), processor!(Processor::<{ TOKEN_COUNT }>::process));

            program_test.set_bpf_compute_max_units(200_000);

            let mut test_state = rt.block_on(program_test.start_with_context());
            println!("[DEV] Starting fuzz run with fuzz_data: {:?}", &fuzz_data);
            rt.block_on(run_fuzz_instructions(
                &mut test_state.banks_client,
                test_state.payer,
                test_state.last_blockhash,
                &fuzz_data,
            ));
            println!("[DEV] Finished ruzz run with fuzz_data: {:?}", &fuzz_data);
        });
    }
}
async fn run_fuzz_instructions<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    correct_payer: Keypair,
    recent_blockhash: Hash,
    fuzz_data: &FuzzData<TOKEN_COUNT>,
) {
    /** Prep/Initialize pool. TODO: Refactor this into separate method */
    let amp_factor = DecimalU64::from(1000);
    let lp_fee = DecimalU64::new(2000, 5).unwrap();
    let governance_fee = DecimalU64::new(1000, 5).unwrap();
    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();
    let pool = PoolInfo::<{ TOKEN_COUNT }>::new();

    //creates pool's token mints & token accounts
    pool.init_pool(
        banks_client,
        &correct_payer,
        &user_accounts_owner,
        amp_factor,
        lp_fee,
        governance_fee,
    )
    .await;

    // need to do initial add from a user's token accounts
    // TODO: focus on just executing the fuzz_ixs then worry about how to handle validations

    let mut init_prep_add_ixs = vec![];
    // create user token accounts that will do initial add
    for token_idx in 0..TOKEN_COUNT {
        let token_mint_keypair = &pool.token_mint_keypairs[token_idx];
        init_prep_add_ixs.push(create_associated_token_account(
            &correct_payer.pubkey(),
            &user_accounts_owner.pubkey(),
            &token_mint_keypair.pubkey(),
        ));
    }
    init_prep_add_ixs.push(create_associated_token_account(
        &correct_payer.pubkey(),
        &user_accounts_owner.pubkey(),
        &pool.lp_mint_keypair.pubkey(),
    ));
    println!("[DEV] finished setting up ixs for user ATAs");
    let mut transaction = Transaction::new_with_payer(&init_prep_add_ixs, Some(&correct_payer.pubkey()));
    transaction.sign(&[&correct_payer], recent_blockhash);
    println!("[DEV] signed txn for setting up user ATAs");
    let result = banks_client.process_transaction(transaction).await;
    println!("[DEV] finished creating ATA. Result: {:?}", result);
    //mint inital token amounts to user token accounts
    let mut init_user_token_accounts: [Pubkey; TOKEN_COUNT] = [Pubkey::new_unique(); TOKEN_COUNT];
    let mut magnitude_rng = rand_chacha::ChaCha8Rng::seed_from_u64(fuzz_data.magnitude_seed);
    let mut deposit_amounts: [AmountT; TOKEN_COUNT] = [0; TOKEN_COUNT];

    let mut init_user_bases = [0 as u8; TOKEN_COUNT];
    // none of the amounts for the initial deposit is allowed to be 0.
    magnitude_rng.fill(&mut init_user_bases[..]);
    for i in 0..TOKEN_COUNT {
        while init_user_bases[i] == 0 {
            init_user_bases[i] = magnitude_rng.gen::<u8>();
        }
    }
    for token_idx in 0..TOKEN_COUNT {
        let token_mint_keypair = &pool.token_mint_keypairs[token_idx];
        let user_token_pubkey =
            get_associated_token_address(&user_accounts_owner.pubkey(), &token_mint_keypair.pubkey());
        init_user_token_accounts[token_idx] = user_token_pubkey;
        let user_base = *init_user_bases.choose(&mut magnitude_rng).unwrap() as u64;
        let initial_user_token_amount = fuzz_data.user_magnitude as u64 * user_base;
        mint_tokens_to(
            banks_client,
            &correct_payer,
            &recent_blockhash,
            &token_mint_keypair.pubkey(),
            &user_token_pubkey,
            &user_accounts_owner,
            initial_user_token_amount,
        )
        .await
        .unwrap();

        approve_delegate(
            banks_client,
            &correct_payer,
            &recent_blockhash,
            &user_token_pubkey,
            &user_transfer_authority.pubkey(),
            &user_accounts_owner,
            initial_user_token_amount,
        )
        .await
        .unwrap();
        deposit_amounts[token_idx] = initial_user_token_amount;
    }
    let user_lp_token_account =
        get_associated_token_address(&user_accounts_owner.pubkey(), &pool.lp_mint_keypair.pubkey());

    pool.execute_add(
        banks_client,
        &correct_payer,
        &user_accounts_owner,
        &user_transfer_authority,
        init_user_token_accounts,
        &spl_token::id(),
        &user_lp_token_account,
        deposit_amounts,
        0,
    )
    .await;

    let pool_token_account_balances = pool.get_token_account_balances(banks_client).await;
    println!("[DEV] pool_token_account_balances: {:?}", pool_token_account_balances);

    let user_lp_token_balance = get_token_balance(banks_client, user_lp_token_account).await;
    println!("[DEV] user_lp_token_balance: {}", user_lp_token_balance);

    // Map<accountId, wallet_key>
    let mut user_wallets: HashMap<AccountId, Keypair> = HashMap::new();
    //Map<user_wallet_key>, associated_token_account_pubkey
    let mut user_token_accounts: HashMap<usize, HashMap<AccountId, Pubkey>> = HashMap::new();
    let mut user_lp_token_accounts: HashMap<AccountId, Pubkey> = HashMap::new();
    for token_idx in 0..TOKEN_COUNT {
        user_token_accounts.insert(token_idx, HashMap::new());
    }
    //[HashMap<AccountId, Pubkey>; TOKEN_COUNT] = [HashMap::new(); TOKEN_COUNT];

    let fuzz_instructions = &fuzz_data.fuzz_instructions;
    //add all the pool & token accounts that will be needed
    for fuzz_ix in fuzz_instructions {
        let user_id = fuzz_ix.user_acct_id;
        user_wallets.entry(user_id).or_insert_with(|| Keypair::new());
        let user_wallet_keypair = user_wallets.get(&user_id).unwrap();
        for token_idx in 0..TOKEN_COUNT {
            let token_mint_keypair = &pool.token_mint_keypairs[token_idx];
            if !user_token_accounts[&token_idx].contains_key(&user_id) {
                let user_base = *fuzz_data.user_bases.choose(&mut magnitude_rng).unwrap() as u64;
                let initial_user_token_amount = fuzz_data.user_magnitude as u64 * user_base;
                let user_ata_pubkey = create_assoc_token_acct_and_mint(
                    banks_client,
                    &correct_payer,
                    recent_blockhash,
                    &user_accounts_owner,
                    &user_wallet_keypair.pubkey(),
                    &token_mint_keypair.pubkey(),
                    initial_user_token_amount,
                )
                .await
                .unwrap();
                user_token_accounts
                    .get_mut(&token_idx)
                    .unwrap()
                    .insert(user_id, user_ata_pubkey);
            }
        }

        // create user ATA for LP Token
        if !user_lp_token_accounts.contains_key(&user_id) {
            let user_lp_ata_pubkey = create_assoc_token_acct_and_mint(
                banks_client,
                &correct_payer,
                recent_blockhash,
                &Keypair::new(), // this is dummy value not used since we don't mint any LP tokens here
                &user_wallet_keypair.pubkey(),
                &pool.lp_mint_keypair.pubkey(),
                0,
            )
            .await
            .unwrap();
            user_lp_token_accounts.insert(user_id, user_lp_ata_pubkey);
        }
    }

    println!(
        "[DEV] Initial pool token balances: {:?}",
        pool.get_token_account_balances(banks_client).await
    );

    println!("[DEV] Finished prepping pool");

    println!("[DEV] processing fuzz_instructions: {:?}", fuzz_instructions);
    for fuzz_ix in fuzz_instructions {
        execute_fuzz_instruction(
            banks_client,
            &correct_payer,
            recent_blockhash,
            &fuzz_ix,
            &pool,
            &user_wallets,
            &user_token_accounts,
            &user_lp_token_accounts,
        )
        .await;
    }

    println!(
        "[DEV] All fuzz_ixs processed successfully! pool account balances: {:?}. Fuzz_ixs executed: {:?}",
        pool.get_token_account_balances(banks_client).await,
        fuzz_instructions
    );
}

async fn execute_fuzz_instruction<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    correct_payer: &Keypair,
    recent_blockhash: Hash,
    fuzz_instruction: &FuzzInstruction<TOKEN_COUNT>,
    pool: &PoolInfo<TOKEN_COUNT>,
    user_wallets: &HashMap<AccountId, Keypair>,
    all_user_token_accounts: &HashMap<usize, HashMap<AccountId, Pubkey>>,
    all_user_lp_token_accounts: &HashMap<AccountId, Pubkey>,
) {
    println!("[DEV] execute_fuzz_instruction: {:?}", fuzz_instruction);
    let mut global_output_ixs = vec![];
    let mut global_signer_keys = vec![];
    let user_acct_id = fuzz_instruction.user_acct_id;
    let user_acct_owner = user_wallets.get(&user_acct_id).unwrap();
    let user_transfer_authority = Keypair::new();

    let user_token_accts = get_user_token_accounts(user_acct_id, all_user_token_accounts);
    let user_lp_token_acct = all_user_lp_token_accounts.get(&user_acct_id).unwrap();
    let pool_token_account_balances_before_ix = pool.get_token_account_balances(banks_client).await;

    let (mut output_ix, mut signer_keys) = generate_ix(
        pool,
        banks_client,
        correct_payer,
        recent_blockhash,
        user_acct_id,
        user_acct_owner,
        user_transfer_authority,
        user_token_accts,
        user_lp_token_acct,
        &fuzz_instruction.instruction,
    )
    .await;

    global_output_ixs.append(&mut output_ix);
    global_signer_keys.append(&mut signer_keys);

    let mut tx = Transaction::new_with_payer(&global_output_ixs, Some(&correct_payer.pubkey()));
    //println!("[DEV] created tx");
    let signers = [correct_payer]
        .iter()
        .map(|&v| v) // deref &Keypair
        .chain(global_signer_keys.iter())
        .collect::<Vec<&Keypair>>();
    // println!("[DEV] created signers vec");
    //Sign using some subset of required keys if recent_blockhash
    //  is not the same as currently in the transaction,
    //  clear any prior signatures and update recent_blockhash
    let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
    // println!("[DEV] got recent_blockhash");
    tx.partial_sign(&signers, recent_blockhash);
    // println!("[DEV] finished partial sign");
    let res = banks_client.process_transaction(tx).await;
    println!(
        "[DEV] finished processing transcation for execute_fuzz_instruction: {:?}. Res: {:?}",
        fuzz_instruction, res
    );
    match res {
        Ok(_) => {
            println!(
                "[DEV] txn processed successfully! pool account balances: {:?}. fuzz_ix processed: {:?}",
                pool.get_token_account_balances(banks_client).await,
                fuzz_instruction,
            )
        }
        Err(ref error) => match error {
            TransportError::TransactionError(te) => {
                match te {
                    //[2021-10-06T08:34:26.892127400Z DEBUG solana_runtime::message_processor] Program 4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM consumed 200000 of 200000 compute units
                    //[2021-10-06T08:34:26.893069000Z DEBUG solana_runtime::message_processor] Program failed to complete: exceeded maximum number of instructions allowed (200000) at instruction #11720
                    TransactionError::InstructionError(_, ie) => match ie {
                        InstructionError::InvalidArgument
                        | InstructionError::InsufficientFunds
                        | InstructionError::Custom(1) // TokenError::InsufficientFunds
                        | InstructionError::Custom(118) //PoolError::OutsideSpecifiedLimits
                        | InstructionError::Custom(120) //PoolError::ImpossibleRemove
                        => {
                            println!("[DEV] received expected InstructionError: {:?}. Fuzz_ix: {:?}", ie, fuzz_instruction);
                        }
                        // Note - the instruction error is ProgramFailedToComplete for Compute Budget exceeded (not sure why not InstructionError::ComputationalBudgetExceeded)
                        InstructionError::ProgramFailedToComplete => {
                            println!("[DEV] Received ProgramFailedToComplete. PoolState: {:?}. Pool balances before ix: {:?}. fuzz_ix: {:?}", pool.get_pool_state(banks_client).await, pool_token_account_balances_before_ix, fuzz_instruction);
                            // Computation Budget expected for now until decimal/math optimization is done.
                            //Err(ie).unwrap()
                        }
                        InstructionError::InvalidInstructionData => {
                            if !is_invalid_instruction_expected(pool, fuzz_instruction, banks_client).await {
                                println!("[DEV] received UNEXPECTED InstructionError::InvalidInstructionData for fuzz_ix: {:?}. PoolState: {:#?}. Pool balances before ix: {:?}", fuzz_instruction, pool.get_pool_state(banks_client).await, pool_token_account_balances_before_ix);
                                Err(ie).unwrap()
                            }
                            else {
                                println!("[DEV] received EXPECTED InstructionError::InvalidInstructionData for fuzz_ix: {:?}. PoolState: {:#?}. Pool balances before ix: {:?}", fuzz_instruction, pool.get_pool_state(banks_client).await, pool_token_account_balances_before_ix);
                            }
                        }
                        _ => {
                            println!("[DEV] received unexpected InstructionError: {:?}. Fuzz_ix: {:?}", ie, fuzz_instruction);
                            Err(ie).unwrap()
                        }
                    },
                    TransactionError::SignatureFailure
                    | TransactionError::InvalidAccountForFee
                    | TransactionError::InsufficientFundsForFee => {
                        println!(
                            "[DEV] received expected TransactionError that wasn't InstructionError: {:?}. Fuzz_ix: {:?}",
                            te, fuzz_instruction
                        );
                        panic!()
                    }
                    _ => {
                        println!(
                            "[DEV] received unexpected type of TransactionError: {:?}. Fuzz_ix: {:?}",
                            te, fuzz_instruction
                        );
                        panic!()
                    }
                }
            }
            _ => {
                println!(
                    "[DEV] received unexpected type of TransportError: {:?}. Fuzz_instruction: {:?}",
                    error, fuzz_instruction
                );
                panic!()
            }
        },
    }
}

async fn is_invalid_instruction_expected<const TOKEN_COUNT: usize>(
    pool: &PoolInfo<TOKEN_COUNT>,
    fuzz_instruction: &FuzzInstruction<TOKEN_COUNT>,
    banks_client: &mut BanksClient,
) -> bool {
    match fuzz_instruction.instruction {
        DeFiInstruction::Add {
            input_amounts,
            minimum_mint_amount,
        } => input_amounts.iter().all(|amount| *amount == 0),
        DeFiInstruction::RemoveUniform {
            exact_burn_amount,
            minimum_output_amounts,
        } => {
            let lp_total_supply = get_mint_state(banks_client, &pool.lp_mint_keypair.pubkey())
                .await
                .supply;
            exact_burn_amount == 0 || exact_burn_amount > lp_total_supply
        }
        DeFiInstruction::SwapExactInput {
            exact_input_amounts,
            output_token_index,
            minimum_output_amount,
        } => {
            let output_token_index = output_token_index as usize;
            exact_input_amounts.iter().all(|amount| *amount == 0)
                || output_token_index >= TOKEN_COUNT
                || exact_input_amounts[output_token_index] != 0
        }
        DeFiInstruction::SwapExactOutput {
            maximum_input_amount,
            input_token_index,
            exact_output_amounts,
        } => {
            let input_token_index = input_token_index as usize;
            let pool_balances = pool.get_token_account_balances(banks_client).await;
            exact_output_amounts.iter().all(|amount| *amount == 0)
                || input_token_index >= TOKEN_COUNT
                || exact_output_amounts[input_token_index] != 0
                || exact_output_amounts
                    .iter()
                    .zip(pool_balances.iter())
                    .any(|(output_amount, pool_balance)| *output_amount >= *pool_balance)
        }
        DeFiInstruction::RemoveExactBurn {
            exact_burn_amount,
            output_token_index,
            minimum_output_amount,
        } => {
            let output_token_index = output_token_index as usize;
            let lp_total_supply = get_mint_state(banks_client, &pool.lp_mint_keypair.pubkey())
                .await
                .supply;
            output_token_index >= TOKEN_COUNT || exact_burn_amount == 0 || exact_burn_amount >= lp_total_supply
        }
        DeFiInstruction::RemoveExactOutput {
            maximum_burn_amount,
            exact_output_amounts,
        } => {
            let pool_balances = pool.get_token_account_balances(banks_client).await;
            exact_output_amounts.iter().all(|amount| *amount == 0)
                || maximum_burn_amount == 0
                || exact_output_amounts
                    .iter()
                    .zip(pool_balances.iter())
                    .any(|(output_amount, pool_balance)| *output_amount >= *pool_balance)
        }
        _ => false,
    }
}

async fn generate_ix<const TOKEN_COUNT: usize>(
    pool: &PoolInfo<TOKEN_COUNT>,
    banks_client: &mut BanksClient,
    correct_payer: &Keypair,
    recent_blockhash: Hash,
    user_acct_id: u8,
    user_acct_owner: &Keypair,
    user_transfer_authority: Keypair,
    user_token_accts: [Pubkey; TOKEN_COUNT],
    user_lp_token_acct: &Pubkey,
    defi_instruction: &DeFiInstruction<TOKEN_COUNT>,
) -> (Vec<Instruction>, Vec<Keypair>) {
    match defi_instruction {
        DeFiInstruction::Add {
            input_amounts,
            minimum_mint_amount,
        } => {
            let mut ix_vec = vec![];
            let kp_vec = vec![clone_keypair(&user_transfer_authority)];
            for token_idx in 0..TOKEN_COUNT {
                approve_delegate(
                    banks_client,
                    correct_payer,
                    &recent_blockhash,
                    &user_token_accts[token_idx],
                    &user_transfer_authority.pubkey(),
                    user_acct_owner,
                    input_amounts[token_idx],
                )
                .await
                .unwrap();
            }
            let add_ix = DeFiInstruction::Add {
                input_amounts: *input_amounts,
                minimum_mint_amount: *minimum_mint_amount,
            };
            ix_vec.push(
                create_defi_ix(
                    add_ix,
                    &pool::id(),
                    &pool.pool_keypair.pubkey(),
                    &pool.authority,
                    &pool.get_token_account_pubkeys(),
                    &pool.lp_mint_keypair.pubkey(),
                    &pool.governance_fee_keypair.pubkey(),
                    &user_transfer_authority.pubkey(),
                    //&user_acct_owner.pubkey(),
                    &user_token_accts,
                    &spl_token::id(),
                    Some(&user_lp_token_acct),
                )
                .unwrap(),
            );
            (ix_vec, kp_vec)
        }
        DeFiInstruction::SwapExactInput {
            exact_input_amounts,
            output_token_index,
            minimum_output_amount,
        } => {
            let mut ix_vec = vec![];
            let kp_vec = vec![clone_keypair(&user_transfer_authority)];
            for token_idx in 0..TOKEN_COUNT {
                let input_amount = exact_input_amounts[token_idx];
                if input_amount > 0 {
                    approve_delegate(
                        banks_client,
                        correct_payer,
                        &recent_blockhash,
                        &user_token_accts[token_idx],
                        &user_transfer_authority.pubkey(),
                        user_acct_owner,
                        input_amount,
                    )
                    .await
                    .unwrap();
                }
            }
            let swap_exact_input_ix = DeFiInstruction::SwapExactInput {
                exact_input_amounts: *exact_input_amounts,
                output_token_index: *output_token_index,
                minimum_output_amount: *minimum_output_amount,
            };
            ix_vec.push(
                create_defi_ix(
                    swap_exact_input_ix,
                    &pool::id(),
                    &pool.pool_keypair.pubkey(),
                    &pool.authority,
                    &pool.get_token_account_pubkeys(),
                    &pool.lp_mint_keypair.pubkey(),
                    &pool.governance_fee_keypair.pubkey(),
                    &user_transfer_authority.pubkey(),
                    &user_token_accts,
                    &spl_token::id(),
                    None,
                )
                .unwrap(),
            );
            (ix_vec, kp_vec)
        }
        DeFiInstruction::SwapExactOutput {
            maximum_input_amount,
            input_token_index,
            exact_output_amounts,
        } => {
            let mut ix_vec = vec![];
            let mut kp_vec = vec![clone_keypair(&user_transfer_authority)];
            approve_delegate(
                banks_client,
                correct_payer,
                &recent_blockhash,
                &user_token_accts[*input_token_index as usize],
                &user_transfer_authority.pubkey(),
                user_acct_owner,
                *maximum_input_amount,
            )
            .await
            .unwrap();
            let swap_exact_output_ix = DeFiInstruction::SwapExactOutput {
                maximum_input_amount: *maximum_input_amount,
                input_token_index: *input_token_index,
                exact_output_amounts: *exact_output_amounts,
            };
            ix_vec.push(
                create_defi_ix(
                    swap_exact_output_ix,
                    &pool::id(),
                    &pool.pool_keypair.pubkey(),
                    &pool.authority,
                    &pool.get_token_account_pubkeys(),
                    &pool.lp_mint_keypair.pubkey(),
                    &pool.governance_fee_keypair.pubkey(),
                    &user_transfer_authority.pubkey(),
                    &user_token_accts,
                    &spl_token::id(),
                    None,
                )
                .unwrap(),
            );
            (ix_vec, kp_vec)
        }
        DeFiInstruction::RemoveUniform {
            exact_burn_amount,
            minimum_output_amounts,
        } => {
            let mut ix_vec = vec![];
            let kp_vec = vec![clone_keypair(&user_transfer_authority)];
            approve_delegate(
                banks_client,
                correct_payer,
                &recent_blockhash,
                &user_lp_token_acct,
                &user_transfer_authority.pubkey(),
                user_acct_owner,
                *exact_burn_amount,
            )
            .await
            .unwrap();
            let remove_uniform_ix = DeFiInstruction::RemoveUniform {
                exact_burn_amount: *exact_burn_amount,
                minimum_output_amounts: *minimum_output_amounts,
            };
            ix_vec.push(
                create_defi_ix(
                    remove_uniform_ix,
                    &pool::id(),
                    &pool.pool_keypair.pubkey(),
                    &pool.authority,
                    &pool.get_token_account_pubkeys(),
                    &pool.lp_mint_keypair.pubkey(),
                    &pool.governance_fee_keypair.pubkey(),
                    &user_transfer_authority.pubkey(),
                    &user_token_accts,
                    &spl_token::id(),
                    Some(&user_lp_token_acct),
                )
                .unwrap(),
            );

            (ix_vec, kp_vec)
        }
        DeFiInstruction::RemoveExactBurn {
            exact_burn_amount,
            output_token_index,
            minimum_output_amount,
        } => {
            let mut ix_vec = vec![];
            let kp_vec = vec![clone_keypair(&user_transfer_authority)];
            approve_delegate(
                banks_client,
                correct_payer,
                &recent_blockhash,
                &user_lp_token_acct,
                &user_transfer_authority.pubkey(),
                user_acct_owner,
                *exact_burn_amount,
            )
            .await
            .unwrap();
            let remove_exact_burn_ix = DeFiInstruction::RemoveExactBurn {
                exact_burn_amount: *exact_burn_amount,
                output_token_index: *output_token_index,
                minimum_output_amount: *minimum_output_amount,
            };
            ix_vec.push(
                create_defi_ix(
                    remove_exact_burn_ix,
                    &pool::id(),
                    &pool.pool_keypair.pubkey(),
                    &pool.authority,
                    &pool.get_token_account_pubkeys(),
                    &pool.lp_mint_keypair.pubkey(),
                    &pool.governance_fee_keypair.pubkey(),
                    &user_transfer_authority.pubkey(),
                    &user_token_accts,
                    &spl_token::id(),
                    Some(&user_lp_token_acct),
                )
                .unwrap(),
            );
            (ix_vec, kp_vec)
        }
        DeFiInstruction::RemoveExactOutput {
            maximum_burn_amount,
            exact_output_amounts,
        } => {
            let mut ix_vec = vec![];
            let kp_vec = vec![clone_keypair(&user_transfer_authority)];
            approve_delegate(
                banks_client,
                correct_payer,
                &recent_blockhash,
                &user_lp_token_acct,
                &user_transfer_authority.pubkey(),
                user_acct_owner,
                *maximum_burn_amount,
            )
            .await
            .unwrap();
            let remove_exact_output_ix = DeFiInstruction::RemoveExactOutput {
                maximum_burn_amount: *maximum_burn_amount,
                exact_output_amounts: *exact_output_amounts,
            };
            ix_vec.push(
                create_defi_ix(
                    remove_exact_output_ix,
                    &pool::id(),
                    &pool.pool_keypair.pubkey(),
                    &pool.authority,
                    &pool.get_token_account_pubkeys(),
                    &pool.lp_mint_keypair.pubkey(),
                    &pool.governance_fee_keypair.pubkey(),
                    &user_transfer_authority.pubkey(),
                    &user_token_accts,
                    &spl_token::id(),
                    Some(&user_lp_token_acct),
                )
                .unwrap(),
            );
            (ix_vec, kp_vec)
        }
        _ => {
            let ix_vec = vec![];
            let kp_vec = vec![];
            (ix_vec, kp_vec)
        }
    }
}

/** Helper fns  **/
pub fn get_user_token_accounts<const TOKEN_COUNT: usize>(
    user_acct_id: AccountId,
    user_token_accounts: &HashMap<usize, HashMap<AccountId, Pubkey>>,
) -> [Pubkey; TOKEN_COUNT] {
    let mut user_token_accts_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for token_idx in 0..TOKEN_COUNT {
        let user_token_account = user_token_accounts.get(&token_idx).unwrap().get(&user_acct_id).unwrap();
        user_token_accts_arrvec.push(*user_token_account);
    }
    user_token_accts_arrvec.into_inner().unwrap()
}

/// Creates an associated token account and mints
/// `amount` for a user
pub async fn create_assoc_token_acct_and_mint(
    banks_client: &mut BanksClient,
    correct_payer: &Keypair,
    recent_blockhash: Hash,
    mint_authority: &Keypair,
    user_wallet_pubkey: &Pubkey,
    token_mint: &Pubkey,
    amount: u64,
) -> Result<Pubkey, TransportError> {
    let create_ix = create_associated_token_account(&correct_payer.pubkey(), user_wallet_pubkey, token_mint);
    let ixs = vec![create_ix];
    let mut transaction = Transaction::new_with_payer(&ixs, Some(&correct_payer.pubkey()));
    transaction.sign(&[correct_payer], recent_blockhash);
    let result = banks_client.process_transaction(transaction).await;

    let user_token_pubkey = get_associated_token_address(user_wallet_pubkey, token_mint);
    if amount > 0 {
        mint_tokens_to(
            banks_client,
            &correct_payer,
            &recent_blockhash,
            token_mint,
            &user_token_pubkey,
            mint_authority,
            amount,
        )
        .await
        .unwrap();
    }
    Ok(user_token_pubkey)
}
/// Creates and initializes a token account
pub async fn create_token_account(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    account: &Keypair,
    mint: &Pubkey,
    owner: &Pubkey,
) -> Result<(), TransportError> {
    let rent = banks_client.get_rent().await.unwrap();
    let account_rent = rent.minimum_balance(spl_token::state::Account::LEN);

    let mut transaction = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),
                &account.pubkey(),
                account_rent,
                spl_token::state::Account::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_account(&spl_token::id(), &account.pubkey(), mint, owner).unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, account], *recent_blockhash);
    banks_client.process_transaction(transaction).await?;
    Ok(())
}

pub async fn mint_tokens_to(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Keypair,
    amount: u64,
) -> Result<(), TransportError> {
    let mut transaction = Transaction::new_with_payer(
        &[spl_token::instruction::mint_to(
            &spl_token::id(),
            mint,
            destination,
            &authority.pubkey(),
            &[&authority.pubkey()],
            amount,
        )
        .unwrap()],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, authority], *recent_blockhash);
    banks_client.process_transaction(transaction).await?;
    Ok(())
}

pub async fn approve_delegate(
    banks_client: &mut BanksClient,
    payer: &Keypair,
    recent_blockhash: &Hash,
    source: &Pubkey,
    delegate: &Pubkey,
    source_owner: &Keypair,
    amount: u64,
) -> Result<(), TransportError> {
    let mut transaction = Transaction::new_with_payer(
        &[spl_token::instruction::approve(
            &spl_token::id(),
            source,
            delegate,
            &source_owner.pubkey(),
            &[&source_owner.pubkey()],
            amount,
        )
        .unwrap()],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, source_owner], *recent_blockhash);
    banks_client.process_transaction(transaction).await?;
    Ok(())
}

pub async fn get_account(banks_client: &mut BanksClient, pubkey: &Pubkey) -> Account {
    banks_client
        .get_account(*pubkey)
        .await
        .expect("account not found")
        .expect("account empty")
}

pub async fn get_mint_state(banks_client: &mut BanksClient, pubkey: &Pubkey) -> Mint {
    let acct = get_account(banks_client, pubkey).await;
    Mint::unpack_from_slice(acct.data.as_slice()).unwrap()
}

pub async fn get_token_balance(banks_client: &mut BanksClient, token_account_pubkey: Pubkey) -> u64 {
    let token_account = get_account(banks_client, &token_account_pubkey).await;
    let account_info = Token::unpack_from_slice(token_account.data.as_slice()).unwrap();
    account_info.amount
}

pub async fn get_token_balances<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    token_accounts: [Pubkey; TOKEN_COUNT],
) -> [AmountT; TOKEN_COUNT] {
    let mut token_accounts_arrvec = ArrayVec::<_, TOKEN_COUNT>::new();
    for i in 0..TOKEN_COUNT {
        token_accounts_arrvec.push(get_token_balance(banks_client, token_accounts[i]).await);
    }
    token_accounts_arrvec.into_inner().unwrap()
}

pub async fn get_token_balances_map<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    token_accounts: [Pubkey; TOKEN_COUNT],
) -> BTreeMap<Pubkey, u64> {
    let mut btree = BTreeMap::<Pubkey, u64>::new();
    for i in 0..TOKEN_COUNT {
        let token_account = get_account(banks_client, &token_accounts[i]).await;
        let account_info = Token::unpack_from_slice(token_account.data.as_slice()).unwrap();
        btree.insert(token_accounts[i], account_info.amount);
    }
    btree
}

pub async fn print_user_token_account_owners<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    token_accounts: [Pubkey; TOKEN_COUNT],
) {
    for i in 0..TOKEN_COUNT {
        let token_account = get_account(banks_client, &token_accounts[i]).await;
        let spl_token_account_info = Token::unpack_from_slice(token_account.data.as_slice()).unwrap();
        println!(
            "token_account.key: {} token_account.owner: {} spl_token_account_info.owner: {}",
            &token_accounts[i], token_account.owner, spl_token_account_info.owner
        );
    }
}

fn clone_keypair(keypair: &Keypair) -> Keypair {
    return Keypair::from_bytes(&keypair.to_bytes().clone()).unwrap();
}
