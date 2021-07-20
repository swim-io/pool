use solana_program::{
    msg,
    entrypoint,
    entrypoint::ProgramResult,
	account_info::AccountInfo,
    pubkey::Pubkey,
};
use crate::{
	processor::Processor,
	error::to_error_msg,
};

const TOKEN_COUNT: usize = 4; //TODO find a proper way to set/configure this

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
