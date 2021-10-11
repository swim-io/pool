use std::{collections::HashMap, convert::TryInto, str::FromStr};

use pool::error::*;
use pool::instruction::{DeFiInstruction, GovernanceInstruction, PoolInstruction, PoolInstruction::*};
use pool::TOKEN_COUNT;
use pool::{decimal::*, instruction::*, invariant::*, processor::Processor};
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

/// Use u8 as an account id to simplify the address space and re-use accounts
/// more often.
type AccountId = u8;

type AmountT = u64;
type DecT = DecimalU64;

//const INITIAL_USER_TOKEN_AMOUNT: u64 = 1_000_000_000;

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

    pub async fn get_depth(&self, banks_client: &mut BanksClient, amp_factor: DecT) -> DecT {
        let token_account_balances: [AmountT; TOKEN_COUNT] = self.get_token_account_balances(banks_client).await;
        //let pool_state = Self::deserialize_pool_state(banks_client).unwrap();
        //println!("######################################## {:?}", token_account_balances);
        DecT::from(Invariant::calculate_depth(&token_account_balances, amp_factor))
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
        amp_factor: DecT, // DecimalU64::new(value, decimals).unwrap()
        lp_fee: DecT,
        governance_fee: DecT,
    ) {
        let rent = banks_client.get_rent().await.unwrap();

        // let token_mint_pubkeys: [Pubkey; TOKEN_COUNT] = to_key_array(&self.token_mint_keypairs);
        // let token_account_pubkeys: [Pubkey; TOKEN_COUNT] = to_key_array(&self.token_account_keypairs);
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
                token_mint_pubkeys,
                token_account_pubkeys,
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
        let mut transaction = Transaction::new_with_payer(
            &[create_add_ix(
                &pool::id(),
                &self.pool_keypair.pubkey(),
                &self.authority,
                *(&self.get_token_account_pubkeys()),
                &self.lp_mint_keypair.pubkey(),
                &self.governance_fee_keypair.pubkey(),
                &user_transfer_authority.pubkey(),
                user_token_accounts,
                token_program_account,
                user_lp_token_account,
                deposit_amounts,
                minimum_amount,
            )
            .unwrap()],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[payer, user_transfer_authority], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();
    }
}

#[derive(Debug)]
pub struct FuzzData<const TOKEN_COUNT: usize> {
    fuzz_instructions: Vec<FuzzInstruction<TOKEN_COUNT>>,
    magnitude_range: [u64; 3],
    initial_user_token_amount: u64, // amount of EACH token that user will initially have
}

#[derive(Debug)]
pub struct FuzzInstruction<const TOKEN_COUNT: usize> {
    instruction: DeFiInstruction<TOKEN_COUNT>,
    user_acct_id: AccountId,
}
/*
quick summary on the compute budget and pool math:
Ethereum natively supports u256 and hence calculations are dirt cheap (ADD and SUB are 3 gas each, MUL and DIV are 5 gas).

On Solana we don't have to pay for gas on the one hand, but on the other the compute budget is very restrictive
    because there's just no good (=cheap in terms of compute budget cost), native support for arithmetic
    with types larger than u64,
which are required for the invariant math to work.

This means that the more the pool goes out of equilibrium, the larger the (intermediate) numbers get while calculating the invariant
    (hence requiring larger, even more expensive datatypes) while also requiring more steps till convergence (hence more operations with more expensive data types).
    An eventual solution to this problem is calculating the (approximate) solution off-chain, including it in the instruction,
    and using this value an initial guess for the Newton iteration in the smart contract itself
    which will sharply cut down on the required number of iterations
    (all the way down to 1 if the pool hasn't changed between when the number was calculated off-chain and when the transaction is being processed on-chain).
For now, for demo purposes (and realistically also for essentially all real-world scenarios I suppose)
the pool balances should not diverge by more than 2 orders of magnitude (i.e. a pool with [100, 1, 1, 1, 1, 1] or [1, 100, 100, 100, 100, 100] should still be fine).

*/

