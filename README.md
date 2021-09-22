# pool

## TODOs
1. posssibly refactor ix names to follow standard semantics
   1. Add -> Deposit
   2. RemoveX -> WithdrawX

## Developer Notes
- don't use `msg!()` in test-bpf tests. use `println!` and  `cargo test-bpf -- --show-output` 
- run `cargo fmt` to use `rustfmt`
  - configurations are in `rustfmt.toml`
### Running Tests
```bash
$ cd swim/pool
# run all tests
$ cargo test-bpf -- --show-output
# run specific test
$ cargo test-bpf -- --test test_pool_init --show-output
```

