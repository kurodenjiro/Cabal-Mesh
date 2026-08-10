# Identity, unlock, and mesh recovery — design

**Status: proposal. None of this is built yet.** Written 2026-08-10.
Current behaviour is described first so the gap is visible.

## Where things stand

`BlockchainBridge::load_identities` (`src/blockchain_bridge.rs`) already does the
right thing on startup:

```
vault has a key?  ──yes──>  use it, ask nothing
                  ──no───>  generate_new_identity()
                            └─ PrivateKeySigner::random()   (OS CSPRNG — correct)
                            └─ store in AES-256-GCM vault
```

That behaviour is kept. It is deliberately zero-friction: **no seed-phrase
ceremony, no quiz, no onboarding wall.**

What is missing is everything around it:

- **No way to export the key.** `get_primary_private_key` went out with the
  `legacy` module (3e18664) and nothing replaced it. Grep the current command
  surface: there is no export, no import, no backup.
- **No recovery of any kind.** `Vault → KEYS` says so out loud:
  `RECOVERY PHRASE: NONE — NOT BACKED UP`.
- **Vault key is a plain file**, `0o600` (`src/vault_key.rs`), not held by any
  device key store.

Net effect today: **lose or wipe the device and the funds are gone, permanently,
with no recourse.** Anyone funding a current build is exposed to this. Fixing it
is more urgent than any feature in this document.

## Principle

> The user never invents key material. The machine generates it from a CSPRNG.
> The user's job is to *keep* access, not to *create* it.

Anything a human can memorise, a GPU can enumerate. Brain wallets — deriving a
key from personal answers, a memorable phrase, or an LLM conversation — have
been drained by bots for a decade. This is not a tunable trade-off; it is a
prohibition.

Two corollaries:

- **Never put a key or seed into an LLM prompt**, local models included. Prompts
  are logged (Ollama logs them), swapped to disk, and captured in crash dumps. A
  key that has touched a prompt is burned.
- **Never let an LLM generate a mnemonic.** Model output is not cryptographic
  randomness.

## Unlock — without biometrics

Biometrics are not the security boundary. The key store is. Face ID is only a
*gesture* that unlocks it, and it can be swapped for another gesture without
weakening anything. It is also, for a product whose pitch is "you are Nobody",
the most personally-identifying input available — worth avoiding on principle,
not only on preference.

Layered, so no single platform capability is required:

```
Layer 3 (optional):  biometric        ← pure shortcut, can be disabled entirely
Layer 2 (if present): device key store holds half the key   ← "something you have"
Layer 1 (always):     PIN or passphrase → Argon2id          ← "something you know"
```

The vault key is derived from **both halves**. A copied vault file is useless
without the device; a stolen device is useless without the PIN.

Platform constraints that force layer 1 to exist:

| Platform | Hardware-counted retry limit | Consequence |
|---|---|---|
| iOS / Android | yes (Secure Enclave / StrongBox) | a 6-digit PIN is sufficient |
| macOS / Windows | partial (Secure Enclave / TPM) | PIN acceptable |
| **Linux** | no | **passphrase required** — a PIN is brute-forceable once the file is copied |

A 6-digit PIN has only 10⁶ combinations. It is safe *only* when a hardware
counter enforces the retry limit. Counting attempts in software is worthless:
the attacker copies the vault file to another machine where nothing counts.

## Mesh unlock

Optional, additive, opt-in. Uses the mesh itself as the authentication factor —
the one capability no other wallet has.

### Enrollment (once)

```
vault key VK ──Shamir(K=3, N=5)──> 5 shares
                                     │
   each share encrypted to that       │
   guardian's mesh public key         ▼
owner ──[EncShare_i]──BLE──> guardian_i

owner stores:      guardian list, K, N
owner does NOT store: the shares (storing them defeats the mechanism)
guardian stores:   {owner node id, encrypted share, enrolled_at}
```

Shares deliver opportunistically. A guardian out of range simply receives theirs
at the next contact; enrollment is usable as soon as K have landed.

### Unlock

