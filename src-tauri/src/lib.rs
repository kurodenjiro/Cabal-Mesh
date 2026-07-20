mod app_initializer;
mod mesh;
mod agent;
mod zk_handler;
mod ollama_manager;
mod blockchain_bridge;

use app_initializer::SystemBootstrap;
use mesh::{MeshNetwork, PrivacyIntent};
use agent::{SharkAgent, SharkNegotiation};
use zk_handler::{ZKHandler, ProofRequest, ZKProof};
use ollama_manager::OllamaManager;
use blockchain_bridge::BlockchainBridge;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tauri::{State, Manager, Emitter};

// Global state for mesh network
pub struct AppState {
    pub mesh_tx: Option<mpsc::UnboundedSender<PrivacyIntent>>,
    pub agent: Arc<SharkAgent>,
    pub zk_handler: Arc<ZKHandler>,
    pub ollama: Arc<OllamaManager>,
    pub bridge: Arc<Mutex<BlockchainBridge>>,
}

#[tauri::command]
async fn send_intent_to_mesh(
    payload: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let state_lock = state.lock().await;
    
    // Check if payload is a settlement/deal message (contains "type" field)
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&payload) {
        if json_val.get("type").is_some() {
            // This is a settlement/deal message, send as raw JSON
            println!("📤 Sending settlement message: {}", payload);
            if let Some(tx) = &state_lock.mesh_tx {
                // We need to broadcast this differently - it's not a PrivacyIntent
                // For now, wrap it in a special intent type
                let intent = PrivacyIntent {
                    intent_type: "settlement".to_string(),
                    payload: payload.clone(),
                    encrypted: false,
                    relay_path: vec!["origin_node".to_string()],
                    relay_fee: None, // Settlements don't carry relay fees
                };
                tx.send(intent).map_err(|e| e.to_string())?;
                return Ok(format!("Settlement message broadcasted: {}", payload));
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
async fn negotiate_with_shark(
    intent: String,
    price_ceiling: f64,
    market_price: f64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<SharkNegotiation, String> {
    let state = state.lock().await;
    state
        .agent
        .negotiate(&intent, price_ceiling, market_price)
        .await
        .map_err(|e| e.to_string())
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
) -> Result<u64, String> {
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
async fn get_active_peers(
    _state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<usize, String> {
    Ok(0)
}

#[tauri::command]
async fn generate_new_identity(
    state: State<'_, Arc<Mutex<AppState>>>,
    alias: String, 
    emoji: String, // Accept Emoji
) -> Result<Vec<IdentityView>, String> {
    let state = state.lock().await;
    let mut bridge = state.bridge.lock().await;
    bridge.generate_new_identity(alias, emoji).map_err(|e| e.to_string())
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
                            zk_handler: Arc::new(ZKHandler::new(None)),
                            ollama: ollama_state,
                            bridge: bridge, 
                        }));

                        app_handle.manage(state);
                    }
                    Err(e) => {
                        eprintln!("❌ Bootstrap Failed: {}", e);
                        // Initialize state even on failure
                        let state = Arc::new(Mutex::new(AppState {
                            mesh_tx: None,
                            agent: Arc::new(SharkAgent::new(None)),
                            zk_handler: Arc::new(ZKHandler::new(None)),
                            ollama: ollama_state,
                            bridge: bridge,
                        }));
                        app_handle.manage(state);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_intent_to_mesh,
            negotiate_with_shark,
            generate_zk_proof,
            sync_blockchain_state,
            enable_instant_session,
            create_escrow,
            release_escrow,
            refund_escrow,
            get_escrow_status,
            get_bridge_status,
            get_wallet_snapshot,
            delete_wallet_snapshot,
            app_initializer::kill_switch,
            get_identity,
            get_active_peers,
            generate_new_identity
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
