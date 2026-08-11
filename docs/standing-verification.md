# Independently verifiable marketplace standing

**Decision status:** accepted for CabalMesh standing v1 on 2026-08-12.

Standing shown beside a marketplace listing is a public, independently
verifiable fact about the listing's seller wallet. It is not a reputation
prediction, star rating, average, device-local counter, or claim supplied by
the seller.

## Canonical definition

For seller wallet `S` at accepted block `B`:

> `standing(S, B)` is the number of unique CabalMesh end-to-end commercial
> settlements credited to `S` by an authorized settlement source at or before
> `B`, minus those credits authoritatively reversed or refunded at or before
> `B`.

The value is the `activeSettlements` count returned by the canonical
`CabalStandingRegistry.standingOf(S)` contract on the configured chain. It is a
lifetime net count, not a time-windowed score.

One credit represents one completed end-to-end transaction for which `S` was
the accountable fulfiller/seller of record:

- a marketplace deal after the asset transfer and seller payment are both
  finally released;
- an intent after its promised end result and payment to the fulfiller are
  finally settled.

Infrastructure steps are not separate standing credits. Relay hops, gateway
byte windows, route retries, escrow creation, listings, bids, approvals,
module minting, and standing-badge minting do not count. Pending, processing,
waiting, failed, cancelled, expired, self-dealing, and rejected settlements do
not count. A multi-hop intent still creates at most one credit for its
accountable fulfiller; relay compensation is governed separately.

Each authorized source assigns an opaque 32-byte `sourceSettlementId` and may
record it once. The registry derives
`recordId = keccak256(abi.encode(source, sourceSettlementId))`, so two sources
cannot collide. Sources must reject self-dealing and duplicates before calling
the registry; this semantic policy is part of SOURCE_ROLE admission and is not
something the registry can infer from a hash.

If a completed settlement is later refunded, unwound, charged back, or proven
invalid, its original source or CORRECTOR_ROLE calls `reverseSettlement` once.
That decrements the seller's count and appends a reason commitment while
retaining the original evidence. A reversal before the initial credit means
the source never credits it. Repeated reversal cannot decrement twice.

## Authoritative evidence and governance

`contracts/contracts/CabalStandingRegistry.sol` is the canonical state
machine. It emits enough data to recompute the count from `StandingCredited`
and `StandingReversed` events and also exposes the current value directly.
Neither path requires the seller's device or database.

Authority is deliberately split:

- `DEFAULT_ADMIN_ROLE` is protected by OpenZeppelin's two-step, two-day admin
  transfer delay and manages role membership;
- `SOURCE_ROLE` is granted only to contracts/services that can establish final
  end-to-end completion and may add credits;
- `CORRECTOR_ROLE` may remove an invalid/refunded credit, including after a
  compromised source is revoked, but cannot add one.

Production release configuration identifies the source by the tuple
`(EVM chain ID, CabalStandingRegistry address)`. A contract name, ABI, explorer
label, RPC URL, or address without its chain is not an identity. Until a build
has a non-zero reviewed registry address for its selected chain, public
standing is `UNKNOWN`. Deployment and SOURCE_ROLE grants are recorded in the
network deployment manifest; they are not inferred from local `.env` values.

## Seller identity binding

The seller identity displayed by marketplace is exactly the EVM address stored
in the on-chain listing. `Marketplace.createListing` records `msg.sender` as
the seller, so creating the listing requires a valid transaction signature
from that account. Purchase/release logic uses the same stored address, and
the standing query uses that exact address byte-for-byte.

The UI displays a checksummed shortened wallet address, for example
`SELLER 0x7F3A…C2 · VERIFIED 42`. It never labels the value with a mutable
profile name supplied by the seller.

The libp2p/BLE node identifier is intentionally **not** the public standing
identity. It is session/installation networking material and may rotate; tying
it to a permanent marketplace history would both break verification and leak
the seller's physical mesh activity. A future signed session binding may prove
that a wallet currently controls a node, but it cannot change the standing key
or let a node ID inherit another wallet's count.

## Buyer verification algorithm

The pure implementation is `src-tauri/crates/cabal-standing`. Network adapters
must perform these steps without accepting a number from listing metadata:

1. Load the reviewed chain ID, registry address, maximum age (v1: five
   minutes), and distinct-provider quorum (v1 minimum: two) from release
   configuration. Missing/invalid configuration yields `UNKNOWN`.
2. Take `seller` directly from the on-chain listing. Zero or any mismatch
   between the query and response yields `UNKNOWN`.
3. Ask independently operated RPC providers for an accepted block. Choose a
   height all quorum providers can serve, then fetch that exact block from each
   provider and require the same non-zero block hash.
