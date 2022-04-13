#!/usr/bin/env python3

import re
import sys
import argparse

from termcolor import colored
from colorama import init

init(autoreset=True)  # For colorama

TOKEN_PROGRAM_ID = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
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
    r"Program (?P<program>\w+) consumed (?P<units>\d+) of (?P<budget>\d+) compute units"
)
# executionRE = re.compile(
#     r"(?P<type>(invoke)|(consumed)|(success)|(failed))(?P<tail>.*)"
# )
# remainingRE = re.compile(r"(?P<units>\d+) units remaining")
# finalRE = re.compile(r"")

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

    program_log = programRE.match(sol_line.group("tail"))
    if program_log is None:
        continue

    if program_log.group("program") == pool_program_id:
        print(pool_prefix, "consumed: ", program_log.group("units"))
