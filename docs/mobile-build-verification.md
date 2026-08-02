# Mobile build verification record

Running log of what has actually been proven to build and run, as opposed to what the plans assume. Updated per ticket.

---

## iOS cross-compile — **GO** (2026-08-02, ticket 07)

The question the probe existed to answer: do `alloy` and `libp2p` cross-compile for arm64 iOS? If not, the dependency strategy has to change before any refactoring is worth starting.

**They do.** No patches, no forks, no vendored C.

| Check | Result |
|---|---|
| `cargo build --lib --target aarch64-apple-ios` | ✅ 1m 25s |
| `cargo build --lib --target aarch64-apple-ios-sim` | ✅ 58s |
| `alloy` 1.8.3 full contract/provider/signer stack | ✅ |
| `libp2p` 0.54.1 — tcp, mdns, noise, yamux, gossipsub | ✅ |
| `ring` (rustls backend) | ✅ |
| App bundle builds, installs, launches on iOS 17.5 simulator | ✅ |
| Process alive after launch, renders UI | ✅ |

Verified against the current unrefactored code, on Tauri 2.11.5 with the trimmed dependency set from ticket 02.

**What it does not prove.** The simulator renders the frozen desktop UI, because the mobile UI does not exist yet — panels overlap, text is clipped, the layout is plainly built for a wide window. That is expected and is the whole point of tickets 26–36. This probe answers "can it build and run", not "does it look right".

It also does not prove anything about the App Store: the installed Xcode is 15.4, and Apple has required Xcode 26 with a version-26 SDK for App Store Connect uploads since 2026-04-28. Simulator work is unaffected. See ticket 37.

### Repeatable simulator builds

`tauri ios build --target aarch64-sim` fails on a **second** run with:

```
failed to rename app .../cabalmesh_iOS.xcarchive/Products/Applications/CabalMesh.app:
Directory not empty (os error 66)
```

The message names the source path, but the non-empty directory is the *destination* — `build/arm64-sim/CabalMesh.app` left by the previous run. The Xcode build itself succeeds; only Tauri's post-build move fails, which makes it easy to misread as a compile error.

Use `npm run ios:sim`, which clears the previous output first. `npm run ios:clean-build` on its own if the state needs resetting.

### Install and launch by hand

```bash
npm run ios:sim
xcrun simctl install booted src-tauri/gen/apple/build/arm64-sim/CabalMesh.app
xcrun simctl launch booted com.cabalmesh.app
xcrun simctl io booted screenshot shot.png
```

---

## Security baseline — enforced and proven (2026-08-02, ticket 06)

The project shipped `csp: null` and `withGlobalTauri: true`, with the app's own
commands outside the ACL entirely. All three are now closed.

| Check | Result |
|---|---|
| Explicit CSP configured (`default-src 'self'`, no CDN sources) | ✅ |
| Webview renders identically under CSP — fonts, images, sprites | ✅ |
| `withGlobalTauri: false`; no `window.__TAURI__` usage anywhere in the frontend | ✅ |
| `freezePrototype: true` | ✅ |
| Shared `default.json` deleted; per-platform capabilities with explicit `platforms` | ✅ |
| AppManifest declares all **50** current commands (audit said 47 — the codebase grew) | ✅ |
| Desktop grants all 50; IPC works end to end | ✅ |
| Mobile grants `core:default` only — no app command reachable | ✅ |

`connect-src` is deliberately tight (`'self' ipc:`). The webview makes no
external requests: `src/avalanche-settlement.ts` would need the RPC host
allowlisted, but nothing imports it — it is dead code. Chain calls happen in
Rust, outside the webview's CSP.

### The ACL is genuinely enforced, not decorative

Removing a single permission and rebuilding proves the boundary is live rather
than nominally configured:

| Build | Wallet address in UI |
|---|---|
| `allow-get-identity` granted | `0xC24c...0B2e` shown in balance pill and onboarding chip |
| `allow-get-identity` removed | **absent from both**, everything else renders normally |
| restored | shown again |

One permission removed denied exactly one command and nothing else.

### Mobile grants nothing on purpose

