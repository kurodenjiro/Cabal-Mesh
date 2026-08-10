# Product status — what is real, what is not

Written 2026-08-10, by reading the code rather than the READMEs. Every claim
below carries the file it was checked against, so this can be re-verified
instead of trusted.

The point of this document is that CabalMesh's pitch names three big things —
mesh, AI, ZK — and only one of them is actually running. Knowing which is which
is the difference between a demo that survives questions and one that does not.

## Works, end to end

| Feature | Evidence |
|---|---|
| Mesh networking (libp2p, mDNS, gossipsub) | `src/mesh.rs`; 9 passing tests in `tests/ble_loopback.rs` |
| BLE offline plane (Apple + Android) | `src/ble/`, `crates/cabal-ble`; demonstrated across two Android emulators, see `mobile-build-verification.md` |
| Intent lifecycle: compose → broadcast → settle → proof | `src/commands.rs`, `src/intents.rs` |
| On-chain Escrow on Fuji | `contracts/contracts/Escrow.sol`, real addresses in `contracts/deployments/fuji.json` |
| Offline signing + relay queue + auto-confirm on reconnect | `src/blockchain_bridge.rs` (`sign_offline`, queue replay) |
| Vault encryption (AES-256-GCM) | `crates/cabal-vault` |
| Standing (settlement count, real) | `src/standing.rs` |

**The offline path is the crown jewel.** Sign a transaction with no network,
relay it over the mesh, watch it confirm on-chain when a gateway appears. No
other part of this project is as differentiated, and it demonstrably works.

## UI exists, behaviour does not

These render, accept input, and are stored — but nothing downstream reads them.

| Control | What actually happens |
|---|---|
| **PRIVACY** (LOW/MEDIUM/HIGH) | Parsed into the draft and echoed in the review dialog (`commands.rs:879`). `grep` for `PrivacyLevel` across `mesh.rs`, `ble/`, and `crates/cabal-ble/src/router.rs` returns **no routing code**. It changes nothing. |
| **MODE** (SHARK/GHOST/PATIENT) | Labels and descriptions live in `crates/cabal-core/src/intent.rs:39-58`. No hit for `ExecutionMode` in `mesh.rs`, `matcher.rs`, or `blockchain_bridge.rs`. No strategy differs. |
| **SWAP / STAKE** actions | Parsed at `commands.rs:798-799`, then never branched on. `run_settlement` reads only `draft.amount` — every action takes the identical Escrow path. There is no DEX and no staking contract. |
| **USDC / WETH / BTC.b** | Offered by `commands.rs:643` (`const ASSETS`), but `sync_state` in `blockchain_bridge.rs` builds a snapshot containing exactly one asset: native AVAX. The other three always show `BALANCE NOT KNOWN`. |

## Not wired up at all

- **AI / Ollama.** `SharkAgent` (`src/agent.rs`) only implements `analyze_content`,
  a PDF classifier. `MatchAgent` (`src/matcher.rs`) implements `match_intent`.
  **Neither is called from `src/commands.rs`** — grep returns nothing. They were
  only reachable through the `legacy` module, deleted in 3e18664. So "AI agents
  negotiate" is currently 0% live.
- **Marketplace / vouchers.** `Marketplace.sol` and `CabalMeshVoucher.sol` are
  deployed to Fuji, but no command in the current 21-command surface exposes
  them. The frontend cannot mint, list, or buy anything.
- **Key export / import / backup.** Also removed with `legacy`. See
  `identity-design.md` — this is the most urgent gap, because a wallet is
  auto-created with no way to ever get the key out of it.
- **ZK verification.** `zk_handler::verify_proof` now shells out to
  `nargo verify` (47e81d8) instead of checking for non-empty strings, but it
  has **not been run against a real circuit** — this environment has no `nargo`.
  Nothing calls it yet either.
- **Confidential compute (FHE/MPC).** No code exists.
- **Private Swap.** No interface exists, despite older README wording.

## Platform reality

| Platform | Secure element | BLE | Notes |
|---|---|---|---|
| iOS | Secure Enclave | yes | ZK proving unavailable (`nargo` cannot spawn) |
| Android | Keystore / StrongBox | yes | same |
| macOS | Secure Enclave (Apple Silicon) | yes (CoreBluetooth) | needs `NSBluetoothAlwaysUsageDescription`, see 4dfa252 |
| Windows | TPM | **no** | BLE plane silently does not run |
| Linux | usually none | **no** | BLE plane silently does not run |

`ble::backend::choose` returns `None` outside Apple/Android, and the app falls
back to the IP plane without telling the user the offline plane is off.

## Suggested MVP cut

One true story beats three half-built ones. The offline mesh settlement path is
provable today; AI and ZK are not.

**Remove or disable** so the UI stops implying capabilities that do not exist:
SWAP/STAKE, the three unfunded assets, and either wire PRIVACY/MODE to real
behaviour or hide them.

**Add**, in order: identity backup (see `identity-design.md`), then reconnect the
AI as intent parsing (see `intent-chat-and-modules-design.md`), then everything
else.
