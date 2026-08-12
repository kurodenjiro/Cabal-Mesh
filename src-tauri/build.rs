/// Declares every command this app exposes over IPC so Tauri generates an
/// `allow-*` / `deny-*` permission for each one.
///
/// Without a manifest, the app's own commands sit outside the ACL entirely.
/// Tauri 2.11.1 closed the hole where IPC from a remote origin bypassed access
/// control when no manifest was configured — but relying on the old behaviour
/// was never safe, and a declared manifest is what makes least privilege
/// expressible at all.
///
/// A generated permission does nothing until a capability references it, so
/// this list and `capabilities/desktop.json` (which now grants the same set
/// as `capabilities/mobile.json` — desktop and mobile share one frontend and
/// one handler in `lib.rs`) must stay in step. Declaring a command here
/// without granting it there makes the command unreachable.
const COMMANDS: &[&str] = &[
    // reshaped surface (src/commands.rs) — registered on every platform
    "unsubscribe",
    "session_status",
    "enter_mesh",
    "mesh_snapshot",
    "subscribe_mesh_log",
    "list_nearby_nodes",
    "ble_status",
    "market_modules",
    "module_purchase_quote",
    "module_deals",
    "buy_module_listing",
    "release_module_deal",
    "request_module_refund",
    "refund_module_deal",
    "module_listing_status",
    "approve_module_listing",
    "create_module_listing",
    "cancel_module_listing",
    "list_intents",
    "intent_form_options",
    "intent_affordability",
    "propose_intent",
    "preview_intent",
    "broadcast_intent",
    "intent_detail",
    "subscribe_settlement_log",
    "settle_intent",
    "cancel_intent",
    "intent_proof",
    "vault_assets",
    "vault_modules",
    "module_loadout",
    "equip_module",
    "unequip_module",
    "vault_identities",
    "vault_keys",
    "profile_summary",
    "set_offline_mode",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