An earlier pass granted mobile all 50 commands, reasoning that mobile still
serves the desktop frontend so it needs them. That was wrong: it hands a
surface with no screens the full command set — private-key export and raw
transaction submission included — so that a placeholder UI does not look
broken. Convenience during development is not a reason to widen an authority
boundary.

The mobile build's job is to prove the graph compiles, links, launches and
renders. IPC-dependent fields coming up empty is correct behaviour, not a
defect. The surface opens per screen from ticket 29 onward.

---

## Frozen IPC contract — baselined (2026-08-02, ticket 09)

`src-tauri/tests/ipc_contract.rs`, 23 snapshots, **0.02s**, no network, no
external binaries, no device.

Shapes rather than live output: most of the 50 commands need a reachable
Avalanche RPC, a running Ollama, the `nargo` binary or a live mesh, so their
runtime output is neither reproducible nor CI-safe. What the frozen UI depends
on is the serialized shape — field names, casing, enum tagging, how
optionality is represented — and that is what is pinned, from fixtures.

Covered: identity and wallet, marketplace and vouchers, deals, transaction
results, the relay queue, content, matching, ZK proofs, all 10 `MeshEvent`
variants, the two hand-built `serde_json::Value` payloads that no type
protects, and the 50-command inventory.

**Verified that it actually catches breakage.** Adding a single
`#[serde(rename)]` to one field failed exactly one snapshot; reverting went
green again. It detects the class of change that otherwise produces
`undefined` in the webview and no Rust error at all.

### Two things the baseline exposed

**Casing is inconsistent across the boundary.** `TxResult::Queued` serializes
`queueId` in camelCase, while its sibling `QueuedTx` uses `raw_tx_hex` and
`tx_hash` in snake_case. Both are now pinned. Whatever the reshaped API
settles on, the compatibility adapter has to keep emitting these exact spellings
for the frozen UI.

**Five modules became `pub`.** `agent`, `blockchain_bridge`, `matcher`, `mesh`
and `zk_handler` were private, which is a fiction: every type in them already
serializes to the webview, so they were public API in everything but the
keyword.

---

## Domain crate — extracted and property-tested (2026-08-02, ticket 10)

`src-tauri/crates/cabal-core`. 29 unit tests + 16 property tests, **0.07s**.

The constraint that makes it worth having: `serde` and `thiserror`, nothing
else. No `tauri`, `tokio`, `reqwest`, `alloy` or `libp2p`. That is why roughly
four thousand generated cases run in fifty milliseconds instead of behind a
multi-minute cross-compile and link. If something in there needs I/O, it
belongs in a different crate.

| Invariant | Why it matters |
|---|---|
| Terminal states never transition | A settled intent that could re-settle is money moving twice |
| Nothing returns to `Draft` | Broadcasting is irreversible |
| Only `Negotiating` may repeat | Every other self-loop is meaningless |
| Every live state can be cancelled | Otherwise the UI shows a cancel button that does nothing |
| Settlement requires routing | Settling from `Broadcast` means settling through a path never found |
| Active and terminal are disjoint | The two predicates drive different affordances |
| Amounts round-trip through display | A value the user typed and the app silently changed is a bug |
| Separators never change value | Users paste back exactly what the UI showed |
| Parsing arbitrary input never panics | Everything from the webview is hostile until parsed |
| Addition overflows rather than wraps | A wrapped total is a plausible-looking wrong balance |
| Mixing assets always fails | Adding AVAX to USDC is a bug, not a saturating op |
| USD always renders two decimals | The brand's number rules are exact, never approximate |

Two bugs the tests caught during writing:

- `NodeId::truncated` guarded on **byte** length while slicing by character,
  so a nine-character CJK identifier (27 bytes) was abbreviated when it should
  have been left whole.
- `prop_assert!` stringifies its expression into a format string, so an inline
  struct literal's braces break compilation. Struct values must be bound to
  locals first.

Verified after extraction: full workspace tests green including the 23 IPC
contract snapshots, clippy clean on the new crate, desktop app builds and
reaches the mesh, and the workspace still cross-compiles for `aarch64-apple-ios`.

---

## Legacy compatibility seam — in place (2026-08-02, ticket 11)

`src-tauri/src/legacy/` holds the 50 frozen commands, gated on
`cfg(all(desktop, feature = "desktop-legacy"))`. Handler registration is split
by surface: desktop gets the legacy arm, mobile gets an empty one.

