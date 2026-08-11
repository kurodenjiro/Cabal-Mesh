# Genuine relay proof protocol

- **Status:** accepted v1 protocol for Fuji implementation, 2026-08-12
- **Executable verifier:** `src-tauri/crates/cabal-relay-proof`
- **Reward policy:** `cabal-rewards-v1`

This protocol makes one sender-requested route eligible for payment only when
the sender, every ordered relay, and the recipient sign the same payload and
route evidence. It prevents a wallet from occupying multiple roles, prevents
the same signed work from being paid twice, and states the identity attacks it
cannot solve. Ticket 13 still has to reproduce this verifier and its atomic
replay state in the Fuji settlement contract before any displayed reward is
real.

## Identity, domain, and signature rules

Each role is an Avalanche C-Chain wallet address recovered from a 65-byte
secp256k1 ECDSA signature. Signatures must use canonical low-`s` form. The
operator wallet is both the protocol identity and the eventual reward address;
the sender, recipient, and every relay address must be non-zero and pairwise
distinct.

All three messages use [EIP-712](https://eips.ethereum.org/EIPS/eip-712) with
this exact domain:

```text
name              = "CabalMesh Relay Proof"
version           = "1"
chainId           = settlement chain ID (Fuji: 43113)
verifyingContract = deployed settlement contract address
```

The chain and contract fields make a valid Fuji signature invalid on another
chain or contract. EIP-712 does not itself provide replay protection; the
single-use state below is mandatory. The implementation follows the standard
domain/hash/recovery model described by
[OpenZeppelin's cryptography API](https://docs.openzeppelin.com/contracts/5.x/api/utils/cryptography).

## Exact signed messages

EIP-712 field order and integer widths are part of v1. Clients and the contract
must use these canonical type strings verbatim:

```text
RelayAuthorization(bytes32 policyHash,bytes32 routeNonce,bytes32 payloadCommitment,uint8 deliveryMode,bytes32 relayRouteHash,address sender,address recipient,uint64 authorizedBytes,uint8 relayCount,uint64 maximumChargeNavax,uint64 issuedAt,uint64 expiresAt)

RelayContribution(bytes32 authorizationHash,uint8 hopIndex,address relayer,address ingress,address egress,bytes32 payloadCommitment,uint64 deliveredBytes,uint64 forwardedAt)

RecipientAcknowledgement(bytes32 authorizationHash,bytes32 contributionsHash,address recipient,bytes32 payloadCommitment,uint64 deliveredBytes,uint64 receivedAt)
```

`deliveryMode` is signed by the sender:

| Value | Meaning | Completeness rule |
|---:|---|---|
| `0` | `CompletePayload` | A discrete intent is eligible only when `deliveredBytes == authorizedBytes`. |
| `1` | `AcknowledgedByteWindow` | A gateway may settle any non-zero `deliveredBytes <= authorizedBytes`; unused authorization is refunded. |

Any other value is invalid. A sender creates a non-zero, unpredictable
`routeNonce` for each authorization and never reuses it. Authorization lifetime
is 2–30 minutes, with a 10-minute default. `policyHash` is
`keccak256("cabal-rewards-v1")`; `maximumChargeNavax` must exactly equal the
integer quote for `authorizedBytes` and `relayCount`, not merely fall below a
cap.

## Commitments and identifiers

Concatenation below is byte concatenation, not ABI encoding. Counts are one
unsigned byte; addresses are raw 20-byte values; hashes are raw 32-byte values.

```text
payloadCommitment = keccak256(
  "CABAL_PAYLOAD_V1\0" || exact logical ciphertext bytes
)

relayRouteHash = keccak256(
  "CABAL_RELAY_ROUTE_V1\0" || relayCount || relay[0] || ... || relay[n-1]
)

authorizationHash = EIP712SigningHash(domain, RelayAuthorization)
contributionId[i]  = EIP712SigningHash(domain, RelayContribution[i])

contributionsHash = keccak256(
  "CABAL_CONTRIBUTIONS_V1\0" || relayCount ||
  contributionId[0] || ... || contributionId[n-1]
)
```

The payload commitment covers logical application ciphertext once. BLE/Wi-Fi
fragmentation, headers, retries, and duplicate frames do not create additional
billable bytes. `authorizationHash` is also the single-use route ID.

## Three-party flow

1. The sender chooses the recipient, one to three ordered relay wallets,
   delivery mode, payload commitment, byte maximum, reward quote, nonce, and
   expiry. It funds the matching route escrow and signs `RelayAuthorization`.
2. Relay `i` accepts only the authorization at its signed route position. After
   forwarding the payload, it signs `RelayContribution` with the authorization
   hash, its `hopIndex`, its expected ingress and egress wallets, the same
   payload commitment and acknowledged logical byte count, and its forwarding
   time.
3. The recipient validates the payload, ordered route, and every relay
   signature. It signs one `RecipientAcknowledgement` over the authorization
   hash and ordered contribution IDs. For a discrete intent it must not issue a
   receipt until the complete payload is usable. For a gateway window it may
   acknowledge the exact non-zero prefix/window received.
4. A settlement executor submits the complete bundle. Missing acknowledgement
   means zero eligible work; sender or relayer attestations alone cannot earn.

For route `sender -> R0 -> R1 -> recipient`, contribution 0 must name
`sender/R1` as ingress/egress and contribution 1 must name `R0/recipient`.
All contributions must report the same payload and delivered byte count, and
their timestamps must be non-decreasing. The recipient receipt commits to the
ordered contribution IDs, so reordering or substituting one relay invalidates
the receipt.

## Verification and atomic replay state

The verifier rejects the bundle unless all of these hold:

1. Chain, settlement address, active policy, nonce, lifetime, participant, and
   relay-count bounds are valid.
2. Ordered route hash, distinct-wallet rule, exact maximum charge, delivery
   mode, and authoritative payload commitment match.
3. The sender signature recovers the signed sender and the route ID is not in
   `consumedRoutes`.
4. There is exactly one contribution per signed route position. Its route
   adjacency, payload, bytes, time, relayer signature, and unconsumed
   contribution ID all match.
5. A recipient acknowledgement exists and commits to the exact route,
   contribution order, payload, byte count, recipient, and a valid time. Its
   signature must recover the signed recipient.
6. `CompletePayload` delivered every authorized byte. A gateway window
   delivered a non-zero bounded count.

The settlement transaction must atomically mark `consumedRoutes[routeId]` and
every `consumedContributions[id]` before crediting any payout. A revert changes
neither markers nor credits. A second transaction using the same authorization
or any already-paid contribution receives zero additional payment. This state
must be authoritative on-chain; a device-local set is only a preflight cache.

## Multi-relay eligibility and payout

Only the one to three wallets in the signed ordered route are eligible. Every
hop needs its own valid contribution, and the recipient receipt covers all of
them; v1 does not pay a partial multi-hop route with a missing hop. After proof
verification, `cabal-rewards-v1` recomputes work from recipient-acknowledged
logical bytes, divides the base reward equally among the eligible contributions
with integer round-down, applies each independently verified module bonus, and
returns every remainder to the sender. A contribution ID can be credited once,
regardless of how many bundles or executors submit it.

## Common control and remaining Sybil risk

The verifier catches common control visible as the same wallet: sender equals
recipient, sender or recipient equals a relay, or one relay wallet appears more
than once. That stops literal self-relay and repeated route positions.

It cannot prove that distinct wallets or devices have distinct human owners.
One operator can still create several wallets, and colluding sender/recipient
operators can recruit or control another wallet. Bluetooth proximity, IP
address, device identifiers, and signatures are not civil identity and must not
be presented as such. V1 limits damage through sender-funded/no-emission
economics, bounded byte windows, one-to-three paid hops, expiry, exact receipts,
and single-use IDs. Stronger defenses such as stake/slashing, proof of personhood,
trusted hardware attestation, or social-graph reputation are explicitly out of
scope until their privacy and centralization costs are accepted.

## Executable vectors

The primary three-node vector is intentionally language-portable. Private keys
are fixed test data only: sender `0x...01`, relay `0x...02`, recipient
`0x...0a`. It uses chain `43113`, settlement contract
`0x9999999999999999999999999999999999999999`, route nonce `0x42` repeated 32
times, the payload text `cabalmesh encrypted intent payload test vector v1`,
mode `0`, 100,000 authorized/delivered bytes, maximum charge 2,200,000 nAVAX,
issue time `1800000000`, relay time `1800000060`, receipt time `1800000120`, and
expiry `1800000600`.

```text
sender address       0x7e5f4552091a69125d5dfcb7b8c2659029395bdf
relay address        0x2b5ad5c4795c026514f8317c7a215e218dccd6cf
recipient address    0x4cceba2d7d2b4fdce4304d3e09a1fea9fbeb1528
policyHash           0x7d3821fdcb04674be80351b9825999ac97df54c20641a2385b8358417c3fe715
payloadCommitment    0x3d4e45347523184a29acfdeb1d303b18024c39d66b0dadffa328d200537eabde
relayRouteHash       0x2de7af3f987af09739b79eb9552a2a47f16fbe81073c6cbdab153789424634fe
authorizationHash    0xecfc329182f65e5e88e1c7fbb590e7d9211dac8e56d1c80474366c38140a80a1
contributionId       0xffa46f8d9747fc79416946ed100014aa2fb1c85ded231f5905714ac8c2aa2919
contributionsHash    0x4c0dae468e63953940bc7e3ae9336684bbc368b4065762d280fd19d5964d3e04
acknowledgementHash  0xaf71c0faf912001a2fa5a2a0afba72978f84f1977ae3692b76a84451775ade56
```

The unit test asserts every value above before signature recovery and
settlement, so a field-order, prefix, domain, or numeric-width change breaks the
vector.

Run:

```bash
cd src-tauri
cargo test -p cabal-relay-proof
```

The deterministic tests use fixed private keys and cover a valid
sender-relay-recipient route, a valid three-relay route with one payout per hop,
and a partial gateway window. Rejection vectors cover missing receipt; bad
sender, relay, and recipient signatures; altered payload, route, reward, bytes,
and timestamps; incomplete discrete delivery; expired/future/invalid windows;
zero or repeated participant wallets; route and contribution replays; wrong
contribution count; and signatures replayed across a chain or contract domain.
