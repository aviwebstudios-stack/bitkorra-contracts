# BitKorra contracts

The on-chain code behind [BitKorra](https://bitkorra.vercel.app), a self-custody money app: keep your crypto, send it to anyone, swap coins, and buy or sell for naira directly with other people.

This repository holds BitKorra's own smart contracts and programs, written twice where it matters: **Solidity on Ethereum Sepolia** (Foundry) and **Rust with Anchor on Solana devnet** (LiteSVM tests). Every one is deployed, used by the live app, and linked below with its explorer page and real transactions.

> **Test networks only.** Everything here runs on Ethereum Sepolia and Solana devnet, where coins have no value. Nothing has been audited. Do not deploy any of it to a real network as it is. A professional audit, multisig owners and a legal review come before any real money.

## What's here

| Name | Chain | Language | What it does |
|---|---|---|---|
| **BerylEscrow** | Sepolia | Solidity | P2P escrow. The seller locks ETH or test USDC for one order; the buyer pays in naira off chain and marks it paid; the seller releases. Refund after the deadline only if the buyer never marked paid. A dispute after "paid" can only be settled by a two-step arbiter, and money only ever goes to the seller, the buyer or the treasury. Fee 0 by default, capped at 2% and fixed at funding. |
| **Beryl Escrow** | Solana devnet | Rust (Anchor) | The same escrow for SOL and devnet USDC, with a vault PDA per order and rent back to the seller. |
| **BerylTips** | Sepolia | Solidity | Tips in ETH or test USDC, paid straight to the creator in the same transaction. |
| **BerylPaidPosts** | Sepolia | Solidity | Paid posts: a creator registers a price, a buyer pays the creator and gets an on-chain access record. Price changes never touch existing access. |
| **Beryl Tips** | Solana devnet | Rust (Anchor) | Tips and paid posts in SOL or devnet USDC, with an access PDA per buyer. |
| **Beryl Names** | Solana devnet | Rust (Anchor) | Usernames as accounts (`ada.korra` in BitKorra): register, records (ETH, SOL, BTC address, avatar), primary name for reverse lookup, transfer, release. |
| **BitKorra Swap** (`BeryvaFactory`, `BeryvaPair`, `BeryvaRouter`) | Sepolia | Solidity | A constant-product AMM in the Uniswap V2 style: 0.3% to liquidity providers, slippage limits and deadlines, WETH for ETH. |

The names in the code (Beryl…, Beryva…) are the project's earlier code names. They stay because renaming deployed contracts means new addresses.

## Deployments

### Ethereum Sepolia (chain id 11155111)

Deployer and owner: [`0x84857708F2a373afd841c7507db739030bE80b13`](https://sepolia.etherscan.io/address/0x84857708F2a373afd841c7507db739030bE80b13) (a throwaway test-network key). Compiler: solc 0.8.30, optimizer 200 runs, `cancun`.

| Contract | Address | Source verified |
|---|---|---|
| BerylEscrow | [`0xe5E5beEf9820615d606753e490554ef77BfCd6F8`](https://sepolia.etherscan.io/address/0xe5E5beEf9820615d606753e490554ef77BfCd6F8) | [Etherscan](https://sepolia.etherscan.io/address/0xe5E5beEf9820615d606753e490554ef77BfCd6F8#code), [Sourcify](https://repo.sourcify.dev/11155111/0xe5E5beEf9820615d606753e490554ef77BfCd6F8) |
| BerylTips | [`0x60C687622a8dDABe6aB4F9A935211cAC1D26113a`](https://sepolia.etherscan.io/address/0x60C687622a8dDABe6aB4F9A935211cAC1D26113a) | [Etherscan](https://sepolia.etherscan.io/address/0x60C687622a8dDABe6aB4F9A935211cAC1D26113a#code), [Sourcify](https://repo.sourcify.dev/11155111/0x60C687622a8dDABe6aB4F9A935211cAC1D26113a) |
| BerylPaidPosts | [`0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b`](https://sepolia.etherscan.io/address/0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b) | [Etherscan](https://sepolia.etherscan.io/address/0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b#code), [Sourcify](https://repo.sourcify.dev/11155111/0x08410EEE4f0F558Bf17Aa6c6488faF9B866fa04b) |
| BeryvaFactory | [`0xDf38C32C3d72a11782d240d9BC362C8091D20ED7`](https://sepolia.etherscan.io/address/0xDf38C32C3d72a11782d240d9BC362C8091D20ED7) | [Etherscan](https://sepolia.etherscan.io/address/0xDf38C32C3d72a11782d240d9BC362C8091D20ED7#code), [Sourcify](https://repo.sourcify.dev/11155111/0xDf38C32C3d72a11782d240d9BC362C8091D20ED7) |
| BeryvaRouter | [`0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2`](https://sepolia.etherscan.io/address/0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2) | [Etherscan](https://sepolia.etherscan.io/address/0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2#code), [Sourcify](https://repo.sourcify.dev/11155111/0x00BAE4bf12c1261176B7a90C74E0C0DB5A97D0C2) |
| ETH/USDC pool (BeryvaPair) | [`0x4B05eaBF44b802122952F476CfF98D2c056B8F1C`](https://sepolia.etherscan.io/address/0x4B05eaBF44b802122952F476CfF98D2c056B8F1C) | [Etherscan](https://sepolia.etherscan.io/address/0x4B05eaBF44b802122952F476CfF98D2c056B8F1C#code), [Sourcify](https://repo.sourcify.dev/11155111/0x4B05eaBF44b802122952F476CfF98D2c056B8F1C) |

Outside contracts used: WETH9 `0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9` and Circle's test USDC `0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238`.

### Solana devnet

Deployer and upgrade authority: `21FxQJZ4xA4jXf2gJ4WPzY3RgT3bbzW3t9qkkZstRrVe` (a throwaway devnet key). Built with Anchor 1.2.1 and Agave 4.3.2 as SBPF v2 (the version LiteSVM 0.10 can load, so the tests run the same binary that is deployed).

| Program | Program ID |
|---|---|
| Beryl Escrow | [`8wDapxboCTEKfBzNQzrzZUnspmTixKFfCuNmUuQLrcDA`](https://explorer.solana.com/address/8wDapxboCTEKfBzNQzrzZUnspmTixKFfCuNmUuQLrcDA?cluster=devnet) |
| Beryl Tips | [`B9oecLNJC7SCTMbZc6G7UJSwj9mUpL9AwF6r3CFE5CEf`](https://explorer.solana.com/address/B9oecLNJC7SCTMbZc6G7UJSwj9mUpL9AwF6r3CFE5CEf?cluster=devnet) |
| Beryl Names | [`92jgXoEDydnpZhUFbcqoFf7aPasLGPP1S2E7CcpLYuPR`](https://explorer.solana.com/address/92jgXoEDydnpZhUFbcqoFf7aPasLGPP1S2E7CcpLYuPR?cluster=devnet) |

Verified builds: see [Verifying the Solana programs](#verifying-the-solana-programs).

## Live transactions

Real runs on the test networks, end to end through the app's own code. The full logs are in [docs/deployments.md](docs/deployments.md).

**P2P escrow, Sepolia**

- Seller locks 1 test USDC: [`0xe8a85f32…`](https://sepolia.etherscan.io/tx/0xe8a85f32bc91c986aa85ed14f2450514cda6cd7f10d28bd2fd0bcdd3e2ade790), buyer marks paid: [`0xacd12392…`](https://sepolia.etherscan.io/tx/0xacd12392837a7a6bcf6a475581084b427500453752afeede8fc5e02fb3d23af6), seller releases: [`0x8606d424…`](https://sepolia.etherscan.io/tx/0x8606d424298e46ba01345da737c73b0afe62dd7b6c9bbd4348716e83d576daca)
- Buyer never pays, seller takes it back after the deadline: [`0xae73aec0…`](https://sepolia.etherscan.io/tx/0xae73aec0a4415e8787f43969eba8e9ecc4d9ca74e062d2b27ac1217fa5a38ecd)
- Dispute decided for the seller, signed by the arbiter key: [`0xf92fbae9…`](https://sepolia.etherscan.io/tx/0xf92fbae97b328543761452550ec53dbf89acfc98a46dd7e3364af34f6dcdf0d3)

**P2P escrow, Solana devnet**

- Seller locks 1 devnet USDC: [`5xgMnmZN…`](https://explorer.solana.com/tx/5xgMnmZNqUfLz9erFmeE9Jo98EtTNg4fVbu7C7tZz24AiMC7LVRFm9RzWwXcfAGXmYkNUEpowvJgTKdaYF2izGNE?cluster=devnet), buyer marks paid: [`ZvNMTh4S…`](https://explorer.solana.com/tx/ZvNMTh4SgdZp2UXAVzz9EQenyVURGkvUUVKosRSx3yatbKY9d8qVLxjvEGRxhd8p3MpNMeNW1au8JV9PPQqMErQ?cluster=devnet), seller releases: [`5D96TXfa…`](https://explorer.solana.com/tx/5D96TXfa3KgyXGQsKznPXHZbV8jeVjymuCXHkBnUR4ZW1RnXXzw1vamfcWNw2ukD59uSnYV3VezoqNZVPABnG49X?cluster=devnet)
- Dispute decided for the buyer, signed by the arbiter key: [`zGvcLsmT…`](https://explorer.solana.com/tx/zGvcLsmTYi4HMhpUJr1McPnGXSbrqkSQRuSVnw3DnjsjX63TyXH7LBqoywEc8kqG21EeQ4NCUCwXCD8KYr8Z7we?cluster=devnet)

**Tips and paid posts**

- Sepolia: tip 0.0005 ETH [`0x7f8399a9…`](https://sepolia.etherscan.io/tx/0x7f8399a97977c9f491c0f95e094c67779ae70a7473be029c7d235fe6ac4ebc01), tip 0.5 test USDC [`0xeccaa67b…`](https://sepolia.etherscan.io/tx/0xeccaa67bdbb9bd73c884c630e033d0350602f6d6a109c57c60236cea1ab08c68), buy a post with ETH [`0x09b96870…`](https://sepolia.etherscan.io/tx/0x09b96870121ae74719c2a1f85e58b1df5fd77c0b1c2dec83b94e338fb2e4765f)
- Solana devnet: tip 0.01 SOL [`4d9vrSxG…`](https://explorer.solana.com/tx/4d9vrSxGjWxw4XSdecNZBbR3HwHRxkK7oVPbDRw49k1maVTk9b8cpkwKPKUGm15KPxfvaHCwBv5UCjYBYCWzZV65?cluster=devnet), buy the SOL post [`2cBUWpR7…`](https://explorer.solana.com/tx/2cBUWpR7gmpMFqkadypRhnU1edsGUrEa458LtaM1MspbH3u1gKv51Atzja17nNmxTQQ7Wh2sK3NiY52AxZG46a3N?cluster=devnet)

**Swap, Sepolia**

- 0.0001 ETH for 0.235522 test USDC, exactly the router's quote: [`0x2d83090a…`](https://sepolia.etherscan.io/tx/0x2d83090ae7ae70ea5188dd48f53ba36ae67faee559528eb0f8606a5250c859dc), and back: [`0x7ce1429a…`](https://sepolia.etherscan.io/tx/0x7ce1429a629da15e83b57cc4c7cf10bef99d69b8da6ea9954a716d84887e64c1)

**Names, Solana devnet**

- register [`5YcX4kj3…`](https://explorer.solana.com/tx/5YcX4kj3pzTgkPLvjrqiiwKvygVpYVV1xJ4RKAGJjSZFP2fxifhGRmWhdydsSfFDykkP54GxGYYeFQfTxrgjz9w6?cluster=devnet), set records [`2apCXwLC…`](https://explorer.solana.com/tx/2apCXwLCHmns9hXswc3f27zoZpeSp7a6ZxwW3a7EkkuEkXoskCnRPYEn1GH7GtHKNsFwxQYoZ7xda4vitWKW8zM?cluster=devnet), transfer [`2qMjNqyc…`](https://explorer.solana.com/tx/2qMjNqycErEGHYdZnEQNFYjzS7iaPuM9nwKtTzjJtBbgawHiVFcb8imC3KHbj7a3BDqnp6MrebAwzxSvHPA4iXcr?cluster=devnet), release [`4jrwHrP2…`](https://explorer.solana.com/tx/4jrwHrP2GnB9D6YJtnzksmisNoUZopEwWVgUQbmaf8WtCD9znXYqkwmH3F9pXiT4UDXVwzCZZedpF2bJvGZ4T97F?cluster=devnet)

## Layout

```
contracts/            Foundry workspace (Solidity)
  src/                the contracts
  test/               unit, fuzz and invariant tests (test/fork needs a Sepolia RPC)
  script/             deployment scripts (checked with forge script, sent with cast send)
solana/               Anchor workspace (Rust)
  programs/<name>/src the programs
  programs/<name>/tests LiteSVM tests and proptest properties
  idl/                the IDLs anchor build writes
  scripts/            test, build and deploy scripts (run on Linux or WSL)
docs/deployments.md   every deployment, test result and live run, with static analysis notes
```

## Running the tests

### Solidity (Foundry)

Needs [Foundry](https://book.getfoundry.sh/getting-started/installation). The libraries are pinned and installed, not committed:

```sh
cd contracts
forge install --no-git OpenZeppelin/openzeppelin-contracts@v5.7.0 foundry-rs/forge-std@v1.17.0
forge build
forge test
```

The fork test (`test/fork`) is skipped unless `BERYVA_FORK_RPC` is set to a Sepolia RPC URL. Fuzz runs: 1,000 per property; invariants: 256 runs, depth 64 (`foundry.toml`).

Static analysis: `slither src/` (Slither 0.11.6). Every finding is listed and explained in [docs/deployments.md](docs/deployments.md).

### Rust (Anchor)

Needs Linux or WSL with Rust (pinned in `solana/rust-toolchain.toml`), Agave 4.3.2 and Anchor 1.2.1 (`avm install 1.2.1`). Then:

```sh
bash solana/scripts/test.sh
```

That runs `cargo fmt --check`, the SBPF v2 build, `cargo clippy -D warnings` and every LiteSVM test and proptest property.

## Security model, in short

- **Nothing here holds money it doesn't have to.** Tips and paid posts pay the creator inside the same transaction. The escrows hold one order's coins and can only send them to the seller, the buyer or the treasury fee.
- **Fees are capped in code**: 5% for tips and paid posts, 2% for the escrow, 0 by default, and the escrow's fee is fixed when an order is funded.
- **Owners, authorities and arbiters change in two steps** (propose, then accept; OpenZeppelin's `Ownable2Step` on Sepolia), so a typo can't hand control to a dead address.
- **Sepolia contracts** use `ReentrancyGuard`, `SafeERC20`, checks-effects-interactions, and a pull-payment fallback when an ETH payout fails.
- **Solana programs** check every account against its seeds and stored bump, every signer, owner and mint, with no unchecked accounts.
- **Test-network keys.** The deployers, owners, upgrade authorities and arbiters are throwaway keys. Before any real network: multisig owners and arbiter, an immutable or multisig-upgraded Solana program set, a written dispute policy, and an independent audit. What that audit must cover is listed per contract in [docs/deployments.md](docs/deployments.md).

## Verifying the Solana programs

The programs are built reproducibly in Docker (`anchor build --verifiable`) and submitted to the public verification registry with [`solana-verify`](https://github.com/Ellipsis-Labs/solana-verifiable-build) from this repository and a fixed commit. Status per program is updated here once it's done.

| Program | Verified build |
|---|---|
| Beryl Escrow | pending |
| Beryl Tips | pending |
| Beryl Names | pending |

## License

[MIT](LICENSE), copyright 2026 Vanessa (Aviwebstudios). The installed libraries keep their own licenses (OpenZeppelin Contracts: MIT; forge-std: MIT or Apache 2.0).

## Reporting a problem

Found a bug? Open an issue. For anything that could put funds at risk if this code were ever used on a real network, please don't post details publicly: say in an issue that you have a security report and ask for a private contact.
