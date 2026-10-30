# Deployments, tests and live runs

This is the full engineering log, copied from the BitKorra app's repository (where it is `docs/contracts.md`).
Paths outside `contracts/` and `solana/` (`lib/`, `server/`, `scripts/`, `.env.local`) point into that app, which isn't in this repository.
Names like Beryl and Beryva are the project's earlier code names; the product is BitKorra.

# Beryva Swap: Beryva's own contracts (phase I-6)

Beryva Swap is Beryva's own constant-product AMM (the Uniswap V2 design), written for this app and deployed on **Ethereum Sepolia only**. It swaps Sepolia ETH and Circle's test USDC. **Mainnet use is a pause point** (`docs/INTEGRATION_PLAN.md` section 13): nothing here goes to a real network without an independent professional audit and the owner's written go-ahead.

## Where things are

| Path | What |
|---|---|
| `contracts/src/BeryvaFactory.sol` | Creates one pool per token pair (CREATE2, so a pool's address is fixed by its tokens) and keeps the list. No owner. |
| `contracts/src/BeryvaPair.sol` | One pool: holds two tokens, swaps at the price their reserves set (x × y = k), keeps 0.3% of each swap's input for liquidity providers, and issues LP tokens (`BRV-LP`). |
| `contracts/src/BeryvaRouter.sol` | What people (and the app) call: swaps with a minimum to receive and a deadline, adds and removes liquidity, wraps and unwraps ETH through WETH. Never keeps tokens or ETH between calls. |
| `contracts/src/BeryvaLibrary.sol` | The quote maths. `lib/beryva/swap/amm.ts` is the same maths in the app, and both are tested against the same vectors. |
| `contracts/test/` | Foundry unit, fuzz and invariant tests, and a Sepolia fork test. |
| `contracts/script/Deploy.s.sol` | Deploys the factory and router on Sepolia and seeds the ETH/USDC pool. Refuses any other chain. |
| `contracts/script/LiveSwap.s.sol` | A real swap both ways on Sepolia from the throwaway deployer wallet. |
| `lib/beryva/swap-config.ts` | **The deployed addresses (one place)**, slippage, deadline, price-impact and per-swap limits. |

## Tooling (owner decisions, 2026-10-22)

- Foundry (`forge` 1.8.1), Solidity **0.8.30** pinned in `contracts/foundry.toml`, optimizer 200 runs, EVM `cancun`, legacy code generation (no `via_ir`).
- OpenZeppelin Contracts **v5.7.0** (`ERC20`, `SafeERC20`, `ReentrancyGuard`, `Math`) and forge-std **v1.17.0**. They aren't committed; install them from `contracts/` with:
  `forge install --no-git OpenZeppelin/openzeppelin-contracts@v5.7.0 foundry-rs/forge-std@v1.17.0`
- Slither 0.11.6 in its own Python venv outside the repo (`~/.venvs/slither`).

Licences: OpenZeppelin Contracts and forge-std are MIT; Beryva's contracts are MIT.

## What each contract does

**BeryvaPair (a pool).**
- `mint(to)`: the caller sends both tokens first. The first deposit gets `sqrt(x × y) − 1000` LP tokens, and 1,000 are locked forever at `0x…dEaD`, so the share price can't be pushed so high that later deposits round to nothing. Later deposits get the smaller of the two proportional shares (anything beyond the pool's ratio stays in the pool).
- `burn(to)`: the caller sends LP tokens first and gets its share of both tokens, rounded down.
- `swap(amount0Out, amount1Out, to)`: the caller sends the input first. The pool works out what came in, and the new balances (less 0.3% of the input) must keep `reserve0 × reserve1` at least where it was, or the whole call reverts. Checks, then effects (reserves updated), then the one outside call (the transfer out).
- `skim(to)` and `sync()`: handle tokens sent to the pool by mistake.
- Every state-changing function has OpenZeppelin's `nonReentrant`.

**BeryvaRouter.**
- `swapExactETHForTokens`, `swapExactTokensForETH`, `swapExactTokensForTokens`: exact input, a minimum out (`amountOutMin`, from the slippage setting) and a deadline. Multi-hop paths work, but the app only uses `[WETH, USDC]` and `[USDC, WETH]`.
- `addLiquidity`, `addLiquidityETH` (refunds unused ETH), `removeLiquidity`, `removeLiquidityETH`, each with minimums and a deadline. Creating a pool's first liquidity also creates the pool.
- Accepts ETH only from WETH (when unwrapping). Sends ETH with a checked low-level call.

**BeryvaFactory.** `createPair(tokenA, tokenB)` sorts the tokens, refuses identical or zero addresses and duplicates, and deploys the pool with CREATE2.

**Left out on purpose:** flash swaps (no callback into an outside contract during a swap), the protocol-fee switch (all fees go to liquidity providers), TWAP price accumulators (a thin test pool is no price oracle; I-9 uses Chainlink), "exact output" swaps, permit-based removals, and tokens that charge a fee on transfer or rebase.

## Security assumptions

1. **Tokens are standard ERC-20s.** WETH9 and Circle's USDC behave. Fee-on-transfer, rebasing or callback tokens (ERC-777) are not supported; a pool with one could be drained or stuck. The app only ever uses WETH/USDC, and the factory doesn't stop anyone creating other pools on the test network.
2. **The price is the pool's own price.** A thin pool can be moved by anyone with money. People are protected by `amountOutMin` (slippage) and the deadline. The app also refuses swaps with more than 30% price impact and warns from 1% and 5%. The app fixes the minimum at the review; if the price moves past it, the network refuses to estimate the swap and the person gets a new quote, rather than a transaction that would revert and still cost a fee.
3. **Front-running and sandwiching** are possible on any public AMM. Slippage limits how much they cost. On Sepolia there's no real value to take.
4. **No admin keys.** Nothing can be paused, upgraded or changed after deployment. A bug means deploying new contracts and moving liquidity.
5. **The app's approvals are for exact amounts.** Beryva never asks for an unlimited USDC allowance. The approval is its own transaction, confirmed with the Beryva lock (or in the linked wallet).
6. **The key worker checks every swap before signing:** the right router, path and value, the reviewed amount and minimum, a deadline within the next day, and that the output goes **to the wallet itself**. A compromised server can't redirect a swap's payout (`lib/beryva/keys/sign.ts`, `lib/beryva/tx/swap.ts`).

## Test results (2026-10-22)

`cd contracts && forge test`: **36 passed, 1 skipped** (the fork test, which only runs with `BERYVA_FORK_RPC`).

- **27 unit tests:** factory (sorting, duplicates, zero and identical addresses, the CREATE2 address); pool (first deposit locks 1,000, too-small first deposit, proportional deposits, withdrawals rounded down, minimums, swaps without input, outputs too big, invalid recipients, skim and sync, the exact quote passes and one unit more reverts); router (the quote is paid exactly, slippage and deadline revert, a price that moved before it ran, allowance needed, wrong paths, zero recipients, unknown pairs, ETH only from WETH, a recipient that refuses ETH, a reentrancy attempt blocked, multi-hop, the fee staying in the pool); the shared quote vectors.
- **8 fuzz tests, 1,000 runs each:** the swap pays exactly the quote; k never decreases on a swap; a round trip never profits; one unit more than the quote always reverts; `getAmountIn` is always enough; deposit then withdraw never profits; donations can't be taken by asking for more than the formula allows.
- **Invariant suite, 256 runs × 64 calls (16,384 calls):** random swaps both ways, deposits, withdrawals, attempts to take tokens for free and time passing, by four actors. It holds:
  - k never decreases on a swap
  - nobody gets tokens for free
  - an LP share's value (`sqrt(k) / supply`) never decreases, because fees raise it and rounding favours the pool
  - reserves equal balances
  - the router holds nothing
  - the 1,000 locked LP tokens stay locked
  - WETH stays fully backed
- **Sepolia fork test** (`BERYVA_FORK_RPC=https://ethereum-sepolia-rpc.publicnode.com forge test --match-path "test/fork/*"`): deploys, seeds and swaps both ways through the real Sepolia WETH9 and Circle USDC. Passed on 2026-10-22.

The app's side: `tests/beryva/swap-amm.test.ts` (the same vectors), `tests/beryva/swap.service.test.ts` (quotes, approval, swap, lock binding, worker refusals, limits, price impact, price moves, reverts, the real payout, linked wallets, history, audit), `tests/components/beryva-swap.test.tsx` (screens).

## Static analysis

**Slither 0.11.6** on `src/` (dependencies, tests and scripts excluded). First run: 14 results, none high or medium.

| Finding | What it was | Outcome |
|---|---|---|
| unused-return: `createPair` in the router | The new pool's address was ignored. | **Fixed.** Kept in a local. |
| missing-zero-check: `BeryvaPair` constructor | Token addresses not checked. The factory already checks them. | **Fixed** anyway (defence in depth). |
| reentrancy-events (forge lint) | `Swap` and `Burn` were emitted after the transfers out. | **Fixed.** Emitted before the transfers. |
| incorrect-equality (4): `amount == 0`, `supply == 0`, `liquidity == 0`, `amountIn == 0` | Strict equality on computed amounts, not on balances an attacker can steer. They are the intended checks. | Accepted. |
| unused-return (2): `sortTokens` second value | Only `token0` is needed. | Accepted. |
| calls-loop (3): `_swap` calls each pool in the path | That's how multi-hop works. The path is the caller's own calldata and gas. | Accepted. |
| low-level-calls: `_sendEth` | Needed to send ETH. The result is checked and reverts on failure. | Accepted. |
| naming-convention: `WETH` | Kept for Uniswap V2 compatibility. | Accepted. |

After the fixes: 11 results, all in the accepted rows above.

