# pool

## Developer Notes
- don't use `msg!()` in test-bpf tests. use `println!` and  `cargo test-bpf -- --show-output` 
- run `cargo fmt` to use `rustfmt`
  - configurations are in `rustfmt.toml`
### Running Tests
```bash
$ cd swim/pool
$ cargo test-bpf -- --show-output
