use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::error::Error;
use std::str::FromStr;
use chrono::{DateTime, Utc};
// Crypto Imports
use alloy::{
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
};
use keyring::Entry;
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng, rand_core::RngCore},
    Aes256Gcm, Nonce, Key
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

const KEYCHAIN_SERVICE: &str = "com.cabalmesh.wallet";
const KEYCHAIN_USER: &str = "snapshot-encryption-key";
pub const DEFAULT_AVAX_RPC_URL: &str = "https://api.avax-test.network/ext/bc/C/rpc";

sol! {
    #[sol(rpc)]
    IEscrow,
    "abi/Escrow.abi.json"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRecord {
    pub alias: String,
    pub emoji: String,
    pub private_key_hex: String, // 0x-prefixed secp256k1 private key
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstantSession {
    pub session_id: String,
    pub authority: String,
    pub expiry: DateTime<Utc>,
    pub is_active: bool,
}

pub struct BlockchainBridge {
    pub identities: Vec<IdentityRecord>,
    pub identity_path: PathBuf,
    pub storage_path: PathBuf,
    pub rpc_url: String,
    pub escrow_address: Option<Address>,
    pub current_session: Option<InstantSession>,
}

impl BlockchainBridge {
    pub fn new(rpc_url_override: Option<String>) -> Self {
        let rpc_url = rpc_url_override.unwrap_or_else(|| DEFAULT_AVAX_RPC_URL.to_string());

        // No fallback here: an absent/invalid address should surface as a clear
        // runtime error the first time a contract call is attempted, not a
        // silently-wrong placeholder.
        let escrow_address = std::env::var("ESCROW_CONTRACT_ADDRESS")
            .ok()
            .filter(|s| !s.is_empty())
            .and_then(|s| Address::from_str(&s).ok());

        let app_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cabalmesh");
        let _ = fs::create_dir_all(&app_dir);

        let mut bridge = Self {
            identities: Vec::new(),
            identity_path: app_dir.join("identities.json"),
            storage_path: app_dir.join("snapshot.enc"),
            rpc_url,
            escrow_address,
            current_session: None,
        };
        let _ = bridge.load_identities();
        bridge
    }

    fn save_identities(&self) -> Result<(), Box<dyn Error>> {
        fs::write(&self.identity_path, serde_json::to_string_pretty(&self.identities)?)?;
        Ok(())
    }

    pub fn load_identities(&mut self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        if self.identity_path.exists() {
            println!("🔑 Loading Identities from {:?}", self.identity_path);
            let content = fs::read_to_string(&self.identity_path)?;

            match serde_json::from_str::<Vec<IdentityRecord>>(&content) {
                Ok(records) => {
                    self.identities = records;
                    if self.identities.is_empty() {
                        return self.generate_new_identity("Primary Fox".to_string(), "🦊".to_string());
                    }
                    self.get_identity_views()
                }
                Err(_) => {
                    println!("⚠️ Failed to parse identity list, creating new...");
                    return self.generate_new_identity("Glitch Fox".to_string(), "👾".to_string());
                }
            }
        } else {
            return self.generate_new_identity("Genesis Fox".to_string(), "🦊".to_string());
        }
    }

    pub fn generate_new_identity(&mut self, alias: String, emoji: String) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        println!("🆕 Generating NEW Identity '{}' [{}]...", alias, emoji);
        let signer = PrivateKeySigner::random();
        let private_key_hex = format!("0x{}", hex::encode(signer.to_bytes()));
        self.identities.push(IdentityRecord { alias, emoji, private_key_hex });
        self.save_identities()?;
        self.get_identity_views()
    }

    pub fn get_identity_views(&self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        let mut views = Vec::new();
        for id in &self.identities {
            let signer = PrivateKeySigner::from_str(&id.private_key_hex)?;
            views.push(IdentityView {
                alias: id.alias.clone(),
                emoji: id.emoji.clone(),
                address: signer.address().to_string(),
            });
        }
        Ok(views)
    }

    pub fn get_primary_address(&self) -> String {
        match self.identities.first() {
            Some(first) => match PrivateKeySigner::from_str(&first.private_key_hex) {
                Ok(signer) => signer.address().to_string(),
                Err(_) => "unknown".to_string(),
            },
            None => "unknown".to_string(),
        }
    }

    fn primary_signer(&self) -> Result<PrivateKeySigner, Box<dyn Error>> {
        let first = self.identities.first().ok_or("No identity available")?;
        Ok(PrivateKeySigner::from_str(&first.private_key_hex)?)
    }

    /// Syncs the native AVAX balance for the primary identity and saves an encrypted snapshot.
    pub async fn sync_state(&self, wallet_address_override: &str) -> Result<Snapshot, Box<dyn Error>> {
        let primary = self.get_primary_address();
        let target = if primary != "unknown" { primary } else { wallet_address_override.to_string() };
        let address = Address::from_str(&target)?;

        println!("🔄 [Bridge] Fetching native AVAX balance from {}", self.rpc_url);

        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let balance_wei: U256 = provider.get_balance(address).await?;

        println!("✅ [Bridge] Fetched balance for {}", target);

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

    fn get_snapshot_key(&self) -> Result<Key<Aes256Gcm>, Box<dyn Error>> {
        let entry = Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)?;

        match entry.get_password() {
            Ok(pass) => {
                let bytes = BASE64.decode(pass)?;
                if bytes.len() != 32 { return Err("Invalid key length in keychain".into()); }
                Ok(*Key::<Aes256Gcm>::from_slice(&bytes))
            }
            Err(_) => {
                println!("🔐 Generating new encryption key in Keychain...");
                let mut key_bytes = [0u8; 32];
                OsRng.fill_bytes(&mut key_bytes);
                let encoded = BASE64.encode(key_bytes);
                entry.set_password(&encoded)?;
                Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
            }
        }
    }

    fn save_snapshot_encrypted(&self, snapshot: &Snapshot) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_vec(&snapshot)?;

        let key = self.get_snapshot_key()?;
        let cipher = Aes256Gcm::new(&key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits; unique per message

        let ciphertext = cipher.encrypt(&nonce, json.as_ref())
            .map_err(|_| "Encryption failed")?;

        // Prepend nonce to ciphertext for storage
        let mut final_data = nonce.to_vec();
        final_data.extend_from_slice(&ciphertext);

        fs::write(&self.storage_path, final_data)?;
        println!("💾 [Bridge] Snapshot ENCRYPTED and saved via Keychain Key.");
        Ok(())
    }

    pub fn get_latest_snapshot(&self) -> Result<Snapshot, Box<dyn Error>> {
        if !self.storage_path.exists() {
            return Err("No snapshot found".into());
        }
        let file_data = fs::read(&self.storage_path)?;
        if file_data.len() < 12 { return Err("Corrupted snapshot file".into()); }

        let (nonce_bytes, ciphertext) = file_data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let key = self.get_snapshot_key()?;
        let cipher = Aes256Gcm::new(&key);

        let plaintext = cipher.decrypt(nonce, ciphertext)
            .map_err(|_| "Decryption failed - Invalid Key or Corrupted Data")?;

        let snapshot: Snapshot = serde_json::from_slice(&plaintext)?;
        Ok(snapshot)
    }

    pub fn delete_snapshot(&self) -> Result<(), Box<dyn Error>> {
        if self.storage_path.exists() {
            fs::remove_file(&self.storage_path)?;
        }
        Ok(())
    }

    pub fn delete_identity(&self) -> Result<(), Box<dyn Error>> {
        if self.identity_path.exists() {
            fs::remove_file(&self.identity_path)?;
        }
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
    /// Returns the generated escrow id.
    pub async fn create_escrow(&self, payee: &str, amount_wei: U256, expiry_unix: u64) -> Result<u64, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self.escrow_address.ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;
        let payee_addr = Address::from_str(payee)?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);

        let contract = IEscrow::new(escrow_address, provider);

        let receipt = contract
            .createEscrow(payee_addr, U256::from(expiry_unix))
            .value(amount_wei)
            .send()
            .await?
            .get_receipt()
            .await?;

        let escrow_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IEscrow::EscrowCreated>().ok())
            .map(|l| l.inner.data.escrowId.to::<u64>())
            .ok_or("EscrowCreated event not found in receipt")?;

        println!("✅ [Bridge] Escrow {} created. Tx: {:?}", escrow_id, receipt.transaction_hash);
        Ok(escrow_id)
    }

    pub async fn release_escrow(&self, escrow_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self.escrow_address.ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        let receipt = contract.release(U256::from(escrow_id)).send().await?.get_receipt().await?;
        println!("✅ [Bridge] Escrow {} released. Tx: {:?}", escrow_id, receipt.transaction_hash);
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    pub async fn refund_escrow(&self, escrow_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self.escrow_address.ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new().wallet(signer).connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        let receipt = contract.refund(U256::from(escrow_id)).send().await?.get_receipt().await?;
        println!("✅ [Bridge] Escrow {} refunded. Tx: {:?}", escrow_id, receipt.transaction_hash);
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
}