impl<'a, const TOKEN_COUNT: usize> Arbitrary<'a> for FuzzData<TOKEN_COUNT> {
    fn arbitrary(u: &mut Unstructured<'a>) -> ArbResult<Self> {
        let mut fuzz_instructions = vec![];
        let mut magnitude_range: [u64; 3] = [0; 3];
        let magnitude = u.int_in_range(5..=10).unwrap();
        for m_idx in 0..magnitude_range.len() {
            magnitude_range[m_idx] = (10 as u64).pow(magnitude + (m_idx as u32));
        }
        let initial_user_token_amount =
            (u.int_in_range(1..=9).unwrap() * magnitude_range[magnitude_range.len() - 1]) as u64;
        let ixs_len = u.int_in_range(1..=10)? as usize;
        for ix_idx in 0..ixs_len {
            let base_defi_ix: DeFiInstruction<TOKEN_COUNT> = u.arbitrary()?;
            // let bounded_end = (TOKEN_COUNT - 1) as int32;
            let bounded_index = u.int_in_range(0..=TOKEN_COUNT - 1)? as u8;
            let ix: DeFiInstruction<TOKEN_COUNT> = match base_defi_ix {
                pool::instruction::DeFiInstruction::Add {
                    input_amounts,
                    minimum_mint_amount,
                } => {
                    let mut m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                    let mut magnitude = magnitude_range[m_idx];
                    let minimum_mint_amount = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    let mut input_amounts = [u64::MIN; TOKEN_COUNT];
                    for tkn_idx in 0..TOKEN_COUNT {
                        m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                        magnitude = magnitude_range[m_idx];
                        input_amounts[tkn_idx] = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    }
                    pool::instruction::DeFiInstruction::Add {
                        input_amounts,
                        minimum_mint_amount,
                    }
                }
                pool::instruction::DeFiInstruction::SwapExactInput {
                    mut exact_input_amounts,
                    output_token_index,
                    minimum_output_amount,
                } => {
                    for tkn_idx in 0..TOKEN_COUNT {
                        let m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                        let magnitude = magnitude_range[m_idx];
                        exact_input_amounts[tkn_idx] = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    }
                    exact_input_amounts[bounded_index as usize] = 0;
                    pool::instruction::DeFiInstruction::SwapExactInput {
                        exact_input_amounts,
                        output_token_index: bounded_index,
                        minimum_output_amount,
                    }
                }
                pool::instruction::DeFiInstruction::SwapExactOutput {
                    maximum_input_amount: _maximum_input_amount,
                    input_token_index,
                    exact_output_amounts: _exact_output_amounts,
                } => {
                    let mut m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                    let mut magnitude = magnitude_range[m_idx];
                    let maximum_input_amount = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    let mut exact_output_amounts = [0; TOKEN_COUNT];
                    for tkn_idx in 0..TOKEN_COUNT {
                        m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                        magnitude = magnitude_range[m_idx];
                        exact_output_amounts[tkn_idx] = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    }
                    exact_output_amounts[input_token_index as usize] = 0;
                    pool::instruction::DeFiInstruction::SwapExactOutput {
                        maximum_input_amount,
                        input_token_index: bounded_index,
                        exact_output_amounts,
                    }
                }
                pool::instruction::DeFiInstruction::RemoveUniform {
                    exact_burn_amount: _exact_burn_amount,
                    minimum_output_amounts: _minimum_output_amounts,
                } => {
                    let mut m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                    let mut magnitude = magnitude_range[m_idx];
                    let exact_burn_amount = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    let mut minimum_output_amounts = [0; TOKEN_COUNT];
                    for tkn_idx in 0..TOKEN_COUNT {
                        m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                        magnitude = magnitude_range[m_idx];
                        minimum_output_amounts[tkn_idx] = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    }
                    pool::instruction::DeFiInstruction::RemoveUniform {
                        exact_burn_amount,
                        minimum_output_amounts,
                    }
                }
                pool::instruction::DeFiInstruction::RemoveExactBurn {
                    exact_burn_amount,
                    output_token_index,
                    minimum_output_amount,
                } => {
                    let m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                    let magnitude = magnitude_range[m_idx];
                    let exact_burn_amount = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    pool::instruction::DeFiInstruction::RemoveExactBurn {
                        exact_burn_amount,
                        output_token_index: bounded_index,
                        minimum_output_amount,
                    }
                }
                pool::instruction::DeFiInstruction::RemoveExactOutput {
                    maximum_burn_amount,
                    mut exact_output_amounts,
                } => {
                    let mut m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                    let mut magnitude = magnitude_range[m_idx];
                    let maximum_burn_amount = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    for tkn_idx in 0..TOKEN_COUNT {
                        m_idx = u.int_in_range(0..=magnitude_range.len() - 1).unwrap() as usize;
                        magnitude = magnitude_range[m_idx];
                        exact_output_amounts[tkn_idx] = (u.int_in_range(1..=9).unwrap() * magnitude) as u64;
                    }
                    pool::instruction::DeFiInstruction::RemoveExactOutput {
                        maximum_burn_amount,
                        exact_output_amounts,
                    }
                }
            };
            fuzz_instructions.push(FuzzInstruction {
                instruction: ix,
                user_acct_id: u.arbitrary()?,
            });
        }
        Ok(FuzzData {
            fuzz_instructions,
            magnitude_range,
            initial_user_token_amount,
        })
    }
}

