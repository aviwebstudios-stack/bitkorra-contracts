#!/usr/bin/env bash
# Phase W-1 (W-2: both programs): build the programs, then run every check: rustfmt,
# clippy, and the LiteSVM tests (each instruction, each failure path, proptest fuzzing).
#
#   wsl -d Ubuntu-24.04 -- bash solana/scripts/test.sh      (from the repo, in PowerShell)
#   bash solana/scripts/test.sh                              (inside Ubuntu)
. "$(dirname "${BASH_SOURCE[0]}")/wsl-env.sh"
echo "== rustfmt"; cargo fmt --all -- --check
echo "== anchor build ($SBPF_ARCH)"; anchor build --arch "$SBPF_ARCH"
echo "== clippy"; cargo clippy --all-targets -- -D warnings
echo "== tests"; cargo test -- --test-threads=2
