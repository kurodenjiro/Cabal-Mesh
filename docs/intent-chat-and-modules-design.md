# Chat intents, marketplace, and node modules — design

**Status: implementation in progress.** Written 2026-08-10. The always-available
[local intent inference runtime](intent-inference-runtime.md) was accepted and
proved on desktop, iOS, and Android on 2026-08-12; the conversational UI and
marketplace/module loop remain to be implemented.

Three connected changes: composing intents by conversation instead of by form,
a marketplace for NFTs, and NFTs that measurably improve what a node earns.

## Naming, deliberately

The frozen desktop RPG UI was deleted in 0a71f59 for being off-brand. This
design does **not** reintroduce it under another name. There is no inventory, no
loot, no potions.

The metaphor is **hardware modules slotted into a node** — which is both on-brand
for an instrument/terminal aesthetic and a literal fit for a mesh device.

## The loop

```
   relay traffic for peers  ──>  earn MB  ──>  earn AVAX
            ▲                                     │
            │                                     ▼
     modules raise the rate  <──  buy / sell on MARKET
            ▲                                     │
            └──────── mint at milestones ─────────┘
```

Modules are NFTs on `CabalMeshVoucher` (ERC721, deployed to Fuji). Minting is
gated on an issuer-managed minter set, so "mint at milestones" is a thing an
authority does rather than something any wallet can do for itself. The contract
still has no on-chain slot, rarity or effect, and no soulbound tokens — see the
open questions at the end. Three slots:

| Slot | Example modules | Effect |
|---|---|---|
| RADIO | Relay Amplifier, Range Extender | +% relay yield, more peers counted |
| CRYPTO | Ghost Cloak, Proof Accelerator | +privacy hops, cheaper/faster settlement |
| POWER | Gateway License | earn while acting as an internet gateway |
| — | Standing Badge | **soulbound**, earned by settling, not tradable |

## Chat intent

```
┌──────────────────────────────────────────┐
│  ‹ BACK          NEW INTENT              │
├──────────────────────────────────────────┤
│                                          │
│                   ┌────────────────────┐ │
│                   │ buy 10 avax under  │ │
│                   │ 95, shark mode     │ │
│                   └────────────────────┘ │
│                                          │
│  ┌─────────────────────────────────────┐ │
│  │ ◇ PARSED · LOCAL MODEL              │ │
│  │                                     │ │
│  │  [BUY]  [10 AVAX]  [UNDER $95]      │ │
│  │  [SHARK MODE]  [PRIVACY HIGH]       │ │
│  │                                     │ │
│  │  TAP A CHIP TO CHANGE IT.           │ │
│  └─────────────────────────────────────┘ │
│                                          │
│  ┌─────────────────────────────────────┐ │
│  │ ⚠ BALANCE 8.2 AVAX — SHORT BY 1.8   │ │
│  └─────────────────────────────────────┘ │
│                                          │
├──────────────────────────────────────────┤
│  ┌────────────────────────────────────┐  │
│  │           REVIEW INTENT            │  │
│  └────────────────────────────────────┘  │
│  ┌───────────────────────────────┐ ┌───┐ │
│  │ say what you want to do…      │ │ ▶ │ │
│  └───────────────────────────────┘ └───┘ │
└──────────────────────────────────────────┘
```

Editable chips are what make a wrong parse cheap to fix without falling back to
a full form:

```
        tap [10 AVAX]
              ▼
┌─ AMOUNT ─────────────────────────────────┐
│  ┌───────────────────────┐               │
│  │  10.0          AVAX   │       [ MAX ] │
│  └───────────────────────┘               │
│  AVAILABLE 8.2 AVAX                      │
│                                          │
│  ┌──────────┐  ┌──────────────────────┐  │
│  │  CANCEL  │  │         SET          │  │
│  └──────────┘  └──────────────────────┘  │
└──────────────────────────────────────────┘
```

**The safety property from the current form is preserved exactly.** The model
only produces `IntentFields`. `parse_draft` still validates, `preview_intent`
still builds the review rows from the draft, and broadcasting still re-parses
the same fields. The AI proposes; Rust decides; the user confirms. The model
never broadcasts.

## Marketplace

