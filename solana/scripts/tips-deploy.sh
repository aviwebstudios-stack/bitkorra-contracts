#!/usr/bin/env bash
# Phase W-2: deploy Beryl Tips to Solana devnet (inside WSL Ubuntu).
#
#   wsl -d Ubuntu-24.04 -- bash solana/scripts/tips-deploy.sh             (normal build)
#   wsl -d Ubuntu-24.04 -- bash solana/scripts/tips-deploy.sh verifiable  (Docker build, for solana-verify)
#
# - The same throwaway devnet deployer as Beryl Names (BERYL_NAMES_DEPLOYER_KEYPAIR in
#   .env.local, never committed). It pays and stays the upgrade authority, so only it can
#   run `initialize` afterwards (npm run tips:devnet -- init). Written to a 0600 file, removed after.
# - The program's own address key lives in ~/.config/beryl (wsl-env.sh), never in the repo.
. "$(dirname "${BASH_SOURCE[0]}")/wsl-env.sh"
ENV_FILE="$SOLANA_DIR/../.env.local"
KEY="$HOME/.config/beryl/devnet-deployer.json"
PROGRAM_ID="B9oecLNJC7SCTMbZc6G7UJSwj9mUpL9AwF6r3CFE5CEf"

umask 077
grep '^BERYL_NAMES_DEPLOYER_KEYPAIR=' "$ENV_FILE" | head -1 | cut -d= -f2- > "$KEY"
trap 'rm -f "$KEY"' EXIT
[ -s "$KEY" ] || { echo "BERYL_NAMES_DEPLOYER_KEYPAIR isn't set in .env.local"; exit 1; }
echo "Deployer: $(solana-keygen pubkey "$KEY")  balance: $(solana balance "$(solana-keygen pubkey "$KEY")" -u devnet)"

if [ "${1:-}" = "verifiable" ]; then
  anchor build --verifiable --arch "$SBPF_ARCH" -p beryl_tips
  SO="$CARGO_TARGET_DIR/verifiable/beryl_tips.so"
else
  anchor build --arch "$SBPF_ARCH" -p beryl_tips
  SO="$CARGO_TARGET_DIR/deploy/beryl_tips.so"
fi
cp "$CARGO_TARGET_DIR/idl/beryl_tips.json" "$SOLANA_DIR/idl/beryl_tips.json"
echo "Program hash: $(sha256sum "$SO" | cut -d' ' -f1)"

solana program deploy "$SO" \
  --program-id "$CARGO_TARGET_DIR/deploy/beryl_tips-keypair.json" \
  --keypair "$KEY" --upgrade-authority "$KEY" \
  --with-compute-unit-price 10000 --max-sign-attempts 20 \
  -u devnet
solana program show "$PROGRAM_ID" -u devnet -k "$KEY"
