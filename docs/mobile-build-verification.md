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

## Android — not yet attempted (ticket 08)

No Rust targets, no SDK, no NDK, and none of `JAVA_HOME` / `ANDROID_HOME` / `NDK_HOME` set. The iOS result is encouraging but does not transfer: Android is where the TLS backend actually matters, since it ships no system OpenSSL. That is why ticket 02 moved to rustls, but it is unproven until an Android build runs.
