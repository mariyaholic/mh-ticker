<!-- SPDX-License-Identifier: MIT -->
# Contributing

Use Rust 1.99.1. Build the application on Windows with `cargo build --locked -p mh_ticker`.
Run `cargo test --locked` for portable tests and `cargo test --workspace --locked` on Windows.
Run `cargo fmt --all --check`, `cargo clippy --all-targets --locked`, and
`cargo run --locked -p mh-specgen -- --check`. See [testing](docs/TESTING.md).
Keep pull requests small and focused. No DCO sign-off is required. Contributions are licensed
under the repository's [MIT licence](LICENSE).