| Check | Result |
|---|---|
| 50 commands moved with signatures byte-identical | ✅ |
| 23 IPC contract snapshots still green | ✅ |
| Builds with the feature on and off | ✅ |
| Legacy symbols present with gate on (40) / absent with gate off (1) | ✅ |
| Desktop builds, launches, completes bootstrap, reaches the mesh | ✅ |
| iOS build excludes legacy entirely and still launches | ✅ |

### A module, not a crate — and why

The plan called for a `cabal-legacy` crate. Not viable yet: these commands take
`State<'_, Arc<Mutex<AppState>>>` and return types that still live in the app
crate. A separate crate would either depend on the app crate — a cycle — or
need those types extracted first, which is tickets 17–24.

A feature-gated module gives the same seam today: one place to review, one flag
to disable, no leakage into the new surface. Extracting the crate becomes
mechanical once the services move.

### Desktop windows cannot be screenshotted from here

`screencapture` returns only the wallpaper and menu bar for this app. Verified
identical on the pre-ticket-11 baseline by stashing the change and recapturing,
so it is **not a regression** — it is the signature of missing Screen Recording
permission (macOS TCC). Simulator screenshots are unaffected because `simctl`
does not go through that path.

Consequence for every desktop-side ticket: visual verification is unavailable
until Screen Recording is granted to the terminal. Desktop claims here rest on
process liveness, bootstrap logs, the snapshot suite and symbol inspection —
all mechanical, none visual. Where a ticket needs a human eye on the desktop
window, that is called out rather than assumed.

---

## Error taxonomy — typed and redacting (2026-08-02, ticket 12)

`src-tauri/src/error.rs`. `AppError` serializes as a discriminated union tagged
on `kind`, so the frontend switches on a variant and renders its own on-voice
copy. The variant is the contract; the sentence is not.

Before, every command returned `Result<T, String>` built from `e.to_string()`,
which has two costs. The frontend could only display prose — no branching, no
on-voice copy, no localisation. And it leaked: an I/O error's `Display`
contains the filesystem path, a transport error's contains the RPC URL, and
both travelled to the webview.

**Redaction is enforced by test, not by convention.** `no_variant_leaks_infrastructure_detail`
serializes every variant — including one built from an error containing a vault
path, an RPC URL and a hex key — and asserts none of `/Users`, `http`, `://`,
`.network`, `0xdeadbeef` or `vault.enc` survives. That is the test that fails
if someone later adds a `message: String` field "just for debugging".

`AppError::Chain` deliberately has no message field at all, only a `retryable`
flag. `AppError::Internal` is unit-shaped: `AppError::internal(source)` logs
the real error and returns a variant carrying none of it.

37 legacy call sites now flatten through `legacy::adapt::flatten_error` rather
than inline `e.to_string()`, making the compatibility seam real rather than
notional. The 23 frozen-contract snapshots are unchanged by that move, which is
the proof it is behaviour-preserving — the whole requirement for a
compatibility layer.

Test count across the workspace: **84**.

---

## Diagnostics on device — working (2026-08-02, ticket 13)

79 `println!`/`eprintln!` calls became `tracing`. On a desktop terminal those
were merely untidy; on a device they were **invisible** — nothing written to
stdout from an iOS app reaches Console.app.

**Proven on the iOS simulator**, not merely configured:

```
CabalMesh: [com.cabalmesh.app:default] diagnostics initialised  subsystem="com.cabalmesh.app"
CabalMesh: [com.cabalmesh.app:default] Checking connection...  phase="PHASE_1_SYNC" progress=10
CabalMesh: [com.cabalmesh.app:default] ephemeral peer id generated  peer_id=12D3KooWEtgdSP1H…
CabalMesh: [com.cabalmesh.app:default] listening  address=/ip4/192.168.2.111/tcp/59365
```

Read it with:

```sh
xcrun simctl spawn booted log stream --predicate 'subsystem == "com.cabalmesh.app"'
```

| Platform | Destination |
|---|---|
| iOS | unified log — Console.app or `simctl … log stream` |
| macOS | unified log **and** stderr (a Finder-launched bundle has no visible stderr) |
| Android | logcat, `adb logcat -s cabalmesh` — untested until ticket 08 |
| Linux / Windows | stderr |

