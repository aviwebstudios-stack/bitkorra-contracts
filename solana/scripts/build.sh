#!/usr/bin/env bash
# Phase W-1 (W-2: both programs): build the programs and their IDLs (inside WSL Ubuntu). The IDL is copied to
# solana/idl/, where the app's tests check the hand-written client against it.
. "$(dirname "${BASH_SOURCE[0]}")/wsl-env.sh"
anchor build --arch "$SBPF_ARCH"
for P in beryl_names beryl_tips beryl_escrow; do cp "$CARGO_TARGET_DIR/idl/$P.json" "$SOLANA_DIR/idl/$P.json"; done
ls -la "$CARGO_TARGET_DIR/deploy" "$SOLANA_DIR/idl"
