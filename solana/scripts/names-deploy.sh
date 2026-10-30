#!/usr/bin/env bash
# Phase W-1: deploy Beryl Names to Solana devnet (inside WSL Ubuntu).
#
#   wsl -d Ubuntu-24.04 -- bash solana/scripts/names-deploy.sh             (normal build)
#   wsl -d Ubuntu-24.04 -- bash solana/scripts/names-deploy.sh verifiable  (Docker build, for solana-verify)
#
# - The deployer is the throwaway devnet key in BERYL_NAMES_DEPLOYER_KEYPAIR (.env.local,
#   never committed). It pays for the deploy and stays the program's upgrade authority
#   (owner decision, 2026-10-27). It's written to a 0600 file for the CLI and removed after.
# - The program's own address key lives in ~/.config/beryl (wsl-env.sh), never in the repo.
. "$(dirname "${BASH_SOURCE[0]}")/wsl-env.sh"
ENV_FILE="$SOLANA_DIR/../.env.local"
KEY="$HOME/.config/beryl/devnet-deployer.json"
PROGRAM_ID="92jgXoEDydnpZhUFbcqoFf7aPasLGPP1S2E7CcpLYuPR"

umask 077
grep '^BERYL_NAMES_DEPLOYER_KEYPAIR=' "$ENV_FILE" | head -1 | cut -d= -f2- > "$KEY"
trap 'rm -f "$KEY"' EXIT
[ -s "$KEY" ] || { echo "BERYL_NAMES_DEPLOYER_KEYPAIR isn't set in .env.local"; exit 1; }
echo "Deployer: $(solana-keygen pubkey "$KEY")  balance: $(solana balance "$(solana-keygen pubkey "$KEY")" -u devnet)"

if [ "${1:-}" = "verifiable" ]; then
  anchor build --verifiable --arch "$SBPF_ARCH"
  SO="$CARGO_TARGET_DIR/verifiable/beryl_names.so"
else
  anchor build --arch "$SBPF_ARCH"
  SO="$CARGO_TARGET_DIR/deploy/beryl_names.so"
fi
cp "$CARGO_TARGET_DIR/idl/beryl_names.json" "$SOLANA_DIR/idl/beryl_names.json"
echo "Program hash: $(sha256sum "$SO" | cut -d' ' -f1)"

solana program deploy "$SO" \
  --program-id "$CARGO_TARGET_DIR/deploy/beryl_names-keypair.json" \
  --keypair "$KEY" --upgrade-authority "$KEY" \
  --with-compute-unit-price 10000 --max-sign-attempts 20 \
  -u devnet
solana program show "$PROGRAM_ID" -u devnet -k "$KEY"
