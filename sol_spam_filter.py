#!/usr/bin/env python3

# sample logs:
#      Running tests/functional.rs (target/debug/deps/functional-3f857a20a9ae6e73)
#
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

from termcolor import colored
from colorama import init

init(autoreset=True)  # For colorama

POOL_PROGRAM_ID = "SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC"
TESTCASE_FORMAT_SIZE = 100

sol_parser_name = sys.argv[0]
description = (
    r""""script to parse and filter Solana validator output for the relevant parts

first, set Solana validator log output levels in ~/.bashrc (or equivalent) (and don't forget to reload via 'source ~/.bashrc'):
export RUST_LOG=solana_runtime::system_instruction_processor=info,solana_runtime::message_processor=debug,solana_bpf_loader=info,solana_rbpf=info

then run with e.g.:
RUST_BACKTRACE=1 cargo test-bpf -- --nocapture --test-threads=1 2>&1 | """
    + sol_parser_name
)

argparser = argparse.ArgumentParser(description=description)
argparser.add_argument("-i", "--pool_program_id", default=POOL_PROGRAM_ID)
args = argparser.parse_args()
pool_program_id = args.pool_program_id

test_suiteRE = re.compile(r"\s+Running (?P<suitename>.*) \((?P<path>.*)\)")
test_startRE = re.compile(
    r"test (?P<testcase>[:\w]+) (?P<should_panic>- should panic )?... "
)
test_endRE = re.compile(r"(?P<outcome>(ok)|(FAILED)|(ignored))")
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

program_stack = []
full_budget = 200000  # this is an initial default that will be overwritten once the actual budget becomes known
previous_remaining = None
suppress = False
pool_prefix = colored(">", "blue", attrs=["bold"])


def print_with_framing(frame_char, text, color):
    midlen = TESTCASE_FORMAT_SIZE - (len(text) + 2)
    print(
        frame_char * int((midlen + 1) / 2),
        colored(text, color, attrs=["bold"]),
        frame_char * int(midlen / 2),
    )


for line in sys.stdin:
    # re.search looks for the pattern anywhere in the string, re.match only in the beginning,
    # ^ (in the regex string) at the beginning and after every newline

    # look for "running <test suite>" lines
    suite = test_suiteRE.match(line)
    if suite:
        suitename = suite.group("suitename")
        print()
        print("#" * TESTCASE_FORMAT_SIZE)
        print_with_framing("#", suitename.upper(), "magenta")
        print("#" * TESTCASE_FORMAT_SIZE)
        continue

    # look for "test <testcase>" lines
    test_start = test_startRE.match(line)
    if test_start:
        testcase = test_start.group("testcase")
        print()
        print_with_framing("=", testcase, "cyan")
        line = line[test_start.end() :]
        if test_start.group("should_panic"):
            suppress = True

    # look for end of testcase lines
    test_end = test_endRE.match(line)
    if test_end:
        suppress = False
        outcome = test_end.group("outcome")
        if outcome == "ok":
            color = "green"
        elif outcome == "FAILED":
            color = "red"
        else:
            color = "grey"
        print_with_framing("=", outcome, color)
        print()
        continue

    # suppress output of tets that have "should panic" modifier
    if suppress:
        continue

    # check if (remained) of line contains solana output, if not just print line
    sol_line = solanaRE.match(line)
    if sol_line is None:
        print(line, end="")
        continue

    # ignore all Solana output that doesn't come from the message_processor component
    if sol_line.group("sol_component") != "solana_runtime::message_processor":
        continue

    # ignore all Solana output that's not program output
    program_log = programRE.match(sol_line.group("tail"))
    if program_log is None:
        continue

    # tease apart what kind of program output we got
    log_type = program_log.group("type")
    log_tail = program_log.group("tail")
    if log_type == "log:":
        # ouput of msg!() in smart contract (= Solana program)
        if program_stack[-1] == pool_program_id:
            print(pool_prefix, "    log: ", log_tail)
    elif log_type == "consumption:":
        # output of sol_log_compute_units() in smart contract
        if program_stack[-1] == pool_program_id:
            remaining = int(remainingRE.match(log_tail).group("units"))
            if not previous_remaining is None:
                print(
                    pool_prefix,
                    f"compute:  {full_budget-remaining:6} (+ {previous_remaining-remaining:6})",
                )
            previous_remaining = remaining
    elif log_type == "failed to complete:":
        # Solana program encountered an error during execution
        print(
            pool_prefix,
            "!" * int(TESTCASE_FORMAT_SIZE / 4 - 2),
            colored("execution failed", "red", attrs=["bold"]),
        )
        if len(program_stack) > 1:
            print("emitted by a Solana program invoked by pool program:")
        print(pool_prefix, log_tail)
    else:
        # log_type is actually the id of the program that's just now starting/finishing
        program_id = log_type
        execution_log = executionRE.match(log_tail)
        execution_type = execution_log.group("type")
        if execution_type == "invoke":
            # a new Solana program is invoked, push it on program_stack
            if program_id == pool_program_id:
                # start pool block
                print()
                print(
                    pool_prefix * int(TESTCASE_FORMAT_SIZE / 4),
                    colored("Pool Instruction Start", attrs=["bold"]),
                )
            program_stack.append(program_id)
        elif execution_type == "consumed":
            # final compute budget output line
            final = finalRE.match(execution_log.group("tail"))
            total = int(final.group("units"))
            if program_stack[-1] == pool_program_id:
                print(
                    pool_prefix,
                    f"compute:  {total:6} (+ {total - (full_budget-previous_remaining):6}) final",
                )
            if len(program_stack) == 1:
                # we learn what the actual compute budget is, overwritting the initially set default
                full_budget = int(final.group("budget"))
                previous_remaining = full_budget
        elif execution_type == "success" or execution_type == "failed":
            # a Solana program has finished, pop it from program_stack
            if program_id == pool_program_id:
                # end pool block
                print(
                    pool_prefix * int(TESTCASE_FORMAT_SIZE / 4),
                    colored(
                        execution_type,
                        "green" if execution_type == "success" else "red",
                        attrs=["bold"],
                    ),
                )
                print()
            program_stack.pop()
