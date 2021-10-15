#![allow(dead_code)]
use borsh::BorshDeserialize;
use pool::{common::*, decimal::*, entrypoint::TOKEN_COUNT, instruction::*, state::PoolState};
use solana_program::{program_pack::Pack, pubkey::Pubkey, rent::Rent};

use solana_program_test::*;
use solana_sdk::{
    account::Account as AccountState,
    instruction::{Instruction, InstructionError},
    signature::{Keypair, Signer},
    system_instruction::create_account,
    transaction::{Transaction, TransactionError},
    transport::TransportError,
};
use spl_token::state::{Account as TokenState, Mint as MintState};

// limit to track compute unit increase.
// Mainnet compute budget as of 08/25/2021 is 200_000
pub const COMPUTE_BUDGET: u64 = 200_000;

pub type AmountT = u64;
pub type DecT = DecimalU64;

fn copy_keypair(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).unwrap()
}

pub struct SolanaNode {
    banks_client: BanksClient,
    payer: Keypair,
    default_delegate: Keypair, //this could just be payer too but nicer to keep it at least a little separate
    rent: Rent,
    instructions: Vec<Instruction>,
    signers: Vec<Keypair>,
}

impl SolanaNode {
    pub async fn new() -> Self {
        let (mut banks_client, payer, _recent_blockhash) = {
            //TODO I don't yet know why these arguments are passed along here
            let mut test = ProgramTest::new(
                "pool",
                pool::id(),
                processor!(pool::processor::Processor::<TOKEN_COUNT>::process),
            );

            test.set_bpf_compute_max_units(COMPUTE_BUDGET);
            test.start().await
        };
        let rent = banks_client.get_rent().await.unwrap();
        let default_delegate = Keypair::new();
        Self {
            banks_client,
            payer,
            default_delegate,
            rent,
            instructions: Vec::new(),
            signers: Vec::new(),
        }
    }

    pub async fn get_account_state(&mut self, pubkey: &Pubkey) -> AccountState {
        self.banks_client
            .get_account(*pubkey)
            .await
            .expect("account not found")
            .expect("account empty")
    }

    fn default_owner(&self) -> &Keypair {
        &self.payer
    }

    fn default_delegate(&self) -> &Keypair {
        &self.default_delegate
    }

    fn push_instruction(&mut self, ix: Instruction) {
        self.instructions.push(ix);
    }

    fn push_signer(&mut self, signer: &Keypair) {
        self.signers.push(copy_keypair(signer));
    }

    fn create_account(&mut self, size: usize, owner: Option<&Pubkey>) -> Keypair {
        let keypair = Keypair::new();
        self.instructions.push(create_account(
            &self.payer.pubkey(),
            &keypair.pubkey(),
            self.rent.minimum_balance(size),
            size as u64,
            owner.unwrap_or(&self.payer.pubkey()),
        ));

        self.push_signer(&keypair);

        keypair
    }

    fn create_mint(&mut self, decimals: u8, owner: &Pubkey) -> Pubkey {
        let keypair = self.create_account(MintState::LEN, Some(&spl_token::id()));
        self.instructions.push(
            spl_token::instruction::initialize_mint(&spl_token::id(), &keypair.pubkey(), owner, None, decimals)
                .unwrap(),
        );

        keypair.pubkey()
    }

    fn create_token_account(&mut self, mint: &Pubkey, owner: &Pubkey) -> Pubkey {
        let keypair = self.create_account(TokenState::LEN, Some(&spl_token::id()));
        self.instructions.push(
            spl_token::instruction::initialize_account(&spl_token::id(), &keypair.pubkey(), mint, owner).unwrap(),
        );

        keypair.pubkey()
    }

