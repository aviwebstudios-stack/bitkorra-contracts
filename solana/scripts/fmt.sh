#!/usr/bin/env bash
# Phase W-1: format the Rust code (rustfmt), the way scripts/test.sh checks it.
. "$(dirname "${BASH_SOURCE[0]}")/wsl-env.sh"
cargo fmt --all
