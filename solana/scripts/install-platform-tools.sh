#!/usr/bin/env bash
# Phase W-1: the Solana compiler toolchain (platform-tools) that cargo-build-sbf
# needs, fetched once. Retries, because the download is large and a slow or
# dropping connection times out.
. "$(dirname "${BASH_SOURCE[0]}")/wsl-env.sh"
solana config set --url https://api.devnet.solana.com >/dev/null
solana config get | grep "RPC URL"
for attempt in 1 2 3 4 5; do
  echo "== platform-tools, attempt $attempt"
  if cargo-build-sbf --install-only; then echo "platform-tools installed"; exit 0; fi
  sleep 10
done
echo "platform-tools still failed after 5 attempts" >&2
exit 1