```
owner ──[UnlockReq{nonce, signature}]──BLE──> mesh
                    │
                    ▼
guardian: prompt shown, a human presses approve
                    │
                    ▼
guardian ──[share, re-encrypted to nonce key]──> owner
                    │
    K shares ───────┴──> Shamir reconstruct VK
                         → open vault → sign
                         → wipe VK from memory after T minutes
```

### What this does and does not protect

| Scenario | Outcome |
|---|---|
| Vault file copied to another machine | **Safe.** Shares live on other people's devices. |
| Shared or team wallet | **Works well.** Spending requires several people present. This is the strongest use case. |
| **Device stolen** | **Not protected.** The thief holds the genuine device; guardians see a familiar node and may approve. **A PIN is still required.** |
| K guardians collude, day-to-day unlock | **Safe.** They hold shares of the key but not the encrypted vault, which sits on the owner's device. Two independent compromises are needed. |
| K guardians collude, **recovery** | **Exposed.** Recovery by definition does not need the vault file. Mitigate with a 24–48h delay plus a notification the original device can veto, if it is still alive. |

Mesh unlock is a **layer, never a replacement for the PIN**. Without a PIN
fallback, travelling beyond guardian range bricks the wallet.

## Screens

### Entry point — `VAULT → SECURITY`

```
┌─ SECURITY ───────────────────────────────┐
│                                          │
│  UNLOCK METHOD                           │
│  PIN · 6 DIGITS                 [CHANGE] │
│                                          │
│  ─────────────────────────────────────   │
│                                          │
│  MESH UNLOCK                       [OFF] │
│  REQUIRE NEARBY NODES TO OPEN VAULT      │
│                                          │
│  GUARDIANS                          NONE │
│  0 ENROLLED · NEED 5             [SET UP]│
│                                          │
│  ─────────────────────────────────────   │
│                                          │
│  ADVANCED                             >  │
│  EXPORT KEY · RESTORE · IMPORT           │
│                                          │
└──────────────────────────────────────────┘
```

### Choosing guardians

```
┌─ CHOOSE GUARDIANS ───────────────────────┐
│  PICK 5. ANY 3 CAN UNLOCK.               │
│                                          │
│  ▣  NODE-7F3A…C2       42 RELAYS      ●  │
│  ▣  NODE-91BE…08       31 RELAYS      ●  │
│  ▣  NODE-2C4D…AA       18 RELAYS      ○  │
│  ▢  NODE-55E1…19        9 RELAYS      ●  │
│  ▢  NODE-A03C…7B        4 RELAYS      ○  │
│                                          │
│  ● IN RANGE NOW    ○ SEEN BEFORE         │
│                                          │
│  SELECTED  3 / 5                         │
│  ┌────────────────────────────────────┐  │
│  │             CONTINUE               │  │
│  └────────────────────────────────────┘  │
└──────────────────────────────────────────┘
```

Ordered by relay count, so trust is measured by work actually done together
rather than by an address book.

### Distributing shares

```
┌─ DISTRIBUTING SHARES ────────────────────┐
│                                          │
│  NODE-7F3A…C2  ████████████  DELIVERED   │
│  NODE-91BE…08  ████████████  DELIVERED   │
│  NODE-2C4D…AA  ███████░░░░░  SENDING     │
│  NODE-55E1…19  ░░░░░░░░░░░░  WAITING     │
│  NODE-A03C…7B  ░░░░░░░░░░░░  OUT OF RANGE│
│                                          │
│  3 OF 5 DELIVERED — ENOUGH TO UNLOCK.    │
│  REMAINING SHARES SEND ON NEXT CONTACT.  │
│                                          │
└──────────────────────────────────────────┘
```

### Unlocking — owner side

```
┌─ MESH UNLOCK ────────────────────────────┐
│                                          │
│              ◇   2 / 3                   │
│           NODES APPROVED                 │
│                                          │
│  ● NODE-7F3A…C2     APPROVED     14:02   │
│  ● NODE-91BE…08     APPROVED     14:02   │
│  ◐ NODE-2C4D…AA     ASKING…              │
│  ○ NODE-55E1…19     OUT OF RANGE         │
│                                          │
│  WAITING FOR 1 MORE.                     │
│                                          │
│  ┌────────────────────────────────────┐  │
│  │          USE PIN INSTEAD           │  │
│  └────────────────────────────────────┘  │
└──────────────────────────────────────────┘
```