    async fn execute_transaction(&mut self) -> Result<(), InstructionError> {
        if self.instructions.is_empty() {
            return Ok(());
        }

        self.signers.push(copy_keypair(&self.payer));

        let transaction = Transaction::new_signed_with_payer(
            &self.instructions,
            Some(&self.payer.pubkey()),
            &self.signers.iter().map(|signer| signer).collect::<Vec<_>>(),
            self.banks_client.get_recent_blockhash().await.unwrap(),
        );
        let result = self.banks_client.process_transaction(transaction).await;

        self.instructions.clear();
        self.signers.clear();

        if let Err(transport_error) = result {
            if let TransportError::TransactionError(tx_error) = transport_error {
                if let TransactionError::InstructionError(_ix_index, ix_error) = tx_error {
                    return Err(ix_error);
                } else {
                    panic!("unexpected transport error: {:?}", tx_error);
                }
            } else {
                panic!("unexpected transport error: {:?}", transport_error);
            }
        }
        Ok(())
    }
}

//all MintAccounts are owned by the default_owner
#[derive(Debug)]
pub struct MintAccount(Pubkey);

impl MintAccount {
    pub fn new(decimals: u8, solnode: &mut SolanaNode) -> Self {
        Self(solnode.create_mint(decimals, &solnode.default_owner().pubkey()))
    }

    pub fn pubkey(&self) -> &Pubkey {
        &self.0
    }

    pub async fn state(&self, solnode: &mut SolanaNode) -> MintState {
        Self::get_state(&self.0, solnode).await
    }

    pub fn mint_to(&self, recipient: &TokenAccount, amount: AmountT, solnode: &mut SolanaNode) {
        if amount > 0 {
            solnode.push_instruction(
                spl_token::instruction::mint_to(
                    &spl_token::id(),
                    &self.0,
                    &recipient.pubkey(),
                    &solnode.default_owner().pubkey(),
                    &[&solnode.default_owner().pubkey()],
                    amount,
                )
                .unwrap(),
            );
        }
    }

    async fn get_state(pubkey: &Pubkey, solnode: &mut SolanaNode) -> MintState {
        let mint_account = solnode.get_account_state(pubkey).await;
        MintState::unpack_from_slice(mint_account.data.as_slice()).unwrap()
    }
}

#[derive(Debug)]
pub struct TokenAccount(Pubkey);

impl TokenAccount {
    pub fn new(mint: &MintAccount, solnode: &mut SolanaNode) -> Self {
        Self::internal_new(&mint.pubkey(), solnode)
    }

    pub fn pubkey(&self) -> &Pubkey {
        &self.0
    }

    pub async fn state(&self, solnode: &mut SolanaNode) -> TokenState {
        Self::get_state(&self.0, solnode).await
    }

    pub async fn balance(&self, solnode: &mut SolanaNode) -> AmountT {
        Self::get_balance(&self.0, solnode).await
    }

    pub fn approve(&self, amount: AmountT, solnode: &mut SolanaNode) {
        solnode.push_instruction(
            spl_token::instruction::approve(
                &spl_token::id(),
                &self.0,
                &solnode.default_delegate().pubkey(),
                &solnode.default_owner().pubkey(),
                &[&solnode.default_owner().pubkey()],
                amount,
            )
            .unwrap(),
        );
    }

    async fn get_state(pubkey: &Pubkey, solnode: &mut SolanaNode) -> TokenState {
        let token_account = solnode.get_account_state(pubkey).await;
        TokenState::unpack_from_slice(token_account.data.as_slice()).unwrap()
    }

    async fn get_balance(pubkey: &Pubkey, solnode: &mut SolanaNode) -> AmountT {
        Self::get_state(pubkey, solnode).await.amount
    }

    fn internal_new(mint: &Pubkey, solnode: &mut SolanaNode) -> Self {
        Self(solnode.create_token_account(&mint, &solnode.default_owner().pubkey()))
    }
}

#[derive(Debug)]
pub struct DeployedPool {
    pool_keypair: Keypair,
    authority: Pubkey,
    lp_mint: Pubkey,
    stable_accounts: [Pubkey; TOKEN_COUNT],
    pub governance_keypair: Keypair,
    pub governance_fee_account: Pubkey,
}

