#!/bin/bash

set -x
set -euo pipefail

# Pin dependencies for MSRV builds and tests. Run from the repository root
# with the package's rust-version toolchain:
#
#   RUSTUP_TOOLCHAIN=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml) ./ci/pin-msrv.sh
#
# Use a disposable checkout: this rewrites Cargo.lock. Do not commit the result.
#
# Keep CI compatibility pins here rather than restricting published dependencies
# solely to make the MSRV job pass.

# home 0.5.11 requires Rust 1.81.
cargo update -p home --precise "0.5.5"

# base64ct 1.7.x requires Rust 1.81 or newer.
cargo update -p base64ct --precise "1.6.0"

# minreq 2.13.4 uses std::sync::LazyLock for rustls HTTPS, unavailable on MSRV.
cargo update -p minreq --precise "2.13.0"
