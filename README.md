<div align="center">

<img src="assets/logo.png" alt="u50 logo" width="128">

# u50

<i>The BusyBox of CS50</i>

[![CI](https://github.com/HoppouDev/u50/actions/workflows/rust.yml/badge.svg)](https://github.com/HoppouDev/u50/actions/workflows/rust.yml)
[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue.svg)](LICENSE.md)

[Quick start](#quick-start) • [Development](#development)

</div>

u50 unifies Harvard CS50's three command-line tools ([check50](https://github.com/cs50/check50), [style50](https://github.com/cs50/style50), and [submit50](https://github.com/cs50/submit50)) into a single Rust binary.

## Quick start

The only dependency for building is Rust because other dependencies are fetched on-demand.

```sh
git clone https://github.com/HoppouDev/u50 && cd u50
cargo build --release
```

## Development

Tests are run for both Linux, macOS and Windows on push by CI.

```sh
cargo build
cargo test
cargo clippy --workspace --all-targets -- -Dwarnings
cargo fmt --all -- --check
```