// impl<'a, const TOKEN_COUNT: usize> Arbitrary<'a> for FuzzInstruction<TOKEN_COUNT> {
//     fn arbitrary(u: &mut Unstructured<'a>) -> ArbResult<Self> {
//         let test = u.arbitrary()?;
//         // let bounded_end = (TOKEN_COUNT - 1) as int32;
//         let bounded_index = u.int_in_range(0..=TOKEN_COUNT - 1)? as u8;
//         let ix = match test {
//             DeFiInstruction::<TOKEN_COUNT>::SwapExactInput {
//                 mut exact_input_amounts,
//                 output_token_index,
//                 minimum_output_amount,
//             } => {
//                 let idx = bounded_index as usize;
//                 exact_input_amounts[idx] = 0;
//                 DeFiInstruction::<TOKEN_COUNT>::SwapExactInput {
//                     exact_input_amounts,
//                     output_token_index: bounded_index,
//                     minimum_output_amount,
//                 }
//             }
//             DeFiInstruction::<TOKEN_COUNT>::SwapExactOutput {
//                 maximum_input_amount,
//                 input_token_index,
//                 exact_output_amounts,
//             } => DeFiInstruction::<TOKEN_COUNT>::SwapExactOutput {
//                 maximum_input_amount,
//                 input_token_index: bounded_index,
//                 exact_output_amounts,
//             },
//             DeFiInstruction::<TOKEN_COUNT>::RemoveExactBurn {
//                 exact_burn_amount,
//                 output_token_index,
//                 minimum_output_amount,
//             } => DeFiInstruction::<TOKEN_COUNT>::RemoveExactBurn {
//                 exact_burn_amount,
//                 output_token_index: bounded_index,
//                 minimum_output_amount,
//             },
//             default => default, //other ixs are fine as-is
//         };
//         Ok(FuzzInstruction {
//             instruction: ix,
//             user_acct_id: u.arbitrary()?,
//         })
//     }
// }

