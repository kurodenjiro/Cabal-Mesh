use serde::{Deserialize, Serialize};
use cabal_store::JsonStore;
use cabal_vault::{Secret, Vault};
use std::fs;
use std::path::PathBuf;
use std::error::Error;
use std::str::FromStr;
use chrono::{DateTime, Utc};
// Crypto Imports
use alloy::{
    consensus::{Transaction as _, TxEnvelope},
    eips::{eip2718::{Decodable2718, Encodable2718}, BlockId},
    network::{EthereumWallet, TransactionBuilder},
    primitives::{keccak256, Address, Bytes, Signature, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::{local::PrivateKeySigner, SignerSync},
    sol,
};
use aes_gcm::aead::{OsRng, rand_core::RngCore};
use tokio::time::{timeout, Duration};

pub const DEFAULT_AVAX_RPC_URL: &str = "https://api.avax-test.network/ext/bc/C/rpc";

sol! {
    #[sol(rpc)]
    IEscrow,
    "abi/Escrow.abi.json"
}

sol! {
    #[sol(rpc)]
    IMarketplace,
    "abi/Marketplace.abi.json"
}

sol! {
    #[sol(rpc)]
    IVoucher,
    "abi/CabalMeshVoucher.abi.json"
}

sol! {
    #[sol(rpc)]
    interface IModules {
        function balanceOf(address owner) external view returns (uint256 balance);
        function tokenOfOwnerByIndex(address owner, uint256 index)
            external
            view
            returns (uint256 tokenId);
        function equippedToken(address operator, uint8 slot)
            external
            view
            returns (uint256 tokenId);
        function equippedBy(uint256 tokenId) external view returns (address operator);
        function ownerOf(uint256 tokenId) external view returns (address owner);
        function locked(uint256 tokenId) external view returns (bool isLocked);
        function revoked(uint256 tokenId) external view returns (bool isRevoked);
        function equip(uint256 tokenId) external;
        function unequip(uint256 tokenId) external;
        function assetData(uint256 tokenId)
            external
            view
            returns (
                bytes32 moduleId,
                bytes32 provenanceHash,
                string displayName,
                uint8 assetClass,
                uint8 slot,
                uint8 rarity,
                uint8 effectType,
                uint32 primaryEffectValue,
                uint32 secondaryEffectValue,
                string artworkUri,
                bytes32 artworkDigest,
                uint16 schemaVersion,
                address mintedBy
            );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdentityRecord {
    pub alias: String,
    pub emoji: String,
    /// 0x-prefixed secp256k1 private key.
    ///
    /// `Secret`, not `String`: this struct derives `Debug`, and any `{:?}` of
    /// it — a `dbg!`, a tracing field, an error formatting its context — would
    /// otherwise print the wallet. Encryption at rest protects the file;
    /// this protects the logs.
    pub private_key_hex: Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityView {
    pub alias: String,
    pub emoji: String,
    pub address: String, // 0x-prefixed EVM address
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedAsset {
    pub id: String,
    pub amount: String, // decimal wei string (avoids f64/JS-number precision loss)
    pub symbol: String,
    pub owner: String,
    pub proof: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub timestamp: DateTime<Utc>,
    pub assets: Vec<CompressedAsset>,
    pub signature: String,
}

/// A Marketplace listing, always backed by a real CabalMeshVoucher tokenId
/// the seller owns on-chain (enforced by the contract itself at list time).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetListingView {
    pub id: u64,
    pub seller: String,
    pub description: String,
    pub price_wei: String,  // decimal wei string, same precision rule as CompressedAsset.amount
    pub price_avax: String, // formatted for display/prompting, e.g. "0.05"
    pub token_id: u64,
    /// Which NFT contract the token belongs to. A token id alone stopped
    /// identifying an asset once the Marketplace accepted more than one
    /// collection, so anything resolving metadata needs this too.
    pub collection: String,
}

/// A voucher NFT owned by a given wallet (used by the Redeem page).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoucherView {
    pub token_id: u64,
    pub voucher_type: String,
    pub description: String,
    pub owner: String,
    /// The address that originally minted this voucher (proof-of-possession at
    /// listing time) — lets the UI distinguish "I bought this from someone"
    /// from "I minted this myself to sell and still hold it unsold".
    pub minted_by: String,
}

/// Structured state read directly from the configured canonical collection.
///
/// This is intentionally not marketplace/listing metadata. The command layer
/// validates and renders these immutable contract fields before they cross IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleChainRecord {
    pub token_id: String,
    pub collection: String,
    pub owner: String,
    pub module_id: String,
    pub provenance_hash: String,
    pub display_name: String,
    pub asset_class: u8,
    pub slot: u8,
    pub rarity: u8,
    pub effect_type: u8,
    pub primary_effect_value: u32,
    pub secondary_effect_value: u32,
    pub artwork_uri: String,
    pub artwork_digest: String,
    pub schema_version: u16,
    pub minted_by: String,
    pub soulbound: bool,
    pub revoked: bool,
}

/// One accepted-head view of the operator wallet's complete on-chain loadout.
///
/// This can be cached for offline display, but only a freshly queried snapshot
/// is eligibility evidence for reward or routing decisions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleLoadoutChainSnapshot {
    pub collection: String,
    pub operator: String,
    pub verified_block: u64,
    pub verified_at: DateTime<Utc>,
    pub modules: Vec<ModuleChainRecord>,
}

/// A confirmed loadout mutation plus the accepted state observed afterwards.
#[derive(Debug, Clone)]
pub struct ModuleMutationOutcome {
    pub tx_hash: String,
    pub loadout: ModuleLoadoutChainSnapshot,
}

/// A Marketplace deal (real on-chain state: Active/Released/Refunded) —
/// the real "someone is transacting on this listing" signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DealView {
    pub deal_id: u64,
    pub buyer: String,
    pub seller: String,
    pub token_id: u64,
    pub amount_avax: String,
    pub status: String, // "active" | "released" | "refunded"
    pub role: String,   // "buyer" | "seller" (relative to the address queried)
    pub collection: String,
    /// Unix seconds after which anyone may release this deal to the seller.
    /// The UI needs it to say how long a buyer's exclusive window has left,
    /// rather than presenting an active deal as if it waits forever.
    pub auto_release_at: u64,
    /// Whether the buyer has consented to cancelling. A seller cannot refund
    /// until they have, so this is the difference between a refund the seller
    /// can act on and one they cannot.
    pub refund_requested: bool,
}

/// A piece of content (e.g. a book page) committed to by its seller: a real
/// EIP-191 signature over the exact text, verifiable by recovering the
/// signer's address — used in place of a literal ZK proof (no `nargo`/Noir
/// available), same honesty tradeoff already made for voucher ownership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentRecord {
    pub token_id: u64,
    pub text: String,
    pub fingerprint: String, // short keccak256 hash of the text, for display
    pub signature: String,
    pub signer_address: String,
}

/// Nonce + gas price snapshot from the last time we successfully reached the
/// RPC, refreshed opportunistically in `sync_state()`. Used to sign
/// transactions offline when the RPC can't be reached at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStateCache {
    pub nonce: u64,
    pub gas_price_wei: String,
    pub cached_at: DateTime<Utc>,
}

/// What creating an escrow actually produced.
///
/// Richer than [`TxResult`], which carries only the escrow id. The proof screen
/// renders a transaction hash and calls it genuine, so the hash has to come
/// from the receipt this call held rather than from a later lookup.
///
/// `Queued` is not a failure: with no route to the RPC the transaction is
/// signed locally and handed to a peer, which is the architecture working as
/// designed rather than settlement going wrong.
#[derive(Debug, Clone)]
pub enum EscrowOutcome {
    Confirmed { escrow_id: u64, tx_hash: String },
    Queued { queue_id: String },
}

/// A transaction signed locally while offline, queued for a mesh peer with
/// real connectivity to submit on our behalf.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTx {
    pub id: String,
    pub raw_tx_hex: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
    pub status: String, // "queued" | "confirmed" | "failed"
    pub tx_hash: Option<String>,
    /// Set when a failure is permanent (e.g. a stale/superseded nonce) so the
    /// UI can say so instead of suggesting a retry that can never succeed.
    #[serde(default)]
    pub reason: Option<String>,
    /// How many times submission has been tried.
    ///
    /// `skip_serializing_if` keeps a never-retried entry serializing exactly as
    /// before, so the frozen desktop contract is unchanged — verified by the
    /// snapshot suite. `default` lets a queue file written before this field
    /// existed still load.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: u8,
}