```
┌──────────────────────────────────────────┐
│  MARKET                          ⌕   ⇅   │
├──────────────────────────────────────────┤
│  [ ALL ] [ RADIO ] [ CRYPTO ] [ POWER ]  │
│                                          │
│  ┌─────────────────────────────────────┐ │
│  │ ▚▚▚  RELAY AMPLIFIER MK-II     RARE │ │
│  │ ▚▚▚  RADIO · +18% RELAY YIELD       │ │
│  │      SELLER  NODE-7F3A…C2   ★ 42    │ │
│  │      2.40 AVAX            [  BUY  ] │ │
│  └─────────────────────────────────────┘ │
│  ┌─────────────────────────────────────┐ │
│  │ ▞▞▞  GHOST CLOAK             COMMON │ │
│  │ ▞▞▞  CRYPTO · +2 HOPS, NO LATENCY   │ │
│  │      SELLER  NODE-91BE…08   ★ 31    │ │
│  │      0.85 AVAX            [  BUY  ] │ │
│  └─────────────────────────────────────┘ │
│  ┌─────────────────────────────────────┐ │
│  │ ▙▙▙  GATEWAY LICENSE         LEGEND │ │
│  │ ▙▙▙  POWER · EARN AS GATEWAY        │ │
│  │      SELLER  NODE-2C4D…AA   ★ 18    │ │
│  │      11.00 AVAX           [  BUY  ] │ │
│  └─────────────────────────────────────┘ │
│                                          │
│  ⓘ ESCROW-BACKED — AVAX LOCKS UNTIL THE  │
│    MODULE ACTUALLY TRANSFERS.            │
└──────────────────────────────────────────┘
```

`★ 42` is the seller's **real** standing — settlement count from
`src/standing.rs`, not an invented score.

`Marketplace.sol` implements the escrow this promises: `buy()` locks AVAX and
pulls the NFT into escrow in one transaction, `releaseDeal` / `refundDeal`
settle it, and `cancelListing` withdraws an unsold listing.

An earlier version of this paragraph said the contract was already good enough
to build against. It was not, and the settlement rules changed under it — worth
reading before designing the deal screens:

- **Release is the default outcome.** The buyer can release at any time; once
  the deal's three-day window passes, anyone can. A buyer who walks away can no
  longer strand the seller's module and payment in the contract forever.
- **Cancelling takes both sides.** The buyer calls `requestRefund`, the seller
  executes `refundDeal`. Neither can unwind a paid deal alone. The old contract
  let the buyer refund themselves at will, which made every listing a free
  option written by the seller.
- **A module has at most one live listing**, and the seller can cancel and
  relist it.

The asset leg of this trade is already on-chain and atomic, so the escrow
window is a *cancellation* window, not a delivery-dispute window. The UI should
say so: there is nothing for a buyer to inspect before releasing, and nothing a
seller still owes.

## Modules — `VAULT → MODULES`

```
┌──────────────────────────────────────────┐
│  VAULT                                   │
│  [ ASSETS ]  [ MODULES ]  [ KEYS ]       │
├──────────────────────────────────────────┤
│  ┌─ NODE LOADOUT ──────────────────────┐ │
│  │                                     │ │
│  │  RADIO    ▚▚▚ RELAY AMP MK-II    ⏻ │ │
│  │  CRYPTO   ▞▞▞ GHOST CLOAK        ⏻ │ │
│  │  POWER    ┈┈┈ EMPTY                 │ │
│  │                                     │ │
│  │  ───────────────────────────────    │ │
│  │  RELAY YIELD     ×1.00  →  ×1.18    │ │
│  │  PRIVACY HOPS        3  →       5   │ │
│  └─────────────────────────────────────┘ │
│                                          │
│  OWNED · 5                    [ MARKET ] │
│  ┌───────────────┐  ┌───────────────┐    │
│  │  ▚▚▚          │  │  ▞▞▞          │    │
│  │  RELAY AMP    │  │  GHOST CLOAK  │    │
│  │  RARE         │  │  COMMON       │    │
│  │  ● EQUIPPED   │  │  ● EQUIPPED   │    │
│  └───────────────┘  └───────────────┘    │
│  ┌───────────────┐  ┌───────────────┐    │
│  │  ▛▛▛          │  │  ▟▟▟          │    │
│  │  PROOF ACC.   │  │  STANDING     │    │
│  │  UNCOMMON     │  │  SOULBOUND    │    │
│  │  [ EQUIP ]    │  │  EARNED       │    │
│  └───────────────┘  └───────────────┘    │
└──────────────────────────────────────────┘
```