impl DeployedPool {
    pub async fn new(
        lp_decimals: u8,
        stable_mints: &[MintAccount; TOKEN_COUNT],
        amp_factor: DecT,
        lp_fee: DecT,
        governance_fee: DecT,
        solnode: &mut SolanaNode,
    ) -> Result<Self, InstructionError> {
        let pool_keypair = solnode.create_account(
            solana_program::borsh::get_packed_len::<pool::state::PoolState<TOKEN_COUNT>>(),
            Some(&pool::id()),
        );
        let (authority, nonce) = Pubkey::find_program_address(&[&pool_keypair.pubkey().to_bytes()[..32]], &pool::id());
        let lp_mint = solnode.create_mint(lp_decimals, &authority);
        let stable_accounts = create_array(|i| solnode.create_token_account(&stable_mints[i].pubkey(), &authority));
        let governance_keypair = solnode.create_account(0, None);
        let governance_fee_account = solnode.create_token_account(&lp_mint, &governance_keypair.pubkey());
        solnode
            .execute_transaction()
            .await
            .expect("transaction failed unexpectedly");

        solnode.push_instruction(
            create_init_ix::<TOKEN_COUNT>(
                &pool::id(),
                &pool_keypair.pubkey(),
                &lp_mint,
                &create_array(|i| *stable_mints[i].pubkey()),
                &stable_accounts,
                &governance_keypair.pubkey(),
                &governance_fee_account,
                nonce,
                amp_factor,
                lp_fee,
                governance_fee,
            )
            .unwrap(),
        );
        solnode.execute_transaction().await?;

        Ok(Self {
            pool_keypair,
            authority,
            lp_mint,
            stable_accounts,
            governance_keypair,
            governance_fee_account,
        })
    }

    pub async fn execute_defi_instruction(
        &self,
        defi_instruction: DeFiInstruction<TOKEN_COUNT>,
        user_stable_accounts: &[TokenAccount; TOKEN_COUNT],
        user_lp_account: Option<&TokenAccount>,
        solnode: &mut SolanaNode,
    ) -> Result<(), InstructionError> {
        solnode
            .execute_transaction()
            .await
            .expect("transaction failed unexpectedly");

        solnode.push_instruction(
            create_defi_ix(
                defi_instruction,
                &pool::id(),
                &self.pool_keypair.pubkey(),
                &self.authority,
                &self.stable_accounts,
                &self.lp_mint,
                &self.governance_fee_account,
                &solnode.default_delegate().pubkey(),
                &create_array(|i| *user_stable_accounts[i].pubkey()),
                &spl_token::id(),
                user_lp_account.map(|account| account.pubkey()),
            )
            .unwrap(),
        );
        solnode.push_signer(&copy_keypair(solnode.default_delegate()));
        solnode.execute_transaction().await
    }

    pub async fn balances(&self, solnode: &mut SolanaNode) -> [AmountT; TOKEN_COUNT] {
        //async closures are unstable...
        let mut balances = [0 as AmountT; TOKEN_COUNT];
        for i in 0..TOKEN_COUNT {
            balances[i] = TokenAccount::get_balance(&self.stable_accounts[i], solnode).await;
        }
        balances
    }

    pub async fn state(&self, solnode: &mut SolanaNode) -> PoolState<TOKEN_COUNT> {
        let pool_account = solnode.get_account_state(&self.pool_keypair.pubkey()).await;
        PoolState::<TOKEN_COUNT>::deserialize(&mut pool_account.data.as_slice()).unwrap()
    }

    pub async fn lp_total_supply(&self, solnode: &mut SolanaNode) -> AmountT {
        MintAccount::get_state(&self.lp_mint, solnode).await.supply
    }

    pub fn create_lp_account(&self, solnode: &mut SolanaNode) -> TokenAccount {
        TokenAccount::internal_new(&self.lp_mint, solnode)
    }
}