// impl<'a, const TOKEN_COUNT: usize> Arbitrary<'a> for FuzzInstruction<TOKEN_COUNT> {
//     fn arbitrary(u: &mut Unstructured<'a>) -> ArbResult<Self> {
//         let base_defi_ix = u.arbitrary()?;
//         // let bounded_end = (TOKEN_COUNT - 1) as int32;
//         let bounded_index = u.int_in_range(0..=TOKEN_COUNT - 1)? as u8;
//         let ix = match base_defi_ix {
//             DeFiInstruction::<TOKEN_COUNT>::SwapExactInput {
//                 mut exact_input_amounts,
//                 output_token_index,
//                 minimum_output_amount,
//             } => {
//                 let idx = bounded_index as usize;
//                 exact_input_amounts[idx] = 0;
//                 DeFiInstruction::<TOKEN_COUNT>::SwapExactInput {
//                     exact_input_amounts,
//                     output_token_index: bounded_index,
//                     minimum_output_amount,
//                 }
//             }
//             DeFiInstruction::<TOKEN_COUNT>::SwapExactOutput {
//                 maximum_input_amount,
//                 input_token_index,
//                 exact_output_amounts,
//             } => DeFiInstruction::<TOKEN_COUNT>::SwapExactOutput {
//                 maximum_input_amount,
//                 input_token_index: bounded_index,
//                 exact_output_amounts,
//             },
//             DeFiInstruction::<TOKEN_COUNT>::RemoveExactBurn {
//                 exact_burn_amount,
//                 output_token_index,
//                 minimum_output_amount,
//             } => DeFiInstruction::<TOKEN_COUNT>::RemoveExactBurn {
//                 exact_burn_amount,
//                 output_token_index: bounded_index,
//                 minimum_output_amount,
//             },
//             default => default, //other ixs are fine as-is
//         };
//         Ok(FuzzInstruction {
//             instruction: ix,
//             user_acct_id: u.arbitrary()?,
//         })
//     }
// }

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
        // fuzz!(|fuzz_ixs: Vec<FuzzInstruction<TOKEN_COUNT>>| {
        //     println!("# of ixs: {}, ix are {:?}", fuzz_ixs.len(), fuzz_ixs);
        //     if fuzz_ixs.is_empty() {
        //         return;
        //     }

        //     let mut program_test =
        //         ProgramTest::new("pool", pool::id(), processor!(Processor::<{ TOKEN_COUNT }>::process));

        //     program_test.set_bpf_compute_max_units(200_000);

        //     let mut test_state = rt.block_on(program_test.start_with_context());

        //     rt.block_on(run_fuzz_instructions(
        //         &mut test_state.banks_client,
        //         test_state.payer,
        //         test_state.last_blockhash,
        //         fuzz_ixs,
        //     ));
        // });
    }
}
async fn run_fuzz_instructions<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    correct_payer: Keypair,
    recent_blockhash: Hash,
    fuzz_data: &FuzzData<TOKEN_COUNT>,
) {
    /** Prep/Initialize pool. TODO: Refactor this into separate method */
    let amp_factor = DecimalU64::from(1);
    let lp_fee = DecimalU64::from(0);
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
    println!("[DEV] signed txn");
    let result = banks_client.process_transaction(transaction).await;
    println!("[DEV] finished creating ATA. Result: {:?}", result);
    //mint inital token amounts to user token accounts
    let mut init_user_token_accounts: [Pubkey; TOKEN_COUNT] = [Pubkey::new_unique(); TOKEN_COUNT];
    for token_idx in 0..TOKEN_COUNT {
        let token_mint_keypair = &pool.token_mint_keypairs[token_idx];
        let user_token_pubkey =
            get_associated_token_address(&user_accounts_owner.pubkey(), &token_mint_keypair.pubkey());
        init_user_token_accounts[token_idx] = user_token_pubkey;
        mint_tokens_to(
            banks_client,
            &correct_payer,
            &recent_blockhash,
            &token_mint_keypair.pubkey(),
            &user_token_pubkey,
            &user_accounts_owner,
            fuzz_data.initial_user_token_amount,
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
            fuzz_data.initial_user_token_amount,
        )
        .await
        .unwrap();
    }
    let user_lp_token_account =
        get_associated_token_address(&user_accounts_owner.pubkey(), &pool.lp_mint_keypair.pubkey());
    let deposit_amounts: [AmountT; TOKEN_COUNT] = [fuzz_data.initial_user_token_amount; TOKEN_COUNT];
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

    // Map<accountId, wallet_key>
    let mut user_wallets: HashMap<AccountId, Keypair> = HashMap::new();
    let mut user_transfer_authorities: HashMap<AccountId, Keypair> = HashMap::new();
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
        user_transfer_authorities
            .entry(user_id)
            .or_insert_with(|| Keypair::new());
        let user_wallet_keypair = user_wallets.get(&user_id).unwrap();
        for token_idx in 0..TOKEN_COUNT {
            let token_mint_keypair = &pool.token_mint_keypairs[token_idx];
            if !user_token_accounts[&token_idx].contains_key(&user_id) {
                let user_ata_pubkey = create_assoc_token_acct_and_mint(
                    banks_client,
                    &correct_payer,
                    recent_blockhash,
                    &user_accounts_owner,
                    &user_wallet_keypair.pubkey(),
                    &token_mint_keypair.pubkey(),
                    fuzz_data.initial_user_token_amount,
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
    let mut before_total_token_amounts = vec![];
    for token_idx in 0..TOKEN_COUNT {
        let before_total_token_amount = fuzz_data.initial_user_token_amount
            + (user_token_accounts[&token_idx].len() as u64 * fuzz_data.initial_user_token_amount);
        before_total_token_amounts.push(before_total_token_amount);
    }
    println!("[DEV] before_total_token_amounts: {:?}", before_total_token_amounts);
    println!(
        "[DEV] before pool token balances: {:?}",
        pool.get_token_account_balances(banks_client).await
    );
    // let mut global_output_ixs = vec![];
    // let mut global_signer_keys = vec![];
    println!("[DEV] processing fuzz_instructions: {:?}", fuzz_instructions);
    for fuzz_ix in fuzz_instructions {
        // let (mut output_ix, mut signer_keys) = run_fuzz_instruction(
        //     banks_client,
        //     correct_payer,
        //     fuzz_ix,
        //     &pool,
        //     &user_wallets,
        //     &user_transfer_authorities,
        //     &user_token_accounts,
        //     &user_lp_token_accounts,
        // );
        // global_output_ixs.append(&mut output_ix);
        // global_signer_keys.append(&mut signer_keys);

        run_fuzz_instruction(
            banks_client,
            &correct_payer,
            recent_blockhash,
            &fuzz_ix,
            &pool,
            &user_wallets,
            &user_transfer_authorities,
            &user_token_accounts,
            &user_lp_token_accounts,
        )
        .await;
    }

    // let mut tx = Transaction::new_with_payer(&global_output_ixs, Some(&correct_payer.pubkey()));
    // let signers = [&correct_payer]
    //     .iter()
    //     .map(|&v| v) // deref &Keypair
    //     .chain(global_signer_keys.iter())
    //     .collect::<Vec<&Keypair>>();

    // //Sign using some subset of required keys if recent_blockhash
    // //  is not the same as currently in the transaction,
    // //  clear any prior signatures and update recent_blockhash
    // tx.partial_sign(&signers, recent_blockhash);

    // /// see comment here

    // let res = banks_client.process_transaction(tx).await;
    // match res {
    //     Ok(_) => {
    //         println!("[DEV] txn processed successfully! pool account balances: {:?}", pool.get_token_account_balances(banks_client).await)
    //     }
    //     Err(ref error) => match error {
    //         TransportError::TransactionError(te) => {
    //             match te {
    //                 TransactionError::InstructionError(_, ie) => match ie {
    //                     InstructionError::InvalidArgument
    //                     // | InstructionError::InvalidInstructionData
    //                     // | InstructionError::InvalidAccountData
    //                     // | InstructionError::InsufficientFunds
    //                     // | InstructionError::AccountAlreadyInitialized
    //                     // | InstructionError::InvalidSeeds
    //                     | InstructionError::Custom(2) // TokenError::InsufficientFunds
    //                     | InstructionError::Custom(118) //PoolError::OutsideSpecifiedLimits
    //                     | InstructionError::Custom(120) //PoolError::ImpossibleRemove
    //                         => {
    //                             println!("[DEV] received expected InstructionError: {:?}", ie);
    //                         }
    //                     _ => {
    //                         println!("[DEV] received unexpected InstructionError{:?}", ie);
    //                         Err(ie).unwrap()
    //                     }
    //                 }
    //                 TransactionError::SignatureFailure
    //                 | TransactionError::InvalidAccountForFee
    //                 | TransactionError::InsufficientFundsForFee => {
    //                     println!("[DEV] received expected TransactionError that wasn't InstructionError: {:?}", te);
    //                 }
    //                 _ => {
    //                     println!("[DEV] received unexpected type of TransactionError: {:?}", te);
    //                     panic!()
    //                 }
    //             }
    //         }
    //         _ => {
    //             println!("[DEV] received unexpected type of error: {:?}", res);
    //             panic!()
    //         }
    //     }
    // }
    // banks_client.process_transaction(tx).await.unwrap_or_else(|e| {
    //     if let TransportError::TransactionError(te) = e {
    //         match te {
    //             // this block is catching/printing expected TransactionError::InstructionErrors
    //             TransactionError::InstructionError(_, ie) => match ie {
    //                     InstructionError::InvalidArgument
    //                     // | InstructionError::InvalidInstructionData
    //                     // | InstructionError::InvalidAccountData
    //                     // | InstructionError::InsufficientFunds
    //                     // | InstructionError::AccountAlreadyInitialized
    //                     // | InstructionError::InvalidSeeds
    //                     | InstructionError::Custom(2) // TokenError::InsufficientFunds
    //                     | InstructionError::Custom(118) //PoolError::OutsideSpecifiedLimits
    //                     | InstructionError::Custom(120) //PoolError::ImpossibleRemove
    //                         => {}
    //                     _ => { // if not one of the aboce specified InstructionError then unexpected
    //                         print!("{:?}", ie);
    //                         Err(ie).unwrap()
    //                     }
    //                 },
    //             //these are expected other types of TransactionErrors errors therefore we're panicing
    //             TransactionError::SignatureFailure
    //             | TransactionError::InvalidAccountForFee
    //             | TransactionError::InsufficientFundsForFee => {}
    //             _ => {
    //                 print!("{:?}", te);
    //                 panic!()
    //             }
    //         }
    //     } else {
    //         print!("{:?}", e);
    //         panic!()
    //     }
    // });

    // println!("[DEV] acct balances after txn: {:?}", pool.get_token_account_balances(banks_client).await)

    // .map_err(|e|{
    //     if !(e == PoolError::OutsideSpecifiedLimits.into()
    //         || e == TokenError::InsufficientFunds.into())
    //     {
    //         println!("Unexpected error: {:?}", e);
    //         Err(e).unwrap()
    //     }
    // })
    // .ok();
    println!(
        "[DEV] All fuzz_ixs processed successfully! pool account balances: {:?}. Fuzz_ixs executed: {:?}",
        pool.get_token_account_balances(banks_client).await,
        fuzz_instructions
    );
}

async fn run_fuzz_instruction<const TOKEN_COUNT: usize>(
    banks_client: &mut BanksClient,
    correct_payer: &Keypair,
    recent_blockhash: Hash,
    fuzz_instruction: &FuzzInstruction<TOKEN_COUNT>,
    pool: &PoolInfo<TOKEN_COUNT>,
    user_wallets: &HashMap<AccountId, Keypair>,
    all_user_transfer_authorities: &HashMap<AccountId, Keypair>,
    all_user_token_accounts: &HashMap<usize, HashMap<AccountId, Pubkey>>,
    all_user_lp_token_accounts: &HashMap<AccountId, Pubkey>,
) {
    let mut global_output_ixs = vec![];
    let mut global_signer_keys = vec![];
    let user_acct_id = fuzz_instruction.user_acct_id;
    let user_acct_owner = user_wallets.get(&user_acct_id).unwrap();
    let user_transfer_authority = all_user_transfer_authorities.get(&user_acct_id).unwrap();
    let user_token_accts = get_user_token_accounts(user_acct_id, all_user_token_accounts);
    let user_lp_token_acct = all_user_lp_token_accounts.get(&user_acct_id).unwrap();
    //Notes:
    //  - bonfida vesting
    //      run_fuzz_ixs
    //          run_fuzz_ix generates (output_ix: Vec<Instruction>, signer_keys: Vec<Keypair>) then appends them to
    //          global_output_ixs: Vec<Instruction> & global_singer_keys: Vec<Keypairs>
    //          after it runs all the ixs in one transaction
    //  - SPL token-swap
    let (mut output_ix, mut signer_keys) = match fuzz_instruction.instruction {
        DeFiInstruction::Add {
            input_amounts,
            minimum_mint_amount,
        } => {
            let mut ix_vec = vec![];
            let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
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
                // let approve_ix = approve(
                //     &spl_token::id(),
                //     &user_token_accts[token_idx],
                //     &user_transfer_authority.pubkey(),
                //     &user_acct_owner.pubkey(),
                //     &[&user_acct_owner.pubkey()],
                //     input_amounts[token_idx],
                // )
                // .unwrap();
                // kp_vec.push(clone_keypair(user_acct_owner));
                // ix_vec.push(approve_ix);
            }
            let add_ix = create_add_ix(
                &pool::id(),
                &pool.pool_keypair.pubkey(),
                &pool.authority,
                pool.get_token_account_pubkeys(),
                &pool.lp_mint_keypair.pubkey(),
                &pool.governance_fee_keypair.pubkey(),
                //&user_transfer_authority.pubkey(),
                &user_acct_owner.pubkey(),
                user_token_accts,
                &spl_token::id(),
                user_lp_token_acct,
                input_amounts,
                minimum_mint_amount,
            )
            .unwrap();
            ix_vec.push(add_ix);
            (ix_vec, kp_vec)
        }
        DeFiInstruction::SwapExactInput {
            exact_input_amounts,
            output_token_index,
            minimum_output_amount,
        } => {
            let mut ix_vec = vec![];
            let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
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
                    //TODO: need to handle if input_amount > user_token_acct.supply
                    // let approve_ix = approve(
                    //     &spl_token::id(),
                    //     &user_token_accts[token_idx],
                    //     &user_transfer_authority.pubkey(),
                    //     &user_acct_owner.pubkey(),
                    //     &[&user_acct_owner.pubkey()],
                    //     input_amount,
                    // )
                    // .unwrap();
                    // kp_vec.push(clone_keypair(user_acct_owner));
                    // ix_vec.push(approve_ix);
                }
            }
            let swap_exact_input_ix = create_swap_exact_input_ix(
                &pool::id(),
                &pool.pool_keypair.pubkey(),
                &pool.authority,
                pool.get_token_account_pubkeys(),
                &pool.lp_mint_keypair.pubkey(),
                &pool.governance_fee_keypair.pubkey(),
                &user_transfer_authority.pubkey(),
                user_token_accts,
                &spl_token::id(),
                exact_input_amounts,
                output_token_index,
                minimum_output_amount,
            )
            .unwrap();

            ix_vec.push(swap_exact_input_ix);

            (ix_vec, kp_vec)
        }
        DeFiInstruction::SwapExactOutput {
            maximum_input_amount,
            input_token_index,
            exact_output_amounts,
        } => {
            let mut ix_vec = vec![];
            let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
            approve_delegate(
                banks_client,
                correct_payer,
                &recent_blockhash,
                &user_token_accts[input_token_index as usize],
                &user_transfer_authority.pubkey(),
                user_acct_owner,
                maximum_input_amount,
            )
            .await
            .unwrap();
            // let approve_ix = approve(
            //     &spl_token::id(),
            //     &user_token_accts[input_token_index as usize],
            //     &user_transfer_authority.pubkey(),
            //     &user_acct_owner.pubkey(),
            //     &[&user_acct_owner.pubkey()],
            //     maximum_input_amount,
            // )
            // .unwrap();
            // kp_vec.push(clone_keypair(user_acct_owner));
            // ix_vec.push(approve_ix);

            let swap_exact_output_ix = create_swap_exact_output_ix(
                &pool::id(),
                &pool.pool_keypair.pubkey(),
                &pool.authority,
                pool.get_token_account_pubkeys(),
                &pool.lp_mint_keypair.pubkey(),
                &pool.governance_fee_keypair.pubkey(),
                &user_transfer_authority.pubkey(),
                user_token_accts,
                &spl_token::id(),
                maximum_input_amount,
                input_token_index,
                exact_output_amounts,
            )
            .unwrap();

            ix_vec.push(swap_exact_output_ix);

            (ix_vec, kp_vec)
        }
        DeFiInstruction::RemoveUniform {
            exact_burn_amount,
            minimum_output_amounts,
        } => {
            let mut ix_vec = vec![];
            let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
            // let approve_ix = approve(
            //     &spl_token::id(),
            //     user_lp_token_acct,
            //     &user_transfer_authority.pubkey(),
            //     &user_acct_owner.pubkey(),
            //     &[&user_acct_owner.pubkey()],
            //     exact_burn_amount,
            // )
            // .unwrap();
            // kp_vec.push(clone_keypair(user_acct_owner));
            // ix_vec.push(approve_ix);
            approve_delegate(
                banks_client,
                correct_payer,
                &recent_blockhash,
                &user_lp_token_acct,
                &user_transfer_authority.pubkey(),
                user_acct_owner,
                exact_burn_amount,
            )
            .await
            .unwrap();

            let remove_uniform_ix = create_remove_uniform_ix(
                &pool::id(),
                &pool.pool_keypair.pubkey(),
                &pool.authority,
                pool.get_token_account_pubkeys(),
                &pool.lp_mint_keypair.pubkey(),
                &pool.governance_fee_keypair.pubkey(),
                &user_transfer_authority.pubkey(),
                user_token_accts,
                &spl_token::id(),
                user_lp_token_acct,
                exact_burn_amount,
                minimum_output_amounts,
            )
            .unwrap();
            ix_vec.push(remove_uniform_ix);

            (ix_vec, kp_vec)
        }
        DeFiInstruction::RemoveExactBurn {
            exact_burn_amount,
            output_token_index,
            minimum_output_amount,
        } => {
            let ix_vec = vec![];
            let kp_vec = vec![];
            (ix_vec, kp_vec)
        }
        DeFiInstruction::RemoveExactOutput {
            maximum_burn_amount,
            exact_output_amounts,
        } => {
            let ix_vec = vec![];
            let kp_vec = vec![];
            (ix_vec, kp_vec)
        }
    };

    global_output_ixs.append(&mut output_ix);
    global_signer_keys.append(&mut signer_keys);
    let mut tx = Transaction::new_with_payer(&global_output_ixs, Some(&correct_payer.pubkey()));
    let signers = [correct_payer]
        .iter()
        .map(|&v| v) // deref &Keypair
        .chain(global_signer_keys.iter())
        .collect::<Vec<&Keypair>>();

    //Sign using some subset of required keys if recent_blockhash
    //  is not the same as currently in the transaction,
    //  clear any prior signatures and update recent_blockhash
    let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
    tx.partial_sign(&signers, recent_blockhash);
    let res = banks_client.process_transaction(tx).await;
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
                    // Note - the instruction error is ProgramFailedToComplete for Compute Budget exceeded (not sure why not InstructionError::ComputationalBudgetExceeded)
                    //[2021-10-06T08:34:26.892127400Z DEBUG solana_runtime::message_processor] Program 4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM consumed 200000 of 200000 compute units
                    //[2021-10-06T08:34:26.893069000Z DEBUG solana_runtime::message_processor] Program failed to complete: exceeded maximum number of instructions allowed (200000) at instruction #11720
                    TransactionError::InstructionError(_, ie) => match ie {
                        InstructionError::InvalidArgument
                        // | InstructionError::InvalidInstructionData
                        // | InstructionError::InvalidAccountData
                        // | InstructionError::InsufficientFunds
                        // | InstructionError::AccountAlreadyInitialized
                        // | InstructionError::InvalidSeeds
                        | InstructionError::Custom(2) // TokenError::InsufficientFunds
                        | InstructionError::Custom(118) //PoolError::OutsideSpecifiedLimits
                        | InstructionError::Custom(120) //PoolError::ImpossibleRemove
                        => {
                            println!("[DEV] received expected InstructionError: {:?}. Fuzz_ix: {:?}", ie, fuzz_instruction);
                        }
                        InstructionError::InvalidInstructionData => {
                            println!("[DEV] received InstructionError::InvalidInstructionData for fuzz_ix: {:?}", fuzz_instruction);
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
                    "[DEV] received unexpected type of error: {:?}. Fuzz_instruction: {:?}",
                    res, fuzz_instruction
                );
                panic!()
            }
        },
    }
}

