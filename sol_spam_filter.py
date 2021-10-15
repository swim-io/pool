#!/usr/bin/env python3

# run with e.g.:
# RUST_BACKTRACE=1 cargo test-bpf -- --show-output --nocapture --test-threads=1 2>&1 | ./sol_spam_filter.py

import re
import sys

# spam = r"^.\d{4}\-\d{2}\-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z [A-Z]* .*$"
spam = r"^.\d{4}.*"
compute = r"^.\d{4}\-\d{2}\-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z [A-Z]* .* consumed (?P<units>\d+) of \d+ compute units$"
spamRE = re.compile(spam)
computeRE = re.compile(compute)

for line in sys.stdin:
    found = False
    for _ in spamRE.finditer(line):
        found = True
        for compute_match in computeRE.finditer(line):
            num = int(compute_match.group(1))
            if num > 10000:
                print("/-----------------------------------------")
                print("| compute budget consumption:", num)
                print("\\-----------------------------------------")
    if not found:
        print(line, end="")
