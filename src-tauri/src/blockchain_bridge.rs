use cabal_store::JsonStore;
use cabal_vault::{Secret, Vault};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
// Crypto Imports
use aes_gcm::aead::{rand_core::RngCore, OsRng};
use alloy::{
    consensus::{BlockHeader as _, Transaction as _, TxEnvelope},
    eips::{
        eip2718::{Decodable2718, Encodable2718},
        BlockId, BlockNumberOrTag,
    },
    network::{
        primitives::HeaderResponse as _, BlockResponse as _, EthereumWallet, TransactionBuilder,
    },
    primitives::{keccak256, Address, Bytes, Signature, B256, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::{local::PrivateKeySigner, SignerSync},
    sol,
};
use tokio::time::{timeout, Duration};

pub const DEFAULT_AVAX_RPC_URL: &str = "https://api.avax-test.network/ext/bc/C/rpc";

sol! {
    #[sol(rpc)]
    IEscrow,
    "abi/Escrow.abi.json"
}

sol! {
    #[sol(rpc)]
    interface IRelaySettlement {
        struct RelayAuthorization {
            bytes32 policyHash;
            bytes32 routeNonce;
            bytes32 payloadCommitment;
            uint8 deliveryMode;
            bytes32 relayRouteHash;
            address sender;
            address recipient;
            uint64 authorizedBytes;
            uint8 relayCount;
            uint64 maximumChargeNavax;
            uint64 issuedAt;
            uint64 expiresAt;
        }

        struct RelayContribution {
            bytes32 authorizationHash;
            uint8 hopIndex;
            address relayer;
            address ingress;
            address egress;
            bytes32 payloadCommitment;
            uint64 deliveredBytes;
            uint64 forwardedAt;
        }

        struct RecipientAcknowledgement {
            bytes32 authorizationHash;
            bytes32 contributionsHash;
            address recipient;
            bytes32 payloadCommitment;
            uint64 deliveredBytes;
            uint64 receivedAt;
        }

        struct RelayProof {
            RelayAuthorization authorization;
            address[] relayers;
            bytes senderSignature;
            RelayContribution[] contributions;
            bytes[] contributionSignatures;
            RecipientAcknowledgement acknowledgement;
            bytes acknowledgementSignature;
        }

        event RouteFunded(
            bytes32 indexed routeId,
            address indexed sender,
            address indexed recipient,
            uint64 maximumChargeNavax,
            uint64 expiresAt
        );
        event RouteSettled(
            bytes32 indexed routeId,
            address indexed executor,
            uint64 deliveredBytes,
            uint64 workPaidNavax,
            uint64 executorPaidNavax,
            uint64 senderRefundNavax
        );
        event RelayRewardCredited(
            bytes32 indexed routeId,
            address indexed relayer,
            uint64 amountNavax
        );

        function fundRoute(
            RelayAuthorization authorization,
            address[] relayers,
            bytes senderSignature
        ) external payable returns (bytes32 routeId);
        function settle(RelayProof proof) external returns (bytes32 routeId);
        function expireRoute(bytes32 routeId) external;
        function withdraw() external;
        function routes(bytes32 routeId)
            external
            view
            returns (
                address sender,
                address recipient,
                bytes32 payloadCommitment,
                uint64 authorizedBytes,
                uint64 maximumChargeNavax,
                uint64 expiresAt,
                uint8 state
            );
        function consumedRoutes(bytes32 routeId) external view returns (bool consumed);
        function settledRelayEarningsNavax(address relayer)
            external
            view
            returns (uint256 amountNavax);
        function withdrawableWei(address account) external view returns (uint256 amountWei);
    }
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
    interface IStandingRegistry {
        function standingOf(address seller)
            external
            view
            returns (uint64 count, uint256 changedAtBlock);
    }
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
        function getApproved(uint256 tokenId) external view returns (address operator);
        function approve(address operator, uint256 tokenId) external;
        function isApprovedForAll(address owner, address operator)
            external
            view
            returns (bool approved);
        function isMarketplaceEligible(uint256 tokenId)
            external
            view
            returns (bool eligible);
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

/// Evidence that a wallet's key has been shown to its owner at least once.
///
/// Holds no key material — an address and a time. It exists so the app can
/// tell "this wallet is recoverable" from "destroying this wallet destroys the
/// funds in it", which is the difference between a replaceable wallet and an
/// irreversible mistake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBackupRecord {
    /// The address that was revealed. Checked against the current wallet, so
    /// one wallet's export never vouches for another's.
    pub address: String,
    pub exported_at: DateTime<Utc>,
}

/// What a restore attempt did, or why it did nothing.
///
/// Refusals are values rather than errors because each has different copy and
/// a different next step for the user, and an error string cannot be switched
/// on.
#[derive(Debug, Clone)]
pub enum WalletRestore {
    Replaced {
        address: String,
        identities: Vec<IdentityView>,
    },
    /// The wallet about to be destroyed has never been revealed, so nobody can
    /// get back into it. Nothing was changed.
    BackupRequired { address: String },
    /// The supplied text is not a private key. Nothing was changed.
    InvalidKey,
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

/// One buyable listing from the reviewed module marketplace at one pinned
/// accepted chain head. Seller prose is deliberately absent: every identity,
/// slot, rarity, and effect field comes from [`ModuleChainRecord`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleListingChainRecord {
    pub listing_id: String,
    pub seller: String,
    pub price_wei: String,
    pub module: ModuleChainRecord,
}

/// Complete result of scanning the reviewed marketplace at one accepted head.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleMarketChainSnapshot {
    pub verified_block: u64,
    pub listings: Vec<ModuleListingChainRecord>,
    /// Active mapping entries omitted because ownership, token existence,
    /// eligibility, or approval no longer makes them buyable.
    pub stale_listings: u32,
    /// Active entries omitted because their structural/contract metadata was
    /// not a valid canonical module record.
    pub malformed_listings: u32,
}

/// Read-only client detached from [`BlockchainBridge`]'s wallet mutex.
///
/// Commands clone this small value while holding the mutex, release the guard,
/// and only then perform network I/O. Catalog refreshes therefore cannot block
/// unrelated signing or vault operations for the duration of many RPC calls.
#[derive(Debug, Clone)]
pub struct ModuleMarketReader {
    rpc_url: String,
    marketplace: Address,
    modules: Address,
    standing_release: Option<crate::network_config::StandingRelease>,
}

/// Current approval path for one canonical module token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleListingApprovalChain {
    Required,
    Token,
    Blanket,
}

/// One live seller listing, without its untrusted description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedModuleListingChainRecord {
    pub listing_id: String,
    pub seller: String,
    pub price_wei: String,
    pub token_id: String,
    pub collection: String,
}

/// Pinned accepted state used to decide whether a module can be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleListingChainState {
    pub verified_block: u64,
    pub token_id: String,
    pub owner: Option<String>,
    pub module: Option<ModuleChainRecord>,
    pub equipped_by: Option<String>,
    pub marketplace_eligible: bool,
    pub approval: ModuleListingApprovalChain,
    pub active_listing: Option<OwnedModuleListingChainRecord>,
}

/// Mutation whose claimed effect has already been re-read from accepted state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleListingMutationKind {
    NoChange,
    ApprovalConfirmed,
    ListingConfirmed,
    ListingCancelled,
    DealRulesActive,
}

#[derive(Debug, Clone)]
pub struct ModuleListingMutationOutcome {
    pub kind: ModuleListingMutationKind,
    pub tx_hash: Option<String>,
    pub state: ModuleListingChainState,
}

/// Accepted-chain purchase preflight for one exact canonical module listing.
///
/// The command layer classifies this as ready, stale, self-purchase, inactive,
/// or unaffordable. Keeping every numeric value as a decimal string prevents
/// JavaScript from rounding token ids, listing ids, prices, balances, or fees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModulePurchaseQuoteChain {
    pub verified_block: u64,
    pub buyer: String,
    pub listing_id: String,
    pub seller: String,
    pub price_wei: String,
    pub token_id: String,
    pub collection: String,
    pub active: bool,
    pub active_listing_id: String,
    pub current_owner: Option<String>,
    pub module: Option<ModuleChainRecord>,
    pub marketplace_eligible: bool,
    pub equipped_by: Option<String>,
    pub approved: bool,
    pub balance_wei: String,
    pub estimated_network_fee_wei: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleDealStatusChain {
    Active,
    Released,
    Refunded,
}

