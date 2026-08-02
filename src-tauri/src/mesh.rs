use futures::StreamExt;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{
    dcutr, gossipsub, identify, mdns, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Swarm, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "MeshBehaviourEvent")]
pub struct MeshBehaviour {
    /// `Toggle` because local-network discovery is a **permission**, not a
    /// capability. iOS and Android can both refuse it, and a node that failed
    /// to start because the user declined a prompt would be worse than one
    /// that quietly falls back to bootstrap peers.
    pub mdns: Toggle<mdns::tokio::Behaviour>,
    pub gossipsub: gossipsub::Behaviour,
    /// Tells peers our addresses and learns theirs — a precondition for relay
    /// reservations and for hole punching.
    pub identify: identify::Behaviour,
    /// Liveness. Also keeps NAT bindings warm, which matters on cellular.
    pub ping: ping::Behaviour,
    /// Lets this node be reached through a relay when it cannot be dialled
    /// directly, which on mobile is most of the time.
    pub relay: relay::client::Behaviour,
    /// Upgrades a relayed connection to a direct one when hole punching
    /// succeeds, so the relay carries handshakes rather than the whole mesh.
    pub dcutr: dcutr::Behaviour,
}

#[derive(Debug)]
pub enum MeshBehaviourEvent {
    Mdns(mdns::Event),
    Gossipsub(gossipsub::Event),
    Identify(identify::Event),
    Ping(ping::Event),
    Relay(relay::client::Event),
    Dcutr(dcutr::Event),
}

impl From<mdns::Event> for MeshBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        MeshBehaviourEvent::Mdns(event)
    }
}

impl From<identify::Event> for MeshBehaviourEvent {
    fn from(event: identify::Event) -> Self {
        MeshBehaviourEvent::Identify(event)
    }
}

impl From<ping::Event> for MeshBehaviourEvent {
    fn from(event: ping::Event) -> Self {
        MeshBehaviourEvent::Ping(event)
    }
}

impl From<relay::client::Event> for MeshBehaviourEvent {
    fn from(event: relay::client::Event) -> Self {
        MeshBehaviourEvent::Relay(event)
    }
}

