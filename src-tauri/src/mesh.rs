use futures::StreamExt;
use libp2p::{
    gossipsub, mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Swarm, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "MeshBehaviourEvent")]
pub struct MeshBehaviour {
    pub mdns: mdns::tokio::Behaviour,
    pub gossipsub: gossipsub::Behaviour,
}

#[derive(Debug)]
pub enum MeshBehaviourEvent {
    Mdns(mdns::Event),
    Gossipsub(gossipsub::Event),
}

impl From<mdns::Event> for MeshBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        MeshBehaviourEvent::Mdns(event)
    }
}

impl From<gossipsub::Event> for MeshBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        MeshBehaviourEvent::Gossipsub(event)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyIntent {
    pub intent_type: String,
    pub payload: String,
    pub encrypted: bool,
    pub relay_path: Vec<String>,
    pub relay_fee: Option<String>,
}

pub struct MeshNetwork {
    pub swarm: Swarm<MeshBehaviour>,
    pub topic: gossipsub::IdentTopic,
    /// Real bytes relayed (sent + received) through the mesh topic, shared
    /// with the Tauri command layer so Relay Mode can show actual traffic
    /// instead of a simulated number.
    pub relay_bytes: Arc<AtomicU64>,
}

impl MeshNetwork {
    pub async fn new() -> Result<Self, Box<dyn Error>> {
        // Generate ephemeral keypair for "Nobody" identity
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = local_key.public().to_peer_id();
        
        println!("🔐 Ephemeral PeerID generated: {}", local_peer_id);

        // Configure Gossipsub for Privacy Intent broadcasting
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .message_id_fn(message_id_fn)
            .build()
            .map_err(|msg| std::io::Error::new(std::io::ErrorKind::Other, msg))?;

        let mut gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        // Create the Privacy Intent topic
        let topic = gossipsub::IdentTopic::new("cabalmesh-privacy-intents");
        gossipsub.subscribe(&topic)?;

        // Set up mDNS for local peer discovery (ShadowWire mesh)
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;

        let behaviour = MeshBehaviour { mdns, gossipsub };

        // Build the Swarm with Noise encryption (ShadowWire philosophy)
        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|_| behaviour)?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        Ok(MeshNetwork { swarm, topic, relay_bytes: Arc::new(AtomicU64::new(0)) })
    }

    pub async fn start(
        &mut self, 
        tx: mpsc::UnboundedSender<MeshEvent>,
        mut intent_rx: mpsc::UnboundedReceiver<PrivacyIntent>
    ) -> Result<(), Box<dyn Error>> {
        // Listen on all interfaces (offline-first mesh)
        self.swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

        loop {
            tokio::select! {
                // Handle incoming intents to broadcast
                Some(intent) = intent_rx.recv() => {
                    println!("📤 Broadcasting intent to mesh: {:?}", intent);
                    if let Err(e) = self.broadcast_intent(intent) {
                        eprintln!("❌ Failed to broadcast intent: {}", e);
                    }
                }

                // Handle libp2p swarm events
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            println!("📡 Listening on {}", address);
                            let _ = tx.send(MeshEvent::ListeningStarted { address: address.to_string() });
                        }
                        SwarmEvent::Behaviour(event) => match event {
                            MeshBehaviourEvent::Mdns(mdns::Event::Discovered(list)) => {
                                for (peer_id, multiaddr) in list {
                                    println!("🔍 Peer discovered: {} at {}", peer_id, multiaddr);
                                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                    let _ = tx.send(MeshEvent::PeerDiscovered {
                                        peer_id: peer_id.to_string(),
                                        address: multiaddr.to_string(),
                                    });
                                }
                            }
                            MeshBehaviourEvent::Mdns(mdns::Event::Expired(list)) => {
                                for (peer_id, _) in list {
                                    println!("👻 Peer expired: {}", peer_id);
                                    self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                }
                            }
                            MeshBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. }) => {
                                self.relay_bytes.fetch_add(message.data.len() as u64, Ordering::Relaxed);
                                // Try to parse as PrivacyIntent first
                                let intent: Result<PrivacyIntent, _> = serde_json::from_slice(&message.data);
                                if let Ok(intent) = intent {
                                    // Check if this is a settlement message
                                    if intent.intent_type == "settlement" {
                                        // Parse the payload as JSON to get the actual message type
                                        if let Ok(settlement) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            let msg_type = settlement.get("type").and_then(|v| v.as_str());

                                            if msg_type == Some("SettlementComplete") {
                                                println!("✅ Received Settlement Confirmation: {:?}", settlement);
                                                let _ = tx.send(MeshEvent::SettlementComplete {
                                                    details: settlement.to_string()
                                                });
                                            } else if msg_type == Some("DealAccepted") {
                                                println!("🤝 Received Deal Acceptance: {:?}", settlement);
                                                let _ = tx.send(MeshEvent::DealAccepted {
                                                    details: settlement.to_string()
                                                });
                                            }
                                        }
                                    } else if intent.intent_type == "relay_tx" {
                                        // A peer offline-signed a transaction and needs someone with
                                        // connectivity to broadcast it — never contains a private key,
                                        // only the already-signed raw transaction bytes.
                                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            let queue_id = payload.get("queue_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let raw_tx_hex = payload.get("raw_tx_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let summary = payload.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            println!("📡 Received relay_tx request: {} ({})", queue_id, summary);
                                            let _ = tx.send(MeshEvent::RelayTxReceived { queue_id, raw_tx_hex, summary });
                                        }
                                    } else if intent.intent_type == "relay_confirmed" {
                                        // A relay peer is reporting back the outcome of a relay_tx it submitted.
                                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            let queue_id = payload.get("queue_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("failed").to_string();
                                            let tx_hash = payload.get("tx_hash").and_then(|v| v.as_str()).map(|s| s.to_string());
                                            println!("📨 Received relay_confirmed: {} -> {}", queue_id, status);
                                            let _ = tx.send(MeshEvent::RelayConfirmed { queue_id, status, tx_hash });
                                        }
                                    } else if intent.intent_type == "content_request" {
                                        // A buyer is asking whoever sold this tokenId to deliver the content.
                                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            if let Some(token_id) = payload.get("token_id").and_then(|v| v.as_u64()) {
                                                println!("📨 Received content_request for token #{}", token_id);
                                                let _ = tx.send(MeshEvent::ContentRequested { token_id });
                                            }
                                        }
                                    } else if intent.intent_type == "presence" {
                                        // A peer announcing its real AVAX wallet address (not just its
                                        // ephemeral libp2p PeerID) so the mesh view can show who's who.
                                        // `message.source` comes from gossipsub's signed authenticity,
                                        // not from the payload itself, so it can't be spoofed by the sender.
                                        if let (Some(source), Ok(payload)) = (message.source, serde_json::from_str::<serde_json::Value>(&intent.payload)) {
                                            if let Some(address) = payload.get("address").and_then(|v| v.as_str()) {
                                                let _ = tx.send(MeshEvent::PeerIdentity {
                                                    peer_id: source.to_string(),
                                                    address: address.to_string(),
                                                });
                                            }
                                        }
                                    } else if intent.intent_type == "content_delivery" {
                                        // The seller is delivering the signed content for a purchased tokenId.
                                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            let token_id = payload.get("token_id").and_then(|v| v.as_u64()).unwrap_or(0);
                                            let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let signature = payload.get("signature").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let signer_address = payload.get("signer_address").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            println!("📬 Received content_delivery for token #{}", token_id);
                                            let _ = tx.send(MeshEvent::ContentDelivered { token_id, text, signature, signer_address });
                                        }
                                    } else {
                                        // Regular trade intent
                                        println!("📬 Received Intent: {:?}", intent);
                                        let _ = tx.send(MeshEvent::IntentReceived { intent });
                                    }
                                } else {
                                    // Try to parse as raw settlement confirmation
                                    let settlement: Result<serde_json::Value, _> = serde_json::from_slice(&message.data);
                                    if let Ok(settlement) = settlement {
                                        let msg_type = settlement.get("type").and_then(|v| v.as_str());
                                        
                                        if msg_type == Some("SettlementComplete") {
                                            println!("✅ Received Settlement Confirmation: {:?}", settlement);
                                            let _ = tx.send(MeshEvent::SettlementComplete { 
                                                details: settlement.to_string() 
                                            });
                                        } else if msg_type == Some("DealAccepted") {
                                            println!("🤝 Received Deal Acceptance: {:?}", settlement);
                                            let _ = tx.send(MeshEvent::DealAccepted { 
                                                details: settlement.to_string() 
                                            });
                                        }
                                    }
                                }
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn broadcast_intent(&mut self, intent: PrivacyIntent) -> Result<(), Box<dyn Error>> {
        if !Self::verify_relay_integrity(&intent) {
            return Err("Integrity check failed: Malformed relay path".into());
        }

        let payload = serde_json::to_vec(&intent)?;
        let payload_len = payload.len() as u64;
        match self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), payload)
        {
            Ok(_) => {
                self.relay_bytes.fetch_add(payload_len, Ordering::Relaxed);
                println!("📤 Intent broadcasted to mesh (Relay Hop: {})", intent.relay_path.len());
                Ok(())
            }
            Err(gossipsub::PublishError::InsufficientPeers) => {
                println!("⚠️  Note: No peers connected (Single-Node Mode). Intent processed locally.");
                Ok(())
            }
            Err(e) => Err(Box::new(e))
        }
    }

    pub fn broadcast_raw(&mut self, message: String) -> Result<(), Box<dyn Error>> {
        let payload = message.as_bytes().to_vec();
        match self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), payload) 
        {
            Ok(_) => {
                println!("📤 Raw message broadcasted to mesh");
                Ok(())
            }
            Err(gossipsub::PublishError::InsufficientPeers) => {
                println!("⚠️  Note: No peers connected (Single-Node Mode).");
                Ok(())
            }
            Err(e) => Err(Box::new(e))
        }
    }
    pub fn verify_relay_integrity(intent: &PrivacyIntent) -> bool {
        // Basic integrity check:
        // 1. Relay path should not be empty (should contain at least sender)
        // 2. Relay fee should be formatted correctly (simple check for now)
        
        if intent.relay_path.is_empty() {
            println!("❌ Integrity Check Failed: Empty relay path");
            return false;
        }

        if let Some(fee) = &intent.relay_fee {
            if !fee.contains("AVAX") {
                 println!("⚠️  Warning: Unknown fee format: {}", fee);
                 // We don't fail validation here for now, just warn
            }
        }

        // In a real implementation, we would verify signatures of each hop
        true
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum MeshEvent {
    ListeningStarted { address: String },
    PeerDiscovered { peer_id: String, address: String },
    IntentReceived { intent: PrivacyIntent },
    DealAccepted { details: String },
    SettlementComplete { details: String },
    RelayTxReceived { queue_id: String, raw_tx_hex: String, summary: String },
    RelayConfirmed { queue_id: String, status: String, tx_hash: Option<String> },
    ContentRequested { token_id: u64 },
    ContentDelivered { token_id: u64, text: String, signature: String, signer_address: String },
    /// A peer's real AVAX wallet address, learned from its "presence" broadcast
    /// (not from mDNS discovery, which only knows the ephemeral libp2p PeerID).
    PeerIdentity { peer_id: String, address: String },
}