**forge lint** on `src/`: style notes (named constants, mixedCase function names such as `addLiquidityETH`, immutables' casing), the `uint112` casts (each is guarded by the overflow check just before it), `block.timestamp` for deadlines (intended), and the same items as Slither. Nothing that changes behaviour.

## Deployment (Sepolia)

Status: **deployed on 2026-10-22** from the throwaway deployer wallet `0x84857708F2a373afd841c7507db739030bE80b13` (its key is in `.env.local` as `BERYVA_DEPLOYER_PRIVATE_KEY`, never committed). The addresses are in `lib/beryva/swap-config.ts` (`DEPLOYED`).

| Contract | Address | Verified |
|---|---|---|
| BeryvaFactory | [`0xDf38C32C3d72a11782d240d9BC362C8091D20ED7`](https://sepolia.etherscan.io/address/0xDf38C32C3d72a11782d240d9BC362C8091D20ED7) | [Etherscan](https://sepolia.etherscan.io/address/0xDf38C32C3d72a11782d240d9BC362C8091D20ED7#code) and [Sourcify, exact match](https://repo.sourcify.dev/11155111/0xDf38C32C3d72a11782d240d9BC362C8091D20ED7) |
| BeryvaRouter | [`0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2`](https://sepolia.etherscan.io/address/0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2) | [Etherscan](https://sepolia.etherscan.io/address/0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2#code) and [Sourcify, exact match](https://repo.sourcify.dev/11155111/0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2) |
| ETH/USDC pool (BeryvaPair) | [`0x4B05eaBF44b802122952F476CfF98D2c056B8F1C`](https://sepolia.etherscan.io/address/0x4B05eaBF44b802122952F476CfF98D2c056B8F1C) | [Etherscan](https://sepolia.etherscan.io/address/0x4B05eaBF44b802122952F476CfF98D2c056B8F1C#code) and [Sourcify, exact match](https://repo.sourcify.dev/11155111/0x4B05eaBF44b802122952F476CfF98D2c056B8F1C) |
| WETH9 (existing) | `0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9` | yes (Uniswap's Sepolia WETH) |
| Test USDC (Circle, existing) | `0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238` | yes |

Deploy block: 11970636. Verified on Sepolia Etherscan on 2026-10-22 (solc 0.8.30, optimizer 200 runs).

**Seed.** 15 test USDC and 0.00625 Sepolia ETH, an opening price of $2,400 per ETH (CoinGecko showed $2,397.65). The deployer holds the LP tokens (306,186,216,847 `BRV-LP`); 1,000 are locked at `0x…dEaD`.

**Transactions.**

| Step | Hash | Gas used |
|---|---|---|
| Factory | [`0x54478a45…7019f6a`](https://sepolia.etherscan.io/tx/0x54478a450b301f34c28a0ca318f50a3d5f1f0a9fbf636ac9d16bde8497019f6a) | 13,068,914 |
| Router | [`0x25ca516b…b674eb0`](https://sepolia.etherscan.io/tx/0x25ca516b72f785291839e7545ff662d0d0a5bf4e9ec59509a366bfa41b674eb0) | 13,232,803 |
| Approve 15 USDC (exact) | [`0x44c767f5…649d4fd`](https://sepolia.etherscan.io/tx/0x44c767f57d109025aba490cf25bdb2b4a17bfc1f70223c6ad600d124e649d4fd) | |
| Seed (`addLiquidityETH`, creates the pool) | [`0x290ff51a…a2387b1`](https://sepolia.etherscan.io/tx/0x290ff51a8c4fc452d1cda9766ac727446443593a7c43b6ee94dfe38b6a2387b1) | 11,713,215 |

**Live swaps** (the steps of `LiveSwap.s.sol`, 0.5% slippage from the router's own quote):

| Step | Hash | Result |
|---|---|---|
| 0.0001 ETH to USDC | [`0x2d83090a…250c859dc`](https://sepolia.etherscan.io/tx/0x2d83090ae7ae70ea5188dd48f53ba36ae67faee559528eb0f8606a5250c859dc) | 0.235522 USDC, exactly the quote (minimum 0.234344). Gas 140,887. |
| Approve 0.235522 USDC (exact) | [`0x844eadeb…c156524e2`](https://sepolia.etherscan.io/tx/0x844eadeb505097a9d5fd39713d079647f2f27da59328217f52dbdb9c156524e2) | Gas 137,745. |
| 0.235522 USDC to ETH | [`0x7ce1429a…4d84887e64c1`](https://sepolia.etherscan.io/tx/0x7ce1429a629da15e83b57cc4c7cf10bef99d69b8da6ea9954a716d84887e64c1) | 0.000099409883036227 ETH, exactly the quote. The allowance went back to 0. Gas 152,184. |

The round trip left 0.00000059 ETH in the pool (the 0.3% fee both ways and the price impact), as expected.

**Sepolia's gas costs changed, and `forge script` no longer sets enough gas for deployments.** Creating a contract on Sepolia now costs about 7 times what forge 1.8.1's local simulation computes (the factory: 13.07M on the network against 1.85M in forge). `forge script --broadcast` sets each gas limit from that simulation, so the first try's factory transaction ([`0x4b8e4970…fa341a22`](https://sepolia.etherscan.io/tx/0x4b8e4970111a14b0307938e42c3800a1f55dc1e2e84b7c6280497576fa341a22)) ran out of gas and reverted (about 0.0001 ETH lost, nothing deployed). The deployment then sent `Deploy.s.sol`'s four steps one by one with `cast send`, which takes the network's own estimate. Writing a new storage slot costs more too: a first USDC approval to the router estimates 140,005 gas and a swap paying a wallet that has never held USDC 275,385. So `APPROVE_GAS_ESTIMATE` (the gas limit used when the network can't estimate an approval) went from 60,000 to 160,000, and `SWAP_GAS_ESTIMATE` (the review's figure) from 160,000 to 280,000. Every contract still fits under the 16.7M per-transaction cap. `LiveSwap.s.sol` may hit the same limit problem, so the live swaps were sent with `cast send` as well.

To deploy again (for example new contracts after a fix), from `contracts/`, with `ETH_RPC_URL` set to a Sepolia RPC and the key loaded from `.env.local` without printing it:

```sh
cast send --private-key "$BERYVA_DEPLOYER_PRIVATE_KEY" --create $(forge inspect src/BeryvaFactory.sol:BeryvaFactory bytecode)
cast send --private-key "$BERYVA_DEPLOYER_PRIVATE_KEY" --create $(forge inspect src/BeryvaRouter.sol:BeryvaRouter bytecode)$(cast abi-encode "constructor(address,address)" <factory> 0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9 | cut -c3-)
cast send --private-key "$BERYVA_DEPLOYER_PRIVATE_KEY" 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238 'approve(address,uint256)' <router> <SEED_USDC>
cast send --private-key "$BERYVA_DEPLOYER_PRIVATE_KEY" <router> 'addLiquidityETH(address,uint256,uint256,uint256,address,uint256)' 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238 <SEED_USDC> <SEED_USDC> <SEED_ETH_WEI> <deployer> <deadline> --value <SEED_ETH_WEI>
```

`forge script script/Deploy.s.sol --rpc-url sepolia` without `--broadcast` still checks the steps first. Verify on Sourcify, which needs no key (the pool's constructor takes its sorted tokens, USDC then WETH):

`forge verify-contract <address> src/BeryvaRouter.sol:BeryvaRouter --chain sepolia --verifier sourcify --constructor-args $(cast abi-encode "constructor(address,address)" <factory> 0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9)`

Sepolia Etherscan needs `ETHERSCAN_API_KEY` (in `.env.local`): the same command with `--verifier etherscan --etherscan-api-key "$ETHERSCAN_API_KEY"`. The pool's constructor arguments are USDC then WETH.

**Live checks with the real keys (2026-10-22).** CoinGecko (Demo key): one `/simple/price` call returned all four coins in USD and NGN with their 24-hour change, and the ETH 24H chart returned 286 points. Alchemy indexer (`alchemy_getAssetTransfers`): the deployer's transfers from block 11970000 came back as 4 in and 4 out, matching the funding, the seed and both live swaps, with exact amounts and the swap's ETH payout found as an internal transfer.

## What the professional audit must check before any mainnet use

This is a **pause point**. Before any real network:

1. A full audit of all four contracts by an independent firm, including the changes from Uniswap V2: the order of checks, effects and the transfer out in `swap` and `burn`; OpenZeppelin's `ReentrancyGuard` instead of V2's `lock`; the `DEAD` address for the locked minimum; the factory looked up instead of a computed pair address; no flash swaps.
2. Rounding everywhere (mint, burn, quotes), especially with 6- and 18-decimal tokens and very small or very large reserves. Overflow at the `uint112` limits.
3. Token assumptions: what happens with fee-on-transfer, rebasing, ERC-777 or blacklisting tokens (USDC can blacklist an address, which can freeze a pool's USDC).
4. Price manipulation and MEV: whether the app's slippage and impact defaults are safe for real value, and whether any other contract (I-8 escrow, I-9 futures) must never read this pool's price.
5. The lack of admin controls: whether real money needs a pause, and who would hold it.
6. The app side: the key worker's swap checks (`lib/beryva/keys/sign.ts`), exact-amount approvals, deadlines, the relay that only accepts the approved transaction, and linked wallets sending exactly the reviewed call.
7. Formal or symbolic checks of the k invariant (for example Halmos or Certora), on top of the fuzz and invariant tests here.
8. Deployment: the deployer becomes irrelevant after deployment (no admin), but mainnet deployment needs a reviewed script, verified sources, and published addresses.

---

# Solana: Beryl Names (phase W-1)

Beryl Names gives people usernames like `vanessa.beryl` as accounts on **Solana devnet only**. It is a Rust program written with Anchor. Mainnet is a pause point, like Beryva Swap.

## Where things are

| Path | What |
|---|---|
| `solana/programs/beryl-names/src/lib.rs` | The program: `initialize`, `set_authority`, `register`, `transfer_name`, `update_records`, `set_primary`, `clear_primary`, `release`, and their account checks and events. |
| `solana/programs/beryl-names/src/name_rules.rs` | The name rules and the reserved list. `lib/beryva/names/rules.ts` and `config.ts` are the same rules in the app; a test compares the two lists. |
| `solana/programs/beryl-names/src/state.rs` | The accounts: `Config` (`["config"]`), `NameRecord` (`["name", name]`) and `PrimaryName` (`["primary", wallet]`). |
| `solana/programs/beryl-names/tests/` | LiteSVM tests of every instruction and failure path, and proptest fuzzing of the name rules. |
| `solana/idl/beryl_names.json` | The IDL `anchor build` writes. `tests/beryva/names.test.ts` checks the app's hand-written client (`lib/beryva/names/program.ts`) against it: discriminators, account order and roles. |
| `solana/scripts/` | `test.sh` (rustfmt, build, clippy, tests), `build.sh`, `fmt.sh`, `names-deploy.sh`, and `wsl-env.sh` (the shared setup). |
| `lib/beryva/names/config.ts` | **The program ID (one place)**, the reserved list, cache time and the faucet link. |
| `scripts/names-devnet.ts` | `npm run names:devnet -- init` (sets the reserved-names authority) and `-- demo` (the live run below). |

## Tooling (owner decisions, 2026-10-27)

- WSL Ubuntu 24.04. Rust **1.99.1** pinned in `solana/rust-toolchain.toml`, Agave (Solana CLI) **4.3.2**, Anchor **1.2.1** through avm (pinned in `solana/Anchor.toml`).
- Tests: **LiteSVM 0.10** in Rust (`cargo test`), **proptest** for the name rules, then `cargo clippy -D warnings` and `cargo fmt --check`.
- Built as **SBPF v2** (`SBPF_ARCH` in `solana/scripts/wsl-env.sh`), not Anchor 1.2.1's default v3. LiteSVM 0.10 can't load v3 programs yet, and the tests must run the same binary that is deployed. v2 is active on devnet (SIMD-0173 and SIMD-0174). Move to v3 when LiteSVM can load it.
- Verifiable build: `anchor build --verifiable` (Docker), then `solana-verify` to submit it to the public registry.

How to run the Rust checks yourself, from the repo in PowerShell:

```
wsl -d Ubuntu-24.04 -- bash solana/scripts/test.sh
```

Inside Ubuntu: `bash solana/scripts/test.sh`. Build output lives on Ubuntu's own disk (`~/.cache/beryl-solana-target`), because building on `/mnt/c` is many times slower.

## What the program does

- **register(name)**: makes the account at `["name", name]` for the signer. The name must be 3 to 32 characters of `a-z`, `0-9` and `-`, with no hyphen at either end. The program never normalises: the app sends the lowercase form. Reserved names (`admin`, `beryl`, `beryva`, `support` and the rest in `name_rules.rs`) can only be registered by the authority in `Config`. A name exists at most once, because its address comes from the name.
- **transfer_name(new_owner)**: the owner only. Records are cleared, so a name never keeps pointing at its old owner's addresses. The new owner can't be the old one or the default key.
- **update_records(records)**: the owner only. An Ethereum address (20 bytes), a Solana address, a Bitcoin signet or testnet address (26 to 90 characters: `tb1…` bech32, or base58 starting `m`, `n` or `2`; the app checks the checksum first) and an avatar link (`https://` or `ipfs://`, up to 200 characters).
- **set_primary / clear_primary**: one primary name per wallet, at `["primary", wallet]`, for reverse lookup. It must point at a name that wallet owns. A primary left behind after a transfer is ignored by the app and can be closed or replaced.
- **release**: the owner closes the name (and, if given, their primary pointing at it). The rent goes back to them, and anyone can claim the name again.
- **initialize / set_authority**: only the program's **upgrade authority** can create the config, once. That makes the reserved-names authority whoever holds the upgrade key at setup, not whoever calls first.

Every change emits an event (`NameRegistered`, `NameTransferred`, `RecordsUpdated`, `PrimarySet`, `PrimaryCleared`, `NameReleased`, `AuthoritySet`).

## Security assumptions

- Every instruction that changes a name checks its owner (`has_one = owner`) and its signer.
- Every account is checked against its seeds and its stored bump. There are no unchecked accounts.
- Space is fixed by the maximum lengths, so no maths on user input can overflow; timestamps come from the `Clock` sysvar.
- **Upgrade authority**: the throwaway devnet deployer (`BERYL_NAMES_DEPLOYER_KEYPAIR` in `.env.local`, never committed; owner decision). Whoever holds it can replace the program, so it can change any name. That is acceptable on devnet only. Before any mainnet use the upgrade authority moves to a multisig or the program becomes immutable, as part of the independent review.
- Beryl's server never holds a key that can change a name. People sign every change in their own wallet: the Beryva wallet (with the Beryva lock, and the key worker checks the exact action first) or a Solana wallet such as Phantom.

## Test results (2026-10-27)

`solana/scripts/test.sh`: rustfmt clean, build clean, clippy with `-D warnings` clean, **24 tests passed**: 4 unit tests of the name rules, 7 proptest properties (any string is accepted exactly when the model says so, near-misses of the alphabet, mixed case, canonical form, reserved names, PDA seeds), and 13 LiteSVM tests (every instruction; taken name, bad characters, reserved name, wrong owner, wrong PDA, initialize by someone other than the upgrade authority, a stale primary after a transfer, release with a primary that points elsewhere).

## Deployment (Solana devnet)

Program ID: [`92jgXoEDydnpZhUFbcqoFf7aPasLGPP1S2E7CcpLYuPR`](https://explorer.solana.com/address/92jgXoEDydnpZhUFbcqoFf7aPasLGPP1S2E7CcpLYuPR?cluster=devnet). Deployer and upgrade authority: `21FxQJZ4xA4jXf2gJ4WPzY3RgT3bbzW3t9qkkZstRrVe`.

Status: **deployed on 2026-10-27** (slot 515781846, 290,832 bytes, SBPF v2, program hash `fde98d1b0702376cfe6a9252d8e3866f2a684447c5dc0098a4a362925abd0250`). ProgramData: `9N3RAXGcdbpYAVosYE1ejC3o8kbHdzM76Y7Tj11XhyfA`. Upgrade authority and reserved-names authority: the deployer.

| Step | Transaction |
|---|---|
| Deploy | [`5PwBVMtvzfge4hmJ…`](https://explorer.solana.com/tx/5PwBVMtvzfge4hmJhrobuXoiyQeoTKdBo2nZYJnbKQsmhNDa1Mo7q7fUoYdJebvJMbqXGLZpzAqoNQNr4VVa7win?cluster=devnet) |
| `initialize` (reserved-names authority) | [`4PrKs8ARnFHKQ5ob…`](https://explorer.solana.com/tx/4PrKs8ARnFHKQ5obJVZvY8bNY4sYNBwKouDpvDfKRrg9uXgWEyr4Bnvuarjo5wPaCrFm9Uf1Xsu41RpoYKUo3yj?cluster=devnet) |

**Phase S-1: the brand's own names (2026-10-30).** BitKorra shows names as `name.korra` and Beryl as `name.beryl`: one registry, the ending is only in the apps. So that nobody else takes the brand's names, the authority (the deployer) registered them for itself with `npm run names:devnet -- claim bitkorra korra`: `bitkorra` [`4F1G7pRQsws15UJv…`](https://explorer.solana.com/tx/4F1G7pRQsws15UJvH4dunmv9BmszBYmsvzYTUTaKbajXqJkTzHTL9zFzEcJCH7Xb1oPTuvVF7oWN9D1AxA2SmAky?cluster=devnet), `korra` [`4JbwxVE3KgEJvKXV…`](https://explorer.solana.com/tx/4JbwxVE3KgEJvKXVr1iH2naXgEyQFbMYrBWY5xDtT9HGGjmC4GrKrX3ahyyjH3qYcXd22rKnMYF4hMUwkGaS76Nv?cluster=devnet). No program change: the reserved list is the same.

**Live run (`npm run names:devnet -- demo`, 2026-10-27).** Name `w1-demo-mvr0last.beryl`, held by the throwaway wallet `tpQVNrboaFrAXJDKQrYHQcERWq4mdo8m8sHnVjgddZK`, then transferred to `9U1rd2GVVQ2pPRUfEBh4wiXB7MTBuCGkgjoqRjX7JCph`.

| Step | Transaction | Result |
|---|---|---|
| Fund the two test wallets | [`3BwBuNjehRapuCXK…`](https://explorer.solana.com/tx/3BwBuNjehRapuCXKv2GZprhmpMCUdLpC8BEXrGRGorCqwCcZbFsLVr9eBW68UJvRf8J21xLfDkWb3bNivMFitKz?cluster=devnet) | |
| `register` | [`5YcX4kj3pzTgkPLv…`](https://explorer.solana.com/tx/5YcX4kj3pzTgkPLvjrqiiwKvygVpYVV1xJ4RKAGJjSZFP2fxifhGRmWhdydsSfFDykkP54GxGYYeFQfTxrgjz9w6?cluster=devnet) | |
| `update_records` | [`2apCXwLCHmns9hXs…`](https://explorer.solana.com/tx/2apCXwLCHmns9hXswc3f27zoZpeSp7a6ZxwW3a7EkkuEkXoskCnRPYEn1GH7GtHKNsFwxQYoZ7xda4vitWKW8zM?cluster=devnet) | Ethereum, Bitcoin and avatar records |
| `set_primary` | [`4oJj8iFYdsVKMRoT…`](https://explorer.solana.com/tx/4oJj8iFYdsVKMRoTTWnzxg4aCDn7tJzQXMHiDGPuMrQcKrNoxAYFXKYbKiz1JtYLWcPEX9TxqsKVcbRnLVjrUrAC?cluster=devnet) | |
| Resolved by the app's server code | (read) | ETH `0xf39F…2266`, SOL the holder, BTC `tb1qquv9…7t8` |
| `clear_primary` | [`3BMXYsBCnBRmrJun…`](https://explorer.solana.com/tx/3BMXYsBCnBRmrJunxSaetapSxEP3iSs9K6yySmw7xPdUjf35pix4UE1N4xpNiAZWPJrya2shwAyg1oTTFJupA5VE?cluster=devnet) | |
| `transfer_name` | [`2qMjNqycErEGHYdZ…`](https://explorer.solana.com/tx/2qMjNqycErEGHYdZnEQNFYjzS7iaPuM9nwKtTzjJtBbgawHiVFcb8imC3KHbj7a3BDqnp6MrebAwzxSvHPA4iXcr?cluster=devnet) | new owner read back |
| `release` | [`4jrwHrP2GnB9D6YJ…`](https://explorer.solana.com/tx/4jrwHrP2GnB9D6YJtnzksmisNoUZopEwWVgUQbmaf8WtCD9znXYqkwmH3F9pXiT4UDXVwzCZZedpF2bJvGZ4T97F?cluster=devnet) | the name is free again |

**Verifiable build: not done yet** (all three programs). Phase S-1 replaced the plan: `solana/scripts/verify.sh` builds each program with `solana-verify` in Docker from a clean clone of the public `bitkorra-contracts` repo at one commit, upgrades devnet to that exact build (same program ID), checks the hashes match and submits it to the registry. It needs Docker Desktop's WSL integration for Ubuntu-24.04 and the public repo; the steps are in `docs/DEPLOY.md` section 10.

---

# Creator tips and paid posts (phase W-2)

Creators earn crypto straight into their own wallets: tips, and paid posts that a buyer unlocks. The same design is written twice, as a Rust/Anchor program on **Solana devnet** and as Solidity contracts on **Ethereum Sepolia**. Test networks only. Mainnet is a pause point (`docs/INTEGRATION_PLAN.md` section 13).

## The shared design

- **Non-custodial.** The money goes from the payer's wallet to the creator's wallet inside the one transaction. Neither the program nor the contracts ever hold it.
- **What goes on chain about a post or a profile** is a 32-byte hash of Beryl's own id (`lib/beryva/creator/reference.ts`): a tip's `reference`, and a paid post's `post_id`. Never content, names or anything personal. Notes stay in Beryl's database.
- **Paid posts.** A creator registers a post with a price in the chain's coin or USDC. A buyer pays the creator and gets an access record: a PDA `["access", post, buyer]` on Solana, `hasAccess[key][buyer]` on Sepolia. The creator can change the price or close and reopen sales, and access records are never touched, so buyers keep access. A buyer passes the price they saw (`max_price` on Solana, `maxPrice` for USDC on Sepolia; ETH must be exactly the price), so a price raised in between can't charge more.
- **Protocol fee.** Basis points of each payment to a treasury. **0 by default**, **hard-capped at 5%** (500 bps) inside the program and the contracts, so even the owner can't set more. Settable only by the program's authority or the contracts' owner. Authority and ownership change hands in two steps (propose, then accept; OpenZeppelin's Ownable2Step on Sepolia), so a typo can't hand them to a dead address. The fee is rounded down, so the creator gets any fraction. Why have one at all: a later public launch could fund itself, and the 5% cap is a promise to creators enforced by code, not by policy.
- **No self-tips or self-purchases.** Refused outright. They'd only move money from a wallet to itself (less any fee) and inflate "top supporters" and sales counts. No counters are kept on chain: totals are built from events, so there's no shared state to inflate or contend on.
- **One token per chain**: Circle's devnet USDC on Solana (mint `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`, classic SPL Token, 6 decimals, handed out by Circle's own faucet), Circle's test USDC on Sepolia. Classic SPL Token only on Solana, so no transfer fees or hooks can change what arrives.
- **Events for everything**: tips, registrations, price changes, sales opened or closed, purchases, fee, treasury and authority changes.

## Solana: Beryl Tips

| Path | What |
|---|---|
| `solana/programs/beryl-tips/src/lib.rs` | The program: `initialize`, `set_fee`, `set_treasury`, `propose_authority`, `accept_authority`, `tip_sol`, `tip_token`, `register_post`, `update_price`, `set_sales_open`, `buy_sol`, `buy_token`; account checks and events. |
| `.../src/fees.rs` | The fee split and the cap (`MAX_FEE_BPS = 500`). `lib/beryva/creator/fees.ts` is the same maths, tested with the same vectors. |
| `.../src/state.rs` | `Config` (`["config"]`), `Post` (`["post", creator, post_id]`), `Access` (`["access", post, buyer]`). |
| `.../tests/` | LiteSVM tests and proptest. |
| `solana/idl/beryl_tips.json` | The IDL. `tests/beryva/creator.test.ts` checks the app's hand-written client (`lib/beryva/creator/solana.ts`) against it. |
| `solana/scripts/tips-deploy.sh` | Builds (SBPF v2, as W-1) and deploys with the W-1 deployer, which stays the upgrade authority. |
| `scripts/tips-devnet.ts` | `npm run tips:devnet -- init` (the config), `-- solana` and `-- sepolia` (the live runs). |

New Rust crates (owner decision, 2026-10-29): `anchor-spl` 1.2.1 (Apache-2.0; its `token_2022` module is compiled only because its IDL builder needs it) and `litesvm-token` 0.10 (Apache-2.0, tests only).

**Security.** Every account is a typed Anchor account with its owner checked; signers are `Signer`; every PDA is checked against its seeds and stored bump; `initialize` only by the program's upgrade authority, once; the creator and treasury must be wallets (system-owned), never program accounts; token accounts are checked for the configured mint and their owner (`token::mint`, `token::authority`), the mint against the config, and the treasury's token account (required only while the fee is above 0) for its owner and mint; checked maths (`overflow-checks` is on in release too); effects (access record, sale count) before the transfers; `transfer_checked` with the mint's decimals.

**Tests** (`wsl -d Ubuntu-24.04 -- bash solana/scripts/test.sh`, 2026-10-29): rustfmt, build, clippy `-D warnings` clean; **27 Beryl Tips tests passed**:

- 3 fee unit tests.
- 5 config tests: only the upgrade authority initializes, once; not a mint; zero keys; the fee cap (501 and u16 max); only the authority sets fee and treasury; two-step handover, wrong acceptor, cancel.
- 7 tip tests: SOL straight to the creator with the program holding nothing; the exact fee split; zero, self, wrong treasury, a program account as creator, too much, a missing signature; token tips with and without a fee, a missing or impostor treasury account, someone else's destination or source account, another mint, self, balance.
- 8 paid-post tests: register once per creator and id, others can't squat; only the creator changes price or sales; buy in SOL with fee and access record; no double buy; closed sales refuse new buyers and old buyers keep access; reopen; a raised price is refused and a lower one charges the actual price; self, wrong creator, wrong treasury, someone else's access address, wrong currency, a fake post account; token purchases with fee, missing treasury account, max price, wrong currency, other people's accounts, another mint, a short balance.
- 4 proptest properties: the split always adds up and never passes the cap (5,000 cases); any fee above the cap is refused (5,000 cases); random SOL tips pay exactly amount minus fee (48 full LiteSVM runs); random token purchases pay exactly price minus fee and give access only after full payment (48 runs).

Beryl Names' 24 tests still pass.

### Deployment (Solana devnet)

Program ID: [`B9oecLNJC7SCTMbZc6G7UJSwj9mUpL9AwF6r3CFE5CEf`](https://explorer.solana.com/address/B9oecLNJC7SCTMbZc6G7UJSwj9mUpL9AwF6r3CFE5CEf?cluster=devnet). Deployer and upgrade authority: `21FxQJZ4xA4jXf2gJ4WPzY3RgT3bbzW3t9qkkZstRrVe` (the W-1 deployer). Config after `init`: authority and treasury the deployer, fee 0, accepted token Circle's devnet USDC.

Status: **deployed on 2026-10-30** (slot 516605369, 351,616 bytes, SBPF v2). Program hash `b77b11478d881c2981ffe089fc7ea47402a5d4df98a9bcc816e62ee418d97a4c`: the binary dumped back from devnet matches the build the Rust tests ran against, byte for byte. ProgramData: `55Hf6voTevUJBQorWNip1fZ1C3VaRoeA31KTHsBjkQze`. The upload buffer was folded into the program (0 lamports left), so nothing is stranded.

| Step | Transaction |
|---|---|
| Deploy | [`2fB1joHsZkRZcQ1P…`](https://explorer.solana.com/tx/2fB1joHsZkRZcQ1PJVfzqavXzAWkJe3hLckcTpNGUiAEJrytVQc5sqAVZa4sSEWfb9L5kDB9h2zj2Yh3VZmBUqyz?cluster=devnet) |
| `initialize` (config, plus the treasury's USDC account) | [`ps9eihaoCEFN2Gfu…`](https://explorer.solana.com/tx/ps9eihaoCEFN2GfuiZ3za69gxXDHffzyM5pxsqM9XR2e3PoiVTMWWGqe5qo2JCUCgiX7pNjd4PzTyVC4LUQ7kim?cluster=devnet) |

**Found by the first `init`: a wrong program ID in the app.** `ASSOCIATED_TOKEN_PROGRAM_ID` in `lib/beryva/creator/config.ts` was mistyped, so every USDC account the app derived on Solana was wrong. The tests derived from the same constant and couldn't see it. Fixed (`ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL`), and `tests/beryva/creator.test.ts` now pins a real devnet account as the vector (the deployer's USDC account `HeGxh7PCSs7KqzYeHS4yATvKRybSpimNRMXZhp6h3CJ5`). The program itself was never affected (it checks token accounts by mint and owner).

**Live run (`npm run tips:devnet -- solana`, 2026-10-30).** Fan (tipper and buyer): the deployer. Creator: a fresh throwaway wallet `GZraKjoRqXffrzyhTTnjHooLW5ZQsDCHxpfVDVG1AVdP`. Built with the app's own transaction builders (the same bytes the key worker checks).

| Step | Transaction |
|---|---|
| Fund the creator (0.02 SOL) | [`5WnjupxbyT74DgNV…`](https://explorer.solana.com/tx/5WnjupxbyT74DgNVoqdvcEBPBnz4bG6GD8SaRApotZ2zTMRYmH6qD6LjMsAxcufJMUdPu6VW3GDDJigXQ3qrRvtv?cluster=devnet) |
| Tip 0.01 SOL | [`4d9vrSxGjWxw4XSd…`](https://explorer.solana.com/tx/4d9vrSxGjWxw4XSdecNZBbR3HwHRxkK7oVPbDRw49k1maVTk9b8cpkwKPKUGm15KPxfvaHCwBv5UCjYBYCWzZV65?cluster=devnet) |
| Tip 0.5 devnet USDC (creates the creator's USDC account) | [`37V1v2rAYa94zcEo…`](https://explorer.solana.com/tx/37V1v2rAYa94zcEoGZDn67Gp6f8ZB12qU2CaSheU1MjcjMZC4BESDp4Ex3v8aBVr4HMYaQJqfcZ6MQRBv3yHJumU?cluster=devnet) |
| Register a post at 0.02 SOL | [`21VX4U24X5VrMbtf…`](https://explorer.solana.com/tx/21VX4U24X5VrMbtfPwoLdUgP1TXHbR3KmNJiCyf3YQ3hWTP3PJzmh6y5aoHYYCXPZbcfyMcEjCdLkFg4mKzwTQ6V?cluster=devnet) |
| Register a post at 0.25 devnet USDC | [`3ajn7w8HmiFRvHPZ…`](https://explorer.solana.com/tx/3ajn7w8HmiFRvHPZwW4hjkM4KF8TrbZowJPJzPZ2b7dgrZy1aMTpcFGTfAowXYgFqKCQwgqD4X4xV1tc6MjbGKZ4?cluster=devnet) |
| Buy the SOL post | [`2cBUWpR7gmpMFqka…`](https://explorer.solana.com/tx/2cBUWpR7gmpMFqkadypRhnU1edsGUrEa458LtaM1MspbH3u1gKv51Atzja17nNmxTQQ7Wh2sK3NiY52AxZG46a3N?cluster=devnet) |
| Buy the USDC post | [`31WFpV4spzd4b394…`](https://explorer.solana.com/tx/31WFpV4spzd4b3946VrqtkerHLrqjCcETdknmyLdunTVPejKh9QFKNooZMkj1NtuUTucYQrifi4YtuGFdULom6Nm?cluster=devnet) |

Both access records exist on chain (`884ANsqCfLHcz6bXrw3ZzvETg7kdQ2q9L1cTq3eEaPqT` and `75k6qecj6dvWms8ijwHVHwoCsSDhxJQu3VN29e5GJhGt`), and the app's event index read back all six events.

An earlier attempt of the same run was cut off by the public devnet RPC's rate limit (HTTP 429) while it followed transaction status; its transactions still landed (creator `9Z69XLURQkDSo3ywbA1yAaWMLtao1khRQTsQ7EZjtYey`: tips [`3K6fL5rD…`](https://explorer.solana.com/tx/3K6fL5rDkepRCJPYUPqNACJmNZDMRP9NGsBP4Ff2cfmQFGvsg3rqkUgjTQmQJa5WYFKinjKyPgCMf8Ai8ThCmcGX?cluster=devnet) and [`5KzYmmFN…`](https://explorer.solana.com/tx/5KzYmmFNpuwowPktZvbADErWoxBomQRLFjgZkePTxVidxeyKVEpdKbTD1to5DwEzP5AyZV4tofcrEZhUQReA1RLj?cluster=devnet), two registrations, and a SOL purchase `4TD2VniF…`). The script now backs off on 429s.

**Found by the live index: two indexer fixes.**

- **Solana:** the index read the newest 100 signatures down to its cursor and then jumped to the newest, so a burst of more than 100 between runs would have been skipped for good. It now pages back to the cursor, reads the oldest first (40 per run, because public devnet rate-limits) and moves the cursor only past what it stored.
- **Sepolia:** Alchemy's free plan answers `eth_getLogs` only for 10-block ranges (2,000 and 500 are refused), so the index advanced about 100 blocks a run and fell behind the chain. It now reads 10-block ranges several at a time, up to 300 reads (3,000 blocks) per run, and moves the cursor only past what it read. At Sepolia's pace (about 7,200 blocks a day) that's roughly 1.6M of the free plan's 30M compute units a month. The six events of the Sepolia live run (2026-10-29) are now indexed.

Both have tests in `tests/beryva/creator.test.ts`.

## Sepolia: BerylTips and BerylPaidPosts

| Path | What |
|---|---|
| `contracts/src/BerylFees.sol` | The shared fee: `feeBps` (0 by default), `MAX_FEE_BPS = 500`, `treasury`, `quote()`, Ownable2Step, a checked ETH send. |
| `contracts/src/BerylTips.sol` | `tipEth(creator, ref)` and `tipToken(creator, amount, ref)`, paid straight to the creator, `TipSent` event. |
| `contracts/src/BerylPaidPosts.sol` | `registerPost(postId, price, currency)` (key `keccak256(abi.encode(creator, postId))`, so nobody can register someone else's post), `updatePrice`, `setSalesOpen`, `buyWithEth(key)` (exactly the price), `buyWithToken(key, maxPrice)`, `hasAccess`. |
| `contracts/test/BerylTips.t.sol`, `BerylPaidPosts.t.sol`, `invariant/BerylCreator.invariant.t.sol` | Unit, fuzz and invariant tests. |
| `contracts/script/DeployCreator.s.sol` | The deployment, Sepolia only (checked without `--broadcast`; deployed with `cast send --create`, see I-6). |

**Security.** `nonReentrant` on every payment; checks, then effects (access, sale count, the event), then the transfers; `SafeERC20`; no `receive` or `fallback`, so plain ETH sent to either contract reverts; a creator that refuses ETH makes the payment revert, so nothing is ever stuck; zero addresses refused.

**Tests** (`cd contracts && forge test`, 2026-10-29): **68 passed** (37 Beryva Swap, as before, and 31 new):

- BerylTips, 16: deployment; ETH tip straight through; fee split and rounding; zero, self, zero creator; a creator refusing ETH; a re-entering creator blocked; plain ETH refused; token tips with and without a fee; no allowance; owner-only fee with the cap; two-step ownership; 4 fuzz tests at 1,000 runs (ETH and token tips pay amount minus fee, `quote` adds up within the cap, any fee above the cap refused).
- BerylPaidPosts, 15: register once, no squatting, zero price; only the creator changes price or sales; buy with ETH and fee, no double buy; ETH must be exactly the price, also after a price change; closed sales and kept access; self, unknown post, wrong currency; a refusing creator; a re-entering creator blocked; plain ETH refused; token buy with fee and max price; a lower price charges the actual price; currency and balance; 3 fuzz tests (an ETH purchase pays price minus fee, access only after full token payment, a wrong ETH amount never buys).
- **Invariant suite**, 256 runs × 64 calls (16,384 calls, no reverts) of random ETH and token tips, full and short purchases, price changes, sales closed and reopened and fee changes by four people. It holds: neither contract ever holds ETH or USDC; the treasury holds exactly the fees; every person's balance is exactly start minus what they paid plus what they received (so every creator got exactly price or tip minus the fee); access exists exactly for those who paid in full.

**Static analysis.** Slither 0.11.6 on the three new contracts: **1 result**, `low-level-calls` in `BerylFees._sendEth` (the checked ETH send; accepted, as for Beryva Swap). forge lint, all accepted: `missing-events-access-control` on `hasAccess` (`PostPurchased` is emitted in the same call; the linter doesn't follow it into the caller), `arbitrary-send-eth` (paying the creator is the point, and only the caller's own `msg.value` moves), immutables not in SCREAMING_SNAKE_CASE, and an inline-assembly keccak suggestion.

### Deployment (Sepolia)

Deployed 2026-10-29 from `0x84857708F2a373afd841c7507db739030bE80b13` (owner and treasury; fee 0). Addresses in `lib/beryva/creator/config.ts`.

| Contract | Address | Verified |
|---|---|---|
| BerylTips | [`0x60C687622a8dDABe6aB4F9A935211cAC1D26113a`](https://sepolia.etherscan.io/address/0x60C687622a8dDABe6aB4F9A935211cAC1D26113a) | [Etherscan](https://sepolia.etherscan.io/address/0x60C687622a8dDABe6aB4F9A935211cAC1D26113a#code) and [Sourcify, exact match](https://repo.sourcify.dev/11155111/0x60C687622a8dDABe6aB4F9A935211cAC1D26113a) |
| BerylPaidPosts | [`0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b`](https://sepolia.etherscan.io/address/0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b) | [Etherscan](https://sepolia.etherscan.io/address/0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b#code) and [Sourcify, exact match](https://repo.sourcify.dev/11155111/0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b) |

Deploy transactions: BerylTips [`0x3d03f98e…`](https://sepolia.etherscan.io/tx/0x3d03f98ed3d74a5bcf272a73fc573698fc08a1dd34b0195d489e326e3967bd13) (block 12021003, gas 4,791,089), BerylPaidPosts [`0x13f008e0…`](https://sepolia.etherscan.io/tx/0x13f008e00da105a73feebd149574d4222597a765f3a00074012a796cd6a7d8f4) (block 12021004, gas 7,710,838).

**Live run (`npm run tips:devnet -- sepolia`, 2026-10-29).** Fan (tipper and buyer): the deployer. Creator: a fresh throwaway wallet `0x75a811A2E326decbEcfDeF69837E36b9a85675f0`. Built with the app's own call builders.

| Step | Transaction |
|---|---|
| Fund the creator (0.003 ETH) | [`0x2a00f3e9…`](https://sepolia.etherscan.io/tx/0x2a00f3e96d787d81d5f80744524c2e501af6018e6da6a1c738e336f1b133fd4b) |
| Tip 0.0005 ETH | [`0x7f8399a9…`](https://sepolia.etherscan.io/tx/0x7f8399a97977c9f491c0f95e094c67779ae70a7473be029c7d235fe6ac4ebc01) |
| Approve exactly 0.5 USDC | [`0x9b423064…`](https://sepolia.etherscan.io/tx/0x9b4230647623bd14f08b846de0e69d9a9ff26478fe6f7944e5094f722fe8a877) |
| Tip 0.5 test USDC | [`0xeccaa67b…`](https://sepolia.etherscan.io/tx/0xeccaa67bdbb9bd73c884c630e033d0350602f6d6a109c57c60236cea1ab08c68) |
| Register a post at 0.001 ETH | [`0x369bb55e…`](https://sepolia.etherscan.io/tx/0x369bb55e2a502f58e8da5828146fe2611d1fde82009c7c7fb5cad566b5bf34cd) |
| Register a post at 0.25 USDC | [`0x1b4e1cb4…`](https://sepolia.etherscan.io/tx/0x1b4e1cb48a726b27d5a0b75f99fd29264e45952ab48a5abce6a6608f31467c91) |
| Buy the ETH post | [`0x09b96870…`](https://sepolia.etherscan.io/tx/0x09b96870121ae74719c2a1f85e58b1df5fd77c0b1c2dec83b94e338fb2e4765f) |
| Approve exactly 0.25 USDC | [`0xd172197b…`](https://sepolia.etherscan.io/tx/0xd172197be97af59c54956d05e8279706ce9a2d9e17b0b56dc0318fa2a1845c20) |
| Buy the USDC post | [`0x219eb2da…`](https://sepolia.etherscan.io/tx/0x219eb2daa85de837cf2265cb6af96e4cda38cc62b2c2f3ce694c01079daab95a) |

`hasAccess` read back `true` for both posts, and the app decoded every event (2 `TipSent`, 2 `PostRegistered`, 2 `PostPurchased`).

## What the professional audit must check before any mainnet use

1. Both implementations in full, including that the fee cap can't be bypassed and that the two-step authority handover can't be pushed into a bad state.
2. Token assumptions: USDC can blacklist an address (a blacklisted creator can't be paid, so the payment reverts and nothing is stranded); fee-on-transfer and rebasing tokens aren't supported, and only one token is accepted per chain.
3. Price changes racing purchases (the max-price argument) and front-running in general.
4. Who holds the authority and the Solana upgrade key (today the throwaway devnet deployer), and whether the program should become immutable.
5. The app side: the key worker's read-back of every creator transaction (`lib/beryva/creator`, `lib/beryva/keys/sign.ts`), and the server-gated access to paid media (`docs/beryva.md`).

# P2P escrow (phase I-8)

The P2P marketplace's escrow, written twice with the same design: a Rust/Anchor program on **Solana devnet** and a Solidity contract on **Ethereum Sepolia**. Test networks only. Mainnet is a pause point (`docs/INTEGRATION_PLAN.md` section 13). How the app uses them: `docs/beryva.md`, "Phase I-8".

## The shared design

- **The seller funds an escrow for one order**: the order id (32 bytes, a hash, nothing personal), the buyer, the coin, the amount and the deadline (at most 24 hours ahead). The money sits in the program or contract, never with Beryl: on Solana the escrow PDA's own lamports for SOL, or a vault token account PDA for USDC; on Sepolia the contract's balance.
- **The seller can release to the buyer at any time** (also during a dispute).
- **The buyer marks it paid before the deadline**, which stops the seller from reclaiming.
- **After the deadline, if never marked paid, the seller reclaims it** (refund). The buyer can also cancel before paying, which returns it to the seller.
- **Either side can open a dispute after "paid"**. Then only the **arbiter** resolves it: release to the buyer or refund to the seller.
- **Money only ever goes to that escrow's seller, its buyer, or (a release's fee) the treasury.** No other path exists.
- **Fee:** basis points, **0 by default, hard-capped at 2%** (200 bps) in the code, charged only on a release to the buyer (including the arbiter's), and **fixed when the escrow is funded**, so a later fee change can't touch open escrows. A refund, cancel or resolution for the seller returns everything.
- **Arbiter:** a configurable address, changed in two steps (the owner proposes, the new arbiter accepts). On the test networks it is a dedicated throwaway arbiter key the app's server holds (owner decision, 2026-10-30), separate from the deployer, which stays the owner. **Mainnet needs a multisig owner and arbiter.** The owner also changes hands in two steps.
- **Tokens:** SOL and Circle's devnet USDC (classic SPL Token only, as W-2) on Solana; ETH and Circle's Sepolia USDC on Sepolia.
- **Events for every change**: funded, marked paid, dispute opened, settled (with the outcome, who did it and the exact amounts to buyer, seller and treasury), fee, treasury, owner and arbiter changes.

## Solana: Beryl Escrow

| Path | What |
|---|---|
| `solana/programs/beryl-escrow/src/lib.rs` | `initialize`, `set_fee`, `set_treasury`, `propose_owner`, `accept_owner`, `propose_arbiter`, `accept_arbiter`, `fund_sol`, `fund_token`, `mark_paid`, `open_dispute`, `release`, `refund`, `cancel`, `resolve`; account checks and events. |
| `.../src/state.rs` | `Config` (`["config"]`), `Escrow` (`["escrow", seller, order_id]`), the vault (`["vault", escrow]`); `MAX_FEE_BPS = 200`, `MAX_WINDOW_SECONDS` (24 hours). |
| `.../tests/` | LiteSVM tests and proptest. |
| `solana/idl/beryl_escrow.json` | The IDL. `tests/beryva/p2p.test.ts` checks the app's hand-written client (`lib/beryva/p2p/solana.ts`) against it. |
| `solana/scripts/escrow-deploy.sh` | Builds (SBPF v2) and deploys with the W-1 deployer, which stays the upgrade authority. |
| `scripts/p2p-devnet.ts` | `npm run p2p:devnet -- init` (the config), `-- live` (the live runs, `scripts/p2p-live.test.ts`, under vitest but never part of `npm test`) and `-- refund <keys file> <funding tx>...` (the seller takes back escrows a stopped live run left behind). |

**Security.** Signers are `Signer`; every PDA is checked against its seeds and stored bump; `initialize` only by the program's upgrade authority, once; seller, buyer, owner, arbiter and treasury checked against the escrow and config (`has_one`); token accounts checked for the escrow's mint and their owner, the mint against the config (classic SPL Token only); checked maths (`overflow-checks` on in release); the state changes before any money moves; **every settlement closes the escrow and the vault and the rent goes back to the seller**.

**Tests** (`wsl -d Ubuntu-24.04 -- bash solana/scripts/test.sh`, 2026-10-30): rustfmt, build, clippy `-D warnings` clean; **21 Beryl Escrow tests passed**:

- 4 config tests: only the upgrade authority initializes, once; the fee cap (2%) and owner-only fee and treasury; the owner changes hands in two steps; the arbiter changes in two steps, proposed by the owner.
- 13 escrow tests: funding locks SOL in the escrow account; bad terms refused (zero amount, self as buyer, a past or too-far deadline); token funding checks the mint and the seller's account; release pays the buyer and closes the escrow with rent back; the fee fixed at funding goes only to the treasury; only the seller releases, and money only goes to that escrow's parties; only the buyer marks paid, only before the deadline; the seller reclaims only after the deadline and only if never marked paid; the buyer cancels before paying; a dispute opens only after paid and only by a party; only the arbiter resolves, only a disputed escrow; token escrows release, refund and resolve and close the vault; token settlements check every account.
- 4 proptest properties: the fee split adds up and never passes the cap (5,000 cases); any fee above the cap is refused (5,000 cases); every ending (release, refund, cancel, resolve either way) conserves money between seller, buyer and treasury (48 full LiteSVM runs); token escrows pay exactly and leave nothing behind (48 runs).

Beryl Names' 24 and Beryl Tips' 27 tests still pass (72 in all).

### Deployment (Solana devnet)

Program ID: [`8wDapxboCTEKfBzNQzrzZUnspmTixKFfCuNmUuQLrcDA`](https://explorer.solana.com/address/8wDapxboCTEKfBzNQzrzZUnspmTixKFfCuNmUuQLrcDA?cluster=devnet). Deployer and upgrade authority: `21FxQJZ4xA4jXf2gJ4WPzY3RgT3bbzW3t9qkkZstRrVe` (the W-1 deployer). Config (`C56Zc9rJ3UfoeDofMRBbNbjdTSgR9vqp6ouQLvZEzzQZ`) after `init`: owner and treasury the deployer, arbiter `AX7g9Ss8skUrArhx2G3iLewLTDUYjXA3bgrAJLvEczzf` (the dedicated arbiter key), accepted token Circle's devnet USDC, fee 0.

Status: **deployed on 2026-10-30** (slot 516619634, 370,720 bytes, SBPF v2). ProgramData: `BKfwgFPwirm2P5fXqSoKmT5B5KmmucoA2MHXAiizvoik`. The binary dumped back from devnet matches the build the Rust tests ran against, byte for byte (SHA-256 of the binary without its zero padding: `d0a1534d29c751e605da1e78fd7e8e78360e5f242670d10ef2e225b5100fad29`). The verifiable build is still to do, as for W-1 and W-2 (Docker in WSL).

| Step | Transaction |
|---|---|
| Deploy | [`3WsEMNGeeuezYef7…`](https://explorer.solana.com/tx/3WsEMNGeeuezYef7fzNwjyqxFHptkZHGRGUpq2rMKrNwoUYAPSgEH3ykzMxWjq9MBkFbZ7ARRa2zLwqRJLCFKdzw?cluster=devnet) |
| `initialize` (config) | [`z5r3HjnCCknnQM33…`](https://explorer.solana.com/tx/z5r3HjnCCknnQM3331iLTtGVkZrQVUnmmuKbX5GZ8XuGbBCwRpRfJRfH4umCtaKbEJt77jaCPvWsV8FZKYV5K6Z?cluster=devnet) |

## Sepolia: BerylEscrow

| Path | What |
|---|---|
| `contracts/src/BerylEscrow.sol` | `fundEth(orderId, buyer, deadline)`, `fundToken(orderId, buyer, amount, deadline)` (key `keccak256(abi.encode(seller, orderId))`, so nobody can take someone else's order id), `markPaid`, `openDispute`, `release`, `refund`, `cancel`, `resolve(key, toBuyer)`, `withdraw(asset)`; `setFee`, `setTreasury`, `proposeArbiter`, `acceptArbiter`; Ownable2Step. |
| `contracts/test/BerylEscrow.t.sol`, `invariant/BerylEscrow.invariant.t.sol` | Unit, fuzz and invariant tests. |

**Security.** `nonReentrant` on every function that moves money; checks, then effects (the state and the locked totals), then the transfers; `SafeERC20`; zero addresses refused; no `receive` or `fallback`. **A payout that can't be delivered** (a buyer contract that refuses ETH, a USDC-blacklisted address) **is credited to `owed` for that address to `withdraw` later**, so neither side can freeze an escrow by refusing payment, and the escrow still settles.

**Tests** (`cd contracts && forge test`, 2026-10-30): **16 BerylEscrow tests and the invariant suite passed** (the earlier contracts' tests still pass):

- 13 unit tests: deployment with no fee and zero addresses refused; the fee cap (2%) and owner only; the arbiter changes in two steps proposed by the owner; funding locks the money and refuses bad terms; release pays the buyer and the fee fixed at funding; mark paid only by the buyer, before the deadline; refund only after the deadline and never after paid; the buyer cancels before paying only; disputes and the arbiter; a resolution for the seller is a full refund without fee; the seller can still release during a dispute; a buyer that refuses ETH is credited and withdraws later; a blacklisted buyer never freezes the escrow.
- 3 fuzz tests at 1,000 runs: a release pays amount minus fee; a refund returns everything and only after the deadline; nobody but the parties settles.
- **Invariant suite** (random funding, marking paid, disputes, releases, refunds, cancels, resolutions, withdrawals and fee changes): the contract's balance always equals the sum of open escrows plus what is owed; funds only ever go to a seller, a buyer or the treasury; no escrow is ever released or refunded twice.

**Static analysis.** Slither 0.11.6: **10 results**, all reviewed and accepted. `reentrancy-eth` and `reentrancy-benign` in `_settle` / `_pay` (the state is final before any transfer, and every caller is `nonReentrant`; the writes after the call are the pull-payment credit for a failed payout); `low-level-calls` (the checked ETH send that falls back to `owed`); `timestamp` (deadlines are minutes to hours, far above any block-time drift); `missing-zero-check` on `proposeArbiter` (proposing zero cancels a proposal, as in Ownable2Step; nobody can accept as the zero address); `pragma` (OpenZeppelin's own version ranges).

### Deployment (Sepolia)

Deployed 2026-10-30 from `0x84857708F2a373afd841c7507db739030bE80b13` (owner and treasury; fee 0), arbiter `0x8Fb8C19A4DeCf7500f6BE7f56522d568e19bFEf5` (the dedicated arbiter key), token Circle's Sepolia USDC. Address in `lib/beryva/p2p/config.ts`.

| Contract | Address | Verified |
|---|---|---|
| BerylEscrow | [`0xe5E5beEf9820615d606753e490554ef77BfCd6F8`](https://sepolia.etherscan.io/address/0xe5E5beEf9820615d606753e490554ef77BfCd6F8) | [Etherscan](https://sepolia.etherscan.io/address/0xe5E5beEf9820615d606753e490554ef77BfCd6F8#code) and [Sourcify, exact match](https://repo.sourcify.dev/11155111/0xe5E5beEf9820615d606753e490554ef77BfCd6F8) |

Deploy transaction: [`0x39719bd6…`](https://sepolia.etherscan.io/tx/0x39719bd686b033c5e26934cbd07daf61ff541abc9cc87acf7749ddb9eaf6b19f) (block 12023611).

## Live runs (`npm run p2p:devnet -- live`, 2026-10-30)

Two throwaway accounts made fresh for the run, going through the app's own services (ads, orders, the key page's read-back, the Beryva lock bound to each transaction, the key worker's signing, the naira ledger, the A-1 dispute case and the arbiter key) on an in-memory database, with the real networks, escrows and arbiter keys. Seller (Tier 2): Solana `GqVKEuWEu44kv8wGtDVVXFbbkaR93gPpxq1xQEVZbunz`, Sepolia `0xE352efB7A538ddDE4a2c8f0Ab680DFA4fa879514`. Buyer (Tier 1, naira from the simulator): Solana `Bf2waCXDc78JfB4TRbcuJokZdir8Nuxv4hQGkS4jtJ6h`, Sepolia `0x6dC2aE4aE1A25A64F9466770836EFFe9EaC43645`. Every order moved on only from the escrow's own events.

**Solana devnet**

| Order | Step | Transaction |
|---|---|---|
| Setup | Fund seller (0.1 SOL, 2 USDC) and buyer (0.02 SOL) | [`4Kt66K8iQqvpZg8p…`](https://explorer.solana.com/tx/4Kt66K8iQqvpZg8pgRYk62x3TAkSbWVXoCVJj9c8QFHrqvfJaNXUDiH9MRbPhcKWQ2NLzFFNKbjQxuzWmF77vQuc?cluster=devnet) |
| Happy path, 1 USDC for ₦1,600 | Seller locks 1 USDC | [`5xgMnmZNqUfLz9er…`](https://explorer.solana.com/tx/5xgMnmZNqUfLz9erFmeE9Jo98EtTNg4fVbu7C7tZz24AiMC7LVRFm9RzWwXcfAGXmYkNUEpowvJgTKdaYF2izGNE?cluster=devnet) |
| | (buyer pays ₦1,600 from the naira balance through the ledger) | |
| | Buyer marks paid | [`ZvNMTh4SgdZp2UXA…`](https://explorer.solana.com/tx/ZvNMTh4SgdZp2UXAVzz9EQenyVURGkvUUVKosRSx3yatbKY9d8qVLxjvEGRxhd8p3MpNMeNW1au8JV9PPQqMErQ?cluster=devnet) |
| | Seller releases (creates the buyer's USDC account) | [`5D96TXfa3KgyXGQs…`](https://explorer.solana.com/tx/5D96TXfa3KgyXGQsKznPXHZbV8jeVjymuCXHkBnUR4ZW1RnXXzw1vamfcWNw2ukD59uSnYV3VezoqNZVPABnG49X?cluster=devnet) |
| Timeout, 0.02 SOL | Seller locks 0.02 SOL | [`qXT9YMPyRhbXfaxw…`](https://explorer.solana.com/tx/qXT9YMPyRhbXfaxwNH6vCXXWbU4DR9f76vuqkfMHX1RkczZQ32HdD2NKMgYkxD2hnZCfamQZkY8EFYnb84yHxS1?cluster=devnet) |
| | Buyer never pays; after the deadline the seller takes it back (escrow closed, rent back) | [`46PRkv5gWF5rAsMt…`](https://explorer.solana.com/tx/46PRkv5gWF5rAsMtRW49rqeWYx8hcuk7fkbsp64MFGVkzvaKoGgXpBUHFMWYMm4JFyYW243aK6WfGNdTGSLmVfqD?cluster=devnet) |
| Dispute for the buyer, 1 USDC | Seller locks 1 USDC | [`5CrkkB1z8gEJnVTL…`](https://explorer.solana.com/tx/5CrkkB1z8gEJnVTLh4JrypEj5nGetJd8xXYa5pnkRn8qxjggDQpYUMYobT1oPu8NqXNqcMtLRcds71UL6LZMZSv7?cluster=devnet) |
| | (buyer pays through the ledger) Buyer marks paid | [`275vWoFmQmX6KFwK…`](https://explorer.solana.com/tx/275vWoFmQmX6KFwKgo6NX3eRMNHm9qtzTr7PszKcwuxXwd8sYvJyoj8fyZhhS4n3S1XbgXW4SZ7zS96Gny5JLcqz?cluster=devnet) |
| | Seller stays silent; buyer opens a dispute (an A-1 case) | [`KY2GKcdLrzP3TXot…`](https://explorer.solana.com/tx/KY2GKcdLrzP3TXotdRXnCrDs9WbCMK4KqX69Sr879vLQhbpLJ9d74jeM2SC5g8DPFutAFxM9WtjZLq1TVfabadU?cluster=devnet) |
| | A compliance admin decides for the buyer; the arbiter key signs `resolve` | [`zGvcLsmTYi4HMhpU…`](https://explorer.solana.com/tx/zGvcLsmTYi4HMhpUJr1McPnGXSbrqkSQRuSVnw3DnjsjX63TyXH7LBqoywEc8kqG21EeQ4NCUCwXCD8KYr8Z7we?cluster=devnet) |

**Sepolia**

| Order | Step | Transaction |
|---|---|---|
| Setup | Fund seller 0.003 ETH | [`0x7f4a271b…`](https://sepolia.etherscan.io/tx/0x7f4a271b7efabda44a0da5c7467dede157ff42fa3468ef8f5d98dd3786cc08f9) |
| | Fund buyer 0.0005 ETH | [`0x90e07ef4…`](https://sepolia.etherscan.io/tx/0x90e07ef4a9edd2964739b2250c5ca58704a760019afb0100524f375c209e14a8) |
| | Fund seller 2 USDC | [`0xb182ca1f…`](https://sepolia.etherscan.io/tx/0xb182ca1f1adb186291cbebe7891ba36da1c10628293bb1fdca3b6a86a8e63dcd) |
| Happy path, 1 USDC for ₦1,600 | Approve exactly 1 USDC | [`0xd7bb89d6…`](https://sepolia.etherscan.io/tx/0xd7bb89d676e49738cfa24f30781a8181842beb20d20b851e7f37592fd7881521) |
| | Seller locks 1 USDC | [`0xe8a85f32…`](https://sepolia.etherscan.io/tx/0xe8a85f32bc91c986aa85ed14f2450514cda6cd7f10d28bd2fd0bcdd3e2ade790) |
| | (buyer pays through the ledger) Buyer marks paid | [`0xacd12392…`](https://sepolia.etherscan.io/tx/0xacd12392837a7a6bcf6a475581084b427500453752afeede8fc5e02fb3d23af6) |
| | Seller releases | [`0x8606d424…`](https://sepolia.etherscan.io/tx/0x8606d424298e46ba01345da737c73b0afe62dd7b6c9bbd4348716e83d576daca) |
| Timeout, 0.001 ETH | Seller locks 0.001 ETH | [`0x66247b82…`](https://sepolia.etherscan.io/tx/0x66247b82ef196b6bfd3ba44f18b0a57f8d68b328e2686a8ab26652d7f35789ce) |
| | Buyer never pays; after the deadline the seller takes it back | [`0xae73aec0…`](https://sepolia.etherscan.io/tx/0xae73aec0a4415e8787f43969eba8e9ecc4d9ca74e062d2b27ac1217fa5a38ecd) |
| Dispute for the seller, 1 USDC by bank transfer | Approve exactly 1 USDC | [`0x0abd2c1c…`](https://sepolia.etherscan.io/tx/0x0abd2c1c07ed82a0c5c342f0a0e6a7b7689a347d13f8bf5179afc005a2fde5e1) |
| | Seller locks 1 USDC | [`0xaf92c154…`](https://sepolia.etherscan.io/tx/0xaf92c1547cc98fbc0d0f9a7b8f3307a4c085b69da02570c9386fde46890602a0) |
| | Buyer marks paid (says they sent a bank transfer) | [`0xaf1d7ed7…`](https://sepolia.etherscan.io/tx/0xaf1d7ed7b7379a8b6cf11dc2212435ffb3b320c26976c047e9041bfa1ba18800) |
| | No money arrived; the seller opens a dispute | [`0x38dee5c8…`](https://sepolia.etherscan.io/tx/0x38dee5c89b6aa382345952ec4bf20e24cadbee861bc1c65c5fc393e6a159fe8e) |
| | A compliance admin decides for the seller; the arbiter key signs `resolve` | [`0xf92fbae9…`](https://sepolia.etherscan.io/tx/0xf92fbae97b328543761452550ec53dbf89acfc98a46dd7e3364af34f6dcdf0d3) |

Both happy-path orders were rated by both sides. The arbiters' Sepolia ETH (0.003, sent in the first attempt below: [`0x600b6d01…`](https://sepolia.etherscan.io/tx/0x600b6d0109bf03b34c6e0d1985ef8643f3010eea7a40f1b26bac7ad86da4d793)) and devnet SOL (0.02) pay for their resolutions.

**The two refunds were sent with `npm run p2p:devnet -- refund`, not by the run itself.** After waiting out the payment windows (the deadline is the 15-minute window plus 5 minutes), the run's first refund step stopped because the public RPC dropped a read ("Couldn't reach the network"). The in-memory database ended with it, so both refunds were sent by the recovery mode: it reads each escrow from its own `EscrowFunded` event, checks the seller and that the deadline has passed, builds the refund with the app's own builders (`buildEscrowMessage` and `checkEscrowMessage`, `buildEscrowCall`) and signs it with the saved throwaway seller key. Both escrows' `EscrowSettled` events say `refunded`, all of it to the seller, and the Solana escrow account is closed. The live script now tries a step again when a read is dropped, and the scripts' devnet calls retry dropped connections as well as 429s.

**A first attempt stranded test coins.** It made the throwaway keys in memory only and opened a fourth order while three were open, which the app refused (`MAX_OPEN_ORDERS`, as designed). Its happy paths completed (Solana [`3CFZXeqp…`](https://explorer.solana.com/tx/3CFZXeqpi6tbHA99Ey5oNtTWm3sUgQorbANVbZgE1Bp6TXNU79MjXrRnXGHqzqNnGBUE9k95k42PNNgcNqjqHDh9?cluster=devnet), [`3uPaNEef…`](https://explorer.solana.com/tx/3uPaNEefvcwz36uPa3228Dxbw93MWJ3qLJESHZFz9BL3mNTxfbJDGGuhgqLaRy7U4hdKkWomgfqkbvZjFM7ofqCt?cluster=devnet), [`1SYytnv9…`](https://explorer.solana.com/tx/1SYytnv9da2cLFbNqugAKmtNHVEQDrtkDNgFX2CJ149CzkukMvfAMK9c7ewgJrfnrPZamma94P5KJuxJPTbespn?cluster=devnet); Sepolia [`0xb95866a3…`](https://sepolia.etherscan.io/tx/0xb95866a3a9664077da1b4341161d23196c9742435be51fafe22042a89fc0664d), [`0x19119e09…`](https://sepolia.etherscan.io/tx/0x19119e0958b507db264ff5eae39dbec2159478400aea8dba9d5ecb2acfdfd429), [`0x21d66749…`](https://sepolia.etherscan.io/tx/0x21d667491ed31c1ed2a820a12b1da1734371fd10e0db2c51e048c6b8f3cb7621), [`0xc77b4d13…`](https://sepolia.etherscan.io/tx/0xc77b4d13a156b4d14bfce11de6fc5311fb376523c90d0cfd29b3e535a745ee76)), but with the keys gone, three escrows can't be settled by their seller: 0.02 devnet SOL ([`58XRwDDs…`](https://explorer.solana.com/tx/58XRwDDsigPZBRTVz7uNgAy1biejRn2gGg84GyeAeCHEwnUkK67ZkVfLJH3cDVwc1Zh2LJPKX84wqN2x5nBuBiqS?cluster=devnet)), 0.001 Sepolia ETH ([`0x9022b20e…`](https://sepolia.etherscan.io/tx/0x9022b20e0477132c103e6b253eeaf4ac6a0848703b3552191ffabc94b27bda57)) and 1 devnet USDC in a disputed escrow ([`4FbMK567…`](https://explorer.solana.com/tx/4FbMK5673vBEPK6b2ork2YNHDEZvZvvhEAg8Pd1yvaEtGVigzsZgUNJAsRtPdtawPdZ4QU4V7f91gPwGYWb6DdbG?cluster=devnet)), plus what was left in that run's wallets (about 0.015 Sepolia ETH, 2 Sepolia USDC and some devnet SOL). Test coins only, and exactly what the design promises: nobody but the escrow's own parties (and, in a dispute, the arbiter) can move them. The live script now saves the throwaway wallets' recovery entropy to a file in the OS temp folder first.

## What the professional audit must check before any mainnet use

1. Both implementations in full: that no path moves money to anyone but the seller, the buyer or the treasury; the deadline and state rules; the fee cap and the fee fixed at funding; the two-step owner and arbiter changes.
2. The arbiter: today one server-held key per chain. Mainnet needs a multisig (or a timelocked committee), monitoring of every `resolve`, and a written dispute policy. Whoever holds the arbiter can decide every disputed escrow.
3. Token assumptions: USDC can blacklist an address (the Sepolia contract credits `owed` instead of freezing; on Solana a frozen token account would make the settlement fail until it is unfrozen); fee-on-transfer and rebasing tokens aren't supported; one token per chain.
4. Who holds the owner and the Solana upgrade key (today the throwaway devnet deployer), and whether the program should become immutable.
5. The app side: the key worker's read-back of every escrow step (`lib/beryva/p2p`, `lib/beryva/keys/sign.ts`), the order state moving only from the escrow's own events, and the arbiter's read-back before it signs (`server/beryva/p2p/arbiter.ts`).
