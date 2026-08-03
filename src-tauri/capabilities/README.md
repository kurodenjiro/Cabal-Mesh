# Capabilities

Two files, one per platform family. There is deliberately **no shared
`default.json`** — see below.

| File | Applies to | Grants |
|---|---|---|
| `desktop.json` | linux, macOS, windows | `core:default`, `opener:default`, all 50 app commands |
| `mobile.json` | iOS, android | `core:default` **only** — no app command is reachable |

## Why the shared capability file was deleted

Capability files **auto-enable** unless the config names identifiers
explicitly, and a window covered by several capabilities receives the
**union** of their permissions.

So adding platform-specific files while leaving a shared one in place scopes
nothing: mobile would still inherit everything the shared file granted,
`opener:default` included, regardless of any `platforms` key on the new files.
The shared file had to go, not be supplemented.

## Where the app command permissions come from

`build.rs` declares every command over IPC in its `COMMANDS` list, and
`tauri-build` generates an `allow-*` and a `deny-*` permission for each.

**A generated permission does nothing until a capability references it.** The
two are only connected by these files, so `COMMANDS` and the `permissions`
arrays here have to move together:

- Add a command to `COMMANDS` but not here → the command exists and is
  unreachable. Calling it fails at runtime, not at compile time.
- Grant a permission here that `COMMANDS` does not declare → the build fails.

Regenerate the list after changing `COMMANDS`:

```sh
python3 - <<'EOF'
import re, json
cmds = re.findall(r'^\s+"([a-z_0-9]+)",\s*$', open("build.rs").read(), re.M)
print(json.dumps(["allow-" + c.replace("_", "-") for c in cmds], indent=2))
EOF
```

## Two things not granted, on purpose

**`core:default` is not minimal.** It bundles `core:app`, `core:event`,
`core:image`, `core:menu`, `core:path`, `core:resources`, `core:tray`,
`core:webview` and `core:window`. Menu and tray are meaningless on a phone.
Narrowing mobile to an enumerated set is worth doing once the command surface
settles — it is listed here so nobody mistakes `core:default` for least
privilege.

**Rust-only plugins get no grant at all.** The `keystore` and
`multicast-lock` plugins are invoked from Rust through `run_mobile_plugin`,
never over IPC. Granting them to the webview would expose vault key
unwrapping to anything that achieves script execution. Only `type-scale` will
ever need a webview grant.

## Mobile grants nothing, on purpose

`mobile.json` lists `core:default` and nothing else. **No app command is
reachable from the mobile webview.**

An earlier pass granted mobile the same 50 commands as desktop, reasoning
that mobile still serves the desktop frontend so it needs them. That was the
wrong instinct: it hands a surface with *no screens* the full command set,
including private-key export and raw transaction submission, purely to keep a
placeholder UI from looking broken. Convenience during development is not a
reason to widen an authority boundary.

What the mobile build proves today is that the Rust and native graph compiles,
links, launches and renders. That is the whole job of tickets 07 and 08. The
frontend it happens to display is the desktop one, and its IPC-dependent
fields come up empty — which is correct, not a defect.

The surface opens later, per screen:

- Ticket 26 splits the builds so mobile stops serving the desktop frontend.
- Tickets 29–36 add exactly the commands each screen calls, as that screen
  lands.

Never add a permission here ahead of a screen that calls it.