### Guardian side

```
┌─ UNLOCK REQUEST ─────────────────────────┐
│                                          │
│  NODE-4B12…9F                            │
│  ASKS TO OPEN THEIR VAULT                │
│                                          │
│  ENROLLED     2026-08-02                 │
│  SIGNAL       STRONG  (~3 M)             │
│  LAST SEEN    4 MINUTES AGO              │
│                                          │
│  ⚠ APPROVE ONLY IF YOU CAN SEE THEM.     │
│    YOUR SHARE IS 1 OF 3 NEEDED.          │
│                                          │
│  ┌──────────────┐ ┌───────────────────┐  │
│  │     DENY     │ │      APPROVE      │  │
│  └──────────────┘ └───────────────────┘  │
└──────────────────────────────────────────┘
```

### Restoring on a new device

A fresh device still auto-creates a wallet and goes straight to Home — the
zero-friction rule is not broken for restore's sake. Restore lives under
`SECURITY → ADVANCED`:

```
┌─ RESTORE FROM GUARDIANS ─────────────────┐
│                                          │
│  ⚠ THIS REPLACES THE CURRENT VAULT.      │
│    EXPORT ITS KEY FIRST IF IT HOLDS      │
│    ANYTHING.                             │
│                                          │
│  STAND NEAR 3 OF YOUR GUARDIANS AND      │
│  ASK THEM TO OPEN CABALMESH.             │
│                                          │
│  SCANNING…                               │
│  ● NODE-7F3A…C2      FOUND               │
│  ◐ NODE-91BE…08      HANDSHAKING…        │
│  ○ …                 1 MORE NEEDED       │
│                                          │
│  ┌────────────────────────────────────┐  │
│  │              CANCEL                │  │
│  └────────────────────────────────────┘  │
└──────────────────────────────────────────┘
```

## Where AI belongs

The LLM works on *intent*, never on key material. Signing lives behind a
boundary it cannot cross.

```
   ┌──────────── AI (local Ollama) ────────────┐
   │  1. parse natural language → intent JSON  │
   │  2. recovery assistant (guided Q&A)       │
   │  3. risk warnings before signing          │
   └────────────────┬──────────────────────────┘
                    │ structured JSON only
                    ▼
   ┌──────────── Rust (hard boundary) ─────────┐
   │  validate → draft → REVIEW → auth → SIGN  │
   │  key in the key store; the LLM never      │
   │  reaches this side                        │
   └───────────────────────────────────────────┘
```

Item 2 is the underrated one. Recovery is when the user is most panicked, and
the flow branches too many ways to express as static UI. A conversation handles
it well:

> **User:** I lost my phone.
> **Assistant:** Do you still have a device signed into the same account? If so
> the wallet returns by itself. If not, you enrolled three guardians. You need
> two of them nearby with the app open. Who is closest?

An AI "interview" about wallet history may serve as a *secondary* signal to help
a guardian decide — never as the sole gate, and **never** as an input to key
derivation. That would be a brain wallet again.

Backup prompting should also be driven by risk rather than by an onboarding
checklist. Asking someone to secure an empty wallet gets a Skip; asking after
their first real deposit does not:

> Your wallet now holds real value but is only backed up on this device. Set up
> three guardians?

## Open decisions

1. **Layer-1 factor: 6-digit PIN or passphrase?** PIN is far nicer to use but
   needs a hardware retry counter, which Linux lacks. Passphrase works
   everywhere. Possible answer: PIN where hardware allows, passphrase elsewhere.
2. **Default K/N.** Proposal: 3-of-5, so two absent guardians do not block an
   unlock.
3. **Delay on recovery?** A 24–48h window with a veto notification defends
   against guardian collusion, at the cost of a day's wait after genuinely
   losing a device.
