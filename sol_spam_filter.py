#!/usr/bin/env python3

# sample logs:
# [2021-10-17T18:46:09.392046419Z DEBUG solana_runtime::message_processor] Program 11111111111111111111111111111111 invoke [1]
# [2021-10-17T18:46:09.392091734Z TRACE solana_runtime::system_instruction_processor] process_instruction: CreateAccount { lamports: 1461600, space: 82, owner: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA }
# [2021-10-17T18:46:09.392157007Z TRACE solana_runtime::system_instruction_processor] keyed_accounts: [KeyedAccount { is_signer: true, is_writable: true, key: GqvbDhbCK1BheJPFRAAAz1MYB1H21rHMF7YTumMR4k3w, account: RefCell { value: Account { lamports: 999999998453400 data.len: 0 owner: 11111111111111111111111111111111 executable: false rent_epoch: 0 } } }, KeyedAccount { is_signer: true, is_writable: true, key: Gm4gfv2u4bH1FgfALB4bsP8dNmX4cK6ajWTHNEmd3aHh, account: RefCell { value: Account { lamports: 0 data.len: 0 owner: 11111111111111111111111111111111 executable: false rent_epoch: 0 } } }]
# [2021-10-17T18:46:09.392342716Z DEBUG solana_runtime::message_processor] Program 11111111111111111111111111111111 success
# [2021-10-17T17:06:46.848529947Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]
# [2021-10-17T17:06:46.849016462Z DEBUG solana_runtime::message_processor] Program log: Instruction: Approve
# [2021-10-17T17:06:46.849589541Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 2423 of 300000 compute units
# [2021-10-17T17:06:46.849683688Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success
# [2021-10-17T17:06:47.005244061Z DEBUG solana_runtime::message_processor] Program SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC invoke [1]
# [2021-10-17T17:06:47.006892975Z DEBUG solana_runtime::message_processor] Program consumption: 296164 units remaining
# [2021-10-17T17:06:47.006973326Z DEBUG solana_runtime::message_processor] Program log: POOL: process
# [2021-10-17T17:06:47.007007050Z DEBUG solana_runtime::message_processor] Program consumption: 296147 units remaining
# [2021-10-17T17:06:47.052774528Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]
# [2021-10-17T17:06:47.053279489Z DEBUG solana_runtime::message_processor] Program log: Instruction: Transfer
# [2021-10-17T17:06:47.054373358Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3248 of 183503 compute units
# [2021-10-17T17:06:47.054590506Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success
# [2021-10-17T17:06:47.064755340Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]
# [2021-10-17T17:06:47.065320303Z DEBUG solana_runtime::message_processor] Program log: Instruction: Transfer
# [2021-10-17T17:06:47.066309155Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3248 of 177470 compute units
# [2021-10-17T17:06:47.066645227Z DEBUG solana_runtime::message_processor] Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success
# ...
# [2021-10-17T17:06:47.139254580Z DEBUG solana_runtime::message_processor] Program SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC consumed 165491 of 300000 compute units
# [2021-10-17T17:06:47.139406837Z DEBUG solana_runtime::message_processor] Program SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC success
#
# error sample:
# [2021-10-17T18:46:10.971858270Z DEBUG solana_runtime::message_processor] Program SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC consumed 180000 of 180000 compute units
# [2021-10-17T18:46:10.971975411Z DEBUG solana_runtime::message_processor] Program failed to complete: exceeded maximum number of instructions allowed (180000) at instruction #36055
# [2021-10-17T18:46:10.972093453Z DEBUG solana_runtime::message_processor] Program SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC failed: Program failed to complete


import re
import sys
import argparse

POOL_PROGRAM_ID = "SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC"
TESTCASE_FORMAT_SIZE = 100

sol_parser_name = sys.argv[0]
description = (
    r""""script to parse and filter Solana validator output for the relevant parts
run with e.g.:
RUST_BACKTRACE=1 cargo test-bpf -- --show-output --nocapture --test-threads=1 2>&1 | """
    + sol_parser_name
)

