mod app_initializer;
mod mesh;
mod agent;
mod matcher;
mod zk_handler;
mod ollama_manager;
mod blockchain_bridge;
mod llm_json;

use app_initializer::SystemBootstrap;
use mesh::{MeshNetwork, PrivacyIntent};
use agent::{ContentAnalysis, SharkAgent};
use matcher::{MatchAgent, MatchResult};
use zk_handler::{ZKHandler, ProofRequest, ZKProof};
use ollama_manager::OllamaManager;
use blockchain_bridge::{BlockchainBridge, AssetListingView, VoucherView, TxResult, QueuedTx};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tauri::{State, Manager, Emitter};

// Global state for mesh network
pub struct AppState {
    pub mesh_tx: Option<mpsc::UnboundedSender<PrivacyIntent>>,
    pub agent: Arc<SharkAgent>,
    pub matcher: Arc<MatchAgent>,
    pub zk_handler: Arc<ZKHandler>,
    pub ollama: Arc<OllamaManager>,
    pub bridge: Arc<Mutex<BlockchainBridge>>,
    pub relay_bytes: Arc<AtomicU64>,
}

#[tauri::command]
async fn send_intent_to_mesh(
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
async fn analyze_pdf_content(
    text: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ContentAnalysis, String> {
    let state = state.lock().await;
    state.agent.analyze_content(&text).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn generate_zk_proof(
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
async fn sync_blockchain_state(
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
async fn enable_instant_session(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let mut bridge = state.bridge.lock().await;
    let session = bridge.init_instant_session();
    Ok(format!("Session Created: {}", session.session_id))
}

#[tauri::command]
async fn create_escrow(
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
async fn release_escrow(
    escrow_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.release_escrow(escrow_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn refund_escrow(
    escrow_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.refund_escrow(escrow_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_escrow_status(
    escrow_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<serde_json::Value, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_escrow_status(escrow_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_bridge_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_status())
}

#[tauri::command]
async fn check_rpc_reachable(state: State<'_, Arc<Mutex<AppState>>>) -> Result<bool, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.check_rpc_reachable().await)
}

#[tauri::command]
async fn get_wallet_snapshot(
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
async fn delete_wallet_snapshot(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    // Atomic Reset: Delete snapshot AND identity
    let _ = bridge.delete_snapshot();
    let _ = bridge.delete_identity();
    Ok(())
}

use crate::blockchain_bridge::IdentityView; // Import View

#[tauri::command]
async fn get_identity(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<IdentityView>, String> { // Return full IdentityView objects
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_identity_views().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_primary_private_key(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<String>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_primary_private_key())
}

#[tauri::command]
async fn logout_wallet(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<IdentityView>, String> {
    let state = state.lock().await;
    let mut bridge = state.bridge.lock().await;
    bridge.logout_identity().map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_wallet(
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
async fn mint_voucher(
    voucher_type: String,
    description: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<u64, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.mint_voucher(&voucher_type, &description).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn approve_voucher(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.approve_voucher(token_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_asset_listing(
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
async fn get_active_asset_listings(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<AssetListingView>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_active_asset_listings().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn buy_listing(
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
async fn submit_raw_transaction(
    raw_tx_hex: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.submit_raw_transaction(&raw_tx_hex).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_pending_relay_txs(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<QueuedTx>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_pending_relay_txs())
}

#[tauri::command]
async fn prune_stale_relay_txs(state: State<'_, Arc<Mutex<AppState>>>) -> Result<usize, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.prune_stale_relay_txs().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn mark_relay_tx_status(
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
async fn record_relayed_tx(
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
async fn get_relayed_history(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<blockchain_bridge::RelayedTxRecord>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_relayed_history())
}

#[tauri::command]
async fn get_relay_boost(state: State<'_, Arc<Mutex<AppState>>>) -> Result<f64, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_relay_boost_multiplier())
}

#[tauri::command]
async fn apply_relay_boost(
    additional: f64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<f64, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.apply_relay_boost(additional).map_err(|e| e.to_string())
}

#[tauri::command]
async fn release_deal(
    deal_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.release_deal(deal_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn refund_deal(
    deal_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.refund_deal(deal_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn redeem_voucher(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.redeem_voucher(token_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_voucher_owner(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_voucher_owner(token_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_owned_vouchers(
    owner: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<VoucherView>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_owned_vouchers(&owner).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_my_deals(
    address: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<blockchain_bridge::DealView>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.get_my_deals(&address).await.map_err(|e| e.to_string())
}

/// Real status of the local Ollama model the Shark Agent / matcher depend on —
/// pings its local API rather than assuming it's ready just because it auto-started.
#[tauri::command]
async fn get_ollama_status(
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<bool, String> {
    let state = state.lock().await;
    Ok(state.ollama.health_check().await)
}

#[tauri::command]
async fn extract_pdf_text(
    pdf_bytes: Vec<u8>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.extract_pdf_text(pdf_bytes).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sign_content(
    text: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<blockchain_bridge::ContentRecord, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.sign_content(&text).map_err(|e| e.to_string())
}

#[tauri::command]
async fn store_content(
    token_id: u64,
    record: blockchain_bridge::ContentRecord,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    bridge.store_content(token_id, record).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_content(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<blockchain_bridge::ContentRecord>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_content(token_id))
}

#[tauri::command]
async fn receive_content(
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
async fn get_received_content(
    token_id: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Option<blockchain_bridge::ContentRecord>, String> {
    let state = state.lock().await;
    let bridge = state.bridge.lock().await;
    Ok(bridge.get_received_content(token_id))
}

#[tauri::command]
async fn match_intent_to_listings(
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
async fn get_relay_stats(
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
async fn record_relay_bytes(
    bytes: u64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let state = state.lock().await;
    state.relay_bytes.fetch_add(bytes, Ordering::Relaxed);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Create consistent Ollama instance
            let ollama_manager = Arc::new(OllamaManager::new(Some("llama2".to_string())));
            let ollama_init = ollama_manager.clone();
            
            // Initialize Ollama in background
            tauri::async_runtime::spawn(async move {
                let ollama = ollama_init;
                println!("🔍 Checking Ollama installation...");
                if !ollama.is_installed() {
                    eprintln!("⚠️  Ollama not found!");
                    eprintln!("📝 Please install from: https://ollama.ai");
                    eprintln!("   Or run: brew install ollama");
                } else {
                    match ollama.initialize().await {
                        Ok(_) => {
                            println!("✅ Ollama ready!");
                            for i in 1..=10 {
                                if ollama.health_check().await {
                                    println!("✅ Ollama service is healthy");
                                    break;
                                }
                                if i == 10 {
                                    eprintln!("⚠️  Ollama service not responding");
                                }
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to initialize Ollama: {}", e);
                        }
                    }
                }
            });

            // Pass strong reference to mesh setup to store in AppState
            let ollama_state = ollama_manager.clone();

            // Initialize System via Bootstrap Workflow
            tauri::async_runtime::spawn(async move {
                // Shared Bridge Resource (Created here first)
                // Shared Bridge Resource (Created here first)
                dotenv::dotenv().ok(); // Load .env file
                let rpc_url = std::env::var("AVAX_RPC_URL")
                    .unwrap_or_else(|_| blockchain_bridge::DEFAULT_AVAX_RPC_URL.to_string());

                let bridge = Arc::new(Mutex::new(BlockchainBridge::new(Some(rpc_url))));

                // 1. Phase 1
                SystemBootstrap::phase_1_sync(&bridge, &app_handle).await;

                // 2. Phase 2
                SystemBootstrap::phase_2_delegate(&bridge, &app_handle).await;

                // 3. Phase 3 & Network Start
                match SystemBootstrap::phase_3_network(&app_handle).await {
                    Ok((mut mesh, intent_tx, mut event_rx, intent_rx, event_tx)) => {
                        println!("✅ System Bootstrap Complete. Mesh Swarm Active.");

                        let relay_bytes = mesh.relay_bytes.clone();

                        // Start Mesh Loop (Background)
                        tokio::spawn(async move {
                            if let Err(e) = mesh.start(event_tx, intent_rx).await {
                                eprintln!("Mesh network error: {}", e);
                            }
                        });

                        // Forward Mesh Events to Frontend
                        let handle_clone = app_handle.clone();
                        tokio::spawn(async move {
                            while let Some(event) = event_rx.recv().await {
                                let _ = handle_clone.emit("mesh-event", event);
                            }
                        });

                        // Initialize Global App State
                        let state = Arc::new(Mutex::new(AppState {
                            mesh_tx: Some(intent_tx),
                            agent: Arc::new(SharkAgent::new(None)),
                            matcher: Arc::new(MatchAgent::new(None)),
                            zk_handler: Arc::new(ZKHandler::new(None)),
                            ollama: ollama_state,
                            bridge: bridge,
                            relay_bytes,
                        }));

                        app_handle.manage(state);
                    }
                    Err(e) => {
                        eprintln!("❌ Bootstrap Failed: {}", e);
                        // Initialize state even on failure
                        let state = Arc::new(Mutex::new(AppState {
                            mesh_tx: None,
                            agent: Arc::new(SharkAgent::new(None)),
                            matcher: Arc::new(MatchAgent::new(None)),
                            zk_handler: Arc::new(ZKHandler::new(None)),
                            ollama: ollama_state,
                            bridge: bridge,
                            relay_bytes: Arc::new(AtomicU64::new(0)),
                        }));
                        app_handle.manage(state);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_intent_to_mesh,
            analyze_pdf_content,
            generate_zk_proof,
            sync_blockchain_state,
            enable_instant_session,
            create_escrow,
            release_escrow,
            refund_escrow,
            get_escrow_status,
            get_bridge_status,
            check_rpc_reachable,
            get_wallet_snapshot,
            delete_wallet_snapshot,
            app_initializer::kill_switch,
            get_identity,
            get_primary_private_key,
            logout_wallet,
            import_wallet,
            mint_voucher,
            approve_voucher,
            create_asset_listing,
            get_active_asset_listings,
            buy_listing,
            release_deal,
            refund_deal,
            submit_raw_transaction,
            get_pending_relay_txs,
            prune_stale_relay_txs,
            mark_relay_tx_status,
            record_relayed_tx,
            get_relayed_history,
            get_relay_boost,
            apply_relay_boost,
            redeem_voucher,
            get_voucher_owner,
            get_owned_vouchers,
            get_my_deals,
            get_ollama_status,
            extract_pdf_text,
            sign_content,
            store_content,
            get_content,
            receive_content,
            get_received_content,
            match_intent_to_listings,
            get_relay_stats,
            record_relay_bytes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