/*
// fn run_fuzz_instruction<const TOKEN_COUNT: usize>(
//     banks_client: BankClient,
//     correct_payer: Keypair,
//     fuzz_instruction: FuzzInstruction<TOKEN_COUNT>,
//     pool: &PoolInfo<TOKEN_COUNT>,
//     user_wallets: &HashMap<AccountId, Keypair>,
//     all_user_transfer_authorities: &HashMap<AccountId, Keypair>,
//     all_user_token_accounts: &HashMap<usize, HashMap<AccountId, Pubkey>>,
//     all_user_lp_token_accounts: &HashMap<AccountId, Pubkey>,
// ) -> (Vec<Instruction>, Vec<Keypair>) {
//     let user_acct_id = fuzz_instruction.user_acct_id;
//     let user_acct_owner = user_wallets.get(&user_acct_id).unwrap();
//     let user_transfer_authority = all_user_transfer_authorities.get(&user_acct_id).unwrap();
//     let user_token_accts = get_user_token_accounts(user_acct_id, all_user_token_accounts);
//     let user_lp_token_acct = all_user_lp_token_accounts.get(&user_acct_id).unwrap();
//     //Notes:
//     //  - bonfida vesting
//     //      run_fuzz_ixs
//     //          run_fuzz_ix generates (output_ix: Vec<Instruction>, signer_keys: Vec<Keypair>) then appends them to
//     //          global_output_ixs: Vec<Instruction> & global_singer_keys: Vec<Keypairs>
//     //          after it runs all the ixs in one transaction
//     //  - SPL token-swap
//     match fuzz_instruction.instruction {
//         DeFiInstruction::Add {
//             input_amounts,
//             minimum_mint_amount,
//         } => {
//             let mut ix_vec = vec![];
//             let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
//             for token_idx in 0..TOKEN_COUNT {
//                 let approve_ix = approve(
//                     &spl_token::id(),
//                     &user_token_accts[token_idx],
//                     &user_transfer_authority.pubkey(),
//                     &user_acct_owner.pubkey(),
//                     &[&user_acct_owner.pubkey()],
//                     input_amounts[token_idx],
//                 )
//                 .unwrap();
//                 kp_vec.push(clone_keypair(user_acct_owner));
//                 ix_vec.push(approve_ix);
//             }
//             let add_ix = create_add_ix(
//                 &pool::id(),
//                 &pool.pool_keypair.pubkey(),
//                 &pool.authority,
//                 pool.get_token_account_pubkeys(),
//                 &pool.lp_mint_keypair.pubkey(),
//                 &pool.governance_fee_keypair.pubkey(),
//                 &user_transfer_authority.pubkey(),
//                 user_token_accts,
//                 &spl_token::id(),
//                 user_lp_token_acct,
//                 input_amounts,
//                 minimum_mint_amount,
//             )
//             .unwrap();
//             ix_vec.push(add_ix);
//             (ix_vec, kp_vec)
//         }
//         DeFiInstruction::SwapExactInput {
//             exact_input_amounts,
//             output_token_index,
//             minimum_output_amount,
//         } => {
//             let mut ix_vec = vec![];
//             let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
//             for token_idx in 0..TOKEN_COUNT {
//                 let input_amount = exact_input_amounts[token_idx];
//                 if input_amount > 0 {
//                     //TODO: need to handle if input_amount > user_token_acct.supply
//                     let approve_ix = approve(
//                         &spl_token::id(),
//                         &user_token_accts[token_idx],
//                         &user_transfer_authority.pubkey(),
//                         &user_acct_owner.pubkey(),
//                         &[&user_acct_owner.pubkey()],
//                         input_amount,
//                     )
//                     .unwrap();
//                     kp_vec.push(clone_keypair(user_acct_owner));
//                     ix_vec.push(approve_ix);
//                 }
//             }
//             let swap_exact_input_ix = create_swap_exact_input_ix(
//                 &pool::id(),
//                 &pool.pool_keypair.pubkey(),
//                 &pool.authority,
//                 pool.get_token_account_pubkeys(),
//                 &pool.lp_mint_keypair.pubkey(),
//                 &pool.governance_fee_keypair.pubkey(),
//                 &user_transfer_authority.pubkey(),
//                 user_token_accts,
//                 &spl_token::id(),
//                 exact_input_amounts,
//                 output_token_index,
//                 minimum_output_amount,
//             )
//             .unwrap();

//             ix_vec.push(swap_exact_input_ix);

//             (ix_vec, kp_vec)
//         }
//         DeFiInstruction::SwapExactOutput {
//             maximum_input_amount,
//             input_token_index,
//             exact_output_amounts,
//         } => {
//             let mut ix_vec = vec![];
//             let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
//             let approve_ix = approve(
//                 &spl_token::id(),
//                 &user_token_accts[input_token_index as usize],
//                 &user_transfer_authority.pubkey(),
//                 &user_acct_owner.pubkey(),
//                 &[&user_acct_owner.pubkey()],
//                 maximum_input_amount,
//             )
//             .unwrap();
//             kp_vec.push(clone_keypair(user_acct_owner));
//             ix_vec.push(approve_ix);

//             let swap_exact_output_ix = create_swap_exact_output_ix(
//                 &pool::id(),
//                 &pool.pool_keypair.pubkey(),
//                 &pool.authority,
//                 pool.get_token_account_pubkeys(),
//                 &pool.lp_mint_keypair.pubkey(),
//                 &pool.governance_fee_keypair.pubkey(),
//                 &user_transfer_authority.pubkey(),
//                 user_token_accts,
//                 &spl_token::id(),
//                 maximum_input_amount,
//                 input_token_index,
//                 exact_output_amounts,
//             )
//             .unwrap();

//             ix_vec.push(swap_exact_output_ix);

//             (ix_vec, kp_vec)
//         }
//         DeFiInstruction::RemoveUniform {
//             exact_burn_amount,
//             minimum_output_amounts,
//         } => {
//             let mut ix_vec = vec![];
//             let mut kp_vec = vec![clone_keypair(user_transfer_authority)];
//             let approve_ix = approve(
//                 &spl_token::id(),
//                 user_lp_token_acct,
//                 &user_transfer_authority.pubkey(),
//                 &user_acct_owner.pubkey(),
//                 &[&user_acct_owner.pubkey()],
//                 exact_burn_amount,
//             )
//             .unwrap();
//             kp_vec.push(clone_keypair(user_acct_owner));
//             ix_vec.push(approve_ix);

//             let remove_uniform_ix = create_remove_uniform_ix(
//                 &pool::id(),
//                 &pool.pool_keypair.pubkey(),
//                 &pool.authority,
//                 pool.get_token_account_pubkeys(),
//                 &pool.lp_mint_keypair.pubkey(),
//                 &pool.governance_fee_keypair.pubkey(),
//                 &user_transfer_authority.pubkey(),
//                 user_token_accts,
//                 &spl_token::id(),
//                 user_lp_token_acct,
//                 exact_burn_amount,
//                 minimum_output_amounts,
//             ).unwrap();
//             ix_vec.push(remove_uniform_ix);

//             (ix_vec, kp_vec)
//         }
//         DeFiInstruction::RemoveExactBurn {
//             exact_burn_amount,
//             output_token_index,
//             minimum_output_amount,
//         } => {
//             let ix_vec = vec![];
//             let kp_vec = vec![];
//             (ix_vec, kp_vec)
//         }
//         DeFiInstruction::RemoveExactOutput {
//             maximum_burn_amount,
//             exact_output_amounts,
//         } => {
//             let ix_vec = vec![];
//             let kp_vec = vec![];
//             (ix_vec, kp_vec)
//         }
//     }
// }

*/

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
