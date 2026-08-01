//! Frozen IPC surface for the desktop UI.
//!
//! # Why this module exists
//!
//! The desktop frontend is frozen — untouched and unmaintained — while every
//! service behind it is being reshaped. Those two facts are in direct
//! conflict: the frozen UI invokes these names with these exact shapes, and
//! the reshaped API changes all of them.
//!
//! So freezing the desktop is a **commitment to maintain a compatibility
//! layer**, not the absence of work. This module is that layer.
//!
//! # Rules
//!
//! - **Signatures are frozen verbatim**, including `Result<T, String>`. When
//!   the new error union lands, it is flattened back to a string at this
//!   boundary so the frozen UI never sees it.
//! - **This is the only place stringly-typed shapes are permitted.** The
//!   `anti-stringly-typed` rule is deliberately suspended here and nowhere
//!   else. Conversions belong in [`adapt`], so the boundary stays one
//!   reviewable file rather than fifty scattered casts.
//! - **Append-only.** This module never gains features. A new capability goes
//!   in the new command surface, not here.
//! - **Desktop only.** Registered under `cfg(all(desktop, feature =
//!   "desktop-legacy"))`; the mobile handler never sees any of it.
//!
//! # Why a module and not a separate crate
//!
//! The plan called for a `cabal-legacy` crate. That is not viable yet: these
//! commands take `State<'_, Arc<Mutex<AppState>>>` and return types that still
//! live in the app crate (`blockchain_bridge`, `matcher`, `agent`,
//! `zk_handler`, `mesh`). A separate crate would either depend on the app crate
//! — a cycle — or need those types extracted first, which is the job of
//! tickets 17 through 24.
//!
//! Extracting the crate becomes mechanical once the services move. Until then
//! a feature-gated module provides the same seam: one place to review, one
//! flag to disable, and no leakage into the new surface.
//!
//! # What guards it
//!
//! `tests/ipc_contract.rs` pins the serialized shape of everything crossing
//! this boundary. If a refactor changes a field name, casing or enum tag, that
//! suite fails — which is the whole reason it was written before this module
//! existed.

use crate::blockchain_bridge::{self, AssetListingView, QueuedTx, TxResult, VoucherView};
use crate::blockchain_bridge::IdentityView;
use crate::agent::ContentAnalysis;
use crate::matcher::MatchResult;
use crate::mesh::PrivacyIntent;
use crate::zk_handler::{ProofRequest, ZKProof};
use crate::{ollama_config, platform, AppState};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

pub mod adapt;

#[tauri::command]
pub async fn send_intent_to_mesh(
    payload: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state_lock = state.lock().await;
    
    // Check if payload is a settlement/deal/relay message (contains "type" field)
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&payload) {
        if let Some(type_field) = json_val.get("type").and_then(|v| v.as_str()) {
            // These get their own outer intent_type so mesh.rs's receive handler can
            // route them without inspecting the inner payload; everything else keeps
            // the existing "settlement" wrapping behavior.
            let intent_type = match type_field {
                "RelayTx" => "relay_tx",
                "RelayConfirmed" => "relay_confirmed",
                "ContentRequest" => "content_request",
                "ContentDelivery" => "content_delivery",
                "Presence" => "presence",
                _ => "settlement",
            };
            println!("📤 Sending {} message: {}", intent_type, payload);
            if let Some(tx) = &state_lock.mesh_tx {
                let intent = PrivacyIntent {
                    intent_type: intent_type.to_string(),
                    payload: payload.clone(),
                    encrypted: false,
                    relay_path: vec!["origin_node".to_string()],
                    relay_fee: None, // Settlements/relay messages don't carry relay fees
                };
                tx.send(intent).map_err(|e| e.to_string())?;
                return Ok(format!("{} message broadcasted: {}", intent_type, payload));
            } else {
                return Err("Mesh network not initialized".to_string());
            }
        }
    }
    
    // Regular intent message
    let intent = PrivacyIntent {
        intent_type: "trade".to_string(),
        payload: payload.clone(),
        encrypted: true,
        relay_path: vec!["origin_node".to_string()], // Initial hop
        relay_fee: Some("0.005 AVAX".to_string()),     // Default fee
    };

    if let Some(tx) = &state_lock.mesh_tx {
        tx.send(intent).map_err(|e| e.to_string())?;
        Ok(format!("Intent broadcasted: {}", payload))
    } else {
        Err("Mesh network not initialized".to_string())
    }
}

