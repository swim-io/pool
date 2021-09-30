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
4. To adjust the number of constituent tokens for the Pool Program, adjust the `TOKEN_COUNT` const in `src/entrypoint.rs` then rebuild and deploy the program to a new program_id

## Audits and Security
Audit scheduled, starting ~Nov 1st 2021

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

## Disclaimer
Use at your own risk. Swim Protocol Foundation, and its representatives and agents disclaim all warranties, express or implied, related to the application you are accessing, and are not liable for any transactions you conduct thereon or losses that may result therefrom. US Persons are not permitted to access or use this application.
# Fuzzing
- workaround for honggfuzz macos x incompatability found [here](https://github.com/ilmoi/rebuild-token-vesting)
  - TLDR: use docker 
- docker run -it -v $(pwd):/app/ pool bash