/// One canonical module deal whose custody agrees with its contract status at
/// the same accepted block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDealChainRecord {
    pub verified_block: u64,
    pub observed_at: u64,
    pub deal_id: String,
    pub buyer: String,
    pub seller: String,
    pub token_id: String,
    pub amount_wei: String,
    pub status: ModuleDealStatusChain,
    pub collection: String,
    pub auto_release_at: u64,
    pub refund_requested: bool,
    pub current_owner: String,
    pub module: ModuleChainRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDealChainSnapshot {
    pub verified_block: u64,
    pub observed_at: u64,
    pub wallet: String,
    pub deals: Vec<ModuleDealChainRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleDealMutationKind {
    PurchaseConfirmed,
    ReleaseConfirmed,
    RefundRequested,
    RefundConfirmed,
}

#[derive(Debug, Clone)]
pub struct ModuleDealMutationOutcome {
    pub kind: ModuleDealMutationKind,
    pub tx_hash: Option<String>,
    pub deal: ModuleDealChainRecord,
}

/// Signing client detached from [`BlockchainBridge`]'s mutex before any RPC.
///
/// This type deliberately has no `Debug`: its signer must never enter a span
/// or error message through derived formatting.
#[derive(Clone)]
pub struct ModuleMarketWriter {
    rpc_url: String,
    marketplace: Address,
    modules: Address,
    signer: PrivateKeySigner,
}

/// Sender authorization carried from the accepted funding transaction to the
/// one named relay. Every integer remains inside the protocol's u64 bounds;
/// hashes, signatures, and addresses stay explicit hex strings on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelayAuthorizationWire {
    pub policy_hash: String,
    pub route_nonce: String,
    pub payload_commitment: String,
    pub delivery_mode: u8,
    pub relay_route_hash: String,
    pub sender: String,
    pub recipient: String,
    pub authorized_bytes: u64,
    pub relay_count: u8,
    pub maximum_charge_navax: u64,
    pub issued_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaidRelayRequestWire {
    pub intent_id: String,
    pub authorization: RelayAuthorizationWire,
    pub relayers: Vec<String>,
    pub sender_signature: String,
    pub route_id: String,
    pub funding_tx_hash: String,
    /// Exact draft JSON committed by `payload_commitment` and forwarded once
    /// by the selected relay. It is never logged by the reward path.
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelayContributionWire {
    pub authorization_hash: String,
    pub hop_index: u8,
    pub relayer: String,
    pub ingress: String,
    pub egress: String,
    pub payload_commitment: String,
    pub delivered_bytes: u64,
    pub forwarded_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaidRelayDeliveryWire {
    pub request: PaidRelayRequestWire,
    pub contribution: RelayContributionWire,
    pub contribution_signature: String,
    pub contribution_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaidRelaySettlementWire {
    pub intent_id: String,
    pub route_id: String,
    pub settlement_tx_hash: String,
    pub funding_tx_hash: String,
    pub sender: String,
    pub relayer: String,
    pub recipient: String,
    pub settled_reward_navax: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayFundingQuoteChain {
    pub chain_id: u64,
    pub contract: String,
    pub sender: String,
    pub relayer: String,
    pub recipient: String,
    pub authorized_bytes: u64,
    pub billed_bytes: u64,
    pub base_reward_navax: u64,
    pub maximum_work_navax: u64,
    pub settlement_gas_reserve_navax: u64,
    pub maximum_charge_navax: u64,
    pub balance_wei: String,
    pub estimated_wallet_gas_wei: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySettlementOutcome {
    pub route_id: String,
    pub funding_tx_hash: String,
    pub settlement_tx_hash: String,
    pub relayer_reward_navax: u64,
}

/// Detached relay settlement client. It deliberately has no `Debug`: the
/// cloned signer must never enter logs or IPC error text.
#[derive(Clone)]
pub struct RelaySettlementWriter {
    rpc_url: String,
    chain_id: u64,
    contract: Address,
    signer: PrivateKeySigner,
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
    pub identity_vault: Vault<crate::vault_key::WrappedKeyProvider>,
    /// A second handle on the same key provider the vault holds.
    ///
    /// `Vault` does not expose its provider, and unlocking is not a vault
    /// operation — it is what has to happen before one is possible. Both
    /// handles share the unlock slot, so opening this one opens the vault.
    pub vault_key: crate::vault_key::WrappedKeyProvider,
    pub storage_path: PathBuf,
    pub chain_cache_path: PathBuf,
    pub module_loadout_cache_path: PathBuf,
    pub pending_relay_path: PathBuf,
    pub relayed_history_path: PathBuf,
    pub content_store_path: PathBuf,
    pub received_content_path: PathBuf,
    pub relay_boost_path: PathBuf,
    /// Records that this wallet's key has been shown to its owner.
    ///
    /// Plaintext on purpose: it holds an address and a timestamp, never key
    /// material, and encrypting it would put the record of "you can recover
    /// this wallet" behind the very thing the user may have lost access to.
    pub key_backup_path: PathBuf,
    pub rpc_url: String,
    pub chain_id: u64,
    pub escrow_address: Option<Address>,
    pub marketplace_address: Option<Address>,
    /// Marketplace reviewed and deployed together with
    /// `module_market_modules_address`.
    /// Separate from the legacy voucher marketplace above.
    pub module_marketplace_address: Option<Address>,
    /// Canonical module collection from the same reviewed MARKET release.
    /// Separate from the development-overridable inventory collection below.
    pub module_market_modules_address: Option<Address>,
    pub voucher_address: Option<Address>,
    pub modules_address: Option<Address>,
    pub relay_settlement_address: Option<Address>,
    /// Reviewed registry and independent RPC quorum for public seller standing.
    pub standing_release: Option<crate::network_config::StandingRelease>,
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
        let marketplace_address = network
            .marketplace()
            .and_then(|s| Address::from_str(&s).ok());
        let module_marketplace_address = network
            .module_marketplace()
            .and_then(|s| Address::from_str(&s).ok());
        let module_market_modules_address = network
            .module_market_modules()
            .and_then(|s| Address::from_str(&s).ok());
        let voucher_address = network.voucher().and_then(|s| Address::from_str(&s).ok());
        let relay_settlement_address = network
            .relay_settlement()
            .and_then(|address| Address::from_str(&address).ok());
        // Once a reviewed release pair exists, VAULT and loadout must use the
        // same collection as MARKET so a confirmed purchase appears in the
        // buyer's MODULES view. Development overrides remain useful only
        // while no reviewed pair has been published.
        let modules_address = module_market_modules_address.or_else(|| {
            network
                .modules()
                .and_then(|address| Address::from_str(&address).ok())
        });
        let standing_release = network.standing_release();

        let app_dir = crate::app_paths::data_dir();

        let vault_key = crate::vault_key::platform_provider(
            app_dir.join("vault.key"),
            crate::vault_key::VaultUnlock::new(),
        );
        let mut bridge = Self {
            identities: Vec::new(),
            identity_vault: Vault::new(app_dir.join("vault.enc"), vault_key.clone()),
            vault_key,
            storage_path: app_dir.join("snapshot.enc"),
            chain_cache_path: app_dir.join("chain_cache.json"),
            module_loadout_cache_path: app_dir.join("module_loadout_cache.json"),
            pending_relay_path: app_dir.join("pending_relay_txs.json"),
            relayed_history_path: app_dir.join("relayed_history.json"),
            content_store_path: app_dir.join("content_store.json"),
            received_content_path: app_dir.join("received_content.json"),
            relay_boost_path: app_dir.join("relay_boost.json"),
            key_backup_path: app_dir.join("key_backup.json"),
            rpc_url,
            chain_id: network.network.chain_id(),
            escrow_address,
            marketplace_address,
            module_marketplace_address,
            module_market_modules_address,
            voucher_address,
            modules_address,
            relay_settlement_address,
            standing_release,
            current_session: None,
        };
        let _ = bridge.load_identities();
        bridge
    }

    fn save_identities(&self) -> Result<(), Box<dyn Error>> {
        self.identity_vault.save(&self.identities)?;
        Ok(())
    }

    /// Whether the vault can be read yet, and whether one exists at all.
    #[must_use]
    pub fn vault_state(&self) -> crate::vault_key::VaultState {
        self.vault_key.state()
    }

    /// Supplies the passphrase and, if it opens the vault, loads the
    /// identities it protects.
    ///
    /// Loading here rather than leaving it to the caller is what makes the
    /// unlock complete: a vault that opened but whose identities were never
    /// read would leave every signing path reporting an empty wallet.
    pub fn unlock_vault(
        &mut self,
        secret: &Secret,
    ) -> Result<(), crate::vault_key::UnlockFailure> {
        self.vault_key.unlock(secret)?;

        if self.identity_vault.exists() {
            match self.identity_vault.load::<Vec<IdentityRecord>>() {
                Ok(records) => self.identities = records,
                Err(error) => {
                    // The key opened but the vault did not. Refusing here
                    // rather than generating a replacement wallet is the
                    // difference between a bad day and a destroyed one.
                    tracing::error!(target: "cabalmesh::vault", %error, "vault key opened but identities did not");
                    self.vault_key.lock();
                    return Err(crate::vault_key::UnlockFailure::Unusable);
                }
            }
        }

        if self.identities.is_empty() {
            let _ = self.generate_new_identity("Genesis Fox".to_string(), "🦊".to_string());
        }
        Ok(())
    }

    /// Forgets the key. The passphrase is required again.
    pub fn lock_vault(&mut self) {
        self.identities.clear();
        self.vault_key.lock();
    }

    pub fn load_identities(&mut self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        // Nothing below is possible without the key, and every branch of it
        // would do the wrong thing without one: migration would fail, and the
        // "no vault yet" branch would generate a wallet on top of one that is
        // merely locked. A locked vault is a state, not a fresh install.
        if !self.vault_key.is_unlocked() {
            return Err(Box::new(cabal_vault::VaultError::KeyUnavailable));
        }

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
                        return self
                            .generate_new_identity("Primary Fox".to_string(), "🦊".to_string());
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

    pub fn generate_new_identity(
        &mut self,
        alias: String,
        emoji: String,
    ) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        tracing::info!("🆕 Generating NEW Identity '{}' [{}]...", alias, emoji);
        let signer = PrivateKeySigner::random();
        let private_key_hex = format!("0x{}", hex::encode(signer.to_bytes()));
        self.identities.push(IdentityRecord {
            alias,
            emoji,
            private_key_hex: private_key_hex.into(),
        });
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

    /// Whether the current wallet's key has been shown to its owner.
    ///
    /// A record naming a **different** address is not evidence about this
    /// wallet, which is the entire reason the address is stored alongside the
    /// timestamp. Exporting wallet A and then restoring wallet B must not make
    /// B look backed up.
    #[must_use]
    pub fn key_backup(&self) -> Option<KeyBackupRecord> {
        let address = self.get_primary_address();
        fs::read_to_string(&self.key_backup_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<KeyBackupRecord>(&raw).ok())
            .filter(|record| record.address.eq_ignore_ascii_case(&address))
    }

    /// Reveals the current wallet's private key, and records that it was
    /// revealed.
    ///
    /// Revealing and recording are one operation on purpose. The record is
    /// what later permits this wallet to be replaced, so a path that showed
    /// the key without recording it would leave the owner unable to restore,
    /// and a path that recorded without showing would be a lie.
    ///
    /// The key is returned and never logged. Nothing in this function may
    /// gain a `tracing` call that takes the key, however useful it looks.
    pub fn reveal_primary_key(&self) -> Result<(String, Secret, KeyBackupRecord), Box<dyn Error>> {
        let identity = self.identities.first().ok_or("no identity to reveal")?;
        let address = PrivateKeySigner::from_str(identity.private_key_hex.expose())?
            .address()
            .to_string();

        let record = KeyBackupRecord {
            address: address.clone(),
            exported_at: Utc::now(),
        };
        JsonStore::new(&self.key_backup_path).save(&record)?;

        Ok((address, identity.private_key_hex.clone(), record))
    }

    /// Replaces the current wallet with one derived from a user-supplied
    /// private key.
    ///
    /// Refuses unless the wallet being replaced has been revealed at least
    /// once, because this is the operation that destroys it. The check lives
    /// here rather than in the command layer so that a second caller cannot
    /// arrive later and skip it.
    pub fn restore_identity(
        &mut self,
        private_key_hex: &str,
        alias: String,
        emoji: String,
    ) -> Result<WalletRestore, Box<dyn Error>> {
        let trimmed = private_key_hex.trim();
        let normalized = if trimmed.starts_with("0x") {
            trimmed.to_string()
        } else {
            format!("0x{trimmed}")
        };
        // Validated before anything is persisted: a rejected key must leave
        // the existing wallet exactly as it was.
        let Ok(signer) = PrivateKeySigner::from_str(&normalized) else {
            return Ok(WalletRestore::InvalidKey);
        };

        if !self.identities.is_empty() && self.key_backup().is_none() {
            return Ok(WalletRestore::BackupRequired {
                address: self.get_primary_address(),
            });
        }

        let address = signer.address().to_string();
        self.identities = vec![IdentityRecord {
            alias,
            emoji,
            private_key_hex: normalized.into(),
        }];
        // State belonging to the replaced wallet, not this one. A balance
        // snapshot or a relay multiplier carried across would describe an
        // address that is no longer here.
        let _ = self.delete_snapshot();
        let _ = fs::remove_file(&self.relay_boost_path);
        // The restored wallet has not been exported *from this device*. The
        // user may hold the key they just typed, but this device has no
        // evidence of that, and inheriting the previous wallet's record would
        // let one export authorise unlimited replacements.
        let _ = fs::remove_file(&self.key_backup_path);
        self.save_identities()?;

        Ok(WalletRestore::Replaced {
            address,
            identities: self.get_identity_views()?,
        })
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
    async fn sign_offline(
        &self,
        to: Address,
        calldata: Bytes,
        value: U256,
        summary: &str,
    ) -> Result<QueuedTx, Box<dyn Error>> {
        let cache = self
            .load_chain_cache()
            .ok_or("No cached chain state available — never been online yet")?;
        let signer = self.primary_signer()?;
        let wallet = EthereumWallet::from(signer);

        // +20% buffer on the cached gas price in case it's gone slightly stale.
        let gas_price: u128 = cache
            .gas_price_wei
            .parse::<u128>()
            .unwrap_or(30_000_000_000);
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
        let id = format!(
            "tx-{}-{}",
            Utc::now().timestamp_millis(),
            hex::encode(suffix)
        );

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

        tracing::info!(
            "📡 [Bridge] Signed offline, queued for mesh relay: {} ({})",
            queued.id,
            summary
        );
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
        let receipt = provider
            .send_raw_transaction(&raw_bytes)
            .await?
            .get_receipt()
            .await?;

        tracing::info!(
            "✅ [Bridge] Relayed transaction confirmed. Tx: {:?}",
            receipt.transaction_hash
        );
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
            let Ok(raw_bytes) = hex::decode(hex_str) else {
                return true;
            };
            let Ok(envelope) = TxEnvelope::decode_2718(&mut raw_bytes.as_slice()) else {
                return true;
            };
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
    pub fn record_relayed_tx(
        &self,
        summary: &str,
        tx_hash: &str,
        reward_avax: &str,
    ) -> Result<(), Box<dyn Error>> {
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

    pub fn mark_relay_tx_status(
        &self,
        id: &str,
        status: &str,
        tx_hash: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
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
        let Ok(url) = self.rpc_url.parse() else {
            return false;
        };
        let provider = ProviderBuilder::new().connect_http(url);
        matches!(
            timeout(Duration::from_secs(4), provider.get_block_number()).await,
            Ok(Ok(_))
        )
    }

    /// Spanned so the RPC round trip and everything it logs is attributable to
    /// one sync, which matters when several run concurrently after a reconnect.
    #[tracing::instrument(skip(self), fields(rpc = %self.rpc_url))]
    pub async fn sync_state(
        &self,
        wallet_address_override: &str,
    ) -> Result<Snapshot, Box<dyn Error>> {
        let primary = self.get_primary_address();
        let target = if primary != "unknown" {
            primary
        } else {
            wallet_address_override.to_string()
        };
        let address = Address::from_str(&target)?;

        tracing::info!(
            "🔄 [Bridge] Fetching native AVAX balance from {}",
            self.rpc_url
        );

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
            Some(s) if s.is_active => format!(
                "Instant Session Engine: Active [Agent: {}...]",
                &s.authority[..6]
            ),
            _ => "Instant Session Engine: Inactive".to_string(),
        }
    }

    /// Creates an on-chain escrow deal, locking `amount_wei` for `payee`.
    /// If the RPC can't be reached within a few seconds, falls back to
    /// signing the transaction offline and queuing it for mesh relay.
    /// `skip(self)` keeps the bridge — which holds signers — out of the span,
    /// while the escrow parameters stay visible.
    pub async fn create_escrow(
        &self,
        payee: &str,
        amount_wei: U256,
        expiry_unix: u64,
    ) -> Result<TxResult, Box<dyn Error>> {
        // A thin wrapper so the frozen desktop surface keeps the exact
        // signature ticket 09 snapshotted. Everything below is shared.
        Ok(
            match self
                .create_escrow_detailed(payee, amount_wei, expiry_unix)
                .await?
            {
                EscrowOutcome::Confirmed { escrow_id, .. } => TxResult::Confirmed { id: escrow_id },
                EscrowOutcome::Queued { queue_id } => TxResult::Queued { queue_id },
            },
        )
    }

    /// Creates an escrow and reports the transaction hash alongside the id.
    ///
    /// Exists because `TxResult` carries only the escrow id, and the proof
    /// screen renders a hash it claims is genuine. Reading the hash back with a
    /// second call would be a different transaction's worth of trust — the
    /// receipt in hand is the only thing that actually proves this settlement.
    #[tracing::instrument(skip(self), fields(payee = %payee, expiry = expiry_unix))]
    pub async fn create_escrow_detailed(
        &self,
        payee: &str,
        amount_wei: U256,
        expiry_unix: u64,
    ) -> Result<EscrowOutcome, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self
            .escrow_address
            .ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;
        let payee_addr = Address::from_str(payee)?;

        // Build calldata once — reused for both the online path and the offline fallback.
        let unsigned_provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let unsigned_contract = IEscrow::new(escrow_address, unsigned_provider);
        let calldata = unsigned_contract
            .createEscrow(payee_addr, U256::from(expiry_unix))
            .calldata()
            .clone();

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
        })
        .await;

        let receipt = match online_result {
            Ok(Ok(receipt)) => receipt,
            Ok(Err(e)) => return Err(e.into()),
            Err(_timed_out) => {
                tracing::warn!(
                    "⚠️  [Bridge] RPC unreachable — signing create_escrow offline for mesh relay."
                );
                let queued = self
                    .sign_offline(escrow_address, calldata, amount_wei, "Create escrow")
                    .await?;
                return Ok(EscrowOutcome::Queued {
                    queue_id: queued.id,
                });
            }
        };

        let escrow_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IEscrow::EscrowCreated>().ok())
            .map(|l| l.inner.data.escrowId.to::<u64>())
            .ok_or("EscrowCreated event not found in receipt")?;

        tracing::info!(
            "✅ [Bridge] Escrow {} created. Tx: {:?}",
            escrow_id,
            receipt.transaction_hash
        );
        Ok(EscrowOutcome::Confirmed {
            escrow_id,
            tx_hash: format!("{:?}", receipt.transaction_hash),
        })
    }

    pub async fn release_escrow(&self, escrow_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self
            .escrow_address
            .ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        let receipt = contract
            .release(U256::from(escrow_id))
            .send()
            .await?
            .get_receipt()
            .await?;
        tracing::info!(
            "✅ [Bridge] Escrow {} released. Tx: {:?}",
            escrow_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    pub async fn refund_escrow(&self, escrow_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let escrow_address = self
            .escrow_address
            .ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IEscrow::new(escrow_address, provider);

        let receipt = contract
            .refund(U256::from(escrow_id))
            .send()
            .await?
            .get_receipt()
            .await?;
        tracing::info!(
            "✅ [Bridge] Escrow {} refunded. Tx: {:?}",
            escrow_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Reads the on-chain state of a deal (no signer required).
    pub async fn get_escrow_status(
        &self,
        escrow_id: u64,
    ) -> Result<serde_json::Value, Box<dyn Error>> {
        let escrow_address = self
            .escrow_address
            .ok_or("ESCROW_CONTRACT_ADDRESS not configured")?;
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
    pub async fn mint_voucher(
        &self,
        voucher_type: &str,
        description: &str,
    ) -> Result<u64, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let voucher_address = self
            .voucher_address
            .ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
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

        tracing::info!(
            "✅ [Bridge] Voucher {} minted. Tx: {:?}",
            token_id,
            receipt.transaction_hash
        );
        Ok(token_id)
    }

    /// Approves the Marketplace contract to pull a specific voucher out of
    /// the seller's wallet, required before that voucher can be listed.
    pub async fn approve_voucher(&self, token_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let voucher_address = self
            .voucher_address
            .ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let receipt = contract
            .approve(marketplace_address, U256::from(token_id))
            .send()
            .await?
            .get_receipt()
            .await?;

        tracing::info!(
            "✅ [Bridge] Voucher {} approved for Marketplace. Tx: {:?}",
            token_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Publishes a real on-chain listing backed by an owned, approved voucher.
    /// Returns the generated listing id.
    pub async fn create_asset_listing(
        &self,
        description: &str,
        price_wei: U256,
        token_id: u64,
    ) -> Result<u64, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

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

        tracing::info!(
            "✅ [Bridge] Listing {} created. Tx: {:?}",
            listing_id,
            receipt.transaction_hash
        );
        Ok(listing_id)
    }

    /// Reads all active listings from the Marketplace contract (no signer required).
    pub async fn get_active_asset_listings(&self) -> Result<Vec<AssetListingView>, Box<dyn Error>> {
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;
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
    pub async fn buy_listing(
        &self,
        listing_id: u64,
        price_wei: U256,
    ) -> Result<TxResult, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let unsigned_provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let unsigned_contract = IMarketplace::new(marketplace_address, unsigned_provider);

        // The contract rejects a seller buying their own listing, so this is
        // not the safety check — it is the error message. Catching it here
        // turns a raw revert into a sentence.
        let listing = unsigned_contract
            .listings(U256::from(listing_id))
            .call()
            .await?;
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
            if voucher_contract
                .ownerOf(listing.tokenId)
                .call()
                .await
                .is_err()
            {
                return Err("This listing's item no longer exists — it was likely redeemed by the seller before anyone bought it".into());
            }
        }

        let calldata = unsigned_contract
            .buy(U256::from(listing_id))
            .calldata()
            .clone();

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
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
        })
        .await;

        let receipt = match online_result {
            Ok(Ok(receipt)) => receipt,
            Ok(Err(e)) => return Err(e.into()),
            Err(_timed_out) => {
                tracing::warn!(
                    "⚠️  [Bridge] RPC unreachable — signing buy_listing offline for mesh relay."
                );
                let queued = self
                    .sign_offline(marketplace_address, calldata, price_wei, "Buy listing")
                    .await?;
                return Ok(TxResult::Queued {
                    queue_id: queued.id,
                });
            }
        };

        let deal_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IMarketplace::DealCreated>().ok())
            .map(|l| l.inner.data.dealId.to::<u64>())
            .ok_or("DealCreated event not found in receipt")?;

        tracing::info!(
            "✅ [Bridge] Deal {} created (voucher + AVAX locked). Tx: {:?}",
            deal_id,
            receipt.transaction_hash
        );
        Ok(TxResult::Confirmed { id: deal_id })
    }

    /// Withdraws an unsold listing. Only the seller can, and only while it is
    /// still active — once a buyer has paid, the deal rules take over.
    pub async fn cancel_listing(&self, listing_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract
            .cancelListing(U256::from(listing_id))
            .send()
            .await?
            .get_receipt()
            .await?;
        tracing::info!(
            "✅ [Bridge] Listing {} cancelled. Tx: {:?}",
            listing_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Releases a deal: pays the seller and transfers the voucher to the buyer.
    ///
    /// The buyer can call this at any time. Anyone can once the deal's
    /// `autoReleaseAt` has passed — that is what stops a buyer who walks away
    /// from stranding the seller's voucher and payment in the contract.
    pub async fn release_deal(&self, deal_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract
            .releaseDeal(U256::from(deal_id))
            .send()
            .await?
            .get_receipt()
            .await?;
        tracing::info!(
            "✅ [Bridge] Deal {} released. Tx: {:?}",
            deal_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// The buyer's consent to cancel a deal. Moves nothing on its own — it
    /// only unlocks [`BlockchainBridge::refund_deal`] for the seller.
    pub async fn request_refund(&self, deal_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract
            .requestRefund(U256::from(deal_id))
            .send()
            .await?
            .get_receipt()
            .await?;
        tracing::info!(
            "✅ [Bridge] Refund requested on deal {}. Tx: {:?}",
            deal_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Refunds a deal: returns AVAX to the buyer and the voucher to the seller.
    ///
    /// Callable by the seller, and only after the buyer has called
    /// [`BlockchainBridge::request_refund`]. Cancelling a paid deal takes both
    /// sides, so neither holds a unilateral option over the other.
    pub async fn refund_deal(&self, deal_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IMarketplace::new(marketplace_address, provider);

        let receipt = contract
            .refundDeal(U256::from(deal_id))
            .send()
            .await?
            .get_receipt()
            .await?;
        tracing::info!(
            "✅ [Bridge] Deal {} refunded. Tx: {:?}",
            deal_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Burns a voucher the caller owns, claiming the service it represents.
    /// Requires real on-chain ownership (`ownerOf(tokenId) == msg.sender`).
    pub async fn redeem_voucher(&self, token_id: u64) -> Result<String, Box<dyn Error>> {
        let signer = self.primary_signer()?;
        let voucher_address = self
            .voucher_address
            .ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;

        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let receipt = contract
            .redeemVoucher(U256::from(token_id))
            .send()
            .await?
            .get_receipt()
            .await?;
        tracing::info!(
            "✅ [Bridge] Voucher {} redeemed. Tx: {:?}",
            token_id,
            receipt.transaction_hash
        );
        Ok(format!("{:?}", receipt.transaction_hash))
    }

    /// Reads the current on-chain owner of a voucher (no signer required).
    pub async fn get_voucher_owner(&self, token_id: u64) -> Result<String, Box<dyn Error>> {
        let voucher_address = self
            .voucher_address
            .ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let contract = IVoucher::new(voucher_address, provider);

        let owner = contract.ownerOf(U256::from(token_id)).call().await?;
        Ok(owner.to_string())
    }

    /// Lists every voucher the given address currently owns on-chain — used
    /// by the Redeem page so it only ever shows vouchers the caller really
    /// holds, never a claim it has to trust.
    pub async fn get_owned_vouchers(
        &self,
        owner: &str,
    ) -> Result<Vec<VoucherView>, Box<dyn Error>> {
        let voucher_address = self
            .voucher_address
            .ok_or("VOUCHER_CONTRACT_ADDRESS not configured")?;
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

    /// A detached reader for the reviewed canonical module-market pair.
    ///
    /// Both addresses must exist. The legacy voucher marketplace is kept in a
    /// different field and can never satisfy this constructor.
    #[must_use]
    pub fn module_market_reader(&self) -> Option<ModuleMarketReader> {
        Some(ModuleMarketReader {
            rpc_url: self.rpc_url.clone(),
            marketplace: self.module_marketplace_address?,
            modules: self.module_market_modules_address?,
            standing_release: self.standing_release,
        })
    }

    /// A detached signer for the same reviewed pair used by MARKET reads.
    ///
    /// Development module overrides and the legacy voucher marketplace cannot
    /// satisfy this constructor. The signer is cloned while the bridge mutex
    /// is held; callers release the guard before awaiting chain I/O.
    pub fn module_market_writer(&self) -> Result<Option<ModuleMarketWriter>, Box<dyn Error>> {
        let (Some(marketplace), Some(modules)) = (
            self.module_marketplace_address,
            self.module_market_modules_address,
        ) else {
            return Ok(None);
        };
        Ok(Some(ModuleMarketWriter {
            rpc_url: self.rpc_url.clone(),
            marketplace,
            modules,
            signer: self.primary_signer()?,
        }))
    }

    /// A detached signer for the versioned sender-funded relay settlement.
    /// `None` is the safe release state until an address is explicitly
    /// configured; the old local reward estimate can never satisfy it.
    pub fn relay_settlement_writer(&self) -> Result<Option<RelaySettlementWriter>, Box<dyn Error>> {
        let Some(contract) = self.relay_settlement_address else {
            return Ok(None);
        };
        Ok(Some(RelaySettlementWriter {
            rpc_url: self.rpc_url.clone(),
            chain_id: self.chain_id,
            contract,
            signer: self.primary_signer()?,
        }))
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
        let balance = contract
            .balanceOf(owner)
            .block(snapshot_block)
            .call()
            .await?;
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
            let data = contract
                .assetData(token_id)
                .block(snapshot_block)
                .call()
                .await?;
            let soulbound = contract
                .locked(token_id)
                .block(snapshot_block)
                .call()
                .await?;
            let revoked = contract
                .revoked(token_id)
                .block(snapshot_block)
                .call()
                .await?;
            let current_owner = contract
                .ownerOf(token_id)
                .block(snapshot_block)
                .call()
                .await?;
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
    pub async fn get_module_loadout(&self) -> Result<ModuleLoadoutChainSnapshot, Box<dyn Error>> {
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

            let current_owner = contract
                .ownerOf(token_id)
                .block(snapshot_block)
                .call()
                .await?;
            let equipped_by = contract
                .equippedBy(token_id)
                .block(snapshot_block)
                .call()
                .await?;
            let data = contract
                .assetData(token_id)
                .block(snapshot_block)
                .call()
                .await?;
            let soulbound = contract
                .locked(token_id)
                .block(snapshot_block)
                .call()
                .await?;
            let revoked = contract
                .revoked(token_id)
                .block(snapshot_block)
                .call()
                .await?;

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
        (snapshot.collection == collection.to_string() && snapshot.operator == operator.to_string())
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
        let contract = IModules::new(collection, provider.clone());
        let canonical_market = match (
            self.module_marketplace_address,
            self.module_market_modules_address,
        ) {
            (Some(marketplace), Some(modules)) if modules == collection => {
                let market = IMarketplace::new(marketplace, provider);
                if market.activeListingOf(collection, token_id).call().await? != U256::ZERO {
                    return Err("listed module must be cancelled before equipping".into());
                }
                Some(market)
            }
            _ => None,
        };
        let receipt = contract.equip(token_id).send().await?.get_receipt().await?;
        if !receipt.status() {
            return Err("module equip reverted".into());
        }
        let loadout = self.get_module_loadout().await?;
        let token_id_text = token_id.to_string();
        if !loadout
            .modules
            .iter()
            .any(|module| module.token_id == token_id_text)
        {
            return Err("module equip was not confirmed by accepted state".into());
        }
        if let Some(market) = canonical_market {
            if market.activeListingOf(collection, token_id).call().await? != U256::ZERO {
                let rollback = contract
                    .unequip(token_id)
                    .send()
                    .await?
                    .get_receipt()
                    .await?;
                if !rollback.status() {
                    return Err("listed module equip rollback was not confirmed".into());
                }
                let rolled_back = self.get_module_loadout().await?;
                if rolled_back
                    .modules
                    .iter()
                    .any(|module| module.token_id == token_id_text)
                {
                    return Err("listed module remained equipped after rollback".into());
                }
                return Err("module became listed while equip was confirming".into());
            }
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
        let receipt = contract
            .unequip(token_id)
            .send()
            .await?
            .get_receipt()
            .await?;
        if !receipt.status() {
            return Err("module unequip reverted".into());
        }
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
        let marketplace_address = self
            .marketplace_address
            .ok_or("MARKETPLACE_CONTRACT_ADDRESS not configured")?;
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
                role: if deal.seller == my_addr {
                    "seller".to_string()
                } else {
                    "buyer".to_string()
                },
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

    fn save_content_store(
        &self,
        store: &std::collections::HashMap<u64, ContentRecord>,
    ) -> Result<(), Box<dyn Error>> {
        JsonStore::new(&self.content_store_path).save(store)?;
        Ok(())
    }

    fn load_received_content(&self) -> std::collections::HashMap<u64, ContentRecord> {
        fs::read_to_string(&self.received_content_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    fn save_received_content(
        &self,
        store: &std::collections::HashMap<u64, ContentRecord>,
    ) -> Result<(), Box<dyn Error>> {
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
    pub fn store_content(
        &self,
        token_id: u64,
        mut record: ContentRecord,
    ) -> Result<(), Box<dyn Error>> {
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
    pub fn receive_content(
        &self,
        token_id: u64,
        text: &str,
        signature: &str,
        expected_seller: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let sig = Signature::from_str(signature)?;
        let recovered = sig.recover_address_from_msg(text.as_bytes())?;
        let expected = Address::from_str(expected_seller)?;

        if recovered != expected {
            tracing::warn!(
                "⚠️  Content delivery rejected: signature recovered {} but expected seller {}",
                recovered,
                expected
            );
            return Ok(false);
        }

        let fingerprint = format!("0x{}", hex::encode(&keccak256(text.as_bytes())[..8]));
        let mut store = self.load_received_content();
        store.insert(
            token_id,
            ContentRecord {
                token_id,
                text: text.to_string(),
                fingerprint,
                signature: signature.to_string(),
                signer_address: recovered.to_string(),
            },
        );
        self.save_received_content(&store)?;
        Ok(true)
    }

    pub fn get_received_content(&self, token_id: u64) -> Option<ContentRecord> {
        self.load_received_content().get(&token_id).cloned()
    }
}

impl ModuleMarketReader {
    const PAGE_SIZE: u64 = 128;
    const MAX_LISTING_HISTORY: u64 = 100_000;

    /// Independently verifies public standing for each exact seller address.
    ///
    /// Provider errors are data, not command failures: every seller receives
    /// an explicit `UNKNOWN` reason from `cabal-standing`. No local settlement
    /// count or seller-supplied field participates.
    pub async fn seller_standing(
        &self,
        sellers: &[Address],
        now_ms: u64,
    ) -> std::collections::BTreeMap<Address, cabal_standing::PublicStanding> {
        use cabal_standing::{
            verify_public_standing, EvmAddress, PublicStanding, RegistryConfig,
            UnknownStandingReason,
        };

        let mut result = std::collections::BTreeMap::new();
        let Some(release) = self.standing_release else {
            for seller in sellers {
                result.insert(
                    *seller,
                    verify_public_standing(
                        None,
                        EvmAddress::from_bytes(seller.into_array()),
                        &[],
                        now_ms,
                    ),
                );
            }
            return result;
        };

        let registry = Address::from_str(release.registry).ok();
        let verifier_config = registry.and_then(|registry| {
            RegistryConfig::try_new(
                release.chain_id,
                EvmAddress::from_bytes(registry.into_array()),
                release.maximum_age_ms,
                release.minimum_provider_quorum,
            )
            .ok()
        });
        let provider_ids = release
            .providers
            .iter()
            .map(|provider| cabal_standing::ProviderId::try_new(provider.id))
            .collect::<Result<Vec<_>, _>>();
        let (Some(registry), Some(verifier_config), Ok(provider_ids)) =
            (registry, verifier_config, provider_ids)
        else {
            for seller in sellers {
                result.insert(
                    *seller,
                    PublicStanding::Unknown(UnknownStandingReason::Unconfigured),
                );
            }
            return result;
        };

        let head_reads = futures::future::join_all(
            release
                .providers
                .iter()
                .copied()
                .map(read_accepted_standing_head),
        )
        .await;
        let pinned_block = head_reads
            .iter()
            .filter_map(|read| read.as_ref().ok().map(|head| head.block_number))
            .min();

        for seller in sellers {
            let seller_evm = EvmAddress::from_bytes(seller.into_array());
            let Some(pinned_block) = pinned_block else {
                result.insert(
                    *seller,
                    verify_public_standing(
                        Some(&verifier_config),
                        seller_evm,
                        &provider_ids
                            .iter()
                            .copied()
                            .map(|provider_id| cabal_standing::ProviderObservation {
                                provider_id,
                                read: cabal_standing::ProviderRead::Unavailable,
                            })
                            .collect::<Vec<_>>(),
                        now_ms,
                    ),
                );
                continue;
            };

            let observations = futures::future::join_all(
                release
                    .providers
                    .iter()
                    .copied()
                    .zip(provider_ids.iter().copied())
                    .zip(head_reads.iter().cloned())
                    .map(|((endpoint, provider_id), head)| {
                        read_standing_observation(
                            endpoint,
                            provider_id,
                            head,
                            release.chain_id,
                            registry,
                            *seller,
                            pinned_block,
                        )
                    }),
            )
            .await;
            result.insert(
                *seller,
                verify_public_standing(Some(&verifier_config), seller_evm, &observations, now_ms),
            );
        }

        result
    }

    /// Reads and validates every currently buyable canonical module listing at
    /// one accepted Avalanche C-Chain head.
    ///
    /// A contract listing can remain marked active after its token is burned,
    /// transferred, revoked, or unapproved outside the marketplace. Those
    /// entries are counted as stale and omitted. Transport failures fail the
    /// whole refresh; a vanished token's explicit contract revert only omits
    /// that entry.
    pub async fn active_listings(&self) -> Result<ModuleMarketChainSnapshot, Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider.clone());
        let modules = IModules::new(self.modules, provider);

        // Standard Avalanche C-Chain RPC serves accepted state by default.
        // Pin every view call to this exact head so cancellation, transfer,
        // metadata, and approval cannot be assembled from different states.
        let verified_block = marketplace.provider().get_block_number().await?;
        let block = BlockId::number(verified_block);
        let count = marketplace.listingCount().block(block).call().await?;
        if count > U256::from(Self::MAX_LISTING_HISTORY) {
            return Err("module marketplace history exceeds the supported safety bound".into());
        }
        let total = count.to::<u64>();
        let mut offset = 0_u64;
        let mut listings = Vec::new();
        let mut stale_listings = 0_u32;
        let mut malformed_listings = 0_u32;

        while offset < total {
            let page = marketplace
                .getActiveListingsPaged(U256::from(offset), U256::from(Self::PAGE_SIZE))
                .block(block)
                .call()
                .await?;
            if page.result.len() != page.ids.len() {
                return Err("module marketplace returned mismatched listing arrays".into());
            }

            let next_offset = page.nextOffset;
            if next_offset > U256::from(total) || next_offset <= U256::from(offset) {
                return Err("module marketplace returned an invalid page cursor".into());
            }

            for (listing, listing_id) in page.result.into_iter().zip(page.ids) {
                // Other allowed collections belong to other catalog surfaces.
                // Never resolve their seller prose as module metadata.
                if listing.collection != self.modules {
                    continue;
                }
                if !listing.active
                    || listing_id == U256::ZERO
                    || listing.seller == Address::ZERO
                    || listing.priceWei == U256::ZERO
                {
                    malformed_listings = malformed_listings.saturating_add(1);
                    continue;
                }

                let active_id = marketplace
                    .activeListingOf(self.modules, listing.tokenId)
                    .block(block)
                    .call()
                    .await?;
                if active_id != listing_id {
                    stale_listings = stale_listings.saturating_add(1);
                    continue;
                }

                let owner = match modules.ownerOf(listing.tokenId).block(block).call().await {
                    Ok(owner) => owner,
                    Err(error) if contract_call_reverted(&error) => {
                        stale_listings = stale_listings.saturating_add(1);
                        continue;
                    }
                    Err(error) => return Err(Box::new(error)),
                };
                if owner != listing.seller {
                    stale_listings = stale_listings.saturating_add(1);
                    continue;
                }

                let data = match modules.assetData(listing.tokenId).block(block).call().await {
                    Ok(data) => data,
                    Err(error) if contract_call_reverted(&error) => {
                        malformed_listings = malformed_listings.saturating_add(1);
                        continue;
                    }
                    Err(error) => return Err(Box::new(error)),
                };
                let soulbound = modules.locked(listing.tokenId).block(block).call().await?;
                let revoked = modules.revoked(listing.tokenId).block(block).call().await?;
                let eligible = modules
                    .isMarketplaceEligible(listing.tokenId)
                    .block(block)
                    .call()
                    .await?;
                let equipped_by = modules
                    .equippedBy(listing.tokenId)
                    .block(block)
                    .call()
                    .await?;
                let approved = modules
                    .getApproved(listing.tokenId)
                    .block(block)
                    .call()
                    .await?;
                let approved_for_all = modules
                    .isApprovedForAll(owner, self.marketplace)
                    .block(block)
                    .call()
                    .await?;
                if soulbound
                    || revoked
                    || !eligible
                    || equipped_by != Address::ZERO
                    || (approved != self.marketplace && !approved_for_all)
                {
                    stale_listings = stale_listings.saturating_add(1);
                    continue;
                }

                listings.push(ModuleListingChainRecord {
                    listing_id: listing_id.to_string(),
                    seller: listing.seller.to_string(),
                    price_wei: listing.priceWei.to_string(),
                    module: ModuleChainRecord {
                        token_id: listing.tokenId.to_string(),
                        collection: self.modules.to_string(),
                        owner: owner.to_string(),
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
                    },
                });
            }

            offset = next_offset.to::<u64>();
        }

        Ok(ModuleMarketChainSnapshot {
            verified_block,
            listings,
            stale_listings,
            malformed_listings,
        })
    }
}

impl RelaySettlementWriter {
    const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(90);

    #[must_use]
    pub const fn contract(&self) -> Address {
        self.contract
    }

    #[must_use]
    pub fn operator(&self) -> Address {
        self.signer.address()
    }

    /// Reads a current funding balance and estimates wallet gas for the exact
    /// route that will later be frozen in the confirmation dialog. Reward
    /// arithmetic itself is local and deterministic; RPC state contributes
    /// only balance, gas, chain identity, and accepted time.
    pub async fn funding_quote(
        &self,
        payload: &[u8],
        relayer: Address,
        recipient: Address,
    ) -> Result<RelayFundingQuoteChain, Box<dyn Error>> {
        self.validate_three_distinct(relayer, recipient)?;
        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        self.ensure_chain(&provider).await?;
        let issued_at = accepted_timestamp(&provider).await?;
        let (authorization, signature, quote) =
            self.signed_authorization(payload, relayer, recipient, issued_at)?;
        let contract = IRelaySettlement::new(self.contract, provider.clone());
        let value = U256::from(quote.maximum_charge().raw())
            .checked_mul(U256::from(1_000_000_000_u64))
            .ok_or("relay escrow conversion overflowed")?;
        let gas = contract
            .fundRoute(
                relay_authorization_contract(&authorization),
                vec![relayer],
                signature_bytes(&signature)?,
            )
            .from(self.signer.address())
            .value(value)
            .estimate_gas()
            .await?;
        let gas_price = provider.get_gas_price().await?;
        let estimated_wallet_gas_wei = U256::from(gas)
            .checked_mul(U256::from(gas_price))
            .ok_or("relay funding gas estimate overflowed")?;
        let balance = provider.get_balance(self.signer.address()).await?;

        Ok(RelayFundingQuoteChain {
            chain_id: self.chain_id,
            contract: self.contract.to_string(),
            sender: self.signer.address().to_string(),
            relayer: relayer.to_string(),
            recipient: recipient.to_string(),
            authorized_bytes: u64::try_from(payload.len())?,
            billed_bytes: quote.billed_bytes(),
            base_reward_navax: quote.base_route_reward().raw(),
            maximum_work_navax: quote.maximum_work().raw(),
            settlement_gas_reserve_navax: quote.settlement_gas_cap().raw(),
            maximum_charge_navax: quote.maximum_charge().raw(),
            balance_wei: balance.to_string(),
            estimated_wallet_gas_wei: estimated_wallet_gas_wei.to_string(),
        })
    }

    /// Funds one exact route and waits for its accepted `RouteFunded` event.
    /// The returned request is the only object the selected relayer may carry.
    pub async fn fund_route(
        &self,
        intent_id: String,
        payload: String,
        relayer: Address,
        recipient: Address,
        authorized_maximum_navax: u64,
    ) -> Result<PaidRelayRequestWire, Box<dyn Error>> {
        self.validate_three_distinct(relayer, recipient)?;
        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        self.ensure_chain(&provider).await?;
        let issued_at = accepted_timestamp(&provider).await?;
        let (authorization, signature, quote) =
            self.signed_authorization(payload.as_bytes(), relayer, recipient, issued_at)?;
        if quote.maximum_charge().raw() != authorized_maximum_navax {
            return Err("relay maximum differs from the reviewed authorization".into());
        }

        let escrow_wei = U256::from(authorized_maximum_navax)
            .checked_mul(U256::from(1_000_000_000_u64))
            .ok_or("relay escrow conversion overflowed")?;
        let contract = IRelaySettlement::new(self.contract, provider.clone());
        let call = contract
            .fundRoute(
                relay_authorization_contract(&authorization),
                vec![relayer],
                signature_bytes(&signature)?,
            )
            .from(self.signer.address())
            .value(escrow_wei);
        let gas = call.estimate_gas().await?;
        let gas_price = provider.get_gas_price().await?;
        let required = escrow_wei
            .checked_add(
                U256::from(gas)
                    .checked_mul(U256::from(gas_price))
                    .ok_or("relay gas requirement overflowed")?,
            )
            .ok_or("relay funding requirement overflowed")?;
        if provider.get_balance(self.signer.address()).await? < required {
            return Err("wallet balance does not cover relay escrow and wallet gas".into());
        }

        let receipt = timeout(Self::TRANSACTION_TIMEOUT, async {
            let pending = call.send().await.map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "relay funding transaction timed out")??;
        if !receipt.status() {
            return Err("relay funding transaction reverted".into());
        }
        let domain = cabal_relay_proof::proof_domain(self.chain_id, self.contract);
        let route_id = cabal_relay_proof::authorization_hash(&authorization, &domain);
        let event_matches = receipt
            .logs()
            .iter()
            .filter_map(|log| log.log_decode::<IRelaySettlement::RouteFunded>().ok())
            .any(|event| {
                event.inner.data.routeId == route_id
                    && event.inner.data.sender == self.signer.address()
                    && event.inner.data.recipient == recipient
                    && event.inner.data.maximumChargeNavax == authorized_maximum_navax
            });
        if !event_matches {
            return Err("accepted relay funding receipt did not match the authorization".into());
        }

        Ok(PaidRelayRequestWire {
            intent_id,
            authorization: relay_authorization_wire(&authorization),
            relayers: vec![relayer.to_string()],
            sender_signature: signature.to_string(),
            route_id: format!("{route_id:#x}"),
            funding_tx_hash: format!("{:?}", receipt.transaction_hash),
            payload,
        })
    }

    /// Accepts a request only on the named relayer wallet and only after the
    /// funding route is visible as active on-chain, then signs one contribution.
    pub async fn sign_relay_delivery(
        &self,
        request: PaidRelayRequestWire,
    ) -> Result<PaidRelayDeliveryWire, Box<dyn Error>> {
        let authorization = relay_authorization_protocol(&request.authorization)?;
        let relayers = parse_addresses(&request.relayers)?;
        if relayers.as_slice() != [self.signer.address()] {
            return Err("this wallet is not the authorized relay".into());
        }
        self.validate_request(&request, &authorization, &relayers)
            .await?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let forwarded_at = accepted_timestamp(&provider).await?;
        if forwarded_at > authorization.expiresAt {
            return Err("relay authorization expired before forwarding".into());
        }
        let route_id = B256::from_str(&request.route_id)?;
        let contribution = cabal_relay_proof::RelayContribution {
            authorizationHash: route_id,
            hopIndex: 0,
            relayer: self.signer.address(),
            ingress: authorization.sender,
            egress: authorization.recipient,
            payloadCommitment: authorization.payloadCommitment,
            deliveredBytes: authorization.authorizedBytes,
            forwardedAt: forwarded_at,
        };
        let domain = cabal_relay_proof::proof_domain(self.chain_id, self.contract);
        let contribution_id = cabal_relay_proof::contribution_hash(&contribution, &domain);
        let signature = self.signer.sign_hash_sync(&contribution_id)?;

        Ok(PaidRelayDeliveryWire {
            request,
            contribution: relay_contribution_wire(&contribution),
            contribution_signature: signature.to_string(),
            contribution_id: format!("{contribution_id:#x}"),
        })
    }

    /// Recipient-side verification, acknowledgement, and accepted settlement.
    /// The transaction hash is returned only after the contract confirms the
    /// exact route and proof bundle.
    pub async fn acknowledge_and_settle(
        &self,
        delivery: PaidRelayDeliveryWire,
    ) -> Result<(PaidRelaySettlementWire, String), Box<dyn Error>> {
        let authorization = relay_authorization_protocol(&delivery.request.authorization)?;
        if authorization.recipient != self.signer.address() {
            return Err("this wallet is not the authorized recipient".into());
        }
        let relayers = parse_addresses(&delivery.request.relayers)?;
        let relayer = *relayers
            .first()
            .ok_or("paid relay request has no authorized relay")?;
        self.validate_request(&delivery.request, &authorization, &relayers)
            .await?;
        let contribution = relay_contribution_protocol(&delivery.contribution)?;
        let contribution_signature = Signature::from_str(&delivery.contribution_signature)?;
        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let received_at = accepted_timestamp(&provider).await?;
        if received_at < contribution.forwardedAt || received_at > authorization.expiresAt {
            return Err("recipient acknowledgement time is invalid".into());
        }
        let contribution_id = B256::from_str(&delivery.contribution_id)?;
        let contributions_hash = cabal_relay_proof::ordered_contributions_hash(&[contribution_id])?;
        let acknowledgement = cabal_relay_proof::RecipientAcknowledgement {
            authorizationHash: B256::from_str(&delivery.request.route_id)?,
            contributionsHash: contributions_hash,
            recipient: self.signer.address(),
            payloadCommitment: authorization.payloadCommitment,
            deliveredBytes: contribution.deliveredBytes,
            receivedAt: received_at,
        };
        let domain = cabal_relay_proof::proof_domain(self.chain_id, self.contract);
        let acknowledgement_id = cabal_relay_proof::acknowledgement_hash(&acknowledgement, &domain);
        let acknowledgement_signature = self.signer.sign_hash_sync(&acknowledgement_id)?;

        let sender_signature = Signature::from_str(&delivery.request.sender_signature)?;
        let proof = cabal_relay_proof::RelayProof::new(
            cabal_relay_proof::SignedAuthorization::new(
                authorization.clone(),
                relayers.clone().into_boxed_slice(),
                sender_signature,
            ),
            vec![cabal_relay_proof::SignedContribution::new(
                contribution.clone(),
                contribution_signature,
            )]
            .into_boxed_slice(),
            Some(cabal_relay_proof::SignedAcknowledgement::new(
                acknowledgement.clone(),
                acknowledgement_signature,
            )),
        );
        let consumed_routes = std::collections::HashSet::new();
        let consumed_contributions = std::collections::HashSet::new();
        let verified = cabal_relay_proof::verify(
            &proof,
            &cabal_relay_proof::VerificationContext {
                chain_id: self.chain_id,
                settlement_contract: self.contract,
                now: received_at,
                expected_payload_commitment: cabal_relay_proof::payload_commitment(
                    delivery.request.payload.as_bytes(),
                ),
                consumed_routes: &consumed_routes,
                consumed_contributions: &consumed_contributions,
            },
        )?;

        let contract = IRelaySettlement::new(self.contract, provider);
        let contract_proof = IRelaySettlement::RelayProof {
            authorization: relay_authorization_contract(&authorization),
            relayers,
            senderSignature: signature_bytes(proof.authorization().signature())?,
            contributions: vec![relay_contribution_contract(&contribution)],
            contributionSignatures: vec![signature_bytes(&contribution_signature)?],
            acknowledgement: relay_acknowledgement_contract(&acknowledgement),
            acknowledgementSignature: signature_bytes(&acknowledgement_signature)?,
        };
        // `settle` costs more gas than an estimate of it reports, and the gap
        // is not noise. `_meteredExecutorNavax` returns zero when `tx.gasprice`
        // is zero, which makes the executor credit zero, which makes `_credit`
        // return before it writes. Avalanche's RPC estimates with a zero gas
        // price, so the estimate always walks that cheaper branch while the
        // real transaction — sent at a real gas price — pays for two cold
        // storage writes the estimate never saw. Sending the estimate verbatim
        // runs out of gas at the last instruction and burns the whole limit.
        // Unused gas is refunded, so the headroom costs nothing when unneeded.
        let call = contract.settle(contract_proof);
        let gas_limit = call
            .estimate_gas()
            .await?
            .saturating_mul(2);
        let receipt = timeout(Self::TRANSACTION_TIMEOUT, async {
            let pending = call
                .gas(gas_limit)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "relay settlement transaction timed out")??;
        if !receipt.status() {
            return Err("relay settlement transaction reverted".into());
        }
        let event = receipt
            .logs()
            .iter()
            .filter_map(|log| log.log_decode::<IRelaySettlement::RouteSettled>().ok())
            .find(|event| event.inner.data.routeId == verified.route_id())
            .ok_or("accepted receipt did not contain the expected relay settlement")?;
        let settlement_tx_hash = format!("{:?}", receipt.transaction_hash);
        let relayer_reward_navax = verified.reward_quote().base_route_reward().raw();
        let notice = PaidRelaySettlementWire {
            intent_id: delivery.request.intent_id,
            route_id: delivery.request.route_id,
            settlement_tx_hash: settlement_tx_hash.clone(),
            funding_tx_hash: delivery.request.funding_tx_hash,
            sender: authorization.sender.to_string(),
            relayer: relayer.to_string(),
            recipient: authorization.recipient.to_string(),
            settled_reward_navax: relayer_reward_navax,
        };
        Ok((notice, event.inner.data.senderRefundNavax.to_string()))
    }

    /// Authoritative accepted relay earnings. Pending route values and the old
    /// byte-rate estimate are not inputs to this read.
    pub async fn settled_earnings(&self) -> Result<(U256, U256, u64), Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        self.ensure_chain(&provider).await?;
        let block = provider.get_block_number().await?;
        let contract = IRelaySettlement::new(self.contract, provider);
        let earnings = contract
            .settledRelayEarningsNavax(self.signer.address())
            .block(BlockId::number(block))
            .call()
            .await?;
        let credit = contract
            .withdrawableWei(self.signer.address())
            .block(BlockId::number(block))
            .call()
            .await?;
        Ok((earnings, credit, block))
    }

    /// Accepts a mesh settlement notice only when its transaction receipt and
    /// current contract state independently prove the exact route and relayer
    /// credit. A peer-supplied amount or hash is never enough to update UI.
    pub async fn verify_settlement_notice(
        &self,
        notice: &PaidRelaySettlementWire,
    ) -> Result<bool, Box<dyn Error>> {
        let route_id = B256::from_str(&notice.route_id)?;
        let tx_hash = B256::from_str(&notice.settlement_tx_hash)?;
        let sender = Address::from_str(&notice.sender)?;
        let relayer = Address::from_str(&notice.relayer)?;
        let recipient = Address::from_str(&notice.recipient)?;
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        self.ensure_chain(&provider).await?;
        let Some(receipt) = provider.get_transaction_receipt(tx_hash).await? else {
            return Ok(false);
        };
        if !receipt.status() {
            return Ok(false);
        }
        let contract = IRelaySettlement::new(self.contract, provider);
        let route = contract.routes(route_id).call().await?;
        if route.state != 2
            || route.sender != sender
            || route.recipient != recipient
            || !contract.consumedRoutes(route_id).call().await?
        {
            return Ok(false);
        }
        let settled = receipt
            .logs()
            .iter()
            .filter_map(|log| log.log_decode::<IRelaySettlement::RouteSettled>().ok())
            .any(|event| {
                event.inner.data.routeId == route_id
                    && event.inner.data.workPaidNavax == notice.settled_reward_navax
            });
        let credited = receipt
            .logs()
            .iter()
            .filter_map(|log| {
                log.log_decode::<IRelaySettlement::RelayRewardCredited>()
                    .ok()
            })
            .any(|event| {
                event.inner.data.routeId == route_id
                    && event.inner.data.relayer == relayer
                    && event.inner.data.amountNavax == notice.settled_reward_navax
            });
        Ok(settled && credited)
    }

    async fn validate_request(
        &self,
        request: &PaidRelayRequestWire,
        authorization: &cabal_relay_proof::RelayAuthorization,
        relayers: &[Address],
    ) -> Result<(), Box<dyn Error>> {
        if relayers.len() != 1 {
            return Err("ticket 13 supports exactly one paid relay".into());
        }
        self.validate_three_distinct(relayers[0], authorization.recipient)?;
        if request.payload.len() != usize::try_from(authorization.authorizedBytes)?
            || cabal_relay_proof::payload_commitment(request.payload.as_bytes())
                != authorization.payloadCommitment
            || cabal_relay_proof::relay_route_hash(relayers)? != authorization.relayRouteHash
        {
            return Err("paid relay request does not match its signed payload or route".into());
        }
        let domain = cabal_relay_proof::proof_domain(self.chain_id, self.contract);
        let route_id = cabal_relay_proof::authorization_hash(authorization, &domain);
        let signature = Signature::from_str(&request.sender_signature)?;
        if route_id != B256::from_str(&request.route_id)?
            || signature.recover_address_from_prehash(&route_id)? != authorization.sender
        {
            return Err("paid relay sender authorization is invalid".into());
        }
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        self.ensure_chain(&provider).await?;
        let route = IRelaySettlement::new(self.contract, provider)
            .routes(route_id)
            .call()
            .await?;
        if route.state != 1
            || route.sender != authorization.sender
            || route.recipient != authorization.recipient
            || route.payloadCommitment != authorization.payloadCommitment
            || route.authorizedBytes != authorization.authorizedBytes
            || route.maximumChargeNavax != authorization.maximumChargeNavax
            || route.expiresAt != authorization.expiresAt
        {
            return Err("paid relay route is not active in accepted contract state".into());
        }
        Ok(())
    }

    fn signed_authorization(
        &self,
        payload: &[u8],
        relayer: Address,
        recipient: Address,
        issued_at: u64,
    ) -> Result<
        (
            cabal_relay_proof::RelayAuthorization,
            Signature,
            cabal_rewards::RewardQuote,
        ),
        Box<dyn Error>,
    > {
        let authorized_bytes =
            cabal_rewards::BillableBytes::try_from(u64::try_from(payload.len())?)?;
        let relay_count = cabal_rewards::RelayCount::try_from(1)?;
        let quote = cabal_rewards::quote(authorized_bytes, relay_count)?;
        let mut nonce = [0_u8; 32];
        OsRng.fill_bytes(&mut nonce);
        let authorization = cabal_relay_proof::RelayAuthorization {
            policyHash: cabal_relay_proof::policy_hash(),
            routeNonce: B256::from(nonce),
            payloadCommitment: cabal_relay_proof::payload_commitment(payload),
            deliveryMode: 0,
            relayRouteHash: cabal_relay_proof::relay_route_hash(&[relayer])?,
            sender: self.signer.address(),
            recipient,
            authorizedBytes: authorized_bytes.get(),
            relayCount: 1,
            maximumChargeNavax: quote.maximum_charge().raw(),
            issuedAt: issued_at,
            expiresAt: issued_at
                .checked_add(cabal_rewards::DEFAULT_AUTHORIZATION_SECONDS)
                .ok_or("relay authorization expiry overflowed")?,
        };
        let domain = cabal_relay_proof::proof_domain(self.chain_id, self.contract);
        let route_id = cabal_relay_proof::authorization_hash(&authorization, &domain);
        let signature = self.signer.sign_hash_sync(&route_id)?;
        Ok((authorization, signature, quote))
    }

    fn validate_three_distinct(
        &self,
        relayer: Address,
        recipient: Address,
    ) -> Result<(), Box<dyn Error>> {
        let sender = self.signer.address();
        if relayer == Address::ZERO
            || recipient == Address::ZERO
            || sender == relayer
            || sender == recipient
            || relayer == recipient
        {
            return Err("sender, relayer, and recipient wallets must be distinct".into());
        }
        Ok(())
    }

    async fn ensure_chain<P>(&self, provider: &P) -> Result<(), Box<dyn Error>>
    where
        P: Provider,
    {
        if provider.get_chain_id().await? != self.chain_id {
            return Err("relay settlement RPC chain does not match the configured release".into());
        }
        Ok(())
    }
}

async fn accepted_timestamp<P>(provider: &P) -> Result<u64, Box<dyn Error>>
where
    P: Provider,
{
    provider
        .get_block_by_number(BlockNumberOrTag::Latest)
        .await?
        .map(|block| block.header().timestamp())
        .ok_or_else(|| "accepted chain head is unavailable".into())
}

fn signature_bytes(signature: &Signature) -> Result<Bytes, hex::FromHexError> {
    Ok(Bytes::from(hex::decode(
        signature.to_string().trim_start_matches("0x"),
    )?))
}

fn parse_addresses(values: &[String]) -> Result<Vec<Address>, Box<dyn Error>> {
    values
        .iter()
        .map(|value| Address::from_str(value).map_err(Into::into))
        .collect()
}

fn relay_authorization_contract(
    value: &cabal_relay_proof::RelayAuthorization,
) -> IRelaySettlement::RelayAuthorization {
    IRelaySettlement::RelayAuthorization {
        policyHash: value.policyHash,
        routeNonce: value.routeNonce,
        payloadCommitment: value.payloadCommitment,
        deliveryMode: value.deliveryMode,
        relayRouteHash: value.relayRouteHash,
        sender: value.sender,
        recipient: value.recipient,
        authorizedBytes: value.authorizedBytes,
        relayCount: value.relayCount,
        maximumChargeNavax: value.maximumChargeNavax,
        issuedAt: value.issuedAt,
        expiresAt: value.expiresAt,
    }
}

fn relay_authorization_wire(
    value: &cabal_relay_proof::RelayAuthorization,
) -> RelayAuthorizationWire {
    RelayAuthorizationWire {
        policy_hash: format!("{:#x}", value.policyHash),
        route_nonce: format!("{:#x}", value.routeNonce),
        payload_commitment: format!("{:#x}", value.payloadCommitment),
        delivery_mode: value.deliveryMode,
        relay_route_hash: format!("{:#x}", value.relayRouteHash),
        sender: value.sender.to_string(),
        recipient: value.recipient.to_string(),
        authorized_bytes: value.authorizedBytes,
        relay_count: value.relayCount,
        maximum_charge_navax: value.maximumChargeNavax,
        issued_at: value.issuedAt,
        expires_at: value.expiresAt,
    }
}

fn relay_authorization_protocol(
    value: &RelayAuthorizationWire,
) -> Result<cabal_relay_proof::RelayAuthorization, Box<dyn Error>> {
    Ok(cabal_relay_proof::RelayAuthorization {
        policyHash: B256::from_str(&value.policy_hash)?,
        routeNonce: B256::from_str(&value.route_nonce)?,
        payloadCommitment: B256::from_str(&value.payload_commitment)?,
        deliveryMode: value.delivery_mode,
        relayRouteHash: B256::from_str(&value.relay_route_hash)?,
        sender: Address::from_str(&value.sender)?,
        recipient: Address::from_str(&value.recipient)?,
        authorizedBytes: value.authorized_bytes,
        relayCount: value.relay_count,
        maximumChargeNavax: value.maximum_charge_navax,
        issuedAt: value.issued_at,
        expiresAt: value.expires_at,
    })
}

fn relay_contribution_contract(
    value: &cabal_relay_proof::RelayContribution,
) -> IRelaySettlement::RelayContribution {
    IRelaySettlement::RelayContribution {
        authorizationHash: value.authorizationHash,
        hopIndex: value.hopIndex,
        relayer: value.relayer,
        ingress: value.ingress,
        egress: value.egress,
        payloadCommitment: value.payloadCommitment,
        deliveredBytes: value.deliveredBytes,
        forwardedAt: value.forwardedAt,
    }
}

fn relay_contribution_wire(value: &cabal_relay_proof::RelayContribution) -> RelayContributionWire {
    RelayContributionWire {
        authorization_hash: format!("{:#x}", value.authorizationHash),
        hop_index: value.hopIndex,
        relayer: value.relayer.to_string(),
        ingress: value.ingress.to_string(),
        egress: value.egress.to_string(),
        payload_commitment: format!("{:#x}", value.payloadCommitment),
        delivered_bytes: value.deliveredBytes,
        forwarded_at: value.forwardedAt,
    }
}

fn relay_contribution_protocol(
    value: &RelayContributionWire,
) -> Result<cabal_relay_proof::RelayContribution, Box<dyn Error>> {
    Ok(cabal_relay_proof::RelayContribution {
        authorizationHash: B256::from_str(&value.authorization_hash)?,
        hopIndex: value.hop_index,
        relayer: Address::from_str(&value.relayer)?,
        ingress: Address::from_str(&value.ingress)?,
        egress: Address::from_str(&value.egress)?,
        payloadCommitment: B256::from_str(&value.payload_commitment)?,
        deliveredBytes: value.delivered_bytes,
        forwardedAt: value.forwarded_at,
    })
}

fn relay_acknowledgement_contract(
    value: &cabal_relay_proof::RecipientAcknowledgement,
) -> IRelaySettlement::RecipientAcknowledgement {
    IRelaySettlement::RecipientAcknowledgement {
        authorizationHash: value.authorizationHash,
        contributionsHash: value.contributionsHash,
        recipient: value.recipient,
        payloadCommitment: value.payloadCommitment,
        deliveredBytes: value.deliveredBytes,
        receivedAt: value.receivedAt,
    }
}

impl ModuleMarketWriter {
    const MUTATION_TIMEOUT: Duration = Duration::from_secs(60);
    const CANONICAL_DESCRIPTION: &'static str = "Canonical CabalMesh module";
    const MAX_DEAL_HISTORY: u64 = 10_000;
    const MAX_RECOVERY_SCAN: u64 = 32;

    #[must_use]
    pub fn seller(&self) -> Address {
        self.signer.address()
    }

    #[must_use]
    pub const fn marketplace(&self) -> Address {
        self.marketplace
    }

    /// Re-reads one listing, custody, eligibility, buyer balance, and a
    /// current network-fee estimate at one accepted head.
    pub async fn purchase_quote(
        &self,
        listing_id: U256,
    ) -> Result<ModulePurchaseQuoteChain, Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider.clone());
        let modules = IModules::new(self.modules, provider.clone());
        let verified_block = marketplace.provider().get_block_number().await?;
        let block = BlockId::number(verified_block);
        let listing = marketplace.listings(listing_id).block(block).call().await?;
        let active_listing_id =
            if listing.collection == self.modules && listing.tokenId != U256::ZERO {
                marketplace
                    .activeListingOf(self.modules, listing.tokenId)
                    .block(block)
                .call()
                .await?
        } else {
            U256::ZERO
        };

        let mut current_owner = None;
        let mut module = None;
        let mut marketplace_eligible = false;
        let mut equipped_by = None;
        let mut approved = false;
        if listing.collection == self.modules && listing.tokenId != U256::ZERO {
            match modules.ownerOf(listing.tokenId).block(block).call().await {
                Ok(owner) => {
                    current_owner = Some(owner.to_string());
                    match modules.assetData(listing.tokenId).block(block).call().await {
                        Ok(data) => {
                            let soulbound =
                                modules.locked(listing.tokenId).block(block).call().await?;
                            let revoked =
                                modules.revoked(listing.tokenId).block(block).call().await?;
                            marketplace_eligible = modules
                                .isMarketplaceEligible(listing.tokenId)
                                .block(block)
                                .call()
                                .await?;
                            let equipped = modules
                                .equippedBy(listing.tokenId)
                                .block(block)
                                .call()
                                .await?;
                            equipped_by = (equipped != Address::ZERO).then(|| equipped.to_string());
                            let token_approval = modules
                                .getApproved(listing.tokenId)
                                .block(block)
                                .call()
                                .await?;
                            let blanket_approval = modules
                                .isApprovedForAll(owner, self.marketplace)
                                .block(block)
                                .call()
                                .await?;
                            approved = token_approval == self.marketplace || blanket_approval;
                            module = Some(ModuleChainRecord {
                                token_id: listing.tokenId.to_string(),
                                collection: self.modules.to_string(),
                                owner: owner.to_string(),
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
                        Err(error) if contract_call_reverted(&error) => {}
                        Err(error) => return Err(Box::new(error)),
                    }
                }
                Err(error) if contract_call_reverted(&error) => {}
                Err(error) => return Err(Box::new(error)),
            }
        }

        let buyer = self.signer.address();
        let balance = provider.get_balance(buyer).block_id(block).await?;
        let mut quote = ModulePurchaseQuoteChain {
            verified_block,
            buyer: buyer.to_string(),
            listing_id: listing_id.to_string(),
            seller: listing.seller.to_string(),
            price_wei: listing.priceWei.to_string(),
            token_id: listing.tokenId.to_string(),
            collection: listing.collection.to_string(),
            active: listing.active,
            active_listing_id: active_listing_id.to_string(),
            current_owner,
            module,
            marketplace_eligible,
            equipped_by,
            approved,
            balance_wei: balance.to_string(),
            estimated_network_fee_wei: None,
        };

        // An estimate is meaningful only for a transaction that passes the
        // static checks and has enough value for the listing itself. A wallet
        // below the price is classified as insufficient without asking the
        // node to simulate an impossible value transfer.
        if self.quote_is_buyable(&quote).is_ok()
            && buyer != listing.seller
            && balance >= listing.priceWei
        {
            let gas = marketplace
                .buy(listing_id)
                .from(buyer)
                .value(listing.priceWei)
                .block(block)
                .estimate_gas()
                .await?;
            let gas_price = provider.get_gas_price().await?;
            quote.estimated_network_fee_wei = Some(
                U256::from(gas)
                    .checked_mul(U256::from(gas_price))
                    .ok_or("network fee estimate overflowed")?
                    .to_string(),
            );
        }

        Ok(quote)
    }

    /// Reads every canonical module deal involving this wallet at one accepted
    /// head. The current contract has no party index, so the bounded history is
    /// scanned and an oversized history fails closed instead of hanging a UI.
    pub async fn my_module_deals(&self) -> Result<ModuleDealChainSnapshot, Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider.clone());
        let verified_block = marketplace.provider().get_block_number().await?;
        let block = BlockId::number(verified_block);
        let observed_at = provider
            .get_block_by_number(BlockNumberOrTag::Number(verified_block))
            .await?
            .ok_or("accepted deal block is unavailable")?
            .header()
            .timestamp();
        let next_id = marketplace.nextDealId().block(block).call().await?;
        if next_id > U256::from(Self::MAX_DEAL_HISTORY.saturating_add(1)) {
            return Err("module deal history exceeds the supported safety bound".into());
        }
        let wallet = self.signer.address();
        let mut matching = Vec::new();
        for deal_id in 1..next_id.to::<u64>() {
            let deal = marketplace
                .getDeal(U256::from(deal_id))
                .block(block)
                .call()
                .await?;
            if deal.collection == self.modules && (deal.buyer == wallet || deal.seller == wallet) {
                matching.push(U256::from(deal_id));
            }
        }

        let mut deals = Vec::with_capacity(matching.len());
        for deal_id in matching {
            deals.push(
                self.deal_state_at(deal_id, verified_block, observed_at)
                    .await?,
            );
        }
        deals.sort_by(|left, right| {
            right
                .deal_id
                .parse::<U256>()
                .unwrap_or(U256::ZERO)
                .cmp(&left.deal_id.parse::<U256>().unwrap_or(U256::ZERO))
        });
        Ok(ModuleDealChainSnapshot {
            verified_block,
            observed_at,
            wallet: wallet.to_string(),
            deals,
        })
    }

    pub async fn deal_state(&self, deal_id: U256) -> Result<ModuleDealChainRecord, Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let verified_block = provider.get_block_number().await?;
        let observed_at = provider
            .get_block_by_number(BlockNumberOrTag::Number(verified_block))
            .await?
            .ok_or("accepted deal block is unavailable")?
            .header()
            .timestamp();
        self.deal_state_at(deal_id, verified_block, observed_at)
            .await
    }

    /// Reads listing eligibility, custody, loadout, approval, and the duplicate
    /// guard from one accepted block.
    pub async fn listing_state(
        &self,
        token_id: U256,
    ) -> Result<ModuleListingChainState, Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider.clone());
        let modules = IModules::new(self.modules, provider);
        let verified_block = marketplace.provider().get_block_number().await?;
        let block = BlockId::number(verified_block);

        let active_id = marketplace
            .activeListingOf(self.modules, token_id)
            .block(block)
            .call()
            .await?;
        let active_listing = if active_id == U256::ZERO {
            None
        } else {
            let listing = marketplace.listings(active_id).block(block).call().await?;
            if !listing.active
                || listing.collection != self.modules
                || listing.tokenId != token_id
                || listing.seller == Address::ZERO
                || listing.priceWei == U256::ZERO
            {
                return Err("module marketplace duplicate guard is inconsistent".into());
            }
            Some(OwnedModuleListingChainRecord {
                listing_id: active_id.to_string(),
                seller: listing.seller.to_string(),
                price_wei: listing.priceWei.to_string(),
                token_id: listing.tokenId.to_string(),
                collection: listing.collection.to_string(),
            })
        };

        let owner = match modules.ownerOf(token_id).block(block).call().await {
            Ok(owner) => owner,
            Err(error) if contract_call_reverted(&error) => {
                return Ok(ModuleListingChainState {
                    verified_block,
                    token_id: token_id.to_string(),
                    owner: None,
                    module: None,
                    equipped_by: None,
                    marketplace_eligible: false,
                    approval: ModuleListingApprovalChain::Required,
                    active_listing,
                });
            }
            Err(error) => return Err(Box::new(error)),
        };

        let data = modules.assetData(token_id).block(block).call().await?;
        let soulbound = modules.locked(token_id).block(block).call().await?;
        let revoked = modules.revoked(token_id).block(block).call().await?;
        let marketplace_eligible = modules
            .isMarketplaceEligible(token_id)
            .block(block)
            .call()
            .await?;
        let equipped_by = modules.equippedBy(token_id).block(block).call().await?;
        let token_approval = modules.getApproved(token_id).block(block).call().await?;
        let blanket_approval = modules
            .isApprovedForAll(owner, self.marketplace)
            .block(block)
            .call()
            .await?;
        let approval = if blanket_approval {
            ModuleListingApprovalChain::Blanket
        } else if token_approval == self.marketplace {
            ModuleListingApprovalChain::Token
        } else {
            ModuleListingApprovalChain::Required
        };

        Ok(ModuleListingChainState {
            verified_block,
            token_id: token_id.to_string(),
            owner: Some(owner.to_string()),
            module: Some(ModuleChainRecord {
                token_id: token_id.to_string(),
                collection: self.modules.to_string(),
                owner: owner.to_string(),
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
            }),
            equipped_by: (equipped_by != Address::ZERO).then(|| equipped_by.to_string()),
            marketplace_eligible,
            approval,
            active_listing,
        })
    }

    pub async fn approve_module(
        &self,
        token_id: U256,
    ) -> Result<ModuleListingMutationOutcome, Box<dyn Error>> {
        let before = self.listing_state(token_id).await?;
        if before.active_listing.is_some() {
            return Ok(ModuleListingMutationOutcome {
                kind: ModuleListingMutationKind::NoChange,
                tx_hash: None,
                state: before,
            });
        }
        self.ensure_listable(&before)?;
        if before.approval != ModuleListingApprovalChain::Required {
            return Ok(ModuleListingMutationOutcome {
                kind: ModuleListingMutationKind::NoChange,
                tx_hash: None,
                state: before,
            });
        }

        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let modules = IModules::new(self.modules, provider);
        let receipt = timeout(Self::MUTATION_TIMEOUT, async {
            let pending = modules
                .approve(self.marketplace, token_id)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await;

        let receipt = match receipt {
            Ok(Ok(receipt)) if receipt.status() => Some(receipt),
            _ => {
                let after = self.listing_state(token_id).await?;
                if after.active_listing.is_some() {
                    return Ok(ModuleListingMutationOutcome {
                        kind: ModuleListingMutationKind::NoChange,
                        tx_hash: None,
                        state: after,
                    });
                }
                self.ensure_listable(&after)?;
                if after.approval != ModuleListingApprovalChain::Required {
                    return Ok(ModuleListingMutationOutcome {
                        kind: ModuleListingMutationKind::ApprovalConfirmed,
                        tx_hash: None,
                        state: after,
                    });
                }
                return Err("module approval was not confirmed".into());
            }
        };

        let after = self.listing_state(token_id).await?;
        self.ensure_listable(&after)?;
        if after.approval == ModuleListingApprovalChain::Required {
            return Err("accepted state did not confirm module approval".into());
        }
        Ok(ModuleListingMutationOutcome {
            kind: ModuleListingMutationKind::ApprovalConfirmed,
            tx_hash: receipt.map(|receipt| format!("{:?}", receipt.transaction_hash)),
            state: after,
        })
    }

    pub async fn create_listing(
        &self,
        token_id: U256,
        price_wei: U256,
    ) -> Result<ModuleListingMutationOutcome, Box<dyn Error>> {
        if price_wei == U256::ZERO {
            return Err("module listing price must be positive".into());
        }
        let before = self.listing_state(token_id).await?;
        if before.active_listing.is_some() {
            return Ok(ModuleListingMutationOutcome {
                kind: ModuleListingMutationKind::NoChange,
                tx_hash: None,
                state: before,
            });
        }
        self.ensure_listable(&before)?;
        if before.approval == ModuleListingApprovalChain::Required {
            return Err("module marketplace approval is required".into());
        }

        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider);
        let receipt = timeout(Self::MUTATION_TIMEOUT, async {
            let pending = marketplace
                .createListingFor(
                    self.modules,
                    Self::CANONICAL_DESCRIPTION.to_owned(),
                    price_wei,
                    token_id,
                )
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await;

        let receipt = match receipt {
            Ok(Ok(receipt)) if receipt.status() => receipt,
            _ => {
                let refreshed = self.listing_state(token_id).await?;
                if refreshed.active_listing.is_some() {
                    return Ok(ModuleListingMutationOutcome {
                        kind: ModuleListingMutationKind::NoChange,
                        tx_hash: None,
                        state: refreshed,
                    });
                }
                if self.owner_is_marketplace(&refreshed) {
                    return Ok(ModuleListingMutationOutcome {
                        kind: ModuleListingMutationKind::DealRulesActive,
                        tx_hash: None,
                        state: refreshed,
                    });
                }
                return Err("module listing was not confirmed".into());
            }
        };
        let listing_id = receipt
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<IMarketplace::ListingCreated>().ok())
            .map(|log| log.inner.data.id)
            .ok_or("ListingCreated event not found in receipt")?;
        let after = self.listing_state(token_id).await?;
        if self.owner_is_marketplace(&after) && after.active_listing.is_none() {
            return Ok(ModuleListingMutationOutcome {
                kind: ModuleListingMutationKind::DealRulesActive,
                tx_hash: Some(format!("{:?}", receipt.transaction_hash)),
                state: after,
            });
        }
        self.ensure_listable(&after)?;
        let Some(listing) = after.active_listing.as_ref() else {
            return Err("accepted state did not confirm an active module listing".into());
        };
        if listing.listing_id != listing_id.to_string()
            || listing.price_wei != price_wei.to_string()
            || listing.seller.parse::<Address>().ok() != Some(self.signer.address())
        {
            return Err("accepted module listing does not match the requested sale".into());
        }

        Ok(ModuleListingMutationOutcome {
            kind: ModuleListingMutationKind::ListingConfirmed,
            tx_hash: Some(format!("{:?}", receipt.transaction_hash)),
            state: after,
        })
    }

    pub async fn cancel_listing(
        &self,
        token_id: U256,
        listing_id: U256,
    ) -> Result<ModuleListingMutationOutcome, Box<dyn Error>> {
        let before = self.listing_state(token_id).await?;
        let Some(listing) = before.active_listing.as_ref() else {
            return Ok(ModuleListingMutationOutcome {
                kind: if self.owner_is_marketplace(&before) {
                    ModuleListingMutationKind::DealRulesActive
                } else {
                    ModuleListingMutationKind::NoChange
                },
                tx_hash: None,
                state: before,
            });
        };
        if listing.seller.parse::<Address>().ok() != Some(self.signer.address()) {
            return Err("only the current listing seller can cancel it".into());
        }
        if listing.listing_id != listing_id.to_string() {
            return Ok(ModuleListingMutationOutcome {
                kind: ModuleListingMutationKind::NoChange,
                tx_hash: None,
                state: before,
            });
        }

        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider);
        let receipt = timeout(Self::MUTATION_TIMEOUT, async {
            let pending = marketplace
                .cancelListing(listing_id)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await;

        let receipt = match receipt {
            Ok(Ok(receipt)) if receipt.status() => receipt,
            _ => {
                let refreshed = self.listing_state(token_id).await?;
                if refreshed.active_listing.is_none() {
                    let kind = if refreshed
                        .owner
                        .as_deref()
                        .and_then(|owner| owner.parse::<Address>().ok())
                        == Some(self.signer.address())
                    {
                        ModuleListingMutationKind::ListingCancelled
                    } else {
                        ModuleListingMutationKind::DealRulesActive
                    };
                    return Ok(ModuleListingMutationOutcome {
                        kind,
                        tx_hash: None,
                        state: refreshed,
                    });
                }
                return Err("module listing cancellation was not confirmed".into());
            }
        };
        let after = self.listing_state(token_id).await?;
        if after.active_listing.is_some() {
            return Err("accepted state still reports an active module listing".into());
        }
        Ok(ModuleListingMutationOutcome {
            kind: ModuleListingMutationKind::ListingCancelled,
            tx_hash: Some(format!("{:?}", receipt.transaction_hash)),
            state: after,
        })
    }

    /// Buys one exact listing and reports success only after accepted state
    /// confirms both escrow custody and the resulting deal.
    pub async fn buy_module_listing(
        &self,
        listing_id: U256,
        expected_token_id: U256,
        expected_seller: Address,
        expected_price_wei: U256,
    ) -> Result<ModuleDealMutationOutcome, Box<dyn Error>> {
        let before = self.purchase_quote(listing_id).await?;
        self.quote_is_buyable(&before)?;
        if before.token_id != expected_token_id.to_string()
            || before.seller.parse::<Address>().ok() != Some(expected_seller)
            || before.price_wei != expected_price_wei.to_string()
        {
            return Err("accepted listing no longer matches the purchase confirmation".into());
        }
        if expected_seller == self.signer.address() {
            return Err("seller cannot buy their own module listing".into());
        }
        let balance = before.balance_wei.parse::<U256>()?;
        let estimated_fee = before
            .estimated_network_fee_wei
            .as_deref()
            .unwrap_or("0")
            .parse::<U256>()?;
        let required = expected_price_wei
            .checked_add(estimated_fee)
            .ok_or("purchase requirement overflowed")?;
        if balance < required {
            return Err(
                "buyer balance does not cover the listing and estimated network fee".into(),
            );
        }

        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider);
        let recovery_start = marketplace.nextDealId().call().await?;
        let receipt = timeout(Self::MUTATION_TIMEOUT, async {
            let pending = marketplace
                .buy(listing_id)
                .value(expected_price_wei)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await;

        let (deal_id, tx_hash) = match receipt {
            Ok(Ok(receipt)) if receipt.status() => {
                let deal_id = receipt
                    .logs()
                    .iter()
                    .find_map(|log| log.log_decode::<IMarketplace::DealCreated>().ok())
                    .filter(|log| {
                        log.inner.data.listingId == listing_id
                            && log.inner.data.buyer == self.signer.address()
                            && log.inner.data.tokenId == expected_token_id
                            && log.inner.data.amount == expected_price_wei
                    })
                    .map(|log| log.inner.data.dealId)
                    .ok_or("accepted purchase receipt did not contain the expected deal")?;
                (deal_id, Some(format!("{:?}", receipt.transaction_hash)))
            }
            _ => {
                let deal = self
                    .find_matching_deal(
                        recovery_start,
                        expected_token_id,
                        expected_seller,
                        expected_price_wei,
                    )
                    .await?
                    .ok_or("module purchase was not confirmed")?;
                (deal.deal_id.parse::<U256>()?, None)
            }
        };

        let deal = self.deal_state(deal_id).await?;
        self.ensure_expected_deal(
            &deal,
            expected_token_id,
            expected_seller,
            expected_price_wei,
        )?;
        if deal.status != ModuleDealStatusChain::Active {
            return Err("accepted purchase did not produce an active deal".into());
        }
        let after = self.purchase_quote(listing_id).await?;
        if after.active || after.active_listing_id != U256::ZERO.to_string() {
            return Err("accepted purchase did not retire the module listing".into());
        }

        Ok(ModuleDealMutationOutcome {
            kind: ModuleDealMutationKind::PurchaseConfirmed,
            tx_hash,
            deal,
        })
    }

    pub async fn release_module_deal(
        &self,
        deal_id: U256,
    ) -> Result<ModuleDealMutationOutcome, Box<dyn Error>> {
        let before = self.deal_state(deal_id).await?;
        if before.status == ModuleDealStatusChain::Released {
            return Ok(ModuleDealMutationOutcome {
                kind: ModuleDealMutationKind::ReleaseConfirmed,
                tx_hash: None,
                deal: before,
            });
        }
        if before.status != ModuleDealStatusChain::Active
            || (before.buyer.parse::<Address>().ok() != Some(self.signer.address())
                && before.observed_at < before.auto_release_at)
        {
            return Err("current wallet cannot release this deal yet".into());
        }

        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider);
        let receipt = timeout(Self::MUTATION_TIMEOUT, async {
            let pending = marketplace
                .releaseDeal(deal_id)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await;
        let tx_hash = match receipt {
            Ok(Ok(receipt)) if receipt.status() => Some(format!("{:?}", receipt.transaction_hash)),
            _ => None,
        };
        let after = self.deal_state(deal_id).await?;
        if after.status != ModuleDealStatusChain::Released {
            return Err("module deal release was not confirmed".into());
        }
        Ok(ModuleDealMutationOutcome {
            kind: ModuleDealMutationKind::ReleaseConfirmed,
            tx_hash,
            deal: after,
        })
    }

    pub async fn request_module_refund(
        &self,
        deal_id: U256,
    ) -> Result<ModuleDealMutationOutcome, Box<dyn Error>> {
        let before = self.deal_state(deal_id).await?;
        if before.status != ModuleDealStatusChain::Active
            || before.buyer.parse::<Address>().ok() != Some(self.signer.address())
        {
            return Err("only the active deal buyer can request cancellation".into());
        }
        if before.refund_requested {
            return Ok(ModuleDealMutationOutcome {
                kind: ModuleDealMutationKind::RefundRequested,
                tx_hash: None,
                deal: before,
            });
        }

        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider);
        let receipt = timeout(Self::MUTATION_TIMEOUT, async {
            let pending = marketplace
                .requestRefund(deal_id)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await;
        let tx_hash = match receipt {
            Ok(Ok(receipt)) if receipt.status() => Some(format!("{:?}", receipt.transaction_hash)),
            _ => None,
        };
        let after = self.deal_state(deal_id).await?;
        if after.status != ModuleDealStatusChain::Active || !after.refund_requested {
            return Err("module deal refund request was not confirmed".into());
        }
        Ok(ModuleDealMutationOutcome {
            kind: ModuleDealMutationKind::RefundRequested,
            tx_hash,
            deal: after,
        })
    }

    pub async fn refund_module_deal(
        &self,
        deal_id: U256,
    ) -> Result<ModuleDealMutationOutcome, Box<dyn Error>> {
        let before = self.deal_state(deal_id).await?;
        if before.status == ModuleDealStatusChain::Refunded {
            return Ok(ModuleDealMutationOutcome {
                kind: ModuleDealMutationKind::RefundConfirmed,
                tx_hash: None,
                deal: before,
            });
        }
        if before.status != ModuleDealStatusChain::Active
            || before.seller.parse::<Address>().ok() != Some(self.signer.address())
            || !before.refund_requested
        {
            return Err("seller refund requires an active buyer cancellation request".into());
        }

        let provider = ProviderBuilder::new()
            .wallet(self.signer.clone())
            .connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider);
        let receipt = timeout(Self::MUTATION_TIMEOUT, async {
            let pending = marketplace
                .refundDeal(deal_id)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            pending
                .get_receipt()
                .await
                .map_err(|error| error.to_string())
        })
        .await;
        let tx_hash = match receipt {
            Ok(Ok(receipt)) if receipt.status() => Some(format!("{:?}", receipt.transaction_hash)),
            _ => None,
        };
        let after = self.deal_state(deal_id).await?;
        if after.status != ModuleDealStatusChain::Refunded {
            return Err("module deal refund was not confirmed".into());
        }
        Ok(ModuleDealMutationOutcome {
            kind: ModuleDealMutationKind::RefundConfirmed,
            tx_hash,
            deal: after,
        })
    }

    async fn deal_state_at(
        &self,
        deal_id: U256,
        verified_block: u64,
        observed_at: u64,
    ) -> Result<ModuleDealChainRecord, Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider.clone());
        let modules = IModules::new(self.modules, provider);
        let block = BlockId::number(verified_block);
        let deal = marketplace.getDeal(deal_id).block(block).call().await?;
        let status = match deal.status {
            1 => ModuleDealStatusChain::Active,
            2 => ModuleDealStatusChain::Released,
            3 => ModuleDealStatusChain::Refunded,
            _ => return Err("module deal does not exist".into()),
        };
        if deal_id == U256::ZERO
            || deal.buyer == Address::ZERO
            || deal.seller == Address::ZERO
            || deal.tokenId == U256::ZERO
            || deal.amount == U256::ZERO
            || deal.collection != self.modules
            || deal.autoReleaseAt == 0
        {
            return Err("module deal identity is malformed".into());
        }
        let owner = modules.ownerOf(deal.tokenId).block(block).call().await?;
        let expected_owner = match status {
            ModuleDealStatusChain::Active => self.marketplace,
            ModuleDealStatusChain::Released => deal.buyer,
            ModuleDealStatusChain::Refunded => deal.seller,
        };
        if owner != expected_owner {
            return Err("module custody does not agree with deal status".into());
        }
        let data = modules.assetData(deal.tokenId).block(block).call().await?;
        let soulbound = modules.locked(deal.tokenId).block(block).call().await?;
        let revoked = modules.revoked(deal.tokenId).block(block).call().await?;
        let module = ModuleChainRecord {
            token_id: deal.tokenId.to_string(),
            collection: self.modules.to_string(),
            owner: owner.to_string(),
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
        };
        self.ensure_canonical_module(&module)?;

        Ok(ModuleDealChainRecord {
            verified_block,
            observed_at,
            deal_id: deal_id.to_string(),
            buyer: deal.buyer.to_string(),
            seller: deal.seller.to_string(),
            token_id: deal.tokenId.to_string(),
            amount_wei: deal.amount.to_string(),
            status,
            collection: deal.collection.to_string(),
            auto_release_at: deal.autoReleaseAt,
            refund_requested: deal.refundRequested,
            current_owner: owner.to_string(),
            module,
        })
    }

    async fn find_matching_deal(
        &self,
        recovery_start: U256,
        token_id: U256,
        seller: Address,
        price_wei: U256,
    ) -> Result<Option<ModuleDealChainRecord>, Box<dyn Error>> {
        let provider = ProviderBuilder::new().connect_http(self.rpc_url.parse()?);
        let marketplace = IMarketplace::new(self.marketplace, provider);
        let recovery_end = marketplace.nextDealId().call().await?;
        if recovery_end < recovery_start
            || recovery_end.saturating_sub(recovery_start) > U256::from(Self::MAX_RECOVERY_SCAN)
        {
            return Err("purchase recovery window is inconsistent".into());
        }
        let mut id = recovery_start;
        while id < recovery_end {
            let deal = self.deal_state(id).await?;
            if self
                .ensure_expected_deal(&deal, token_id, seller, price_wei)
                .is_ok()
                && deal.status == ModuleDealStatusChain::Active
            {
                return Ok(Some(deal));
            }
            id = id
                .checked_add(U256::from(1_u8))
                .ok_or("deal id overflowed")?;
        }
        Ok(None)
    }

    fn ensure_expected_deal(
        &self,
        deal: &ModuleDealChainRecord,
        token_id: U256,
        seller: Address,
        price_wei: U256,
    ) -> Result<(), Box<dyn Error>> {
        if deal.buyer.parse::<Address>().ok() != Some(self.signer.address())
            || deal.seller.parse::<Address>().ok() != Some(seller)
            || deal.token_id != token_id.to_string()
            || deal.amount_wei != price_wei.to_string()
            || deal.collection.parse::<Address>().ok() != Some(self.modules)
        {
            return Err("accepted deal does not match the confirmed purchase".into());
        }
        Ok(())
    }

    fn quote_is_buyable(&self, quote: &ModulePurchaseQuoteChain) -> Result<(), Box<dyn Error>> {
        if !quote.active
            || quote.listing_id == U256::ZERO.to_string()
            || quote.active_listing_id != quote.listing_id
            || quote.collection.parse::<Address>().ok() != Some(self.modules)
            || quote
                .seller
                .parse::<Address>()
                .ok()
                .is_none_or(|seller| seller == Address::ZERO)
            || quote
                .price_wei
                .parse::<U256>()
                .ok()
                .is_none_or(|price| price == U256::ZERO)
            || quote
                .token_id
                .parse::<U256>()
                .ok()
                .is_none_or(|token| token == U256::ZERO)
            || quote.current_owner.as_deref() != Some(quote.seller.as_str())
            || !quote.marketplace_eligible
            || quote.equipped_by.is_some()
            || !quote.approved
        {
            return Err("module listing is not currently buyable".into());
        }
        let module = quote
            .module
            .as_ref()
            .ok_or("canonical module metadata is unavailable")?;
        self.ensure_canonical_module(module)
    }

    fn ensure_canonical_module(&self, module: &ModuleChainRecord) -> Result<(), Box<dyn Error>> {
        let canonical_effect = matches!(
            (
                module.slot,
                module.effect_type,
                module.primary_effect_value,
                module.secondary_effect_value,
            ),
            (1, 1, 1..=10_000, 0) | (2, 2, 1..=3, 0) | (3, 3, 1..=32, 1..=1_048_576)
        );
        if module.collection.parse::<Address>().ok() != Some(self.modules)
            || module.asset_class != 0
            || module.schema_version != 1
            || !canonical_effect
            || module.soulbound
        {
            return Err("module metadata is not canonical".into());
        }
        Ok(())
    }

    fn ensure_listable(&self, state: &ModuleListingChainState) -> Result<(), Box<dyn Error>> {
        let owner = state
            .owner
            .as_deref()
            .and_then(|owner| owner.parse::<Address>().ok())
            .ok_or("module token does not exist")?;
        if owner != self.signer.address() {
            return Err("module is not owned by the current seller".into());
        }
        let module = state
            .module
            .as_ref()
            .ok_or("module metadata is unavailable")?;
        if module.token_id != state.token_id
            || module.collection.parse::<Address>().ok() != Some(self.modules)
            || module.owner.parse::<Address>().ok() != Some(owner)
            || module.revoked
            || !state.marketplace_eligible
        {
            return Err("module is not marketplace eligible".into());
        }
        self.ensure_canonical_module(module)?;
        if state.equipped_by.is_some() {
            return Err("equipped module must be unequipped before listing".into());
        }
        Ok(())
    }

    fn owner_is_marketplace(&self, state: &ModuleListingChainState) -> bool {
        state
            .owner
            .as_deref()
            .and_then(|owner| owner.parse::<Address>().ok())
            == Some(self.marketplace)
    }
}

fn contract_call_reverted(error: &alloy::contract::Error) -> bool {
    error.as_revert_data().is_some()
}

#[derive(Debug, Clone, Copy)]
struct StandingHead {
    chain_id: u64,
    block_number: u64,
}

async fn read_accepted_standing_head(
    endpoint: crate::network_config::StandingProvider,
) -> Result<StandingHead, ()> {
    let url = endpoint.rpc_url.parse().map_err(|_| ())?;
    let provider = ProviderBuilder::new().connect_http(url);
    let chain_id = provider.get_chain_id().await.map_err(|_| ())?;
    let block = provider
        .get_block_by_number(BlockNumberOrTag::Finalized)
        .await
        .map_err(|_| ())?
        .ok_or(())?;
    Ok(StandingHead {
        chain_id,
        block_number: block.header().number(),
    })
}

async fn read_standing_observation(
    endpoint: crate::network_config::StandingProvider,
    provider_id: cabal_standing::ProviderId,
    head: Result<StandingHead, ()>,
    expected_chain_id: u64,
    registry: Address,
    seller: Address,
    pinned_block: u64,
) -> cabal_standing::ProviderObservation {
    use cabal_standing::{
        BlockHash, EvmAddress, ProviderObservation, ProviderRead, StandingSnapshot,
    };

    let Ok(head) = head else {
        return ProviderObservation {
            provider_id,
            read: ProviderRead::Unavailable,
        };
    };
    if head.chain_id != expected_chain_id || head.block_number < pinned_block {
        return ProviderObservation {
            provider_id,
            read: ProviderRead::IdentityMismatch,
        };
    }

    let Ok(url) = endpoint.rpc_url.parse() else {
        return ProviderObservation {
            provider_id,
            read: ProviderRead::Malformed,
        };
    };
    let provider = ProviderBuilder::new().connect_http(url);
    let block = match provider
        .get_block_by_number(BlockNumberOrTag::Number(pinned_block))
        .await
    {
        Ok(Some(block)) => block,
        _ => {
            return ProviderObservation {
                provider_id,
                read: ProviderRead::Unavailable,
            };
        }
    };
    if block.header().number() != pinned_block {
        return ProviderObservation {
            provider_id,
            read: ProviderRead::Malformed,
        };
    }

    let contract = IStandingRegistry::new(registry, provider);
    let standing = match contract
        .standingOf(seller)
        .block(BlockId::number(pinned_block))
        .call()
        .await
    {
        Ok(standing) => standing,
        Err(_) => {
            return ProviderObservation {
                provider_id,
                read: ProviderRead::Unavailable,
            };
        }
    };
    if standing.changedAtBlock > U256::from(u64::MAX) {
        return ProviderObservation {
            provider_id,
            read: ProviderRead::Malformed,
        };
    }
    let Some(observed_at_ms) = block.header().timestamp().checked_mul(1_000) else {
        return ProviderObservation {
            provider_id,
            read: ProviderRead::Malformed,
        };
    };

    ProviderObservation {
        provider_id,
        read: ProviderRead::Snapshot(StandingSnapshot {
            chain_id: head.chain_id,
            registry: EvmAddress::from_bytes(registry.into_array()),
            seller: EvmAddress::from_bytes(seller.into_array()),
            count: standing.count,
            last_changed_block: standing.changedAtBlock.to::<u64>(),
            block_number: pinned_block,
            block_hash: BlockHash::from_bytes(block.header().hash().0),
            observed_at_ms,
            accepted: true,
        }),
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

        let vault_key = crate::vault_key::platform_provider(
            tmp_dir.join("vault.key"),
            crate::vault_key::VaultUnlock::new(),
        );
        vault_key.unlock(&Secret::new("test passphrase")).unwrap();
        let mut bridge = BlockchainBridge {
            identities: Vec::new(),
            identity_vault: Vault::new(tmp_dir.join("vault.enc"), vault_key.clone()),
            vault_key,
            storage_path: tmp_dir.join("snapshot.enc"),
            chain_cache_path: tmp_dir.join("chain_cache.json"),
            module_loadout_cache_path: tmp_dir.join("module_loadout_cache.json"),
            pending_relay_path: tmp_dir.join("pending_relay_txs.json"),
            relayed_history_path: tmp_dir.join("relayed_history.json"),
            content_store_path: tmp_dir.join("content_store.json"),
            received_content_path: tmp_dir.join("received_content.json"),
            relay_boost_path: tmp_dir.join("relay_boost.json"),
            key_backup_path: tmp_dir.join("key_backup.json"),
            // Deliberately unreachable — proves sign_offline never touches the network.
            rpc_url: "http://127.0.0.1:9".to_string(),
            chain_id: 43_113,
            escrow_address: None,
            marketplace_address: None,
            module_marketplace_address: None,
            module_market_modules_address: None,
            voucher_address: None,
            modules_address: None,
            relay_settlement_address: None,
            standing_release: None,
            current_session: None,
        };
        bridge
            .generate_new_identity("Test".to_string(), "🧪".to_string())
            .unwrap();

        // Simulate having synced once while online.
        bridge
            .save_chain_cache(&ChainStateCache {
                nonce: 0,
                gas_price_wei: "30000000000".to_string(),
                cached_at: Utc::now(),
            })
            .unwrap();

        let to = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let calldata = Bytes::from(vec![0xde, 0xad, 0xbe, 0xef]);

        let queued = bridge
            .sign_offline(to, calldata, U256::from(0), "test tx")
            .await
            .expect("sign_offline should succeed with zero network access");

        assert!(queued.raw_tx_hex.starts_with("0x"));
        assert!(
            queued.raw_tx_hex.len() > 10,
            "raw tx hex should be non-trivial"
        );
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
        let vault_key = crate::vault_key::platform_provider(
            tmp_dir.join("vault.key"),
            crate::vault_key::VaultUnlock::new(),
        );
        vault_key.unlock(&Secret::new("test passphrase")).unwrap();
        BlockchainBridge {
            identities: Vec::new(),
            identity_vault: Vault::new(tmp_dir.join("vault.enc"), vault_key.clone()),
            vault_key,
            storage_path: tmp_dir.join("snapshot.enc"),
            chain_cache_path: tmp_dir.join("chain_cache.json"),
            module_loadout_cache_path: tmp_dir.join("module_loadout_cache.json"),
            pending_relay_path: tmp_dir.join("pending_relay_txs.json"),
            relayed_history_path: tmp_dir.join("relayed_history.json"),
            content_store_path: tmp_dir.join("content_store.json"),
            received_content_path: tmp_dir.join("received_content.json"),
            relay_boost_path: tmp_dir.join("relay_boost.json"),
            key_backup_path: tmp_dir.join("key_backup.json"),
            rpc_url: "http://127.0.0.1:9".to_string(),
            chain_id: 43_113,
            escrow_address: None,
            marketplace_address: None,
            module_marketplace_address: None,
            module_market_modules_address: None,
            voucher_address: None,
            modules_address: None,
            relay_settlement_address: None,
            standing_release: None,
            current_session: None,
        }
    }

    /// A signed content commitment must verify when the recovered signer matches
    /// the real seller address, and must be rejected — never silently trusted —
    /// when it doesn't. No network needed for any of this (pure crypto).
    #[test]
    fn signs_and_verifies_content_commitment() {
        let tmp_dir =
            std::env::temp_dir().join(format!("cabalmesh_content_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut seller_bridge = test_bridge(&tmp_dir.join("seller"));
        std::fs::create_dir_all(tmp_dir.join("seller")).unwrap();
        seller_bridge
            .generate_new_identity("Seller".to_string(), "📚".to_string())
            .unwrap();
        let seller_address = seller_bridge.get_primary_address();

        let text = "Chapter 1: It was the best of times, it was the worst of times.";
        let record = seller_bridge
            .sign_content(text)
            .expect("sign_content should succeed offline");
        assert_eq!(
            record.signer_address.to_lowercase(),
            seller_address.to_lowercase()
        );
        assert!(!record.signature.is_empty());

        // Buyer's own bridge instance (different identity) verifies the delivered content.
        let mut buyer_bridge = test_bridge(&tmp_dir.join("buyer"));
        std::fs::create_dir_all(tmp_dir.join("buyer")).unwrap();
        buyer_bridge
            .generate_new_identity("Buyer".to_string(), "🛒".to_string())
            .unwrap();

        let accepted = buyer_bridge
            .receive_content(1, text, &record.signature, &seller_address)
            .expect("receive_content should not error");
        assert!(
            accepted,
            "a correctly signed commitment from the real seller must verify"
        );
        assert!(buyer_bridge.get_received_content(1).is_some());

        // A signature that doesn't match the claimed seller must be rejected.
        let wrong_seller = "0x0000000000000000000000000000000000000001";
        let rejected = buyer_bridge
            .receive_content(2, text, &record.signature, wrong_seller)
            .expect("receive_content should not error even on mismatch");
        assert!(
            !rejected,
            "a signature from someone else must never be silently accepted"
        );
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
        let collection = Address::from_str("0x00000000000000000000000000000000000000a7").unwrap();
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
        bridge.modules_address =
            Some(Address::from_str("0x00000000000000000000000000000000000000b8").unwrap());
        assert!(bridge.cached_module_loadout().is_none());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[tokio::test]
    async fn market_reader_requires_reviewed_pair_and_never_substitutes_local_standing() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "cabalmesh_market_reader_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let mut bridge = test_bridge(&tmp_dir);
        let modules = Address::from_str("0x00000000000000000000000000000000000000a7").unwrap();
        let legacy_market =
            Address::from_str("0x00000000000000000000000000000000000000b8").unwrap();
        bridge.modules_address = Some(modules);
        bridge.marketplace_address = Some(legacy_market);
        assert!(
            bridge.module_market_reader().is_none(),
            "legacy voucher market must never activate canonical MARKET"
        );

        bridge.module_marketplace_address = Some(legacy_market);
        assert!(
            bridge.module_market_reader().is_none(),
            "a runtime inventory override must not become the reviewed MARKET collection"
        );
        bridge.module_market_modules_address = Some(modules);
        let reader = bridge
            .module_market_reader()
            .expect("an explicit reviewed pair creates a reader");
        let seller = Address::from_str("0x00000000000000000000000000000000000000c9").unwrap();
        let standing = reader.seller_standing(&[seller], 10_000_000).await;
        assert_eq!(
            standing.get(&seller),
            Some(&cabal_standing::PublicStanding::Unknown(
                cabal_standing::UnknownStandingReason::Unconfigured
            )),
            "an unreachable RPC and local history cannot replace absent reviewed standing config"
        );

        std::fs::remove_dir_all(&tmp_dir).ok();
    }
}

#[cfg(test)]
mod relay_settlement_tests {
    use super::*;

    const PAYLOAD: &[u8] = b"cabalmesh encrypted intent payload test vector v1";

    fn writer() -> RelaySettlementWriter {
        RelaySettlementWriter {
            rpc_url: "http://127.0.0.1:9".into(),
            chain_id: 43_113,
            contract: Address::from_str("0x9999999999999999999999999999999999999999").unwrap(),
            signer: PrivateKeySigner::from_str(
                "0x0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
        }
    }

    fn relay() -> Address {
        PrivateKeySigner::from_str(
            "0x0000000000000000000000000000000000000000000000000000000000000002",
        )
        .unwrap()
        .address()
    }

    fn recipient() -> Address {
        PrivateKeySigner::from_str(
            "0x000000000000000000000000000000000000000000000000000000000000000a",
        )
        .unwrap()
        .address()
    }

    #[test]
    fn funded_request_uses_the_same_versioned_quote_and_signature_domain() {
        let writer = writer();
        let (authorization, signature, quote) = writer
            .signed_authorization(PAYLOAD, relay(), recipient(), 1_800_000_000)
            .unwrap();
        let domain = cabal_relay_proof::proof_domain(writer.chain_id, writer.contract);
        let route_id = cabal_relay_proof::authorization_hash(&authorization, &domain);

        assert_eq!(authorization.policyHash, cabal_relay_proof::policy_hash());
        assert_eq!(
            authorization.payloadCommitment,
            cabal_relay_proof::payload_commitment(PAYLOAD)
        );
        assert_eq!(
            authorization.authorizedBytes,
            u64::try_from(PAYLOAD.len()).unwrap()
        );
        assert_eq!(authorization.relayCount, 1);
        assert_eq!(authorization.maximumChargeNavax, 2_200_000);
        assert_eq!(quote.maximum_charge().raw(), 2_200_000);
        assert_eq!(authorization.expiresAt - authorization.issuedAt, 600);
        assert_eq!(
            signature.recover_address_from_prehash(&route_id).unwrap(),
            writer.operator()
        );
    }

    #[test]
    fn relay_authorization_wire_round_trips_without_numeric_or_hash_loss() {
        let writer = writer();
        let (authorization, _, _) = writer
            .signed_authorization(PAYLOAD, relay(), recipient(), 1_800_000_000)
            .unwrap();
        let wire = relay_authorization_wire(&authorization);
        let decoded = relay_authorization_protocol(&wire).unwrap();
        assert_eq!(decoded, authorization);
        assert_eq!(wire.maximum_charge_navax, 2_200_000);
        assert_eq!(wire.route_nonce.len(), 66);
    }

    #[test]
    fn relay_contribution_wire_round_trips_without_changing_evidence() {
        let contribution = cabal_relay_proof::RelayContribution {
            authorizationHash: B256::repeat_byte(0x11),
            hopIndex: 0,
            relayer: relay(),
            ingress: writer().operator(),
            egress: recipient(),
            payloadCommitment: cabal_relay_proof::payload_commitment(PAYLOAD),
            deliveredBytes: u64::try_from(PAYLOAD.len()).unwrap(),
            forwardedAt: 1_800_000_060,
        };
        let wire = relay_contribution_wire(&contribution);
        assert_eq!(relay_contribution_protocol(&wire).unwrap(), contribution);
    }

    #[test]
    fn visible_same_wallet_role_reuse_is_rejected_before_rpc_or_funding() {
        let writer = writer();
        assert!(writer
            .validate_three_distinct(writer.operator(), recipient())
            .is_err());
        assert!(writer.validate_three_distinct(relay(), relay()).is_err());
        assert!(writer
            .validate_three_distinct(relay(), writer.operator())
            .is_err());
    }
}

#[cfg(test)]
mod key_custody_tests {
    use super::*;
    use tempfile::TempDir;

    /// A bridge with every path inside `dir`, and no network configured.
    ///
    /// Built literally rather than through `BlockchainBridge::new`, which
    /// resolves paths from the process-wide app directory — a test that wrote
    /// there would fight every other test and, worse, the developer's own
    /// wallet.
    fn bridge_in(dir: &TempDir) -> BlockchainBridge {
        let dir = dir.path();
        let vault_key = crate::vault_key::platform_provider(
            dir.join("vault.key"),
            crate::vault_key::VaultUnlock::new(),
        );
        vault_key.unlock(&Secret::new("test passphrase")).unwrap();
        BlockchainBridge {
            identities: Vec::new(),
            identity_vault: Vault::new(dir.join("vault.enc"), vault_key.clone()),
            vault_key,
            storage_path: dir.join("snapshot.enc"),
            chain_cache_path: dir.join("chain_cache.json"),
            module_loadout_cache_path: dir.join("module_loadout_cache.json"),
            pending_relay_path: dir.join("pending_relay_txs.json"),
            relayed_history_path: dir.join("relayed_history.json"),
            content_store_path: dir.join("content_store.json"),
            received_content_path: dir.join("received_content.json"),
            relay_boost_path: dir.join("relay_boost.json"),
            key_backup_path: dir.join("key_backup.json"),
            rpc_url: "http://127.0.0.1:9".to_string(),
            chain_id: 43_113,
            escrow_address: None,
            marketplace_address: None,
            module_marketplace_address: None,
            module_market_modules_address: None,
            voucher_address: None,
            modules_address: None,
            relay_settlement_address: None,
            standing_release: None,
            current_session: None,
        }
    }

    fn with_identity(dir: &TempDir) -> BlockchainBridge {
        let mut bridge = bridge_in(dir);
        bridge
            .generate_new_identity("Test".to_string(), "🧪".to_string())
            .unwrap();
        bridge
    }

    /// A valid key that is not the one under test.
    const OTHER_KEY: &str = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";

    #[test]
    fn a_fresh_wallet_has_no_backup() {
        let dir = TempDir::new().unwrap();
        assert!(with_identity(&dir).key_backup().is_none());
    }

    #[test]
    fn revealing_records_the_backup_for_this_address() {
        let dir = TempDir::new().unwrap();
        let bridge = with_identity(&dir);

        let (address, key, record) = bridge.reveal_primary_key().unwrap();

        assert_eq!(record.address, address);
        assert!(key.expose().starts_with("0x"));
        assert_eq!(bridge.key_backup().unwrap(), record);
    }

    #[test]
    fn the_revealed_key_derives_the_address_it_claims() {
        // The property that makes an export worth anything: typed into
        // another device, it must produce the same wallet.
        let dir = TempDir::new().unwrap();
        let bridge = with_identity(&dir);

        let (address, key, _) = bridge.reveal_primary_key().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let mut restored = bridge_in(&elsewhere);

        let outcome = restored
            .restore_identity(key.expose(), "Restored".into(), "🦊".into())
            .unwrap();

        match outcome {
            WalletRestore::Replaced { address: restored_address, identities } => {
                assert_eq!(restored_address, address);
                assert_eq!(identities[0].address, address);
            }
            other => panic!("expected a restore, got {other:?}"),
        }
    }

    #[test]
    fn restoring_over_an_unexported_wallet_is_refused() {
        // The wallet about to be destroyed has never been shown to anyone.
        // Replacing it would be an unrecoverable loss performed by a tap.
        let dir = TempDir::new().unwrap();
        let mut bridge = with_identity(&dir);
        let before = bridge.get_primary_address();

        let outcome = bridge
            .restore_identity(OTHER_KEY, "Restored".into(), "🦊".into())
            .unwrap();

        assert!(matches!(outcome, WalletRestore::BackupRequired { address } if address == before));
        assert_eq!(bridge.get_primary_address(), before, "the wallet was replaced anyway");
    }

    #[test]
    fn restoring_after_an_export_is_permitted() {
        let dir = TempDir::new().unwrap();
        let mut bridge = with_identity(&dir);
        let before = bridge.get_primary_address();
        bridge.reveal_primary_key().unwrap();

        let outcome = bridge
            .restore_identity(OTHER_KEY, "Restored".into(), "🦊".into())
            .unwrap();

        assert!(matches!(outcome, WalletRestore::Replaced { .. }));
        assert_ne!(bridge.get_primary_address(), before);
    }

    #[test]
    fn a_restored_wallet_does_not_inherit_the_previous_backup() {
        // Otherwise one export would authorise an unlimited chain of
        // replacements, each destroying a wallet nobody has a copy of.
        let dir = TempDir::new().unwrap();
        let mut bridge = with_identity(&dir);
        bridge.reveal_primary_key().unwrap();
        bridge
            .restore_identity(OTHER_KEY, "Restored".into(), "🦊".into())
            .unwrap();

        assert!(bridge.key_backup().is_none());
        assert!(matches!(
            bridge
                .restore_identity(OTHER_KEY, "Again".into(), "🦊".into())
                .unwrap(),
            WalletRestore::BackupRequired { .. }
        ));
    }

    #[test]
    fn a_backup_record_for_another_address_vouches_for_nothing() {
        let dir = TempDir::new().unwrap();
        let bridge = with_identity(&dir);

        JsonStore::new(&bridge.key_backup_path)
            .save(&KeyBackupRecord {
                address: "0x000000000000000000000000000000000000dead".into(),
                exported_at: Utc::now(),
            })
            .unwrap();

        assert!(bridge.key_backup().is_none());
    }

    #[test]
    fn a_malformed_key_changes_nothing() {
        let dir = TempDir::new().unwrap();
        let mut bridge = with_identity(&dir);
        bridge.reveal_primary_key().unwrap();
        let before = bridge.get_primary_address();

        for candidate in ["", "not a key", "0x1234", &"0xff".repeat(40)] {
            assert!(matches!(
                bridge
                    .restore_identity(candidate, "Restored".into(), "🦊".into())
                    .unwrap(),
                WalletRestore::InvalidKey
            ), "{candidate} was accepted as a private key");
        }

        assert_eq!(bridge.get_primary_address(), before);
        assert!(bridge.key_backup().is_some(), "a rejected restore consumed the backup record");
    }

    #[test]
    fn a_key_without_its_prefix_is_accepted() {
        // People copy keys out of tools that do not print `0x`.
        let dir = TempDir::new().unwrap();
        let mut bridge = with_identity(&dir);
        bridge.reveal_primary_key().unwrap();

        let outcome = bridge
            .restore_identity(OTHER_KEY.trim_start_matches("0x"), "Restored".into(), "🦊".into())
            .unwrap();

        assert!(matches!(outcome, WalletRestore::Replaced { .. }));
    }

    #[test]
    fn a_restored_wallet_survives_a_reload() {
        let dir = TempDir::new().unwrap();
        let mut bridge = with_identity(&dir);
        bridge.reveal_primary_key().unwrap();
        bridge
            .restore_identity(OTHER_KEY, "Restored".into(), "🦊".into())
            .unwrap();
        let restored = bridge.get_primary_address();

        let mut reopened = bridge_in(&dir);
        reopened.load_identities().unwrap();

        assert_eq!(reopened.get_primary_address(), restored);
    }
}

#[cfg(test)]
mod vault_unlock_tests {
    use super::*;
    use tempfile::TempDir;

    /// A bridge whose vault is **closed**, as it is on every cold start.
    fn locked_bridge_in(dir: &std::path::Path) -> BlockchainBridge {
        let vault_key = crate::vault_key::platform_provider(
            dir.join("vault.key"),
            crate::vault_key::VaultUnlock::new(),
        );
        BlockchainBridge {
            identities: Vec::new(),
            identity_vault: Vault::new(dir.join("vault.enc"), vault_key.clone()),
            vault_key,
            storage_path: dir.join("snapshot.enc"),
            chain_cache_path: dir.join("chain_cache.json"),
            module_loadout_cache_path: dir.join("module_loadout_cache.json"),
            pending_relay_path: dir.join("pending_relay_txs.json"),
            relayed_history_path: dir.join("relayed_history.json"),
            content_store_path: dir.join("content_store.json"),
            received_content_path: dir.join("received_content.json"),
            relay_boost_path: dir.join("relay_boost.json"),
            key_backup_path: dir.join("key_backup.json"),
            rpc_url: "http://127.0.0.1:9".to_string(),
            chain_id: 43_113,
            escrow_address: None,
            marketplace_address: None,
            module_marketplace_address: None,
            module_market_modules_address: None,
            voucher_address: None,
            modules_address: None,
            relay_settlement_address: None,
            standing_release: None,
            current_session: None,
        }
    }

    #[test]
    fn a_cold_start_is_locked_and_reads_nothing() {
        let dir = TempDir::new().unwrap();
        let mut bridge = locked_bridge_in(dir.path());

        assert_eq!(bridge.vault_state(), crate::vault_key::VaultState::Uninitialized);
        assert!(
            bridge.load_identities().is_err(),
            "identities were readable without a passphrase"
        );
        assert!(bridge.identities.is_empty());
    }

    #[test]
    fn the_first_passphrase_creates_a_wallet() {
        let dir = TempDir::new().unwrap();
        let mut bridge = locked_bridge_in(dir.path());

        bridge.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();

        assert_eq!(bridge.vault_state(), crate::vault_key::VaultState::Unlocked);
        assert!(bridge.get_primary_address().starts_with("0x"));
    }

    #[test]
    fn the_same_passphrase_returns_the_same_wallet_after_a_restart() {
        let dir = TempDir::new().unwrap();
        let mut first = locked_bridge_in(dir.path());
        first.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();
        let address = first.get_primary_address();

        let mut reopened = locked_bridge_in(dir.path());
        assert_eq!(reopened.vault_state(), crate::vault_key::VaultState::Locked);
        reopened.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();

        assert_eq!(reopened.get_primary_address(), address);
    }

    #[test]
    fn a_wrong_passphrase_never_generates_a_replacement_wallet() {
        // The failure that would matter most: answering a wrong passphrase by
        // creating a fresh wallet would present a funded account as an empty
        // one and lose it the moment anything was written.
        let dir = TempDir::new().unwrap();
        let mut first = locked_bridge_in(dir.path());
        first.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();
        let address = first.get_primary_address();
        let vault_before = std::fs::read(dir.path().join("vault.enc")).unwrap();

        let mut attacker = locked_bridge_in(dir.path());
        assert_eq!(
            attacker.unlock_vault(&Secret::new("not the passphrase")),
            Err(crate::vault_key::UnlockFailure::WrongSecret)
        );
        assert!(attacker.identities.is_empty());
        assert_eq!(attacker.get_primary_address(), "unknown");
        assert_eq!(
            std::fs::read(dir.path().join("vault.enc")).unwrap(),
            vault_before,
            "a rejected unlock rewrote the vault"
        );

        // And the real passphrase still works afterwards.
        let mut owner = locked_bridge_in(dir.path());
        owner.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();
        assert_eq!(owner.get_primary_address(), address);
    }

    #[test]
    fn locking_forgets_the_wallet_until_it_is_opened_again() {
        let dir = TempDir::new().unwrap();
        let mut bridge = locked_bridge_in(dir.path());
        bridge.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();
        let address = bridge.get_primary_address();

        bridge.lock_vault();

        assert_eq!(bridge.vault_state(), crate::vault_key::VaultState::Locked);
        assert_eq!(bridge.get_primary_address(), "unknown");

        bridge.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();
        assert_eq!(bridge.get_primary_address(), address);
    }

    #[test]
    fn no_plaintext_key_remains_after_a_full_cycle() {
        // The condition this ticket exists to end, checked by looking at the
        // directory rather than by trusting the code that wrote it.
        let dir = TempDir::new().unwrap();
        let mut bridge = locked_bridge_in(dir.path());
        bridge.unlock_vault(&Secret::new("a long enough passphrase")).unwrap();
        let key = cabal_vault::KeyProvider::data_key(&bridge.vault_key)
            .expect("an unlocked vault has a key")
            .expose_for_storage();

        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            let raw = std::fs::read(&path).unwrap_or_default();
            assert!(
                !contains(&raw, &key),
                "{} contains the raw vault key",
                path.display()
            );
            assert!(
                !contains(&raw, hex::encode(key).as_bytes()),
                "{} contains the hex-encoded vault key",
                path.display()
            );
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|window| window == needle)
    }
}
