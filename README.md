<div align="center">

<img src="assets/logo.png" alt="u50 logo" width="128">

# u50

<i>The BusyBox of CS50</i>

[![CI](https://github.com/HoppouDev/u50/actions/workflows/rust.yml/badge.svg)](https://github.com/HoppouDev/u50/actions/workflows/rust.yml)
[![Harness Score](https://raw.githubusercontent.com/HoppouDev/u50/badges/harness-badge.svg)](https://github.com/HoppouDev/u50/actions/workflows/rust.yml)
[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue.svg)](LICENSE.md)

[Quick start](#quick-start) • [Parity](#parity) • [Development](#development)

</div>

u50 unifies Harvard CS50's three command-line tools ([check50](https://github.com/cs50/check50), [style50](https://github.com/cs50/style50), and [submit50](https://github.com/cs50/submit50)) into a single Rust binary.

## Quick start

The only dependency for building is Rust because other dependencies are fetched on-demand.

```sh
git clone https://github.com/HoppouDev/u50 && cd u50
cargo build --release
```

## Parity

Most significant features are present, but some are currently unimplemented.

| Feature                   | Linux | Windows |
| ------------------------- | ----- | ------- |
| Build & test suite        | ✅    | ✅      |
| `u50 check` (core engine) | ✅    | ✅      |
| Valgrind-decorated checks | ✅    | ❌      |
| `u50 style`               | ✅    | ✅      |
| `u50 submit`              | ❌    | ❌      |

✅ supported and covered by CI · ⚠️ works with caveats · ❌ unsupported.

> [!WARNING]
> macOS is currently unsupported and untested since I do not currently use it. I am planning to add support in the future since some students use it, but until then, Windows and Linux will be the only officially supported platforms.

## Development

Tests are run for both Linux and Windows on push.

```sh
cargo build
cargo test
cargo clippy --workspace --all-targets -- -Dwarnings
cargo fmt --all -- --check
```
