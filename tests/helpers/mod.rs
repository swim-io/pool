// use assert_matches::*;
use solana_program::{program_option::COption, program_pack::Pack, pubkey::Pubkey, account_info::AccountInfo};
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    signature::{read_keypair_file, Keypair, Signer},
    system_instruction::create_account,
    transaction::{Transaction, TransactionError},
};
use spl_token::{
    instruction::approve,
    state::{Account as Token, AccountState, Mint},
};
use pool::{
    instruction::*,
    decimal::*,
};
use arrayvec::ArrayVec;


type AmountT = u64;
type DecT = DecimalU64;

// #[derive(Debug)]
// pub struct TestPoolAccountInfo<const TOKEN_COUNT: usize> {
//     pub nonce: u8,
//     pub pool_key: Pubkey,
//     //pub pool_account: Account,
//     pub lp_mint_key: Pubkey,
//     //pub lp_mint_account: Account,
//     pub token_mint_keys: [Pubkey; TOKEN_COUNT],
//     // pub token_mint_accounts: [Account; TOKEN_COUNT],
//     pub token_account_keys: [Pubkey; TOKEN_COUNT],
//     // pub token_account_accounts: [Account; TOKEN_COUNT],
//     pub governance_key: Pubkey,
//     // pub governance_account: Account,
//     pub governance_fee_key: Pubkey,
//     // pub governance_fee_account: Account,
// }

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
        let (authority, nonce) = 
            Pubkey::find_program_address(&[&pool_keypair.pubkey().to_bytes()[..32]], &pool::id());
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
            governance_fee_keypair
        }
    }

    pub async fn init_pool(
        &self,
        banks_client: &mut BanksClient,
        payer: &Keypair,
        user_accounts_owner: &Keypair,
        amp_factor: DecT, // DecimalU64::new(value, decimals).unwrap()
        lp_fee: DecT,
        governance_fee: DecT,
    )  {
        let rent = banks_client.get_rent().await.unwrap();
        let to_key_array = |account_array: &[Keypair; TOKEN_COUNT]| -> [Pubkey; TOKEN_COUNT] {
            account_array
                .iter()
                .map(|account| account.pubkey())
                .collect::<ArrayVec<_, TOKEN_COUNT>>()
                .into_inner()
                .unwrap()
        };

        let token_mint_pubkeys: [Pubkey; TOKEN_COUNT] = to_key_array(&self.token_mint_keypairs);
        let token_account_pubkeys: [Pubkey; TOKEN_COUNT] = to_key_array(&self.token_account_keypairs);

        let pool_len = solana_program::borsh::get_packed_len::<pool::state::PoolState::<TOKEN_COUNT>>();
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
            ixs_vec.push(
                create_account(
                    &payer.pubkey(),
                    &token_mint_pubkeys[i],
                    //&token_mint_keypairs[i],
                    rent.minimum_balance(Mint::LEN),
                    Mint::LEN as u64,
                    &spl_token::id(),
                ),
            );
            ixs_vec.push(
                spl_token::instruction::initialize_mint(
                    &spl_token::id(),
                    &token_mint_pubkeys[i],
                    &user_accounts_owner.pubkey(),
                    None,
                    0,
                )
                .unwrap()
            );
        }
        for i in 0..TOKEN_COUNT {
            println!("adding create_account & initialize_account ix for {}", i);
            ixs_vec.push(
                create_account(
                    &payer.pubkey(),
                    &token_account_pubkeys[i],
                    //&token_account_keypairs[i],
                    rent.minimum_balance(Token::LEN),
                    Token::LEN as u64,
                    &spl_token::id(),
                )
            );
            ixs_vec.push(
                spl_token::instruction::initialize_account(
                    &spl_token::id(),
                    &token_account_pubkeys[i],
                    &token_mint_pubkeys[i],
                    &self.authority, 
                )
                .unwrap()
            );
        }

        ixs_vec.push(
            create_account(
                &payer.pubkey(),
                &self.governance_keypair.pubkey(),
                rent.minimum_balance(Token::LEN), //TODO: not sure what the len of this should be? data would just be empty?
                Token::LEN as u64,
                &user_accounts_owner.pubkey(), //TODO: randomly assigned owner to the user account owner
            )
        );
        ixs_vec.push(
            create_account(
                &payer.pubkey(),
                &self.governance_fee_keypair.pubkey(),
                rent.minimum_balance(Token::LEN), 
                Token::LEN as u64,
                &spl_token::id(),
            )
        );
        ixs_vec.push(
            spl_token::instruction::initialize_account(
                &spl_token::id(),
                &self.governance_fee_keypair.pubkey(),
                &self.lp_mint_keypair.pubkey(),
                &user_accounts_owner.pubkey(), //TODO: randomly assigned governance_fee token account owner to the user account owner,
            )
            .unwrap()
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
                governance_fee
            ).unwrap()
        );

        let mut transaction = Transaction::new_with_payer(
            &ixs_vec,
            Some(&payer.pubkey()),
        );
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


        transaction.sign(
            &signatures,
            recent_blockhash,
        );

        banks_client
            .process_transaction(transaction)
            .await
            .unwrap();
    }
}

