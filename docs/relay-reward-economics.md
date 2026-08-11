# Relay and gateway reward economics

- **Status:** accepted v1 policy for Fuji implementation, 2026-08-12
- **Executable policy:** `src-tauri/crates/cabal-rewards`
- **Policy identifier:** `cabal-rewards-v1`

This document resolves who funds the AVAX amounts shown for relay and gateway
work. It defines product economics, not an investment return or a promise of
profit. Mainnet constants require a separate review after Fuji measurements.

## Funding decision

Every paid route is **sender-funded**. Before CabalMesh broadcasts a paid
request, the sender deposits its exact maximum charge into a route-specific
AVAX escrow and signs the policy version, payload commitment, authorized bytes,
relay count, maximum charge, and expiry. A relayer never depends on protocol
emission, a module cannot mint AVAX, and one sender's escrow cannot fund another
sender's route.

The same source applies to gateway work: the requester escrows a bounded byte
window before a licensed gateway accepts it. A gateway session longer than the
window opens another authorization. Ticket 03 defines valid proof; ticket 13
implements the first testnet settlement; ticket 16 adds the POWER-license gate.

No reward treasury is required for ordinary use. Any participant may submit a
valid proof and receive the bounded gas reimbursement below. For Fuji bootstrap
only, operations may fund a keeper wallet with 1 AVAX so unattended proofs can
be submitted; that wallet pays gas up front, has no claim on user escrow beyond
the same cap, and stops submitting below 0.05 AVAX. This is an explicit testnet
operations subsidy, not reward emission or a contract liability.

## Exact units and v1 constants

All product arithmetic uses unsigned integer **nAVAX**; 1 AVAX is
1,000,000,000 nAVAX. Contracts convert nAVAX to the C-Chain native 18-decimal
unit by multiplying by 1,000,000,000. No `float`, JavaScript `number`, oracle,
or AVAX/USD conversion participates in a payout.

| Rule | v1 value |
|---|---:|
| Billing quantum | 64 KiB logical delivered bytes |
| Maximum bytes per authorization | 1 GiB |
| Base rate | 25 nAVAX per billed KiB |
| Minimum base route reward | 100,000 nAVAX (0.0001 AVAX) |
| Maximum base route reward | 15,000,000 nAVAX (0.015 AVAX) |
| Maximum paid relay count | 3 |
| Maximum verified module bonus | +10,000 bps (+100%) |
| Maximum route work payout | 30,000,000 nAVAX (0.03 AVAX) |
| Settlement gas reimbursement cap | 2,000,000 nAVAX (0.002 AVAX) |
| Absolute maximum escrow charge | 32,000,000 nAVAX (0.032 AVAX) |
| Proof window | 2–30 minutes; 10-minute default |

