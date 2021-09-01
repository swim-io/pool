// use assert_matches::*;
use arrayvec::ArrayVec;
use pool::{decimal::*, instruction::*};
use solana_program::{
    account_info::AccountInfo, hash::Hash, program_option::COption, program_pack::Pack, pubkey::Pubkey,
    system_instruction,
};
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction::create_account,
    transaction::{Transaction, TransactionError},
    transport::TransportError,
};
use spl_token::{
    instruction::approve,
    state::{Account as Token, AccountState, Mint},
};

type AmountT = u64;
type DecT = DecimalU64;

#[derive(Debug)]
pub struct TestPoolAccountInfo<const TOKEN_COUNT: usize> {
    pub pool_keypair: Keypair,
    pub nonce: u8,
    pub authority: Pubkey,
    pub lp_mint_keypair: Keypair,
    pub token_mint_keypairs: [Keypair; TOKEN_COUNT],
    pub token_account_keypairs: [Keypair; TOKEN_COUNT],
    pub governance_keypair: Keypair,
    pub governance_fee_keypair: Keypair,
}

impl<const TOKEN_COUNT: usize> TestPoolAccountInfo<TOKEN_COUNT> {
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

    fn to_key_array(account_slice: &[Keypair; TOKEN_COUNT]) -> [Pubkey; TOKEN_COUNT] {
        account_slice
            .iter()
            .map(|account| account.pubkey())
            .collect::<ArrayVec<_, TOKEN_COUNT>>()
            .into_inner()
            .unwrap()
    }

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
                0,
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
                    0,
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

    /// Creates user token accounts, mints them tokens
    /// delegate approval to pool authority for transfers
    pub async fn prepare_accounts_for_add(
        &self,
        banks_client: &mut BanksClient,
        payer: &Keypair,
        user_accounts_owner: &Keypair,
        authority: &Pubkey,
        deposit_tokens_to_mint: [AmountT; TOKEN_COUNT],
        deposit_tokens_for_approval: [AmountT; TOKEN_COUNT],
    ) -> ([Keypair; TOKEN_COUNT], Keypair) {
        let mut user_token_keypairs_arrayvec = ArrayVec::<_, TOKEN_COUNT>::new();
        for _i in 0..TOKEN_COUNT {
            user_token_keypairs_arrayvec.push(Keypair::new());
        }
        let mut user_token_keypairs: [Keypair; TOKEN_COUNT] = user_token_keypairs_arrayvec.into_inner().unwrap();
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        for i in 0..TOKEN_COUNT {
            let token_mint = self.token_mint_keypairs[i].pubkey();
            let user_token_keypair = &user_token_keypairs[i];
            create_token_account(
                banks_client,
                payer,
                &recent_blockhash,
                &user_token_keypair,
                &token_mint,
                &user_accounts_owner.pubkey(),
            )
            .await
            .unwrap();

            mint_tokens_to(
                banks_client,
                payer,
                &recent_blockhash,
                &token_mint,
                &user_token_keypair.pubkey(),
                user_accounts_owner,
                deposit_tokens_to_mint[i],
            )
            .await
            .unwrap();

            approve_delegate(
                banks_client,
                payer,
                &recent_blockhash,
                &user_token_keypair.pubkey(),
                authority,
                user_accounts_owner,
                deposit_tokens_for_approval[i],
            )
            .await
            .unwrap();
        }

        let user_lp_token_keypair = Keypair::new();
        create_token_account(
            banks_client,
            payer,
            &recent_blockhash,
            &user_lp_token_keypair,
            &self.lp_mint_keypair.pubkey(),
            &user_accounts_owner.pubkey(),
        )
        .await
        .unwrap();

        (user_token_keypairs, user_lp_token_keypair)
    }

    pub async fn execute_add(
        &self,
        banks_client: &mut BanksClient,
        payer: &Keypair,
        user_accounts_owner: &Keypair,
        authority: &Pubkey, //explicitly passing in authority b/c it can be pool authority or user specified user_transfer_authority
        user_token_accounts: &[Keypair; TOKEN_COUNT],
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
                authority,
                Self::to_key_array(user_token_accounts),
                token_program_account,
                user_lp_token_account,
                deposit_amounts,
                minimum_amount,
            )
            .unwrap()],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[payer], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();
    }
}

/** Helper fns  **/
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
