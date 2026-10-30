#!/usr/bin/env bash
# Phase S-1: verifiable builds of the three programs, and their submission to
# the public registry (solana-verify, owner decision 2026-10-27). Inside WSL
# Ubuntu with Docker Desktop's WSL integration on (docs/DEPLOY.md, "Solana
# verifiable builds"). Needs the public bitkorra-contracts repo pushed first:
# the build is made from a clean clone of it at one commit, exactly what the
# registry rebuilds. One program at a time (memory):
#
#   bash solana/scripts/verify.sh build  beryl_names <repo URL> <commit>   Docker build; prints its hash
#   bash solana/scripts/verify.sh deploy beryl_names <repo URL> <commit>   upgrade the devnet program to it (same ID)
#   bash solana/scripts/verify.sh check  beryl_names <repo URL> <commit>   on-chain hash = that build's hash?
#   bash solana/scripts/verify.sh submit beryl_names <repo URL> <commit>   write the verification record (signed by
#                                                                          the upgrade authority) and send it to the registry
#
# The deployer (upgrade authority) comes from BERYL_NAMES_DEPLOYER_KEYPAIR in
# this repo's .env.local (never committed), written to a 0600 file and removed
# afterwards. SBPF v2, as in wsl-env.sh: the same kind of binary the tests ran.
set -euo pipefail
. "$HOME/.cargo/env"
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.avm/bin:$PATH"
SBPF_ARCH="${SBPF_ARCH:-v2}"
APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

STEP="${1:-}"
LIB="${2:-}"
REPO_URL="${3:-}"
COMMIT="${4:-}"
case "$LIB" in
  beryl_names) PROGRAM_ID="92jgXoEDydnpZhUFbcqoFf7aPasLGPP1S2E7CcpLYuPR" ;;
  beryl_tips) PROGRAM_ID="B9oecLNJC7SCTMbZc6G7UJSwj9mUpL9AwF6r3CFE5CEf" ;;
  beryl_escrow) PROGRAM_ID="8wDapxboCTEKfBzNQzrzZUnspmTixKFfCuNmUuQLrcDA" ;;
  *) LIB="" ;;
esac
if [ -z "$LIB" ] || [ -z "$REPO_URL" ] || [ -z "$COMMIT" ]; then
  echo "Usage: verify.sh build|deploy|check|submit beryl_names|beryl_tips|beryl_escrow <public repo URL> <commit hash>"
  exit 1
fi
command -v solana-verify >/dev/null || { echo "Install it first: cargo install solana-verify --locked"; exit 1; }

# A clean clone per commit, on Ubuntu's own disk.
CLONE="$HOME/.cache/bitkorra-verify/$COMMIT"
SO="$CLONE/solana/target/deploy/$LIB.so"

with_deployer() {
  KEY="$HOME/.config/beryl/devnet-deployer.json"
  umask 077
  grep '^BERYL_NAMES_DEPLOYER_KEYPAIR=' "$APP_DIR/.env.local" | head -1 | cut -d= -f2- > "$KEY"
  trap 'rm -f "$KEY"' EXIT
  [ -s "$KEY" ] || { echo "BERYL_NAMES_DEPLOYER_KEYPAIR isn't set in .env.local"; exit 1; }
}

case "$STEP" in
  build)
    command -v docker >/dev/null || { echo "Docker isn't reachable from WSL. Turn on Docker Desktop's WSL integration for Ubuntu-24.04."; exit 1; }
    if [ ! -d "$CLONE/.git" ]; then
      git clone --quiet "$REPO_URL" "$CLONE"
      git -C "$CLONE" -c advice.detachedHead=false checkout --quiet "$COMMIT"
    fi
    (cd "$CLONE/solana" && solana-verify build --library-name "$LIB" -- --arch "$SBPF_ARCH")
    echo "Verifiable build: $SO"
    echo "Hash: $(solana-verify get-executable-hash "$SO")"
    ;;
  deploy)
    [ -f "$SO" ] || { echo "Build it first: verify.sh build $LIB $REPO_URL $COMMIT"; exit 1; }
    with_deployer
    solana program deploy "$SO" --program-id "$PROGRAM_ID" \
      --keypair "$KEY" --upgrade-authority "$KEY" \
      --with-compute-unit-price 10000 --max-sign-attempts 20 -u devnet
    ;;
  check)
    echo "This build: $(solana-verify get-executable-hash "$SO")"
    echo "On devnet:  $(solana-verify get-program-hash -u devnet "$PROGRAM_ID")"
    ;;
  submit)
    with_deployer
    solana-verify verify-from-repo -u devnet \
      --program-id "$PROGRAM_ID" \
      --library-name "$LIB" \
      --mount-path solana \
      --commit-hash "$COMMIT" \
      -k "$KEY" \
      --remote \
      "$REPO_URL" -- --arch "$SBPF_ARCH"
    ;;
  *)
    echo "Usage: verify.sh build|deploy|check|submit $LIB <public repo URL> <commit hash>"; exit 1 ;;
esac