The base rate is a CabalMesh v1 policy constant, not the C-Chain gas price. The
C-Chain uses dynamic EIP-1559-style fees; its base fee can fall as low as 1
nAVAX per gas and has no upper bound. That is why sender authorization and gas
reimbursement both have explicit caps rather than assuming a current fee. See Avalanche's
[transaction-fee documentation](https://build.avax.network/docs/rpcs/other/guides/txn-fees).

## Quote and settlement formula

For non-zero authorized logical bytes `A` and relay count `H` in `1..=3`:

```text
billed_bytes = ceil(A / 65,536) × 65,536
raw_base      = (billed_bytes / 1,024) × 25 nAVAX
base_route    = clamp(raw_base, 100,000, 15,000,000 nAVAX)
max_work      = min(base_route × 2, 30,000,000 nAVAX)
max_charge    = max_work + 2,000,000 nAVAX gas reserve
```

The sender sees and authorizes `max_charge` before signing or broadcasting.
The escrowable wallet balance, after reserving the authorization transaction's
current gas quote, must be at least that value; otherwise paid broadcast is
rejected with the exact shortfall. The route maximum does not hide wallet gas:
the authorization screen separately shows its `gasLimit × gasFeeCap` maximum,
which the wallet also signs. The user may explicitly choose an unpaid
best-effort route, but it creates no pending or settled earnings.

After a valid recipient acknowledgement proves `D` logical delivered bytes,
where `0 < D <= A`, the base formula is recomputed with `D`. Retransmission,
fragmentation, duplicate acknowledgements, and transport headers do not increase
`D`. For `H` eligible relays:

```text
base_share[i] = floor(delivered_base_route / H)
payout[i]     = floor(base_share[i] × (10,000 + verified_bonus_bps[i]) / 10,000)
gas_paid      = min(protocol_metered_gas_cost, 2,000,000 nAVAX)
sender_refund = max_charge - sum(payout[i]) - gas_paid
```

Every division rounds down in the sender's favor. The unallocated route
remainder, unused module headroom, unused byte authorization, and unused gas
reserve all return to the sender. A bonus above +100% is invalid rather than
clamped. Multiple eligible relays split base work equally; a route cannot pay
the same contribution twice.

For the Solidity implementation, `protocol_metered_gas_cost` is the transaction
entry-to-accounting gas delta plus a versioned 50,000-gas overhead for base
transaction, calldata, final storage writes, and events, multiplied by the
transaction's effective gas price. The wei result rounds up to nAVAX. Ticket 13
must measure and test that overhead against the deployed contract; changing it
requires a new policy version.

## Numeric examples

### One 4-KiB intent, one relay, no module

- 4 KiB rounds to 64 KiB; the calculated 1,600 nAVAX uses the 100,000 nAVAX
  minimum.
- Maximum work is 200,000 nAVAX; maximum charge including gas is 2,200,000
  nAVAX (0.0022 AVAX).
- With protocol-metered gas of 1,400,000 nAVAX, the relayer receives 100,000,
  the executor receives 1,400,000, and 700,000 nAVAX returns to the sender.
- A 2,199,999 nAVAX available balance is insufficient by exactly 1 nAVAX, so
  no paid request is signed or broadcast.

### 412 MiB delivered, one relay, verified +18% RADIO module

- Base work is `421,888 KiB × 25` = 10,547,200 nAVAX.
- The +1,800-bps payout is 12,445,696 nAVAX (0.012445696 AVAX).
- Maximum charge is 23,094,400 nAVAX. With 1,250,000 nAVAX metered gas,
  9,398,704 nAVAX returns to the sender.
- If only 64 MiB of the 412-MiB authorization is acknowledged, base payout is
  1,638,400 nAVAX and all unused byte headroom returns to the sender.

### Three relays and rounding

A minimum 100,000-nAVAX route with three zero-bonus relays pays 33,333 nAVAX
to each. The remaining 1 nAVAX is never assigned by route order; it is part of
the sender refund.

### Gas above the reserve

If metered gas is 3,000,000 nAVAX on the 4-KiB example, reimbursement remains
2,000,000 nAVAX, the relayer still receives 100,000, and 100,000 returns to the
sender. An executor that submits bears the uncovered 1,000,000 nAVAX. The app
must preflight the transaction and normally leave it pending until gas falls or
a subsidized keeper explicitly accepts the excess. Sender spend never exceeds
the signed maximum.

## Lifecycle, failure, and finality

| Case | Work payout | Sender funds | State/result |
|---|---:|---:|---|
| Valid complete route and accepted settlement | Each eligible relay exactly once | Actual work + capped gas; remainder refunded | Settled |
| Gateway delivers fewer acknowledged bytes than authorized | Recomputed from acknowledged logical bytes | Actual work + capped gas; unused byte headroom refunded | Settled for acknowledged window |
| Missing recipient acknowledgement or partial discrete intent | 0 | Full escrow refunded after expiry | Failed/expired |
| Invalid signature, altered payload, common-control violation, or ineligible route | 0 | Escrow remains active until valid proof or expiry | Rejected proof |
| Duplicate or replayed proof | 0 additional | Original settlement is unchanged | Rejected duplicate |
| Proof submitted after expiry | 0 | Full escrow refundable | Expired |
| Settlement transaction reverts or is dropped | 0 state change | Escrow stays pending | Retry or expire |
| Sender asks to cancel after escrow acceptance | 0 immediate | Escrow remains active | Refused; wait for proof or expiry |

Receipt signatures are off-chain and cost no gas. The sender pays the escrow
authorization transaction. A proof executor pays settlement gas up front and
receives at most the signed reserve. The sender pays an expiry transaction;
relayers and the sender each pay their own later withdrawal transaction.

A paid authorization uses a sender-selected 2–30 minute proof window, default
10 minutes. It cannot be cancelled after the escrow transaction is accepted:
otherwise the sender could race a relayer that has already performed the work.
The user may abandon the quote before funding it. At expiry, any caller may
close the route, but that caller pays expiry gas and receives no work reward.

Settlement uses pull-payment credits: the proof transaction consumes proof IDs,
closes the route, credits relayers/executor, and credits the sender refund before
any withdrawal. UI changes **pending** to **settled** only when that transaction
is accepted on C-Chain. Avalanche documents accepted C-Chain state as finalized
and irreversible, so no invented multi-confirmation window is added. See the
[exchange integration finality guidance](https://build.avax.network/docs/primary-network/exchange-integration)
and the accepted-transaction
[RPC subscription](https://build.avax.network/docs/rpcs/c-chain).

## Solvency invariants

The contract must maintain:

```text
sum(active route maximum charges) + sum(withdrawable credits)
    <= contract AVAX balance
```

Creating a route increases both escrow balance and that route's liability by
the same exact amount. Settlement or expiry converts one active liability into
credits whose sum is no greater; withdrawal decreases a credit and balance by
the same amount. State is updated before transfer, each contribution ID is
single-use, and no admin or keeper may withdraw active escrow. Forced or donated
AVAX is excess, never silently assigned as rewards.

This remains solvent under ordinary use because every maximum liability is
pre-funded and bounded. The optional Fuji keeper needs operating gas but cannot
create a reward liability or spend user escrow beyond a route's signed reserve.

## User-facing accounting language

- **ESTIMATED EARNINGS** — a local quote based on unproved bytes and possible
  module eligibility; never included in an earnings total.
- **PENDING EARNINGS** — a complete proof or settlement transaction is awaiting
  acceptance; shown separately and not spendable.
- **SETTLED EARNINGS** — an accepted settlement credited the node on-chain;
  include the transaction hash and distinguish credited from withdrawn.
- **UP TO … AVAX RELAY CHARGE** — the signed escrow maximum shown to the sender,
  with `WORK`, `SETTLEMENT GAS RESERVE`, and `WALLET GAS SEPARATE` rows.
- **REFUND CREDITED** — unused escrow is claimable; do not say “refunded to
  wallet” until its withdrawal transaction is accepted.

The existing `reward_avax` relay history remains an estimate until ticket 13
replaces it with an accepted settlement reference. Estimated and pending values
must never be added to the settled figure shown on HOME or module detail.
