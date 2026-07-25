# Contributing

SIM is human-directed and AI-executed. Changes should keep crate entrypoints
thin, put behavior in the owning component, and include the validation named in
the constellation manifest.

Run the repository gate before proposing changes:

```bash
cargo fmt --all --check
cargo run -p xtask -- check-file-sizes
cargo run -p xtask -- check-recipes
cargo run -p xtask -- check-package-floors
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- simdoc --check
```
