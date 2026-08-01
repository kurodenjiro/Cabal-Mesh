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

## Android — not yet attempted (ticket 08)

No Rust targets, no SDK, no NDK, and none of `JAVA_HOME` / `ANDROID_HOME` / `NDK_HOME` set. The iOS result is encouraging but does not transfer: Android is where the TLS backend actually matters, since it ships no system OpenSSL. That is why ticket 02 moved to rustls, but it is unproven until an Android build runs.
