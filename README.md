# Pool Program

A cross-chain AMM combined with Wormhole's bridging functionality to enable 
native asset cross-chain swaps in a seamless and efficient manner utilizing the stableswap invariant.


## Building
To build the Pool program, use the normal build command for Solana programs:

```bash
cargo build-bpf
```

To adjust the number of constituent tokens for the Pool Program, adjust the `TOKEN_COUNT` const in `src/entrypoint.rs` then rebuild the program

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

## Audits and Security
Coming soon

## Mainnet Deployments
4 Pool: `SWiMBJS9iBU1rMLAKBVfp73ThW1xPPwKdBHEU2JFpuo`
6 Pool: `SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC`

## Running Functional Tests

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