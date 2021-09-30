use {
    arbitrary::Arbitrary,
    honggfuzz::fuzz,
};
use pool::{decimal::*, instruction::*, invariant::*, processor::Processor};
use pool::TOKEN_COUNT;
use solana_program_test::*;
use solana_program::{
    instruction::{Instruction, InstructionError},
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::{self},
};
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    hash::Hash,
    signature::{Keypair, Signer},
    transaction::{Transaction, TransactionError},
    transport::TransportError,
};
//use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::instruction::{initialize_mint, mint_to};



#[derive(Debug, Arbitrary)]
pub struct FuzzInstruction {
    instruction: DeFiInstruction::<{TOKEN_COUNT}>
}

fn main() {
    loop {
        // fuzz!(|data: &[u8]| {
        //     if data.len() != 3 {return}
        //     if data[0] != b'h' {return}
        //     if data[1] != b'e' {return}
        //     if data[2] != b'y' {return}
        //     panic!("BOOM")
        // });

        fuzz!(|fuzz_ixs: Vec<FuzzInstruction> | {
            println!("ix are {:?}", fuzz_ixs);

            // let mut program_test = ProgramTest::new(
            //     "pool",
            //     pool::id(),
            //     processor!(Processor::<{TOKEN_COUNT}>::process),

            // )
        });
    }
}