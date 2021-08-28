use crate::{error::to_error_msg, processor::Processor};
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, msg, pubkey::Pubkey,
};

pub const TOKEN_COUNT: usize = 4; //TODO find a proper way to set/configure this

entrypoint!(process_instruction);
pub fn process_instruction<'a>(
    program_id: &Pubkey,
    accounts: &'a [AccountInfo<'a>],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!(
        "process_instruction: {}: {} accounts, data={:?}",
        program_id,
        accounts.len(),
        instruction_data
    );

    let result = Processor::<TOKEN_COUNT>::process(program_id, accounts, instruction_data);
    if let Err(error) = &result {
        msg!("process_instruction: failed: {}", to_error_msg(&error));
    }

    result
}
