# Authentic module contract and trust model

- **Status:** accepted v1 contract model, 2026-08-12
- **Executable collection:** `contracts/contracts/CabalMeshModules.sol`
- **Marketplace guard:** `contracts/contracts/Marketplace.sol`
- **Metadata schema:** `cabalmesh-module-metadata-v1`
- **Fuji canonical address:** pending replacement deployment; an absent address
  means authentic modules are unavailable, never that a legacy voucher is a
  substitute

This document defines what makes a CabalMesh module authentic, how effects are
represented, which state is allowed to change, and how the existing Fuji
voucher/marketplace deployment is retired. Reward and gateway code must trust a
module only by canonical chain and collection address, on-chain owner/loadout,
unrevoked state, and effect fields. A name, symbol, image, local JSON file, or
arbitrary ERC-721 ownership is never authority.

## Canonical identity and roles

An asset identity is the tuple `(chainId, collectionAddress, tokenId)`. V1 uses
the non-upgradeable `CabalMeshModules` bytecode. The deployment registry must
publish the Fuji collection address after deployment; clients fail closed when
the configured chain or address is absent or different. ERC-721 permits any
contract to reuse a collection name or symbol, so neither `CMM` nor “CabalMesh
Modules” proves authenticity. This follows the collection-discovery limitation
stated in [ERC-721](https://eips.ethereum.org/EIPS/eip-721).

The collection separates three authorities:

| Authority | Contract rule | Production holder |
|---|---|---|
| Default admin | One account, two-step transfer, two-day acceptance delay | A reviewed 2-of-3 or stronger Safe; never a phone wallet |
| `MINTER_ROLE` | The only role that can call `awardMilestone` | Milestone/settlement issuer with narrowly scoped signing operations |
| `REVOKER_ROLE` | May irreversibly quarantine a bad issuance with a public reason hash | Separate incident-response Safe |

OpenZeppelin recommends `AccessControlDefaultAdminRules` because the default
admin can manage other roles and is high risk; the extension limits it to one
holder and adds a delayed two-step transfer. CabalMesh uses that extension with
a two-day delay. See the official
[OpenZeppelin access-control guide](https://docs.openzeppelin.com/contracts/5.x/access-control).

Deployment grants no role to a module recipient. After Fuji verification, the
deployer transfers default administration to the Safe, grants only the planned
minter and revoker, and renounces any obsolete role. Every role event is
monitored. The admin may pause new minting during an incident, but a pause never
freezes transfers by existing module owners.

## Immutable on-chain schema

`MintSpec` is validated and copied to immutable per-token `AssetData`. There is
no metadata setter, base-URI setter, effect setter, upgrade proxy, or admin
rewrite path.

| Field | Type | Meaning |
|---|---|---|
| `moduleId` | `bytes32` | Stable definition/family identifier, normally a namespaced `keccak256` |
| `provenanceHash` | `bytes32` | Unique commitment to the qualifying milestone, recipient, settlement, campaign, or approved legacy reissue evidence; the collection consumes it once |
| `displayName` | Printable ASCII string, 1–80 bytes | Human-readable asset identity; JSON control, quote, backslash, non-ASCII, and invalid UTF-8 bytes are rejected |
| `assetClass` | enum | `Module` or `StandingBadge` |
| `slot` | enum | `None`, `Radio`, `Crypto`, or `Power` |
| `rarity` | enum | `Common`, `Rare`, `Epic`, or `Legendary`; visual/category data only, never an implicit effect |
| `effectType` | enum | `None`, `RelayRewardBps`, `PrivacyHopIncrease`, or `GatewayLicense` |
| `primaryEffectValue` | `uint32` | First parameter with the type-specific meaning below |
| `secondaryEffectValue` | `uint32` | Second parameter with the type-specific meaning below |
| `artworkUri` | `ipfs://...`, 8–200 bytes | Content-addressed artwork reference |
| `artworkDigest` | non-zero `bytes32` | Expected SHA-256 artwork content digest for independent verification |
| `schemaVersion` | `uint16` | Contract-assigned value `1` |
| `mintedBy` | address | Role-bearing issuer that performed this mint |

V1 accepts only these structured slot/effect combinations:

| Slot / class | Effect | Primary | Secondary | Bounds |
|---|---|---:|---:|---|
| RADIO module | `RelayRewardBps` | Additive reward basis points | Must be `0` | `1..=10,000` bps |
| CRYPTO module | `PrivacyHopIncrease` | Additional privacy hops | Must be `0` | `1..=3` hops |
| POWER module | `GatewayLicense` | Maximum concurrent licensed sessions | Maximum authorized gateway window in KiB | Sessions `1..=32`; window `1..=1,048,576` KiB |
| Standing Badge | `None` | `0` | `0` | Slot must be `None`; no reward or runtime effect |

Unknown combinations and out-of-range parameters revert instead of being
clamped or treated as a default. Later tickets still have to measure and apply
the RADIO/CRYPTO/POWER behavior; this contract is the authoritative input, not
proof that the runtime effect happened.

## Standards-compatible metadata and availability

`tokenURI(tokenId)` implements the ERC-721 metadata extension and returns a
`data:application/json;base64,...` URI. The JSON contains standard `name`,
`description`, and `image` properties, marketplace-style `attributes`, and a
`cabalmesh` object with every schema field and typed effect parameter. External
wallets can render the ordinary properties, while CabalMesh clients parse the
versioned object. The standard metadata shape and `tokenURI` mechanism are
defined by [ERC-721](https://eips.ethereum.org/EIPS/eip-721).

The JSON and effect data are generated on-chain and cannot disappear with an
API or mutable web server. Artwork remains off-chain because image bytes are
too expensive for the collection: production issuance requires a content-
addressed IPFS URI, the on-chain digest, and pins held by at least three
independent operators. A client verifies the digest and shows a neutral
placeholder if no pin responds. Artwork unavailability never changes ownership,
loadout eligibility, or an effect value.

## Standing Badges

Standing Badges are ERC-721 tokens with `assetClass=StandingBadge`, `slot=None`,
and no effect. `locked(tokenId)` returns true and the collection advertises the
ERC-5192 interface (`0xb45a3c0e`), so standards-aware clients can discover that
they are soulbound. Every non-mint transfer reverts. ERC-5192 specifies this
minimal interface and discovery behavior; see
[ERC-5192](https://eips.ethereum.org/EIPS/eip-5192).

Token-specific `approve` also reverts for a badge, so a wallet cannot present a
badge approval as if it could lead to a transfer. ERC-721 blanket operator
approval is wallet-wide rather than token-specific and may already exist; it
does not make the badge transferable or marketplace-eligible.

The official Marketplace also queries
`ICabalMeshAsset.isMarketplaceEligible` both when a listing is created and
again before `buy`. A badge therefore cannot become a live official listing,
cannot receive buyer AVAX, and cannot enter escrow even when its owner granted
blanket operator approval. Badges remain wallet-visible evidence and are never
loadout modules or reward multipliers.

## Issuance and confirmed ownership

There is no generic public mint function. The milestone service first verifies
the qualifying evidence and derives a domain-separated `provenanceHash` that
commits to the milestone and intended recipient. It then calls
`awardMilestone`. The contract checks `MINTER_ROLE`, validates the complete v1
definition, and atomically records `tokenForProvenance[provenanceHash]` before
the ERC-721 receiver callback. A replay, a competing transaction, or reentrant
submission for the same commitment therefore reverts with the token that
already consumed it.

The app discovers holdings through ERC-721 Enumerable (`balanceOf` plus
`tokenOfOwnerByIndex`) on the explicitly configured canonical collection. It
reads `assetData`, `locked`, and `revoked`, then rechecks `ownerOf` before a row
crosses IPC. It never promotes pending receipts, replacement transactions,
marketplace descriptions, locally cached mint responses, or legacy vouchers to
confirmed ownership. Failed and replaced transactions never enter the owner
enumeration; a reorg is reflected on the next canonical refresh. When the
collection address is absent, VAULT → MODULES says unavailable rather than
falling back to the Fuji voucher.

## Loadout rules

The paid node identity is the operator wallet already used by the relay-proof
protocol. V1 intentionally does not let one wallet claim an arbitrary Bluetooth
node identifier on-chain.

1. Only the current ERC-721 owner may call `equip(tokenId)` or
   `unequip(tokenId)`.
2. A wallet has at most one equipped token in each RADIO, CRYPTO, and POWER
   slot. A second token for an occupied slot reverts; replacement is an explicit
   unequip then equip.
3. A token has at most one `equippedBy` wallet. Because that wallet must also be
   the current owner/node operator, one NFT cannot boost multiple paid nodes.
4. A Standing Badge or revoked module cannot equip.
5. Approving or listing a transferable module does not change ownership, so it
   remains equipped while the listing is merely open. Cancellation likewise
   leaves the loadout unchanged.
6. Any ownership transfer automatically clears the old loadout before the
   ERC-721 transfer completes. Buying therefore unequips the seller as the
   token enters escrow; release gives the buyer an unequipped token that the
   buyer may equip explicitly. Refund returns it unequipped to the seller.
7. Reward settlement reads owner, `equippedToken`, structured effect, and
   `revoked` at the policy-defined proof/block reference. Local cache state is
   advisory and cannot increase payment.

## Revocation and incident behavior

Revocation is an irreversible eligibility quarantine, not metadata editing or
confiscation. `REVOKER_ROLE` supplies a non-zero hash of a public incident
record. The collection clears any loadout and `isMarketplaceEligible` becomes
false, so official new listings and purchases reject the token. Its original
metadata, owner, mint provenance, and revocation event remain auditable.

A revoked transferable module remains directly ERC-721-transferable. This is
deliberate: if a module is revoked after `buy` already placed it in escrow,
`releaseDeal` and `refundDeal` must still move the NFT and AVAX rather than
strand both forever. It cannot equip or enter another official deal. A bad
revoker can censor effects and official liquidity, so the role belongs to a
separate multisig, reason hashes are public, and there is no silent un-revoke.

## Mutability, compromise, and upgrade policy

- **Metadata/effect mutation:** forbidden for an existing token. Corrections
  mint a new definition/version; they never rewrite history.
- **Loadout mutation:** owner-controlled and cleared automatically on transfer.
  It is runtime state, not NFT metadata.
- **Issuer compromise:** a stolen minter can issue only schema-valid, bounded
  effects but can still create illegitimate supply. Pause minting, revoke the
  minter, publish the incident range, and quarantine each bad token. Honest
  existing tokens do not change.
- **Admin compromise:** the attacker can grant roles or pause issuance. The
  multisig, role-event monitoring, role separation, and delayed two-step admin
  transfer reduce but do not eliminate this trust.
- **Revoker compromise:** the attacker can disable effects/official trading but
  cannot seize or rewrite tokens. Recovery requires a new revoker and public
  governance review; irreversible false revocations are compensated by a new
  issuance rather than erasing the event.
- **Contract bugs:** v1 is deliberately non-upgradeable. A replacement deploys
  a new collection with a new schema version and canonical registry entry after
  audit and opt-in migration. No proxy admin can change bytecode under an
  existing address.

## Fuji replacement and legacy migration

The existing Fuji voucher at
`0x3649E46eCD6A0bd187f0046C4C35a7B31C92bA1E` is a **legacy voucher
collection**. Its current source has minter gating, but it lacks the structured
module schema, immutable on-chain metadata, ERC-5192 badges, loadouts, and
revocation semantics. It and any earlier open-mint collection are permanently
ineligible for module effects, gateway licences, Standing Badges, and authentic
module labels.

Migration is a replacement, not an in-place upgrade:

1. Deploy audited `CabalMeshModules` bytecode to Fuji with the production admin,
   minter, and revoker roles; verify source and record address, deployment
   transaction, compiler settings, and runtime bytecode hash.
2. Deploy the updated non-upgradeable Marketplace with the module collection as
   its default allowed collection. The existing Marketplace at
   `0xb6F2B9415fc599130084b7F20B84738aCBB15930` cannot gain the pre-listing
   soulbound/revocation check because it is not a proxy.
3. Publish the new `modules` and `marketplace` addresses together in
   `contracts/deployments/fuji.json`, both application configs/ABIs, and the
   release manifest. Clients enable the feature only when all entries agree.
4. Stop new legacy listings by calling `setCollectionAllowed(legacy, false)` on
   the old marketplace. Existing listings can be cancelled and already-open
   deals continue to release/refund against their recorded collection.
5. Do not wrap, relabel, or automatically copy legacy vouchers. A holder with
   independently valid issuance evidence may receive a fresh token from the
   authorized minter. Its `provenanceHash` commits to
   `keccak256(abi.encode("CABAL_LEGACY_REISSUE_V1", chainId,
   legacyCollection, legacyTokenId, ownerAtSnapshot, issuanceTxHash))`.
6. UI and indexers show old tokens as `LEGACY VOUCHER — NO MODULE EFFECT` and
   keep their historical redemption path separate. Token ID collisions across
   collections are expected and never merged.

No Fuji mutation is performed by this ticket. Deployment spends AVAX and
changes canonical external state, so it is a separately reviewed operation
after the contract/test gates pass.

## Verification

Run:

```bash
cd contracts
npm test
```

The suite covers unauthorized issuance, one-time milestone provenance, role
changes and pause behavior, current-owner enumeration, structured/range
validation, immutable parseable metadata, ERC interface discovery, soulbound
approval and transfers, official marketplace rejection before value moves,
loadout ownership/slot uniqueness, auto-unequip on escrow transfer, quarantine
before purchase, and liveness when revocation occurs after a deal is already
funded. Rust boundary tests additionally reject untrusted schema/effect
combinations and prevent revoked assets from presenting an active effect.