### Module detail

```
┌─ RELAY AMPLIFIER MK-II ──────────────────┐
│                                          │
│          ▚▚▚▚▚▚▚▚▚▚▚▚                    │
│          ▚▚▚▚▚▚▚▚▚▚▚▚        RARE        │
│          ▚▚▚▚▚▚▚▚▚▚▚▚                    │
│                                          │
│  SLOT          RADIO                     │
│  EFFECT        +18% RELAY YIELD          │
│  TOKEN ID      #1204                     │
│  CONTRACT      0x…CMV                    │
│  MINTED        2026-07-14                │
│                                          │
│  ─── WHILE EQUIPPED ───────────────────  │
│  RELAYED TODAY       412 MB              │
│  BASE EARNED         0.0081 AVAX         │
│  BONUS FROM THIS     0.0015 AVAX         │
│                                          │
│  ┌────────────┐  ┌────────────────────┐  │
│  │  UNEQUIP   │  │  LIST ON MARKET    │  │
│  └────────────┘  └────────────────────┘  │
└──────────────────────────────────────────┘
```

Showing the bonus this specific module actually produced is what makes owning it
feel real rather than decorative.

### Where earnings are visible — HOME

```
┌─ RELAY YIELD ────────────────────────────┐
│                                          │
│  TODAY           0.0096 AVAX             │
│  ▁▂▄▆█▆▄▂▁▂▄▆█                           │
│                                          │
│  RELAYED         412 MB                  │
│  BASE RATE       ×1.00                   │
│  MODULES         ×1.18    ▚▚▚ ▞▞▞        │
│  ────────────────────────────────────    │
│  EFFECTIVE       ×1.18        [ VAULT ]  │
│                                          │
└──────────────────────────────────────────┘
```

## Navigation

```
BEFORE:  HOME   INTENTS   NODES   VAULT   PROFILE

AFTER:   HOME   INTENTS   MARKET  VAULT   PROFILE
                          ▲              ▲
                          new     + MODULES tab

NODES folds into HOME, which already shows mesh status, node id and uptime.
```

Five tabs is already the limit at 390 px with the brand's tracking; a sixth
would wrap.

## Four things to settle before building

**1. The old relay boost was a local JSON file.**
In git history, `apply_relay_boost` simply added a float to `relay_boost.json`
and saved it. If rewards are real AVAX, the multiplier **must** be derived from
verified on-chain NFT ownership. A locally editable file is not a game
mechanic, it is a mint button.

**2. `CabalMeshVoucher` has no slot, rarity, or effect on-chain.**
It stores `voucherType` (string) and `description`. Options: encode structure
into `voucherType`, add ERC721 `tokenURI` metadata, or extend the contract.
Metadata is the recommended route — standard, and external wallets can read it.

**3. Who pays the relay reward?** *(the load-bearing question)*
If node A relays for node B, where does the AVAX come from — a fee the sender
attaches to the intent, or emission from a treasury? A sender-paid fee is
self-sustaining; emission needs funding and inflates.

**Resolved 2026-08-12:** the sender pre-funds a bounded route escrow; no reward
emission is used. Fee caps, exact integer arithmetic, refunds, gas handling,
finality, solvency, and UI language are fixed in the
[relay reward economics](relay-reward-economics.md). The `0.0096 AVAX` remains
an estimate until ticket 13 produces an accepted on-chain settlement.

**4. Farming defence.**
Two devices owned by one person can relay junk to each other forever. Rewards
need evidence that a genuine third party wanted the data — for example, counting
a relay only when it carries both the sender's signature and the recipient's
receipt. This is the hardest part technically and should be designed early
rather than retrofitted.

**Resolved 2026-08-12:** v1 requires an EIP-712 sender authorization, one signed
contribution per ordered relay, and an acknowledgement from a distinct recipient
wallet. Chain/contract domains, payload and route commitments, expiry, exact
economics, atomic single-use identifiers, complete-intent delivery, and bounded
gateway windows are fixed in the
[genuine relay proof protocol](relay-proof-protocol.md). Same-wallet role reuse
is rejected; distinct wallets controlled by one operator remain an explicit
Sybil limitation rather than a solved identity problem.
