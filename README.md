# Leash

Leash gives payment agents a spending limit they cannot raise: a cap per payment and a cap per 24-hour window, enforced on NEAR.

## Problem

- An agent with unrestricted wallet access can spend more than its owner intended.
- A spending check inside the agent's own process can be bypassed by that process.
- Application logs alone do not prove which payments the chain accepted.

## Solution

The owner funds a contract and sets its limits. The agent receives a function-call key restricted to that contract's `spend` method. The contract checks the caller and both caps before requesting a transfer.

- **Owner control:** only the owner can change caps or withdraw, with exactly 1 yoctoNEAR attached to either call.
- **Limited agent access:** the agent's restricted key cannot raise caps or call withdrawal methods.
- **On-chain records:** accepted spend requests emit an event and update the window counter. Transaction receipts report the transfer outcome.

A window resets on the first spend at least 24 hours after its previous start. This is a fixed-duration window, not a continuously rolling total. Counters update before the asynchronous transfer completes; they are not proof of delivery.

## See it working

Open [Leash](https://leash-frontend.vercel.app) to inspect the contract's spending limits and recorded transactions. The page is read-only; it does not connect a wallet or submit payments.

The deployed contract is [`leash-0445ff.testnet`](https://testnet.nearblocks.io/address/leash-0445ff.testnet). Its state is read from NEAR Testnet, not a production network. The contract has not been independently audited; do not use it to hold real funds.

## Tech stack

| Layer | Technology | Role |
|---|---|---|
| Spending policy | Rust, NEAR SDK | Caller checks, payment caps, window accounting and transfers in `contract/src/lib.rs`. |
| Account permissions | NEAR function-call keys | Restrict the agent to the contract's `spend` method. |
| Inspector | HTML, CSS, JavaScript | Read contract state and transaction outcomes through the public Testnet RPC. |
| Hosting | Vercel | Serve the static site from `site/`. |

## Quick start

### Prerequisites

Git, Rust 1.93 or newer with Cargo, and Python 3. Use Linux or WSL for contract tests: NEAR's test runtime does not compile on native Windows. No wallet or signing key is needed to run the tests or open the inspector.

### Setup

```sh
git clone https://github.com/0xdfinity/Leash.git
cd Leash
```

### Test the contract

```sh
cargo test --locked --manifest-path contract/Cargo.toml
```

Tests cover accepted spends, both caps, window expiry, caller permissions, owner-only actions and invalid limits.

### Run

```sh
python -m http.server 8000 --directory site
```

Open `http://localhost:8000`. Internet access is required to read Testnet. If the network is unavailable, the inspector marks values unavailable and offers a refresh.

### Build the contract

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked --manifest-path contract/Cargo.toml --target wasm32-unknown-unknown --release
```

The build writes `contract/target/wasm32-unknown-unknown/release/leash.wasm`. Deploying your own contract requires your own owner and agent accounts; the public inspector's deployment is not a wallet for visitors.

### Environment variables

None are needed for the static site. Keep signing keys outside the site and out of source control.

## License

MIT. See [LICENSE](LICENSE).