#[tauri::command]
pub async fn analyze_pdf_content(
    text: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ContentAnalysis, String> {
    let state = state.lock().await;
    state.agent.analyze_content(&text).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn generate_zk_proof(
    balance: u64,
    bid_amount: u64,
    price_ceiling: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ZKProof, String> {
    println!("🚀 handling generate_zk_proof command");
    let state = state.lock().await;
    let request = ProofRequest {
        balance,
        bid_amount,
        price_ceiling,
    };
    let result = state
        .zk_handler
        .generate_proof(request)
        .await
        .map_err(|e| e.to_string());
    
    match &result {
        Ok(_) => println!("✅ ZK Proof generated successfully"),
        Err(e) => eprintln!("❌ ZK Proof generation failed: {}", e),
    }
    
    result
}

#[tauri::command]
pub async fn sync_blockchain_state(
    wallet: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    match bridge.sync_state(&wallet).await {
        Ok(_) => Ok("Synced".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn enable_instant_session(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let mut bridge = state.bridge.lock().await;
    let session = bridge.init_instant_session();
    Ok(format!("Session Created: {}", session.session_id))
}

#[tauri::command]
pub async fn create_escrow(
    payee: String,
    amount_avax: String,
    expiry_unix: Option<u64>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<TxResult, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    let amount_wei = alloy::primitives::utils::parse_ether(&amount_avax).map_err(|e| e.to_string())?;
    bridge
        .create_escrow(&payee, amount_wei, expiry_unix.unwrap_or(0))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn release_escrow(
    escrow_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.release_escrow(escrow_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refund_escrow(
    escrow_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.refund_escrow(escrow_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_escrow_status(
    escrow_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<serde_json::Value, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_escrow_status(escrow_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_bridge_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_status())
}

#[tauri::command]
pub async fn check_rpc_reachable(state: State<'_, Arc<Mutex<AppState>>>) -> Result<bool, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.check_rpc_reachable().await)
}

#[tauri::command]
pub async fn get_wallet_snapshot(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<serde_json::Value, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    match bridge.get_latest_snapshot() {
        Ok(snapshot) => Ok(serde_json::to_value(snapshot).map_err(|e| e.to_string())?),
        Err(_) => Ok(serde_json::Value::Null), // Return null, not empty object
    }
}

#[tauri::command]
pub async fn delete_wallet_snapshot(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    // Atomic Reset: Delete snapshot AND identity
    let _ = bridge.delete_snapshot();
    let _ = bridge.delete_identity();
    Ok(())
}


#[tauri::command]
pub async fn get_identity(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<IdentityView>, String> { // Return full IdentityView objects
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_identity_views().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_primary_private_key(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<String>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_primary_private_key())
}

#[tauri::command]
pub async fn logout_wallet(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<IdentityView>, String> {
    let state = state.lock().await;
    let mut bridge = state.bridge.lock().await;
    bridge.logout_identity().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_wallet(
    private_key_hex: String,
    alias: String,
    emoji: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<IdentityView>, String> {
    let state = state.lock().await;
    let mut bridge = state.bridge.lock().await;
    bridge.import_identity(private_key_hex, alias, emoji).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mint_voucher(
    voucher_type: String,
    description: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<u64, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.mint_voucher(&voucher_type, &description).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn approve_voucher(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.approve_voucher(token_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_asset_listing(
    description: String,
    price_avax: String,
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<u64, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    let price_wei = alloy::primitives::utils::parse_ether(&price_avax).map_err(|e| e.to_string())?;
    bridge.create_asset_listing(&description, price_wei, token_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_active_asset_listings(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<AssetListingView>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_active_asset_listings().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn buy_listing(
    listing_id: u64,
    price_avax: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<TxResult, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    let price_wei = alloy::primitives::utils::parse_ether(&price_avax).map_err(|e| e.to_string())?;
    bridge.buy_listing(listing_id, price_wei).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn submit_raw_transaction(
    raw_tx_hex: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.submit_raw_transaction(&raw_tx_hex).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_pending_relay_txs(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<QueuedTx>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_pending_relay_txs())
}

#[tauri::command]
pub async fn prune_stale_relay_txs(state: State<'_, Arc<Mutex<AppState>>>) -> Result<usize, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.prune_stale_relay_txs().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_relay_tx_status(
    queue_id: String,
    status: String,
    tx_hash: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.mark_relay_tx_status(&queue_id, &status, tx_hash).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn record_relayed_tx(
    summary: String,
    tx_hash: String,
    reward_avax: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.record_relayed_tx(&summary, &tx_hash, &reward_avax).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_relayed_history(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<blockchain_bridge::RelayedTxRecord>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_relayed_history())
}

#[tauri::command]
pub async fn get_relay_boost(state: State<'_, Arc<Mutex<AppState>>>) -> Result<f64, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_relay_boost_multiplier())
}

#[tauri::command]
pub async fn apply_relay_boost(
    additional: f64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<f64, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.apply_relay_boost(additional).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn release_deal(
    deal_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.release_deal(deal_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refund_deal(
    deal_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.refund_deal(deal_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn redeem_voucher(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.redeem_voucher(token_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_voucher_owner(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_voucher_owner(token_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_owned_vouchers(
    owner: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<VoucherView>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_owned_vouchers(&owner).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_my_deals(
    address: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<blockchain_bridge::DealView>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_my_deals(&address).await.map_err(|e| e.to_string())
}

/// Real status of the Ollama model the Shark Agent / matcher depend on —
/// pings its API rather than assuming it's ready just because it auto-started.
#[tauri::command]
pub async fn get_ollama_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<bool, String> {
    let state = state.lock().await;
    Ok(state.ollama.health_check().await)
}

/// The Ollama server the app is currently pointed at.
#[tauri::command]
pub fn get_ollama_url() -> String {
    ollama_config::url()
}

/// Point the app at a different Ollama server. Required on iOS, which has no
/// local Ollama and cannot spawn one. Pass an empty string to reset to default.
#[tauri::command]
pub fn set_ollama_url(url: String) -> Result<String, String> {
    ollama_config::set_url(&url)
}

/// Whether this build can run helper binaries (`ollama`, `nargo`) locally.
/// False on iOS/Android, where those features need a remote service instead.
#[tauri::command]
pub fn can_spawn_processes() -> bool {
    platform::CAN_SPAWN_PROCESSES
}

#[tauri::command]
pub async fn extract_pdf_text(
    pdf_bytes: Vec<u8>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.extract_pdf_text(pdf_bytes).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sign_content(
    text: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<blockchain_bridge::ContentRecord, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.sign_content(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn store_content(
    token_id: u64,
    record: blockchain_bridge::ContentRecord,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.store_content(token_id, record).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_content(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<blockchain_bridge::ContentRecord>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_content(token_id))
}

#[tauri::command]
pub async fn receive_content(
    token_id: u64,
    text: String,
    signature: String,
    expected_seller: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<bool, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.receive_content(token_id, &text, &signature, &expected_seller).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_received_content(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<blockchain_bridge::ContentRecord>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_received_content(token_id))
}

#[tauri::command]
pub async fn match_intent_to_listings(
    intent: String,
    price_ceiling: f64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<MatchResult>, String> {
    let state = state.lock().await;
    let listings = {
        let bridge = state.bridge.lock().await;
        bridge.get_active_asset_listings().await.map_err(|e| e.to_string())?
    };
    state
        .matcher
        .match_intent(&intent, price_ceiling, &listings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_relay_stats(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<u64, String> {
    let state = state.lock().await;
    Ok(state.relay_bytes.load(Ordering::Relaxed))
}

/// Credits this node's relay-stats counter — called only at the point a
/// transaction is actually relayed on behalf of a peer (see
/// RelayTxReceived handling in App.tsx). Not incremented by ordinary mesh
/// chatter (presence, intent broadcasts, etc.), so it genuinely reflects
/// "bytes relayed for someone else", not just "bytes this node has seen".
#[tauri::command]
pub async fn record_relay_bytes(
    bytes: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    state.relay_bytes.fetch_add(bytes, Ordering::Relaxed);
    Ok(())
}