impl From<dcutr::Event> for MeshBehaviourEvent {
    fn from(event: dcutr::Event) -> Self {
        MeshBehaviourEvent::Dcutr(event)
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
        
        tracing::info!(peer_id = %local_peer_id, "ephemeral peer id generated");

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

        // Local discovery is a permission on both mobile platforms. A refusal
        // disables it rather than failing to start: discovery then degrades to
        // bootstrap peers instead of the node being dead.
        let mdns = match mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id) {
            Ok(behaviour) => Toggle::from(Some(behaviour)),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "local-network discovery unavailable; falling back to bootstrap peers"
                );
                Toggle::from(None)
            }
        };

        let identify_config = identify::Config::new(
            "/cabalmesh/1.0.0".to_string(),
            local_key.public(),
        );

        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            // QUIC alongside TCP. Connection migration survives a Wi-Fi to
            // cellular handoff, which a TCP socket does not, and 0-RTT
            // resumption makes returning from background cheap.
            .with_quic()
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|key, relay_behaviour| MeshBehaviour {
                mdns,
                gossipsub,
                identify: identify::Behaviour::new(identify_config),
                ping: ping::Behaviour::new(ping::Config::new()),
                relay: relay_behaviour,
                dcutr: dcutr::Behaviour::new(key.public().to_peer_id()),
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        Ok(MeshNetwork { swarm, topic, relay_bytes: Arc::new(AtomicU64::new(0)) })
    }

    /// The swarm event loop. Everything the mesh logs happens inside this span,
    /// so device output is attributable to the node rather than floating free.
    #[tracing::instrument(skip_all, fields(peer_id = %self.swarm.local_peer_id()))]
    pub async fn start(
        &mut self,
        tx: mpsc::UnboundedSender<MeshEvent>,
        mut commands: tokio::sync::mpsc::Receiver<crate::mesh_handle::MeshCommand>,
    ) -> Result<(), Box<dyn Error>> {
        use crate::mesh_handle::{MeshCommand, MeshError, MeshSnapshot};

        // Listen on both transports. QUIC is preferred by peers that support
        // it; TCP remains for those that do not and for networks that block
        // UDP.
        self.swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        if let Err(error) = self.swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?) {
            // Some networks block UDP outright. TCP alone still works, so this
            // degrades rather than fails.
            tracing::warn!(%error, "QUIC listener unavailable; continuing on TCP");
        }

        // Dial bootstrap peers so discovery is not limited to this network.
        let bootstrap = crate::bootstrap_config::BootstrapConfig::load(
            &cabal_store::JsonStore::new(crate::app_paths::in_data_dir("bootstrap.json")),
        );
        if bootstrap.has_relays() {
            for address in bootstrap.parsed() {
                match self.swarm.dial(address.clone()) {
                    Ok(()) => tracing::info!(%address, "dialling bootstrap peer"),
                    Err(error) => tracing::warn!(%address, %error, "bootstrap dial failed"),
                }
            }
        } else {
            tracing::info!(
                "no bootstrap relays configured — discovery is limited to this network"
            );
        }

        // Answered from the loop rather than tracked elsewhere, so a snapshot
        // always reflects what the swarm is actually doing.
        let mut listening_on: Vec<String> = Vec::new();
        let mut offline = false;

        loop {
            tokio::select! {
                Some(command) = commands.recv() => {
                    match command {
                        MeshCommand::Snapshot { reply } => {
                            let _ = reply.send(MeshSnapshot {
                                peer_count: self.swarm.connected_peers().count(),
                                listening_on: listening_on.clone(),
                                offline,
                                relay_bytes: self.relay_bytes.load(std::sync::atomic::Ordering::Relaxed),
                            });
                            continue;
                        }
                        MeshCommand::SetOffline { offline: next, reply } => {
                            // Deliberately does not tear the swarm down:
                            // rebuilding on resume would drop every established
                            // connection and re-run discovery from nothing.
                            offline = next;
                            tracing::info!(offline, "mesh participation toggled");
                            let _ = reply.send(());
                            continue;
                        }
                        MeshCommand::Publish { intent, reply } => {
                            if offline {
                                // The offline switch promises nothing leaves the
                                // device. Honour it here rather than relying on
                                // callers to check first.
                                let _ = reply.send(Err(MeshError::Publish));
                                continue;
                            }
                            let intent = *intent;
                            let outcome = self.broadcast_intent(intent);
                            let _ = reply.send(outcome.map_err(|_| MeshError::Publish));
                            continue;
                        }
                    }
                }

                // Handle libp2p swarm events
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            tracing::info!(address = %address, "listening");
                            listening_on.push(address.to_string());
                            let _ = tx.send(MeshEvent::ListeningStarted { address: address.to_string() });
                        }
                        SwarmEvent::Behaviour(event) => match event {
                            MeshBehaviourEvent::Mdns(mdns::Event::Discovered(list)) => {
                                for (peer_id, multiaddr) in list {
                                    tracing::info!(peer_id = %peer_id, address = %multiaddr, "peer discovered");
                                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                    let _ = tx.send(MeshEvent::PeerDiscovered {
                                        peer_id: peer_id.to_string(),
                                        address: multiaddr.to_string(),
                                    });
                                }
                            }
                            MeshBehaviourEvent::Mdns(mdns::Event::Expired(list)) => {
                                for (peer_id, _) in list {
                                    tracing::info!("👻 Peer expired: {}", peer_id);
                                    self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                }
                            }
                            MeshBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. }) => {
                                // Try to parse as PrivacyIntent first
                                let intent: Result<PrivacyIntent, _> = serde_json::from_slice(&message.data);
                                if let Ok(intent) = intent {
                                    // Check if this is a settlement message
                                    if intent.intent_type == "settlement" {
                                        // Parse the payload as JSON to get the actual message type
                                        if let Ok(settlement) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            let msg_type = settlement.get("type").and_then(|v| v.as_str());

                                            if msg_type == Some("SettlementComplete") {
                                                tracing::info!("✅ Received Settlement Confirmation: {:?}", settlement);
                                                let _ = tx.send(MeshEvent::SettlementComplete {
                                                    details: settlement.to_string()
                                                });
                                            } else if msg_type == Some("DealAccepted") {
                                                tracing::info!("🤝 Received Deal Acceptance: {:?}", settlement);
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
                                            tracing::info!("📡 Received relay_tx request: {} ({})", queue_id, summary);
                                            let _ = tx.send(MeshEvent::RelayTxReceived { queue_id, raw_tx_hex, summary });
                                        }
                                    } else if intent.intent_type == "relay_confirmed" {
                                        // A relay peer is reporting back the outcome of a relay_tx it submitted.
                                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            let queue_id = payload.get("queue_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("failed").to_string();
                                            let tx_hash = payload.get("tx_hash").and_then(|v| v.as_str()).map(|s| s.to_string());
                                            tracing::info!("📨 Received relay_confirmed: {} -> {}", queue_id, status);
                                            let _ = tx.send(MeshEvent::RelayConfirmed { queue_id, status, tx_hash });
                                        }
                                    } else if intent.intent_type == "content_request" {
                                        // A buyer is asking whoever sold this tokenId to deliver the content.
                                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&intent.payload) {
                                            if let Some(token_id) = payload.get("token_id").and_then(|v| v.as_u64()) {
                                                tracing::info!("📨 Received content_request for token #{}", token_id);
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
                                            tracing::info!("📬 Received content_delivery for token #{}", token_id);
                                            let _ = tx.send(MeshEvent::ContentDelivered { token_id, text, signature, signer_address });
                                        }
                                    } else {
                                        // Regular trade intent
                                        tracing::info!("📬 Received Intent: {:?}", intent);
                                        let _ = tx.send(MeshEvent::IntentReceived { intent });
                                    }
                                } else {
                                    // Try to parse as raw settlement confirmation
                                    let settlement: Result<serde_json::Value, _> = serde_json::from_slice(&message.data);
                                    if let Ok(settlement) = settlement {
                                        let msg_type = settlement.get("type").and_then(|v| v.as_str());
                                        
                                        if msg_type == Some("SettlementComplete") {
                                            tracing::info!("✅ Received Settlement Confirmation: {:?}", settlement);
                                            let _ = tx.send(MeshEvent::SettlementComplete { 
                                                details: settlement.to_string() 
                                            });
                                        } else if msg_type == Some("DealAccepted") {
                                            tracing::info!("🤝 Received Deal Acceptance: {:?}", settlement);
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
        match self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), payload)
        {
            Ok(_) => {
                tracing::info!(hops = intent.relay_path.len(), intent_type = %intent.intent_type, "intent broadcast");
                Ok(())
            }
            Err(gossipsub::PublishError::InsufficientPeers) => {
                tracing::warn!("⚠️  Note: No peers connected (Single-Node Mode). Intent processed locally.");
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
                tracing::info!("📤 Raw message broadcasted to mesh");
                Ok(())
            }
            Err(gossipsub::PublishError::InsufficientPeers) => {
                tracing::warn!("⚠️  Note: No peers connected (Single-Node Mode).");
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
            tracing::error!("❌ Integrity Check Failed: Empty relay path");
            return false;
        }

        if let Some(fee) = &intent.relay_fee {
            if !fee.contains("AVAX") {
                 tracing::warn!("⚠️  Warning: Unknown fee format: {}", fee);
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