testRE = re.compile(r"test (?P<testcase>\w+) ... ")
solanaRE = re.compile(
    r"\[\d{4}\-\d{2}\-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z (?P<log_level>[A-Z]+) (?P<sol_component>.*)\] (?P<tail>.*)"
)
programRE = re.compile(
    r"Program (?P<type>(log:)|(consumption:)|(failed to complete:)|(\w+)) (?P<tail>.*)"
)
executionRE = re.compile(
    r"(?P<type>(invoke)|(consumed)|(success)|(failed))(?P<tail>.*)"
)
remainingRE = re.compile(r"(?P<units>\d+) units remaining")
finalRE = re.compile(r" (?P<units>\d+) of (?P<budget>\d+) compute units")

argparser = argparse.ArgumentParser(description=description)
argparser.add_argument("-i", "--pool_program_id", default=POOL_PROGRAM_ID)
args = argparser.parse_args()
pool_program_id = args.pool_program_id

program_stack = []
full_budget = 200000  # this is an initial default that will be overwritten once the actual budget becomes known
previous_remaining = None

for line in sys.stdin:
    testcase = testRE.match(line)
    # re.search looks for the pattern anywhere in the string, re.match only in the beginning,
    # ^ (in the regex string) at the beginning and after every newline
    if testcase:
        line = line[testcase.end() :]
        testcase_name = testcase.group("testcase")
        print()
        print("=" * TESTCASE_FORMAT_SIZE)
        midlen = TESTCASE_FORMAT_SIZE - (len(testcase_name) + 2)
        print("=" * int((midlen + 1) / 2), testcase_name, "=" * int(midlen / 2))
        print("=" * TESTCASE_FORMAT_SIZE)

    if line[0:2] == "ok" or line[0:6] == "FAILED":
        reslen = 2 if line[0:2] == "ok" else 6
        midlen = TESTCASE_FORMAT_SIZE - (reslen + 2)
        print("=" * int(midlen / 2), line[0:reslen], "=" * int(midlen / 2))
        print()
        continue

    sol_line = solanaRE.match(line)
    if sol_line is None:
        print(line, end="")
        continue

    if sol_line.group("sol_component") != "solana_runtime::message_processor":
        continue

    program_log = programRE.match(sol_line.group("tail"))
    if program_log is None:
        continue

    log_type = program_log.group("type")
    log_tail = program_log.group("tail")
    if log_type == "log:":
        # ouput of msg!() in smart contract
        if program_stack[-1] == pool_program_id:
            print(">     LOG:", log_tail)
    elif log_type == "consumption:":
        # output of sol_log_compute_units() in smart contract
        if program_stack[-1] == pool_program_id:
            remaining = int(remainingRE.match(log_tail).group("units"))
            if not previous_remaining is None:
                print(
                    f"> COMPUTE: {full_budget-remaining} (+ {previous_remaining-remaining}) compute units consumed cummulative(+additional)"
                )
            previous_remaining = remaining
    elif log_type == "failed to complete:":
        print("> !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")
        print("> !!!!!!!!!  EXECUTION FAILED  !!!!!!!!!")
        print("> !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")
        print("> Cause:")
        print(">", log_tail)
    else:
        program_id = log_type
        execution_log = executionRE.match(log_tail)
        execution_type = execution_log.group("type")
        if execution_type == "invoke":
            if program_id == pool_program_id:
                # start pool block
                print("\n>>>>>>>>>>>>>>>> Pool Instruction Start")
            program_stack.append(program_id)
        elif execution_type == "consumed":
            final = finalRE.match(execution_log.group("tail"))
            total = int(final.group("units"))
            if program_stack[-1] == pool_program_id:
                print(f"> COMPUTE: {total} final compute budget consumption")
            if len(program_stack) == 1:
                full_budget = int(final.group("budget"))
                previous_remaining = full_budget
        elif execution_type == "success" or execution_type == "failed":
            if program_id == pool_program_id:
                # end pool block
                print(">>>>>>>>>>>>>>>>", execution_type, "\n")
            program_stack.pop()