4. On every provider, call `standingOf(seller)` using `eth_call` pinned to that
   exact block number. A `latest` read paired with a block fetched later is not
   valid evidence.
5. Require at least the configured number of **distinct provider IDs** to
   agree on chain, registry, seller, accepted block number/hash, count, and
   `lastChangedBlock`. `lastChangedBlock` cannot exceed the pinned block.
6. Reject observations from the future or older than five minutes. The UI may
   show the accepted height as `VERIFIED 42 · BLOCK 123456` so evidence age is
   inspectable.

One unavailable endpoint may be ignored only if a full matching quorum
remains. Any successful provider that returns a different identity, block,
count, or mutation height makes the whole result unknown; majority voting must
not turn contradictory chain state into a claimed fact.

An auditor may additionally replay `StandingCredited` and
`StandingReversed` logs to derive the active count and compare it with
`standingOf`. This is an audit path, not a requirement for every card render.
Historical RPC pruning may make replay unavailable; the current pinned state
still verifies if quorum succeeds.

## Freshness, finality, and disagreement

Avalanche C-Chain nodes normally expose finalized blocks; the node setting
`allow-unfinalized-queries` defaults to false, and Avalanche documents an
accepted transaction as finalized. Unlike longest-chain confirmation models,
waiting for more depth does not convert an unfinalized Avalanche block into
stronger evidence. CabalMesh therefore accepts only a provider response known
to be accepted/final and never displays processing/pre-accepted state.

References:

- [AvalancheGo C-Chain RPC and accepted transaction subscription](https://build.avax.network/docs/rpcs/c-chain)
- [C-Chain finality/query configuration](https://build.avax.network/docs/nodes/chain-configs/primary-network/c-chain)
- [`eth_getBlockByNumber` reference](https://build.avax.network/docs/rpcs/c-chain/eth/eth_getBlockByNumber)

If providers report different hashes at one height, the response is
unfinalized, the endpoint exposes unfinalized queries, or any authoritative
fields disagree, the card shows `UNKNOWN` and retries later. It does not choose
the largest count, average values, trust the seller's preferred RPC, or reuse a
previous value without its age label.

The rendering contract is exact:

| Evidence state | Buyer-visible value |
|---|---|
| Matching fresh accepted quorum, count 42 | `VERIFIED 42` |
| Matching fresh accepted quorum, count 0 | `VERIFIED 0` |
| Unconfigured, unavailable, stale, malformed, unfinalized, or conflicting | `UNKNOWN` |

`0` is shown only when the registry measurably returned zero and quorum
verified it. `UNKNOWN` must not be sorted or filtered as zero.

## Relationship to the device-local ledger

`src-tauri/src/standing.rs::LocalStanding` counts settled intents visible to
this installation for the owner's home/profile history. That ledger may be
missing after reinstall, contain a locally settled item not yet anchored by an
authorized source, include a settlement where this user was not the credited
seller, or be temporarily ahead/behind public state. It is neither public nor
authoritative for another buyer.

Reconciliation is diagnostic only:

| Public evidence | Local ledger | Behavior |
|---|---|---|
| Unknown | any value | Show `UNKNOWN`; never substitute local |
| Verified | unavailable | Show verified public value; local unavailable |
| Verified `P` | equals `P` | Show public value; report match |
| Verified `P` | less than `P` | Show public value; local history is incomplete |
| Verified `P` | greater than `P` | Show public value; excess is unanchored/non-creditable |

The counts are never added, averaged, or allowed to overwrite each other.

## Privacy decision

Publishing standing creates a permanent activity signal. Anyone can correlate
a seller wallet, exact aggregate, changes over time, source contract, credit
and reversal blocks, and marketplace listings. Reversal is append-only and
cannot erase that history. A standing badge can add more public milestones.

V1 accepts this leakage only for wallets that explicitly opt into public
marketplace selling. Before the first listing, the app discloses that standing
is permanent public wallet history. A seller may use a dedicated marketplace
wallet, but standing is never merged or transferred across wallets; changing
wallets starts at verified zero. Private chat participants and nodes that do
not list publicly are not enrolled merely because they use the mesh.

On-chain records contain only wallet addresses and cryptographic commitments:

- `sourceSettlementId` must be a random/opaque 32-byte identifier, never an
  order number, device ID, BLE peer ID, or hash of a low-entropy value;
- `evidenceHash` must be a domain-separated salted commitment to the private
  proof, not the proof or unsalted personal data;
- sources must not publish payloads, amounts, buyer/recipient identity, route,
  IP address, radio identifiers, location, or chat content in registry calls;
- the app must not log evidence material or RPC credentials.

This is the accepted v1 trade-off: exact public seller accountability with a
deliberately minimal, pseudonymous event surface. It is not anonymity, and the
UI must not imply otherwise.
