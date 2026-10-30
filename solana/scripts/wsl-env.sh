#!/usr/bin/env bash
# Phase W-1 (W-2: also Beryl Tips): the shared setup for every Solana script (run inside WSL Ubuntu).
# - Tools: Rust (rustup, pinned in rust-toolchain.toml), Agave 4.3.2, Anchor 1.2.1.
# - Build output lives on Ubuntu's own disk (CARGO_TARGET_DIR): building on the
#   Windows drive (/mnt/c) is many times slower.
# - Two parallel jobs: WSL here has about 3 GB of memory.
set -euo pipefail
. "$HOME/.cargo/env"
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.avm/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/beryl-solana-target"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
# SBPF v2, not Anchor 1.2.1's default v3: LiteSVM 0.10 (the tests) can't load v3
# programs yet, and the tests must run the exact binary that is deployed. v2 is
# active on devnet (SIMD-0173/0174); move to v3 once LiteSVM loads it.
export SBPF_ARCH="${SBPF_ARCH:-v2}"
SOLANA_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export SOLANA_DIR
cd "$SOLANA_DIR"
# Anchor and the tests look for the build in ./target: point it at the fast one.
if [ ! -e target ]; then ln -s "$CARGO_TARGET_DIR" target; fi
mkdir -p "$CARGO_TARGET_DIR/deploy"
# Each program's own keypair (its address) is kept outside the repo.
for PROGRAM in beryl_names beryl_tips beryl_escrow; do
  PROGRAM_KEY="$HOME/.config/beryl/$PROGRAM-program-keypair.json"
  if [ -f "$PROGRAM_KEY" ] && [ ! -f "$CARGO_TARGET_DIR/deploy/$PROGRAM-keypair.json" ]; then
    cp "$PROGRAM_KEY" "$CARGO_TARGET_DIR/deploy/$PROGRAM-keypair.json"
  fi
done