/// A transaction this node successfully relayed to the chain on behalf of
/// another peer — real, persisted credit for helping while offline peers
/// couldn't reach the network themselves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayedTxRecord {
    pub summary: String,
    pub tx_hash: String,
    /// A deterministic estimate (bytes relayed × the same rate shown in Relay
    /// Mode's stats) — NOT an actual on-chain payout, since there is no
    /// relayer-payment settlement mechanism yet. Labeled as an estimate wherever shown.
    pub reward_avax: String,
    pub relayed_at: DateTime<Utc>,
}

/// Serde helper: an untried entry omits `attempts` so the on-disk and
/// on-the-wire shape matches what the frozen UI already expects.
fn is_zero(value: &u8) -> bool {
    *value == 0
}

/// Result of an action that normally hits the chain directly: either it went
/// through immediately (`Confirmed`), or the RPC was unreachable and it was
/// signed offline and queued for mesh relay instead (`Queued`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TxResult {
    #[serde(rename = "confirmed")]
    Confirmed { id: u64 },
    #[serde(rename = "queued")]
    Queued {
        #[serde(rename = "queueId")]
        queue_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstantSession {
    pub session_id: String,
    pub authority: String,
    pub expiry: DateTime<Utc>,
    pub is_active: bool,
}

pub struct BlockchainBridge {
    pub identities: Vec<IdentityRecord>,
    /// Encrypted store for identities. Replaces the plaintext
    /// `identities.json`, which held private keys in the clear.
    pub identity_vault: Vault<crate::vault_key::FileKeyProvider>,
    pub storage_path: PathBuf,
    pub chain_cache_path: PathBuf,
    pub module_loadout_cache_path: PathBuf,
    pub pending_relay_path: PathBuf,
    pub relayed_history_path: PathBuf,
    pub content_store_path: PathBuf,
    pub received_content_path: PathBuf,
    pub relay_boost_path: PathBuf,
    pub rpc_url: String,
    pub escrow_address: Option<Address>,
    pub marketplace_address: Option<Address>,
    pub voucher_address: Option<Address>,
    pub modules_address: Option<Address>,
    pub current_session: Option<InstantSession>,
}

impl BlockchainBridge {
    pub fn new(rpc_url_override: Option<String>) -> Self {
        let rpc_url = rpc_url_override.unwrap_or_else(|| DEFAULT_AVAX_RPC_URL.to_string());

        // No fallback here: an absent/invalid address should surface as a clear
        // runtime error the first time a contract call is attempted, not a
        // silently-wrong placeholder.
        // Layered configuration rather than bare environment reads: a mobile
        // bundle has no environment, so every address used to resolve to None
        // and every contract call failed looking like a chain problem.
        let network = crate::network_config::NetworkConfig::load(&JsonStore::new(
            crate::app_paths::in_data_dir("network.json"),
        ));

        let escrow_address = network.escrow().and_then(|s| Address::from_str(&s).ok());
        let marketplace_address = network.marketplace().and_then(|s| Address::from_str(&s).ok());
        let voucher_address = network.voucher().and_then(|s| Address::from_str(&s).ok());
        let modules_address = network.modules().and_then(|s| Address::from_str(&s).ok());

        let app_dir = crate::app_paths::data_dir();

        let mut bridge = Self {
            identities: Vec::new(),
            identity_vault: Vault::new(
                app_dir.join("vault.enc"),
                crate::vault_key::platform_provider(app_dir.join("vault.key")),
            ),
            storage_path: app_dir.join("snapshot.enc"),
            chain_cache_path: app_dir.join("chain_cache.json"),
            module_loadout_cache_path: app_dir.join("module_loadout_cache.json"),
            pending_relay_path: app_dir.join("pending_relay_txs.json"),
            relayed_history_path: app_dir.join("relayed_history.json"),
            content_store_path: app_dir.join("content_store.json"),
            received_content_path: app_dir.join("received_content.json"),
            relay_boost_path: app_dir.join("relay_boost.json"),
            rpc_url,
            escrow_address,
            marketplace_address,
            voucher_address,
            modules_address,
            current_session: None,
        };
        let _ = bridge.load_identities();
        bridge
    }

    fn save_identities(&self) -> Result<(), Box<dyn Error>> {
        self.identity_vault.save(&self.identities)?;
        Ok(())
    }

    pub fn load_identities(&mut self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        // Adopt any pre-encryption wallet first. This verifies a full decrypt
        // round trip before removing the plaintext, so a failure here leaves
        // the old file exactly where it was.
        let plaintext = JsonStore::new(self.plaintext_identity_path());
        match self
            .identity_vault
            .migrate_plaintext::<Vec<IdentityRecord>>(&plaintext)
        {
            Ok(true) => tracing::info!("🔐 Identities migrated into the encrypted vault"),
            Ok(false) => {}
            Err(error) => {
                // Do not fall through to generating a fresh wallet: that is
                // how a recoverable problem becomes permanent loss.
                tracing::error!(%error, "identity migration failed; plaintext left intact");
                return Err(Box::new(error));
            }
        }

        if self.identity_vault.exists() {
            tracing::info!("🔑 Loading identities from the encrypted vault");
            match self.identity_vault.load::<Vec<IdentityRecord>>() {
                Ok(records) => {
                    self.identities = records;
                    if self.identities.is_empty() {
                        return self.generate_new_identity("Primary Fox".to_string(), "🦊".to_string());
                    }
                    self.get_identity_views()
                }
                Err(error) => {
                    // A vault that exists but will not open means the key is
                    // wrong or the file was tampered with. Generating a new
                    // wallet over it would discard the user's funds, so this
                    // fails loudly instead.
                    tracing::error!(%error, "vault exists but cannot be decrypted");
                    Err(Box::new(error))
                }
            }
        } else {
            self.generate_new_identity("Genesis Fox".to_string(), "🦊".to_string())
        }
    }

    /// Where the pre-encryption wallet lived. Only used to migrate away from.
    fn plaintext_identity_path(&self) -> PathBuf {
        crate::app_paths::in_data_dir("identities.json")
    }

    pub fn generate_new_identity(&mut self, alias: String, emoji: String) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        tracing::info!("🆕 Generating NEW Identity '{}' [{}]...", alias, emoji);
        let signer = PrivateKeySigner::random();
        let private_key_hex = format!("0x{}", hex::encode(signer.to_bytes()));
        self.identities.push(IdentityRecord { alias, emoji, private_key_hex: private_key_hex.into() });
        self.save_identities()?;
        self.get_identity_views()
    }

    pub fn get_identity_views(&self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        let mut views = Vec::new();
        for id in &self.identities {
            let signer = PrivateKeySigner::from_str(id.private_key_hex.expose())?;
            views.push(IdentityView {
                alias: id.alias.clone(),
                emoji: id.emoji.clone(),
                address: signer.address().to_string(),
            });
        }
        Ok(views)
    }

    /// Discards the current wallet entirely and replaces it with a fresh
    /// randomly-generated one. There's no multi-wallet slot concept anywhere
    /// in this bridge (every signing path hard-codes `identities[0]`), so
    /// "logout" means wiping the vec and generating a brand-new identity to
    /// take its place, not just clearing state and leaving no signer at all.
    pub fn logout_identity(&mut self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        self.identities.clear();
        let _ = self.delete_snapshot();
        let _ = fs::remove_file(&self.relay_boost_path);
        self.generate_new_identity("Genesis Fox".to_string(), "🦊".to_string())
    }

    /// Replaces the current wallet identity with one derived from a
    /// user-supplied private key. The key is validated (it must actually
    /// derive a signer) before anything is persisted to disk.
    pub fn import_identity(
        &mut self,
        private_key_hex: String,
        alias: String,
        emoji: String,
    ) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        let normalized = if private_key_hex.starts_with("0x") {
            private_key_hex.trim().to_string()
        } else {
            format!("0x{}", private_key_hex.trim())
        };
        PrivateKeySigner::from_str(&normalized)?;

        self.identities = vec![IdentityRecord { alias, emoji, private_key_hex: normalized.into() }];
        let _ = self.delete_snapshot();
        let _ = fs::remove_file(&self.relay_boost_path);
        self.save_identities()?;
        self.get_identity_views()
    }

    /// The raw private key for the current wallet, so the user can save it
    /// before logging out / importing a different one — this is the only
    /// way to actually get back to a wallet after switching away from it,
    /// since nothing else persists it anywhere recoverable.
    pub fn get_primary_private_key(&self) -> Option<String> {
        self.identities.first().map(|id| id.private_key_hex.expose().to_owned())
    }

    pub fn get_primary_address(&self) -> String {
        match self.identities.first() {
            Some(first) => match PrivateKeySigner::from_str(first.private_key_hex.expose()) {
                Ok(signer) => signer.address().to_string(),
                Err(_) => "unknown".to_string(),
            },
            None => "unknown".to_string(),
        }
    }

    fn primary_signer(&self) -> Result<PrivateKeySigner, Box<dyn Error>> {
        let first = self.identities.first().ok_or("No identity available")?;
        Ok(PrivateKeySigner::from_str(first.private_key_hex.expose())?)
    }

    // ---- Offline signing + mesh-relay queue -------------------------------

    fn load_chain_cache(&self) -> Option<ChainStateCache> {
        let content = fs::read_to_string(&self.chain_cache_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn save_chain_cache(&self, cache: &ChainStateCache) -> Result<(), Box<dyn Error>> {
        JsonStore::new(&self.chain_cache_path).save(cache)?;
        Ok(())
    }

    fn load_pending_relay_txs(&self) -> Vec<QueuedTx> {
        fs::read_to_string(&self.pending_relay_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn save_pending_relay_txs(&self, txs: &[QueuedTx]) -> Result<(), Box<dyn Error>> {
        JsonStore::new(&self.pending_relay_path).save(&txs)?;
        Ok(())
    }

    /// Refreshes the cached nonce + gas price snapshot from the live RPC.
    /// Called opportunistically whenever we know we're online (piggybacks on
    /// `sync_state`) so a later offline attempt has something recent to sign with.
    async fn refresh_chain_cache(&self, address: Address) -> Result<(), Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let nonce = provider.get_transaction_count(address).pending().await?;
        let gas_price = provider.get_gas_price().await?;

        self.save_chain_cache(&ChainStateCache {
            nonce,
            gas_price_wei: gas_price.to_string(),
            cached_at: Utc::now(),
        })?;
        Ok(())
    }

    /// Signs a contract call fully offline using the cached nonce/gas (bumping
    /// the cached nonce so a second queued call doesn't collide), and queues
    /// the raw signed bytes for a mesh peer with connectivity to relay.
    /// The private key never leaves this function — only the signed bytes do.
    async fn sign_offline(&self, to: Address, calldata: Bytes, value: U256, summary: &str) -> Result<QueuedTx, Box<dyn Error>> {
        let cache = self.load_chain_cache().ok_or("No cached chain state available — never been online yet")?;
        let signer = self.primary_signer()?;
        let wallet = EthereumWallet::from(signer);

        // +20% buffer on the cached gas price in case it's gone slightly stale.
        let gas_price: u128 = cache.gas_price_wei.parse::<u128>().unwrap_or(30_000_000_000);
        let buffered_gas_price = gas_price + (gas_price / 5);

        let tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata)
            .with_value(value)
            .with_nonce(cache.nonce)
            .with_chain_id(43113)
            .with_gas_limit(400_000)
            .with_max_fee_per_gas(buffered_gas_price)
            .with_max_priority_fee_per_gas(buffered_gas_price);

        let envelope = tx.build(&wallet).await?;
        let raw_bytes = envelope.encoded_2718();
        let raw_tx_hex = format!("0x{}", hex::encode(&raw_bytes));

        // Bump the cached nonce immediately so a second offline call (e.g. a
        // second queued intent search) signs with the next nonce, not this one.
        self.save_chain_cache(&ChainStateCache {
            nonce: cache.nonce + 1,
            gas_price_wei: cache.gas_price_wei,
            cached_at: cache.cached_at,
        })?;

        let mut suffix = [0u8; 4];
        OsRng.fill_bytes(&mut suffix);
        let id = format!("tx-{}-{}", Utc::now().timestamp_millis(), hex::encode(suffix));

        let queued = QueuedTx {
            id,
            raw_tx_hex,
            summary: summary.to_string(),
            created_at: Utc::now(),
            status: "queued".to_string(),
            tx_hash: None,
            reason: None,
            attempts: 0,
        };

        let mut pending = self.load_pending_relay_txs();
        pending.push(queued.clone());
        self.save_pending_relay_txs(&pending)?;

        tracing::info!("📡 [Bridge] Signed offline, queued for mesh relay: {} ({})", queued.id, summary);
        Ok(queued)
    }

    /// Maximum submission attempts before an entry is parked as failed.
    ///
    /// Retrying forever would drain the battery re-broadcasting a transaction
    /// the chain will never accept — a bad nonce or an underpriced fee does not
    /// improve by being tried again.
    pub const MAX_ATTEMPTS: u8 = 5;

    /// Submits our own queued transactions now that connectivity is back.
    ///
    /// The existing path relies on a *peer* with Relay Mode picking them up,
    /// which never happens for a user who is simply alone and offline. This is
    /// the self-service path: when the device itself regains connectivity, it
    /// drains its own queue.
    ///
    /// Returns how many were confirmed.
    ///
    /// # Errors
    ///
    /// Only if the queue cannot be persisted. Individual submission failures
    /// are recorded against their entry and retried later rather than aborting
    /// the drain — one bad transaction must not block the rest.
    pub async fn drain_pending(&self) -> Result<usize, Box<dyn Error>> {
        let mut pending = self.load_pending_relay_txs();
        if pending.is_empty() {
            return Ok(0);
        }

        let mut confirmed = 0_usize;
        for entry in &mut pending {
            if entry.status != "queued" || entry.attempts >= Self::MAX_ATTEMPTS {
                continue;
            }
            entry.attempts = entry.attempts.saturating_add(1);

            match self.submit_raw_transaction(&entry.raw_tx_hex).await {
                Ok(tx_hash) => {
                    entry.status = "confirmed".to_string();
                    entry.tx_hash = Some(tx_hash);
                    entry.reason = None;
                    confirmed += 1;
                    tracing::info!(id = %entry.id, "queued transaction confirmed after reconnect");
                }
                Err(error) => {
                    entry.reason = Some(error.to_string());
                    if entry.attempts >= Self::MAX_ATTEMPTS {
                        entry.status = "failed".to_string();
                        tracing::warn!(
                            id = %entry.id,
                            attempts = entry.attempts,
                            "queued transaction parked after repeated failures"
                        );
                    } else {
                        tracing::debug!(id = %entry.id, attempts = entry.attempts, "retry failed");
                    }
                }
            }
        }

        // Persisted after every drain, not only on success: attempt counts are
        // what stop an unminable transaction being retried forever across
        // restarts.
        self.save_pending_relay_txs(&pending)?;
        Ok(confirmed)
    }

    /// Broadcasts a raw signed transaction someone else queued while offline.
    /// Used by a peer with real connectivity and Relay Mode on.
    pub async fn submit_raw_transaction(&self, raw_tx_hex: &str) -> Result<String, Box<dyn Error>> {
        let hex_str = raw_tx_hex.trim_start_matches("0x");
        let raw_bytes = hex::decode(hex_str)?;

        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let receipt = provider.send_raw_transaction(&raw_bytes).await?.get_receipt().await?;

        tracing::info!("✅ [Bridge] Relayed transaction confirmed. Tx: {:?}", receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Detects queued relay txs whose nonce has already been superseded
    /// on-chain (by a different, already-confirmed tx from the same
    /// signer) — such a tx can never be mined and would otherwise sit
    /// "queued" forever, retried for nothing every few seconds. These are
    /// removed from the queue entirely rather than surfaced as a "failed"
    /// entry — there's nothing actionable for the user to do about a
    /// nonce that's already gone.
    pub async fn prune_stale_relay_txs(&self) -> Result<usize, Box<dyn Error>> {
        let signer_address = self.get_primary_address();
        if signer_address == "unknown" {
            return Ok(0);
        }
        let addr = Address::from_str(&signer_address)?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let current_nonce = provider.get_transaction_count(addr).await?;

        let mut pending = self.load_pending_relay_txs();
        let before = pending.len();
        pending.retain(|tx| {
            if tx.status != "queued" {
                return true;
            }
            let hex_str = tx.raw_tx_hex.trim_start_matches("0x");
            let Ok(raw_bytes) = hex::decode(hex_str) else { return true };
            let Ok(envelope) = TxEnvelope::decode_2718(&mut raw_bytes.as_slice()) else { return true };
            envelope.nonce() >= current_nonce
        });
        let pruned = before - pending.len();
        if pruned > 0 {
            self.save_pending_relay_txs(&pending)?;
        }
        Ok(pruned)
    }

    pub fn get_pending_relay_txs(&self) -> Vec<QueuedTx> {
        self.load_pending_relay_txs()
    }

    /// Real, persisted credit for helping other peers: every transaction this
    /// node successfully relayed to the chain on someone else's behalf.
    pub fn record_relayed_tx(&self, summary: &str, tx_hash: &str, reward_avax: &str) -> Result<(), Box<dyn Error>> {
        let mut history = self.load_relayed_history();
        history.push(RelayedTxRecord {
            summary: summary.to_string(),
            tx_hash: tx_hash.to_string(),
            reward_avax: reward_avax.to_string(),
            relayed_at: Utc::now(),
        });
        self.save_relayed_history(&history)
    }

    pub fn get_relayed_history(&self) -> Vec<RelayedTxRecord> {
        self.load_relayed_history()
    }

    fn load_relayed_history(&self) -> Vec<RelayedTxRecord> {
        fs::read_to_string(&self.relayed_history_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn save_relayed_history(&self, history: &[RelayedTxRecord]) -> Result<(), Box<dyn Error>> {
        JsonStore::new(&self.relayed_history_path).save(&history)?;
        Ok(())
    }

    /// Multiplier applied to the (already-estimated, not real-payout) relay
    /// earnings shown in Relay Mode — starts at 1.0 (no boost) and is
    /// permanently increased by redeeming an "AI Compute Credit" or "Relay
    /// Bandwidth Credit" item voucher. Persisted locally, same honesty level
    /// as the rest of the relay-reward estimate (no real fund movement).
    pub fn get_relay_boost_multiplier(&self) -> f64 {
        fs::read_to_string(&self.relay_boost_path)
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(1.0)
    }

    pub fn apply_relay_boost(&self, additional: f64) -> Result<f64, Box<dyn Error>> {
        let updated = self.get_relay_boost_multiplier() + additional;
        JsonStore::compact(&self.relay_boost_path).save(&updated)?;
        Ok(updated)
    }

    pub fn mark_relay_tx_status(&self, id: &str, status: &str, tx_hash: Option<String>) -> Result<(), Box<dyn Error>> {
        let mut pending = self.load_pending_relay_txs();
        if let Some(entry) = pending.iter_mut().find(|t| t.id == id) {
            entry.status = status.to_string();
            entry.tx_hash = tx_hash;
        }
        self.save_pending_relay_txs(&pending)?;
        Ok(())
    }

    /// Syncs the native AVAX balance for the primary identity and saves an encrypted snapshot.
    /// Real RPC reachability check (not the OS/browser's local network
    /// interface flag, which stays "online" even when the actual RPC
    /// endpoint is unreachable or the machine has no real route to it).
    /// Cheap and short-timeout so it's safe to poll frequently.
    pub async fn check_rpc_reachable(&self) -> bool {
        let Ok(url) = self.rpc_url.parse() else { return false };
        let provider = ProviderBuilder::new().connect_http(url);
        matches!(timeout(Duration::from_secs(4), provider.get_block_number()).await, Ok(Ok(_)))
    }

    /// Spanned so the RPC round trip and everything it logs is attributable to
    /// one sync, which matters when several run concurrently after a reconnect.
    #[tracing::instrument(skip(self), fields(rpc = %self.rpc_url))]
    pub async fn sync_state(&self, wallet_address_override: &str) -> Result<Snapshot, Box<dyn Error>> {
        let primary = self.get_primary_address();
        let target = if primary != "unknown" { primary } else { wallet_address_override.to_string() };
        let address = Address::from_str(&target)?;

        tracing::info!("🔄 [Bridge] Fetching native AVAX balance from {}", self.rpc_url);

        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let balance_wei: U256 = provider.get_balance(address).await?;

        tracing::info!("✅ [Bridge] Fetched balance for {}", target);

        // Best-effort: refresh the offline-signing cache while we know we're online.
        // Never let this fail the whole sync if the RPC is flaky for just this call.
        if let Err(e) = self.refresh_chain_cache(address).await {
            tracing::warn!("⚠️  Failed to refresh chain state cache: {}", e);
        }

        let snapshot = Snapshot {
            timestamp: Utc::now(),
            assets: vec![CompressedAsset {
                id: "native-avax".to_string(),
                amount: balance_wei.to_string(),
                symbol: "AVAX".to_string(),
                owner: target,
                proof: None,
            }],
            signature: "verified_by_avalanche_rpc".to_string(),
        };

        self.save_snapshot_encrypted(&snapshot)?;

        Ok(snapshot)
    }

    // This snapshot only ever holds the wallet's native AVAX balance — public
    // on-chain data anyone can already read via RPC — so it's stored as plain
    // JSON rather than wrapped in Keychain-backed AES-GCM. The previous
    // Keychain-based scheme silently broke across dev rebuilds (each ad-hoc
    // build gets a different signing identity, so `entry.get_password()` kept
    // failing, generating a *new* key every read and making decryption of a
    // snapshot written moments earlier fail every time) — real private keys
    // live in the encrypted vault, untouched by this.
    fn save_snapshot_encrypted(&self, snapshot: &Snapshot) -> Result<(), Box<dyn Error>> {
        JsonStore::compact(&self.storage_path).save(&snapshot)?;
        tracing::info!("💾 [Bridge] Snapshot saved.");
        Ok(())
    }

    pub fn get_latest_snapshot(&self) -> Result<Snapshot, Box<dyn Error>> {
        if !self.storage_path.exists() {
            return Err("No snapshot found".into());
        }
        let file_data = fs::read(&self.storage_path)?;
        let snapshot: Snapshot = serde_json::from_slice(&file_data)?;
        Ok(snapshot)
    }

    pub fn delete_snapshot(&self) -> Result<(), Box<dyn Error>> {
        if self.storage_path.exists() {
            fs::remove_file(&self.storage_path)?;
        }
        Ok(())
    }

    pub fn delete_identity(&self) -> Result<(), Box<dyn Error>> {
        // Removes the encrypted vault. The key file is deliberately left: it
        // is useless without a vault, and deleting it would break any backup
        // the user made of vault.enc.
        JsonStore::new(crate::app_paths::in_data_dir("vault.enc")).delete()?;
        Ok(())
    }

    // ... Mock methods for sessions
    pub fn init_instant_session(&mut self) -> InstantSession {
        // Generate a fresh ephemeral signer for the agent
        let agent_signer = PrivateKeySigner::random();
        let authority_address = agent_signer.address().to_string();

        // In a full implementation, we would sign a delegation transaction here
        // For now, we store this valid signer as the "Agent Authority"

        let session = InstantSession {
            session_id: format!("sess_{}", Utc::now().timestamp()),
            authority: authority_address,
            expiry: Utc::now() + chrono::Duration::hours(1),
            is_active: true,
        };
        self.current_session = Some(session.clone());
        session
    }

    pub fn get_status(&self) -> String {
        match &self.current_session {
            Some(s) if s.is_active => format!("Instant Session Engine: Active [Agent: {}...]", &s.authority[..6]),
            _ => "Instant Session Engine: Inactive".to_string(),
        }
    }

    /// Creates an on-chain escrow deal, locking `amount_wei` for `payee`.
    /// If the RPC can't be reached within a few seconds, falls back to
    /// signing the transaction offline and queuing it for mesh relay.
    /// `skip(self)` keeps the bridge — which holds signers — out of the span,
    /// while the escrow parameters stay visible.
    pub async fn create_escrow(&self, payee: &str, amount_wei: U256, expiry_unix: u64) -> Result<TxResult, Box<dyn Error>> {
        // A thin wrapper so the frozen desktop surface keeps the exact
        // signature ticket 09 snapshotted. Everything below is shared.
        Ok(match self.create_escrow_detailed(payee, amount_wei, expiry_unix).await? {
            EscrowOutcome::Confirmed { escrow_id, .. } => TxResult::Confirmed { id: escrow_id },
            EscrowOutcome::Queued { queue_id } => TxResult::Queued { queue_id },
        })
    }

    /// Creates an escrow and reports the transaction hash alongside the id.
    ///
    /// Exists because `TxResult` carries only the escrow id, and the proof
    /// screen renders a hash it claims is genuine. Reading the hash back with a
    /// second call would be a different transaction's worth of trust — the
    /// receipt in hand is the only thing that actually proves this settlement.
    #[tracing::instrument(skip(self), fields(payee = %payee, expiry = expiry_unix))]
    pub async fn create_escrow_detailed(&self, payee: &str, amount_wei: U256, expiry_unix: u64) -> Result<EscrowOutcome, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self.escrow_address.ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;
        let payee_addr = Address::from_str(payee)?;

        // Build calldata once — reused for both the online path and the offline fallback.
        let unsigned_provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let unsigned_contract = IEscrow::new(escrow_address, unsigned_provider);
        let calldata = unsigned_contract.createEscrow(payee_addr, U256::from(expiry_unix)).calldata().clone();

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        // Errors are mapped to a plain (Send-safe) String inside this block — `Box<dyn
        // Error>` isn't Send, and holding one alive across the offline-fallback `.await`
        // below would make the whole Tauri command future non-Send.
        let online_result = timeout(Duration::from_secs(6), async {
            let pending = contract
                .createEscrow(payee_addr, U256::from(expiry_unix))
                .value(amount_wei)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            pending.get_receipt().await.map_err(|e| e.to_string())
        }).await;

        let receipt = match online_result {
            Ok(Ok(receipt)) => receipt,
            Ok(Err(e)) => return Err(e.into()),
            Err(_timed_out) => {
                tracing::warn!("⚠️  [Bridge] RPC unreachable — signing create_escrow offline for mesh relay.");
                let queued = self.sign_offline(escrow_address, calldata, amount_wei, "Create escrow").await?;
                return Ok(EscrowOutcome::Queued { queue_id: queued.id });
            }
        };

        let escrow_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IEscrow::EscrowCreated>().ok())
            .map(|l| l.inner.data.escrowId.to::<u64>())
            .ok_or("EscrowCreated event not found in receipt")?;

        tracing::info!("✅ [Bridge] Escrow {} created. Tx: {:?}", escrow_id, receipt.transaction_hash);
        Ok(EscrowOutcome::Confirmed {
            escrow_id,
            tx_hash: format!("{:?}", receipt.transaction_hash),
        })
    }

    pub async fn release_escrow(&self, escrow_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self.escrow_address.ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        let receipt = contract.release(U256::from(escrow_id)).send().await?.get_receipt().await?;
        tracing::info!("✅ [Bridge] Escrow {} released. Tx: {:?}", escrow_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    pub async fn refund_escrow(&self, escrow_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self.escrow_address.ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        let receipt = contract.refund(U256::from(escrow_id)).send().await?.get_receipt().await?;
        tracing::info!("✅ [Bridge] Escrow {} refunded. Tx: {:?}", escrow_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Reads the on-chain state of a deal (no signer required).
    pub async fn get_escrow_status(&self, escrow_id: u64) -> Result<serde_json::Value, Box<dyn Error>> {
        let escrow_address = self.escrow_address.ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        let deal = contract.getDeal(U256::from(escrow_id)).call().await?;

        Ok(serde_json::json!({
            "depositor": deal.depositor.to_string(),
            "payee": deal.payee.to_string(),
            "amount": deal.amount.to_string(),
            "expiry": deal.expiry.to::<u64>(),
            "status": deal.status,
        }))
    }

    /// Mints a new voucher NFT to the primary identity. This mint call is
    /// itself the proof-of-possession step: only the real key-holder can
    /// mint a token into their own name.
    pub async fn mint_voucher(&self, voucher_type: &str, description: &str) -> Result<u64, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let voucher_address = self.voucher_address.ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let receipt = contract
            .mintVoucher(voucher_type.to_string(), description.to_string())
            .send()
            .await?
            .get_receipt()
            .await?;

        let token_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IVoucher::VoucherMinted>().ok())
            .map(|l| l.inner.data.tokenId.to::<u64>())
            .ok_or("VoucherMinted event not found in receipt")?;

        tracing::info!("✅ [Bridge] Voucher {} minted. Tx: {:?}", token_id, receipt.transaction_hash);
        Ok(token_id)
    }

    /// Approves the Marketplace contract to pull a specific voucher out of
    /// the seller's wallet, required before that voucher can be listed.
    pub async fn approve_voucher(&self, token_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let voucher_address = self.voucher_address.ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let receipt = contract
            .approve(marketplace_address, U256::from(token_id))
            .send()
            .await?
            .get_receipt()
            .await?;

        tracing::info!("✅ [Bridge] Voucher {} approved for Marketplace. Tx: {:?}", token_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Publishes a real on-chain listing backed by an owned, approved voucher.
    /// Returns the generated listing id.
    pub async fn create_asset_listing(&self, description: &str, price_wei: U256, token_id: u64) -> Result<u64, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);

        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract
            .createListing(description.to_string(), price_wei, U256::from(token_id))
            .send()
            .await?
            .get_receipt()
            .await?;

        let listing_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IMarketplace::ListingCreated>().ok())
            .map(|l| l.inner.data.id.to::<u64>())
            .ok_or("ListingCreated event not found in receipt")?;

        tracing::info!("✅ [Bridge] Listing {} created. Tx: {:?}", listing_id, receipt.transaction_hash);
        Ok(listing_id)
    }

    /// Reads all active listings from the Marketplace contract (no signer required).
    pub async fn get_active_asset_listings(&self) -> Result<Vec<AssetListingView>, Box<dyn Error>> {
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let result = contract.getActiveListings().call().await?;

        let views = result
            .result
            .iter()
            .zip(result.ids.iter())
            .map(|(listing, id)| AssetListingView {
                id: id.to::<u64>(),
                seller: listing.seller.to_string(),
                description: listing.description.clone(),
                price_wei: listing.priceWei.to_string(),
                price_avax: alloy::primitives::utils::format_ether(listing.priceWei),
                token_id: listing.tokenId.to::<u64>(),
                collection: listing.collection.to_string(),
            })
            .collect();

        Ok(views)
    }

    /// Atomically locks `price_wei` AVAX and pulls the seller's voucher into
    /// the Marketplace contract in a single transaction. Returns the deal id.
    /// If the RPC can't be reached within a few seconds, falls back to
    /// signing the transaction offline and queuing it for mesh relay.
    pub async fn buy_listing(&self, listing_id: u64, price_wei: U256) -> Result<TxResult, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let unsigned_provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let unsigned_contract = IMarketplace::new(marketplace_address, unsigned_provider);

        // The contract rejects a seller buying their own listing, so this is
        // not the safety check — it is the error message. Catching it here
        // turns a raw revert into a sentence.
        let listing = unsigned_contract.listings(U256::from(listing_id)).call().await?;
        if listing.seller == signer.address() {
            return Err("You can't buy your own listing".into());
        }

        // A listing can go stale if its voucher was burned outside a normal
        // sale (e.g. the seller redeemed their own still-listed item) — the
        // Marketplace contract has no way to know that and never flips
        // `active` back off. Catch it here with a clear message instead of
        // letting the on-chain buy() revert with a raw ERC721NonexistentToken.
        if let Some(voucher_address) = self.voucher_address {
            let voucher_provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
            let voucher_contract = IVoucher::new(voucher_address, voucher_provider);
            if voucher_contract.ownerOf(listing.tokenId).call().await.is_err() {
                return Err("This listing's item no longer exists — it was likely redeemed by the seller before anyone bought it".into());
            }
        }

        let calldata = unsigned_contract.buy(U256::from(listing_id)).calldata().clone();

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        // See create_escrow's comment above: map to String here to keep this future Send.
        let online_result = timeout(Duration::from_secs(6), async {
            let pending = contract
                .buy(U256::from(listing_id))
                .value(price_wei)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            pending.get_receipt().await.map_err(|e| e.to_string())
        }).await;

        let receipt = match online_result {
            Ok(Ok(receipt)) => receipt,
            Ok(Err(e)) => return Err(e.into()),
            Err(_timed_out) => {
                tracing::warn!("⚠️  [Bridge] RPC unreachable — signing buy_listing offline for mesh relay.");
                let queued = self.sign_offline(marketplace_address, calldata, price_wei, "Buy listing").await?;
                return Ok(TxResult::Queued { queue_id: queued.id });
            }
        };

        let deal_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IMarketplace::DealCreated>().ok())
            .map(|l| l.inner.data.dealId.to::<u64>())
            .ok_or("DealCreated event not found in receipt")?;

        tracing::info!("✅ [Bridge] Deal {} created (voucher + AVAX locked). Tx: {:?}", deal_id, receipt.transaction_hash);
        Ok(TxResult::Confirmed { id: deal_id })
    }

    /// Withdraws an unsold listing. Only the seller can, and only while it is
    /// still active — once a buyer has paid, the deal rules take over.
    pub async fn cancel_listing(&self, listing_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract.cancelListing(U256::from(listing_id)).send().await?.get_receipt().await?;
        tracing::info!("✅ [Bridge] Listing {} cancelled. Tx: {:?}", listing_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Releases a deal: pays the seller and transfers the voucher to the buyer.
    ///
    /// The buyer can call this at any time. Anyone can once the deal's
    /// `autoReleaseAt` has passed — that is what stops a buyer who walks away
    /// from stranding the seller's voucher and payment in the contract.
    pub async fn release_deal(&self, deal_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract.releaseDeal(U256::from(deal_id)).send().await?.get_receipt().await?;
        tracing::info!("✅ [Bridge] Deal {} released. Tx: {:?}", deal_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// The buyer's consent to cancel a deal. Moves nothing on its own — it
    /// only unlocks [`BlockchainBridge::refund_deal`] for the seller.
    pub async fn request_refund(&self, deal_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract.requestRefund(U256::from(deal_id)).send().await?.get_receipt().await?;
        tracing::info!("✅ [Bridge] Refund requested on deal {}. Tx: {:?}", deal_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Refunds a deal: returns AVAX to the buyer and the voucher to the seller.
    ///
    /// Callable by the seller, and only after the buyer has called
    /// [`BlockchainBridge::request_refund`]. Cancelling a paid deal takes both
    /// sides, so neither holds a unilateral option over the other.
    pub async fn refund_deal(&self, deal_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract.refundDeal(U256::from(deal_id)).send().await?.get_receipt().await?;
        tracing::info!("✅ [Bridge] Deal {} refunded. Tx: {:?}", deal_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Burns a voucher the caller owns, claiming the service it represents.
    /// Requires real on-chain ownership (`ownerOf(tokenId) == msg.sender`).
    pub async fn redeem_voucher(&self, token_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let voucher_address = self.voucher_address.ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let receipt = contract.redeemVoucher(U256::from(token_id)).send().await?.get_receipt().await?;
        tracing::info!("✅ [Bridge] Voucher {} redeemed. Tx: {:?}", token_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Reads the current on-chain owner of a voucher (no signer required).
    pub async fn get_voucher_owner(&self, token_id: u64) -> Result<String, Box<dyn Error>> {
        let voucher_address = self.voucher_address.ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let owner = contract.ownerOf(U256::from(token_id)).call().await?;
        Ok(owner.to_string())
    }

    /// Lists every voucher the given address currently owns on-chain — used
    /// by the Redeem page so it only ever shows vouchers the caller really
    /// holds, never a claim it has to trust.
    pub async fn get_owned_vouchers(&self, owner: &str) -> Result<Vec<VoucherView>, Box<dyn Error>> {
        let voucher_address = self.voucher_address.ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;
        let owner_addr = Address::from_str(owner)?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let next_id = contract.nextTokenId().call().await?.to::<u64>();

        let mut owned = Vec::new();
        for token_id in 1..next_id {
            let Ok(current_owner) = contract.ownerOf(U256::from(token_id)).call().await else {
                continue; // burned or nonexistent token
            };
            if current_owner != owner_addr {
                continue;
            }
            if let Ok(data) = contract.vouchers(U256::from(token_id)).call().await {
                owned.push(VoucherView {
                    token_id,
                    voucher_type: data.voucherType,
                    description: data.description,
                    owner: current_owner.to_string(),
                    minted_by: data.mintedBy.to_string(),
                });
            }
        }

        Ok(owned)
    }

    /// Whether this build has an explicitly configured canonical collection.
    ///
    /// `false` is a supported state. It must never fall back to the legacy
    /// voucher address because that would turn unstructured tokens into
    /// authentic reward-bearing modules.
    #[must_use]
    pub const fn modules_configured(&self) -> bool {
        self.modules_address.is_some()
    }

    /// Reads every module currently owned by this bridge's primary wallet.
    ///
    /// Ownership comes from ERC-721 Enumerable at one accepted canonical head;
    /// there is no pending-mint cache. Every call is pinned to the same block,
    /// so an ownership change while the inventory is loading cannot combine
    /// indexes and metadata from two different states.
    pub async fn get_owned_modules(&self) -> Result<Vec<ModuleChainRecord>, Box<dyn Error>> {
        const MAX_OWNED_MODULES: usize = 512;

        let collection = self
            .modules_address
            .ok_or("MODULES_CONTRACT_ADDRESS not configured")?;
        let owner = self.primary_signer()?.address();
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IModules::new(collection, provider);

        // Avalanche's ordinary C-Chain RPC exposes accepted/finalized state by
        // default. Pin the returned head anyway: a sequence of independent
        // `latest` calls could otherwise straddle a later accepted transfer.
        let snapshot_block = BlockId::number(contract.provider().get_block_number().await?);
        let balance = contract.balanceOf(owner).block(snapshot_block).call().await?;
        if balance > U256::from(MAX_OWNED_MODULES) {
            return Err("module inventory exceeds the supported bound".into());
        }

        let mut owned = Vec::with_capacity(balance.to::<usize>());
        for index in 0..balance.to::<usize>() {
            let token_id = contract
                .tokenOfOwnerByIndex(owner, U256::from(index))
                .block(snapshot_block)
                .call()
                .await?;
            let data = contract.assetData(token_id).block(snapshot_block).call().await?;
            let soulbound = contract.locked(token_id).block(snapshot_block).call().await?;
            let revoked = contract.revoked(token_id).block(snapshot_block).call().await?;
            let current_owner = contract.ownerOf(token_id).block(snapshot_block).call().await?;
            if current_owner != owner {
                continue;
            }

            owned.push(ModuleChainRecord {
                token_id: token_id.to_string(),
                collection: collection.to_string(),
                owner: current_owner.to_string(),
                module_id: format!("{:#x}", data.moduleId),
                provenance_hash: format!("{:#x}", data.provenanceHash),
                display_name: data.displayName,
                asset_class: data.assetClass,
                slot: data.slot,
                rarity: data.rarity,
                effect_type: data.effectType,
                primary_effect_value: data.primaryEffectValue,
                secondary_effect_value: data.secondaryEffectValue,
                artwork_uri: data.artworkUri,
                artwork_digest: format!("{:#x}", data.artworkDigest),
                schema_version: data.schemaVersion,
                minted_by: data.mintedBy.to_string(),
                soulbound,
                revoked,
            });
        }

        Ok(owned)
    }

    /// Reads the operator wallet's complete loadout at one accepted C-Chain
    /// head and rejects any contract state that does not satisfy the V1
    /// ownership/binding invariants.
    pub async fn get_module_loadout(
        &self,
    ) -> Result<ModuleLoadoutChainSnapshot, Box<dyn Error>> {
        let collection = self
            .modules_address
            .ok_or("MODULES_CONTRACT_ADDRESS not configured")?;
        let operator = self.primary_signer()?.address();
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IModules::new(collection, provider);
        let verified_block = contract.provider().get_block_number().await?;
        let snapshot_block = BlockId::number(verified_block);
        let mut modules = Vec::with_capacity(3);

        for slot in 1_u8..=3 {
            let token_id = contract
                .equippedToken(operator, slot)
                .block(snapshot_block)
                .call()
                .await?;
            if token_id == U256::ZERO {
                continue;
            }

            let current_owner = contract.ownerOf(token_id).block(snapshot_block).call().await?;
            let equipped_by = contract.equippedBy(token_id).block(snapshot_block).call().await?;
            let data = contract.assetData(token_id).block(snapshot_block).call().await?;
            let soulbound = contract.locked(token_id).block(snapshot_block).call().await?;
            let revoked = contract.revoked(token_id).block(snapshot_block).call().await?;

            // A canonical V1 collection clears these mappings on ownership
            // loss, escrow transfer, burn, and revocation. Fail closed if the
            // configured bytecode violates that contract rather than turning
            // inconsistent state into a local boost.
            if current_owner != operator
                || equipped_by != operator
                || data.assetClass != 0
                || data.slot != slot
                || soulbound
                || revoked
            {
                return Err("canonical module loadout invariant failed".into());
            }

            modules.push(ModuleChainRecord {
                token_id: token_id.to_string(),
                collection: collection.to_string(),
                owner: current_owner.to_string(),
                module_id: format!("{:#x}", data.moduleId),
                provenance_hash: format!("{:#x}", data.provenanceHash),
                display_name: data.displayName,
                asset_class: data.assetClass,
                slot: data.slot,
                rarity: data.rarity,
                effect_type: data.effectType,
                primary_effect_value: data.primaryEffectValue,
                secondary_effect_value: data.secondaryEffectValue,
                artwork_uri: data.artworkUri,
                artwork_digest: format!("{:#x}", data.artworkDigest),
                schema_version: data.schemaVersion,
                minted_by: data.mintedBy.to_string(),
                soulbound,
                revoked,
            });
        }

        Ok(ModuleLoadoutChainSnapshot {
            collection: collection.to_string(),
            operator: operator.to_string(),
            verified_block,
            verified_at: Utc::now(),
            modules,
        })
    }

    /// Persists an already validated accepted-head loadout for offline display.
    /// Cached state is advisory and must never be used by effect verifiers.
    pub fn save_module_loadout_cache(
        &self,
        snapshot: &ModuleLoadoutChainSnapshot,
    ) -> Result<(), Box<dyn Error>> {
        JsonStore::compact(&self.module_loadout_cache_path).save(snapshot)?;
        Ok(())
    }

    /// Returns the last loadout only when it belongs to this exact canonical
    /// collection and current primary operator wallet.
    #[must_use]
    pub fn cached_module_loadout(&self) -> Option<ModuleLoadoutChainSnapshot> {
        let collection = self.modules_address?;
        let operator = self.primary_signer().ok()?.address();
        let bytes = fs::read(&self.module_loadout_cache_path).ok()?;
        let snapshot: ModuleLoadoutChainSnapshot = serde_json::from_slice(&bytes).ok()?;
        (snapshot.collection == collection.to_string()
            && snapshot.operator == operator.to_string())
            .then_some(snapshot)
    }

    /// Equips a module online and returns only after the accepted loadout
    /// proves that exact token is bound to the primary operator wallet.
    pub async fn equip_module(
        &self,
        token_id: U256,
    ) -> Result<ModuleMutationOutcome, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let collection = self
            .modules_address
            .ok_or("MODULES_CONTRACT_ADDRESS not configured")?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IModules::new(collection, provider);
        let receipt = contract.equip(token_id).send().await?.get_receipt().await?;
        let loadout = self.get_module_loadout().await?;
        let token_id_text = token_id.to_string();
        if !loadout
            .modules
            .iter()
            .any(|module| module.token_id == token_id_text)
        {
            return Err("module equip was not confirmed by accepted state".into());
        }

        Ok(ModuleMutationOutcome {
            tx_hash: format!("{:?}", receipt.transaction_hash),
            loadout,
        })
    }

    /// Unequips a module online and returns only after accepted state no longer
    /// binds that token to any slot of the primary operator wallet.
    pub async fn unequip_module(
        &self,
        token_id: U256,
    ) -> Result<ModuleMutationOutcome, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let collection = self
            .modules_address
            .ok_or("MODULES_CONTRACT_ADDRESS not configured")?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IModules::new(collection, provider);
        let receipt = contract.unequip(token_id).send().await?.get_receipt().await?;
        let loadout = self.get_module_loadout().await?;
        let token_id_text = token_id.to_string();
        if loadout
            .modules
            .iter()
            .any(|module| module.token_id == token_id_text)
        {
            return Err("module unequip was not confirmed by accepted state".into());
        }

        Ok(ModuleMutationOutcome {
            tx_hash: format!("{:?}", receipt.transaction_hash),
            loadout,
        })
    }

    /// A Marketplace deal this address is involved in (as buyer or seller),
    /// with its real on-chain status — this IS the "an agent is dealing with
    /// this listing" signal: `active` means a buyer has locked funds against
    /// a seller's voucher and it's awaiting release/refund.
    pub async fn get_my_deals(&self, address: &str) -> Result<Vec<DealView>, Box<dyn Error>> {
        let marketplace_address = self.marketplace_address.ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;
        let my_addr = Address::from_str(address)?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let next_id = contract.nextDealId().call().await?.to::<u64>();

        let mut deals = Vec::new();
        for deal_id in 1..next_id {
            let Ok(deal) = contract.getDeal(U256::from(deal_id)).call().await else {
                continue;
            };
            if deal.buyer != my_addr && deal.seller != my_addr {
                continue;
            }
            let status = match deal.status {
                1 => "active",
                2 => "released",
                3 => "refunded",
                _ => "none",
            };
            deals.push(DealView {
                deal_id,
                buyer: deal.buyer.to_string(),
                seller: deal.seller.to_string(),
                token_id: deal.tokenId.to::<u64>(),
                amount_avax: alloy::primitives::utils::format_ether(deal.amount),
                status: status.to_string(),
                role: if deal.seller == my_addr { "seller".to_string() } else { "buyer".to_string() },
                collection: deal.collection.to_string(),
                auto_release_at: deal.autoReleaseAt,
                refund_requested: deal.refundRequested,
            });
        }

        Ok(deals)
    }

    // ---- PDF content commitment + delivery --------------------------------

    fn load_content_store(&self) -> std::collections::HashMap<u64, ContentRecord> {
        fs::read_to_string(&self.content_store_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn save_content_store(&self, store: &std::collections::HashMap<u64, ContentRecord>) -> Result<(), Box<dyn Error>> {
        JsonStore::new(&self.content_store_path).save(store)?;
        Ok(())
    }

    fn load_received_content(&self) -> std::collections::HashMap<u64, ContentRecord> {
        fs::read_to_string(&self.received_content_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn save_received_content(&self, store: &std::collections::HashMap<u64, ContentRecord>) -> Result<(), Box<dyn Error>> {
        JsonStore::new(&self.received_content_path).save(store)?;
        Ok(())
    }

    /// Extracts page 1's text from a PDF's raw bytes (pure-Rust, no system
    /// dependency, no network needed).
    pub fn extract_pdf_text(&self, pdf_bytes: Vec<u8>) -> Result<String, Box<dyn Error>> {
        let doc = lopdf::Document::load_mem(&pdf_bytes)?;
        let text = doc.extract_text(&[1])?;
        Ok(text)
    }

    /// Signs the exact text with this node's identity key — a real,
    /// verifiable commitment standing in for a literal ZK proof (no
    /// `nargo`/Noir available). `token_id` is filled in by the caller once
    /// the voucher has actually been minted.
    pub fn sign_content(&self, text: &str) -> Result<ContentRecord, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let signature = signer.sign_message_sync(text.as_bytes())?;
        let fingerprint = format!("0x{}", hex::encode(&keccak256(text.as_bytes())[..8]));

        Ok(ContentRecord {
            token_id: 0,
            text: text.to_string(),
            fingerprint,
            signature: signature.to_string(),
            signer_address: signer.address().to_string(),
        })
    }

    /// Persists a signed content record for a listing this node sold, so it
    /// can respond when the buyer's node requests delivery over the mesh.
    pub fn store_content(&self, token_id: u64, mut record: ContentRecord) -> Result<(), Box<dyn Error>> {
        record.token_id = token_id;
        let mut store = self.load_content_store();
        store.insert(token_id, record);
        self.save_content_store(&store)
    }

    pub fn get_content(&self, token_id: u64) -> Option<ContentRecord> {
        self.load_content_store().get(&token_id).cloned()
    }

    /// Verifies a delivered piece of content really was signed by the
    /// expected seller before accepting it — never trusts the mesh payload
    /// on its own.
    pub fn receive_content(&self, token_id: u64, text: &str, signature: &str, expected_seller: &str) -> Result<bool, Box<dyn Error>> {
        let sig = Signature::from_str(signature)?;
        let recovered = sig.recover_address_from_msg(text.as_bytes())?;
        let expected = Address::from_str(expected_seller)?;

        if recovered != expected {
            tracing::warn!("⚠️  Content delivery rejected: signature recovered {} but expected seller {}", recovered, expected);
            return Ok(false);
        }

        let fingerprint = format!("0x{}", hex::encode(&keccak256(text.as_bytes())[..8]));
        let mut store = self.load_received_content();
        store.insert(token_id, ContentRecord {
            token_id,
            text: text.to_string(),
            fingerprint,
            signature: signature.to_string(),
            signer_address: recovered.to_string(),
        });
        self.save_received_content(&store)?;
        Ok(true)
    }

    pub fn get_received_content(&self, token_id: u64) -> Option<ContentRecord> {
        self.load_received_content().get(&token_id).cloned()
    }
}

#[cfg(test)]
mod offline_signing_tests {
    use super::*;

    /// Confirms the offline-signing fallback works with zero network access:
    /// given only a cached nonce/gas price (as if we'd synced earlier while
    /// online), `sign_offline` must produce a valid non-empty raw signed
    /// transaction and queue it — this is the exact path `create_escrow`/
    /// `buy_listing` fall back to when the RPC can't be reached.
    #[tokio::test]
    async fn signs_offline_using_cached_nonce_and_gas() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut bridge = BlockchainBridge {
            identities: Vec::new(),
            identity_vault: Vault::new(
                tmp_dir.join("vault.enc"),
                crate::vault_key::platform_provider(tmp_dir.join("vault.key")),
            ),
            storage_path: tmp_dir.join("snapshot.enc"),
            chain_cache_path: tmp_dir.join("chain_cache.json"),
            module_loadout_cache_path: tmp_dir.join("module_loadout_cache.json"),
            pending_relay_path: tmp_dir.join("pending_relay_txs.json"),
            relayed_history_path: tmp_dir.join("relayed_history.json"),
            content_store_path: tmp_dir.join("content_store.json"),
            received_content_path: tmp_dir.join("received_content.json"),
            relay_boost_path: tmp_dir.join("relay_boost.json"),
            // Deliberately unreachable — proves sign_offline never touches the network.
            rpc_url: "http://127.0.0.1:9".to_string(),
            escrow_address: None,
            marketplace_address: None,
            voucher_address: None,
            modules_address: None,
            current_session: None,
        };
        bridge.generate_new_identity("Test".to_string(), "🧪".to_string()).unwrap();

        // Simulate having synced once while online.
        bridge.save_chain_cache(&ChainStateCache {
            nonce: 0,
            gas_price_wei: "30000000000".to_string(),
            cached_at: Utc::now(),
        }).unwrap();

        let to = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let calldata = Bytes::from(vec![0xde, 0xad, 0xbe, 0xef]);

        let queued = bridge
            .sign_offline(to, calldata, U256::from(0), "test tx")
            .await
            .expect("sign_offline should succeed with zero network access");

        assert!(queued.raw_tx_hex.starts_with("0x"));
        assert!(queued.raw_tx_hex.len() > 10, "raw tx hex should be non-trivial");
        assert_eq!(queued.status, "queued");

        let pending = bridge.get_pending_relay_txs();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, queued.id);

        // Second offline signature should use the bumped nonce, not collide.
        let calldata2 = Bytes::from(vec![0xca, 0xfe]);
        let queued2 = bridge
            .sign_offline(to, calldata2, U256::from(0), "second test tx")
            .await
            .expect("second sign_offline should also succeed");
        assert_ne!(queued.raw_tx_hex, queued2.raw_tx_hex);

        std::fs::remove_dir_all(&tmp_dir).ok();
    }
}

#[cfg(test)]
mod content_commitment_tests {
    use super::*;

    fn test_bridge(tmp_dir: &PathBuf) -> BlockchainBridge {
        BlockchainBridge {
            identities: Vec::new(),
            identity_vault: Vault::new(
                tmp_dir.join("vault.enc"),
                crate::vault_key::platform_provider(tmp_dir.join("vault.key")),
            ),
            storage_path: tmp_dir.join("snapshot.enc"),
            chain_cache_path: tmp_dir.join("chain_cache.json"),
            module_loadout_cache_path: tmp_dir.join("module_loadout_cache.json"),
            pending_relay_path: tmp_dir.join("pending_relay_txs.json"),
            relayed_history_path: tmp_dir.join("relayed_history.json"),
            content_store_path: tmp_dir.join("content_store.json"),
            received_content_path: tmp_dir.join("received_content.json"),
            relay_boost_path: tmp_dir.join("relay_boost.json"),
            rpc_url: "http://127.0.0.1:9".to_string(),
            escrow_address: None,
            marketplace_address: None,
            voucher_address: None,
            modules_address: None,
            current_session: None,
        }
    }

    /// A signed content commitment must verify when the recovered signer matches
    /// the real seller address, and must be rejected — never silently trusted —
    /// when it doesn't. No network needed for any of this (pure crypto).
    #[test]
    fn signs_and_verifies_content_commitment() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_content_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut seller_bridge = test_bridge(&tmp_dir.join("seller"));
        std::fs::create_dir_all(tmp_dir.join("seller")).unwrap();
        seller_bridge.generate_new_identity("Seller".to_string(), "📚".to_string()).unwrap();
        let seller_address = seller_bridge.get_primary_address();

        let text = "Chapter 1: It was the best of times, it was the worst of times.";
        let record = seller_bridge.sign_content(text).expect("sign_content should succeed offline");
        assert_eq!(record.signer_address.to_lowercase(), seller_address.to_lowercase());
        assert!(!record.signature.is_empty());

        // Buyer's own bridge instance (different identity) verifies the delivered content.
        let mut buyer_bridge = test_bridge(&tmp_dir.join("buyer"));
        std::fs::create_dir_all(tmp_dir.join("buyer")).unwrap();
        buyer_bridge.generate_new_identity("Buyer".to_string(), "🛒".to_string()).unwrap();

        let accepted = buyer_bridge
            .receive_content(1, text, &record.signature, &seller_address)
            .expect("receive_content should not error");
        assert!(accepted, "a correctly signed commitment from the real seller must verify");
        assert!(buyer_bridge.get_received_content(1).is_some());

        // A signature that doesn't match the claimed seller must be rejected.
        let wrong_seller = "0x0000000000000000000000000000000000000001";
        let rejected = buyer_bridge
            .receive_content(2, text, &record.signature, wrong_seller)
            .expect("receive_content should not error even on mismatch");
        assert!(!rejected, "a signature from someone else must never be silently accepted");
        assert!(buyer_bridge.get_received_content(2).is_none());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn cached_loadout_is_scoped_to_the_current_wallet_and_collection() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "cabalmesh_loadout_cache_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut bridge = test_bridge(&tmp_dir);
        bridge
            .generate_new_identity("Operator".to_string(), "📡".to_string())
            .unwrap();
        let collection = Address::from_str(
            "0x00000000000000000000000000000000000000a7",
        )
        .unwrap();
        bridge.modules_address = Some(collection);
        let snapshot = ModuleLoadoutChainSnapshot {
            collection: collection.to_string(),
            operator: bridge.get_primary_address(),
            verified_block: 42,
            verified_at: Utc::now(),
            modules: Vec::new(),
        };

        bridge.save_module_loadout_cache(&snapshot).unwrap();
        assert_eq!(bridge.cached_module_loadout(), Some(snapshot.clone()));

        let mut wrong_wallet = snapshot.clone();
        wrong_wallet.operator = "0x00000000000000000000000000000000000000ff".into();
        bridge.save_module_loadout_cache(&wrong_wallet).unwrap();
        assert!(bridge.cached_module_loadout().is_none());

        bridge.save_module_loadout_cache(&snapshot).unwrap();
        bridge.modules_address = Some(
            Address::from_str("0x00000000000000000000000000000000000000b8").unwrap(),
        );
        assert!(bridge.cached_module_loadout().is_none());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }
}
