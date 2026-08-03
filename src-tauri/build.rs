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
/// this list and `capabilities/desktop.json` must stay in step. Declaring a
/// command here without granting it there makes the command unreachable.
///
/// These are the commands that exist **today**, which the frozen desktop UI
/// calls. The reshaped screen commands get added as they are registered, not
/// speculatively.
const COMMANDS: &[&str] = &[
    // reshaped surface (src/commands.rs) — registered on every platform
    "unsubscribe",
    "session_status",
    "enter_mesh",
    "mesh_snapshot",
    "subscribe_mesh_log",
    "list_nearby_nodes",
    "list_intents",
    "intent_form_options",
    "preview_intent",
    "vault_assets",
    "vault_identities",
    "vault_keys",
    "profile_summary",
    "set_offline_mode",
    // mesh
    "send_intent_to_mesh",
    "get_relay_stats",
    "record_relay_bytes",
    // bootstrap / session
    "kill_switch",
    "enable_instant_session",
    "sync_blockchain_state",
    "get_bridge_status",
    "check_rpc_reachable",
    // identity / wallet
    "get_identity",
    "get_primary_private_key",
    "logout_wallet",
    "import_wallet",
    "get_wallet_snapshot",
    "delete_wallet_snapshot",
    // escrow
    "create_escrow",
    "release_escrow",
    "refund_escrow",
    "get_escrow_status",
    "release_deal",
    "refund_deal",
    "get_my_deals",
    // marketplace / vouchers
    "mint_voucher",
    "approve_voucher",
    "redeem_voucher",
    "get_voucher_owner",
    "get_owned_vouchers",
    "create_asset_listing",
    "get_active_asset_listings",
    "buy_listing",
    // relaying
    "submit_raw_transaction",
    "get_pending_relay_txs",
    "prune_stale_relay_txs",
    "mark_relay_tx_status",
    "record_relayed_tx",
    "get_relayed_history",
    "get_relay_boost",
    "apply_relay_boost",
    // content
    "sign_content",
    "store_content",
    "get_content",
    "receive_content",
    "get_received_content",
    "extract_pdf_text",
    "analyze_pdf_content",
    // AI / matching
    "get_ollama_status",
    "get_ollama_url",
    "set_ollama_url",
    "can_spawn_processes",
    "match_intent_to_listings",
    // zk
    "generate_zk_proof",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
