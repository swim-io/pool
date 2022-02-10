# Pool Program

Rust smart contract for Solana liquidity pools with variable token number.

## Building

To build the Pool program, use the normal build command for Solana programs:

```bash
cargo build-bpf
```

## Deployment

To deploy the pool program:

1. Check that the `TOKEN_COUNT` const is set to the number of constituent tokens you want the pool program to initialize
2. Build the program:

```bash
cargo build-bpf
```

3. Deploy the program using:

```bash
solana program deploy --program-id <path_to_keypair> ./target/deploy/pool.so
```

4. To adjust the number of constituent tokens for the Pool Program, adjust the `TOKEN_COUNT` const in `src/lib.rs` then rebuild and deploy the program to a new program_id

## Audits and Security

[Kudelski audit](https://swim.io/audits/kudelski.pdf) completed Dec 13th, 2021

## Mainnet Deployments

Pools with 4 Tokens: `SWiMBJS9iBU1rMLAKBVfp73ThW1xPPwKdBHEU2JFpuo`

Pools with 6 Tokens: `SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC`

## Running Tests

```bash
cd swim/pool
# run all tests
cargo test-bpf -- --show-output
# run all tests with suppressed noisy logs
cargo test-bpf -- --show-output --nocapture --test-threads=1 2>&1 | ./sol_spam_filter.py
# run specific test
cargo test-bpf -- --test test_pool_init --show-output
```

# Fuzzing

The honggfuzz library is incompatable with macOS big sur and above (as of 11/01/2021). The workaround is to run the
fuzzing tests within a docker container and was based on the solution found [here](https://github.com/ilmoi/rebuild-token-vesting)

## How to run fuzz tests on mac os x in docker container

```sh
# 1. build docker container (only have to do this one time)
$ docker build -t pool .
# 2. run docker container
# $ docker run -it --security-opt seccomp=unrestricted -v $(pwd):/app/ pool bash
$ docker run -it --privileged -v $(pwd):/app/ pool bash
# should be in the docker container now
$ cd fuzz
$ BPF_OUT_DIR="/app/target/deploy" HFUZZ_RUN_ARGS="-t 100 -N 1000 -Q  " cargo hfuzz run pool_fuzz
# redirect output to files
$ BPF_OUT_DIR="/app/target/deploy" HFUZZ_RUN_ARGS="-t 100 -N 500 -Q -d -v" cargo hfuzz run pool_fuzz > test_output.txt 2>&1
# -t = timeout in seconds
# -n = number of threads
# -N = number of iterations
# --exit_upon_crash

# run fuzz debugger
# file for fuzz can be found in /app/fuzz/hfuzz_workspace/pool_fuzz/HONGGFUZZ.REPORT.TXT
$ BPF_OUT_DIR="/app/target/deploy" cargo hfuzz run-debug pool_fuzz 'hfuzz_workspace/pool_fuzz/SIGABRT.PC.7f3088fedce1.STACK.19a84c71ce.CODE.-6.ADDR.0.INSTR.mov____0x108(%rsp),%rax.fuzz'
```

## Troubleshooting

If you encounter `fatal error: ld terminated with signal 9` during the `docker build` or `docker run` stage, in Docker Desktop go to "Preferences" -> "Resources" and increase the memory and swap. Then try again.

## Disclaimer

Use at your own risk. Swim Protocol Foundation, and its representatives and agents disclaim all warranties, express or implied, related to the application you are accessing, and are not liable for any transactions you conduct thereon or losses that may result therefrom. US Persons are not permitted to access or use this application.