Severity was preserved rather than flattened: the codebase used emoji as
severity markers, so ❌ became `error`, ⚠️/🚨 became `warn`, bare `eprintln!`
became `warn`, and the rest `info`.

Spans on `sync_state`, `create_escrow` and the mesh event loop mean every line
inside them is attributable:

```
INFO sync_state{wallet_address_override="…" rpc=https://…}: fetching native AVAX balance
```

`skip(self)` keeps the bridge — which holds signers — out of span fields.

Default filter is `cabalmesh=info,…,warn`: libp2p and alloy at debug scroll a
device log faster than it can be read, which is the same as no log.
`RUST_LOG` overrides.

`AppError::internal` now records the full `source()` chain as
`outer: middle: root`, since the root cause is the useful part and is exactly
what `Display` on the outermost error discards. **Logs may contain paths and
URLs; return values may not** — that asymmetry is the design, not an
oversight.

Test count: **85**.

---

## State and capabilities — reshaped (2026-08-02, ticket 14)

The global `Arc<Mutex<AppState>>` is gone. Every command used to lock it, lock
a second mutex inside, then await network I/O holding both: two concurrent RPC
calls ran strictly one after the other. Commands now take a cheap `Services`
snapshot and release the lock before awaiting anything.

**Asserted by timing, not by inspection** — `tests/state_concurrency.rs` runs
8 tasks of 150 ms each and fails if the total approaches the 1,200 ms a
serialized run would take. "No global mutex" is easy to claim from a diff and
easy to lose again in a later refactor.

State is now managed **synchronously**, before the webview exists. Previously it
was managed inside a spawned task, so a command arriving during bootstrap found
nothing managed — and a missing `State<'_, T>` is a panic inside the IPC
handler, not an error a command can convert. It is now `AppError::NotReady`,
which is the state the connecting screen already renders as progress.

`PlatformCaps` (build-time, `Copy`) and `RuntimeCaps` (permissions,
connectivity) are separate types. An earlier design had one struct described as
build-time immutable while carrying a permission grant — a contradiction,
because a user can revoke Local Network access while the app is backgrounded.
Conflating them means that revocation is never noticed and mDNS silently stops
finding peers.

### How state resolution was actually verified

The mock runtime cannot prove it: the ACL runs *before* state resolution and
`mock_context` has no resolved capabilities, so every invoke is refused before a
command body runs. `tests/ipc_wiring.rs` was rewritten to assert what it does
prove — that the ticket 06 ACL is enforced on the real invoke path.

Proof came from running the app instead. Bootstrap and the frontend both call
`sync_state`, distinguishable by argument:

```
sync_state{wallet_address_override="ignored_override"}   <- bootstrap
sync_state{wallet_address_override=""}                   <- frontend, over IPC
```

The second line is a frontend command resolving `State<'_, AppState>` and
executing. That is the regression this ticket was most at risk of.

### A trap worth knowing about

**The debug binary is built with `cfg(dev)`, so it loads `devUrl`, not the
bundled frontend.** Run `target/debug/cabalmesh` without Vite serving port 1420
and the webview loads nothing — bootstrap logs look perfect while *no* frontend
command ever runs.

That cost real time here: it looks exactly like a broken IPC layer or a
regression from the CSP work. Before concluding the frontend is broken, check
that `npm run dev` is running.

### A `cfg` bug the desktop build could not catch

`kill_switch` lives in an always-compiled module but called
`crate::legacy::adapt::flatten_error`, and `legacy` is `cfg(desktop)`-gated. It
compiled on desktop and failed only on `aarch64-apple-ios-sim`. `error::flatten`
is now the unconditional home and `legacy` delegates to it.

This is the argument for cross-compiling every ticket rather than at the end.

Test count: **98**.

---

## Android — not yet attempted (ticket 08)

No Rust targets, no SDK, no NDK, and none of `JAVA_HOME` / `ANDROID_HOME` / `NDK_HOME` set. The iOS result is encouraging but does not transfer: Android is where the TLS backend actually matters, since it ships no system OpenSSL. That is why ticket 02 moved to rustls, but it is unproven until an Android build runs.
