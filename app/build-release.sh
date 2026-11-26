#!/bin/sh
# Build the app for a release: like `cargo build --release`, but with the
# build machine's paths (e.g. your home folder) stripped from the binary.
set -eu
cd "$(dirname "$0")"
RUSTFLAGS="--remap-path-prefix=$HOME=/build" cargo build --release
echo "Built target/release/nimo-control-center ($(./target/release/nimo-control-center --version))"
