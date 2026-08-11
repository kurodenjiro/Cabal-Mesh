//! Fail-closed verification of public marketplace seller standing.
//!
//! This crate performs no RPC or storage I/O. Adapters obtain independent
//! reads from the canonical standing registry at one explicitly pinned,
//! accepted block. The verifier accepts only a distinct-provider quorum that
//! agrees on the seller, registry, chain, block hash, count, and last mutation
//! block. Anything else is an explicit [`PublicStanding::Unknown`].
//!
//! A device-local settlement count is deliberately a separate input. It can
//! explain a mismatch but can never increase, replace, or manufacture the
//! public value shown to a marketplace buyer.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeSet;

/// Twenty-byte EVM account or contract address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvmAddress([u8; 20]);

impl EvmAddress {
    /// Creates an address from its canonical bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical address bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Whether this is the reserved zero address.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }
}

/// Thirty-two-byte hash of an accepted EVM block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockHash([u8; 32]);

impl BlockHash {
    /// Creates a block hash from its canonical bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical hash bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether the hash is absent or malformed.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }
}

/// Stable identifier assigned by the app to one independently operated RPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(u16);

impl ProviderId {
    /// Creates a non-zero provider identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderIdError::Zero`] when `value` is zero.
    pub const fn try_new(value: u16) -> Result<Self, ProviderIdError> {
        if value == 0 {
            Err(ProviderIdError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the stable numeric identifier.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Invalid provider identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderIdError {
    /// Zero is reserved for an absent provider.
    #[error("provider identifier must be non-zero")]
    Zero,
}

/// Canonical network and verification policy compiled into the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryConfig {
    chain_id: u64,
    registry: EvmAddress,
    maximum_age_ms: u64,
    minimum_provider_quorum: u8,
}

impl RegistryConfig {
    /// Builds a validated standing registry configuration.
    ///
    /// The quorum must be at least two: repeated reads from one endpoint are
    /// not independent corroboration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for a zero chain, registry, freshness window,
    /// or a provider quorum smaller than two.
    pub fn try_new(
        chain_id: u64,
        registry: EvmAddress,
        maximum_age_ms: u64,
        minimum_provider_quorum: u8,
    ) -> Result<Self, ConfigError> {
        if chain_id == 0 {
            return Err(ConfigError::ZeroChainId);
        }
        if registry.is_zero() {
            return Err(ConfigError::ZeroRegistry);
        }
        if maximum_age_ms == 0 {
            return Err(ConfigError::ZeroFreshnessWindow);
        }
        if minimum_provider_quorum < 2 {
            return Err(ConfigError::InsufficientProviderQuorum);
        }
        Ok(Self {
            chain_id,
            registry,
            maximum_age_ms,
            minimum_provider_quorum,
        })
    }

    /// Canonical EVM chain identifier.
    #[must_use]
    pub const fn chain_id(self) -> u64 {
        self.chain_id
    }

    /// Canonical standing registry contract address.
    #[must_use]
    pub const fn registry(self) -> EvmAddress {
        self.registry
    }

    /// Oldest permitted provider observation, in milliseconds.
    #[must_use]
    pub const fn maximum_age_ms(self) -> u64 {
        self.maximum_age_ms
    }

    /// Number of distinct matching RPC providers required.
    #[must_use]
    pub const fn minimum_provider_quorum(self) -> u8 {
        self.minimum_provider_quorum
    }
}

/// Invalid canonical registry configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Chain ID zero is not a deployable EVM network.
    #[error("standing chain identifier must be non-zero")]
    ZeroChainId,
    /// Registry address zero means no canonical source is configured.
    #[error("standing registry address must be non-zero")]
    ZeroRegistry,
    /// A zero window would make every observation immediately stale.
    #[error("standing freshness window must be non-zero")]
    ZeroFreshnessWindow,
    /// One provider cannot independently corroborate itself.
    #[error("standing verification requires at least two independent providers")]
    InsufficientProviderQuorum,
}

/// Parsed result of `standingOf(seller)` at one pinned block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandingSnapshot {
    /// Chain queried by the adapter.
    pub chain_id: u64,
    /// Contract queried by the adapter.
    pub registry: EvmAddress,
    /// Exact marketplace seller wallet queried.
    pub seller: EvmAddress,
    /// Net active completed settlements returned by the registry.
    pub count: u64,
    /// Registry's last mutation block for this seller.
    pub last_changed_block: u64,
    /// Exact pinned block number used for the contract read.
    pub block_number: u64,
    /// Hash of that exact pinned block.
    pub block_hash: BlockHash,
    /// Local millisecond timestamp when the response was observed.
    pub observed_at_ms: u64,
    /// True only when the provider classifies the block as accepted/final.
    pub accepted: bool,
}

impl StandingSnapshot {
    fn agrees_with(self, other: Self) -> bool {
        self.chain_id == other.chain_id
            && self.registry == other.registry
            && self.seller == other.seller
            && self.count == other.count
            && self.last_changed_block == other.last_changed_block
            && self.block_number == other.block_number
            && self.block_hash == other.block_hash
    }
}

/// One provider's read result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRead {
    /// A parsed registry response at a pinned block.
    Snapshot(StandingSnapshot),
    /// The endpoint timed out, rejected the request, or returned no parseable
    /// response. Details stay in the adapter and must not become a fake value.
    Unavailable,
    /// The endpoint answered, but for a different chain or canonical source.
    /// A successful wrong-identity response must invalidate the whole result;
    /// treating it like downtime would let a quorum silently ignore conflict.
    IdentityMismatch,
    /// The endpoint answered with structurally impossible data.
    Malformed,
}

/// Read result attributed to one independent provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderObservation {
    /// Provider that supplied this result.
    pub provider_id: ProviderId,
    /// Parsed snapshot or explicit absence.
    pub read: ProviderRead,
}

/// Standing proven by a matching distinct-provider quorum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedStanding {
    seller: EvmAddress,
    count: u64,
    last_changed_block: u64,
    block_number: u64,
    block_hash: BlockHash,
    oldest_observation_ms: u64,
    provider_count: usize,
}

impl VerifiedStanding {
    /// Marketplace seller wallet whose standing was read.
    #[must_use]
    pub const fn seller(self) -> EvmAddress {
        self.seller
    }

    /// Canonical net active settlement count.
    #[must_use]
    pub const fn count(self) -> u64 {
        self.count
    }

    /// Last registry mutation block for this seller.
    #[must_use]
    pub const fn last_changed_block(self) -> u64 {
        self.last_changed_block
    }

    /// Accepted block at which the value was verified.
    #[must_use]
    pub const fn block_number(self) -> u64 {
        self.block_number
    }

    /// Hash of the accepted verification block.
    #[must_use]
    pub const fn block_hash(self) -> BlockHash {
        self.block_hash
    }

    /// Oldest observation timestamp in the agreeing quorum.
    #[must_use]
    pub const fn oldest_observation_ms(self) -> u64 {
        self.oldest_observation_ms
    }

    /// Number of distinct providers that agreed.
    #[must_use]
    pub const fn provider_count(self) -> usize {
        self.provider_count
    }
}

/// Why no public standing may be claimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnknownStandingReason {
    /// This build has no canonical chain/registry configuration.
    Unconfigured,
    /// Fewer than the configured number of providers returned a snapshot.
    Unavailable,
    /// A provider answered for a different chain, registry, or seller wallet.
    IdentityMismatch,
    /// At least one response is older than the configured freshness window.
    Stale,
    /// At least one response comes from a block not classified as accepted.
    Unfinalized,
    /// Providers disagree about the block or authoritative value.
    ConflictingProviders,
    /// A response violates structural invariants or repeats a provider ID.
    Malformed,
}

/// Buyer-visible public standing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublicStanding {
    /// Independently corroborated canonical value.
    Verified(VerifiedStanding),
    /// No numeric value is safe to show.
    Unknown(UnknownStandingReason),
}

impl PublicStanding {
    /// Buyer-facing value. Unknown is always explicit and never rendered as
    /// numeric zero; a verified zero remains a valid measured result.
    #[must_use]
    pub fn display_value(self) -> String {
        match self {
            Self::Verified(standing) => standing.count.to_string(),
            Self::Unknown(_) => "UNKNOWN".to_owned(),
        }
    }

    /// Reason verification failed, or `None` for a verified value.
    #[must_use]
    pub const fn unknown_reason(self) -> Option<UnknownStandingReason> {
        match self {
            Self::Verified(_) => None,
            Self::Unknown(reason) => Some(reason),
        }
    }
}

/// Verifies independent snapshots against the canonical configuration.
///
/// RPC adapters must query `standingOf(seller)` at the exact `block_number`
/// whose hash they return; a latest-value read paired with a later block is not
/// valid evidence. Every successful distinct provider is checked. Unavailable
/// providers may be ignored only when the remaining matching providers still
/// meet quorum.
#[must_use]
pub fn verify_public_standing(
    config: Option<&RegistryConfig>,
    seller: EvmAddress,
    observations: &[ProviderObservation],
    now_ms: u64,
) -> PublicStanding {
    let Some(config) = config.copied() else {
        return PublicStanding::Unknown(UnknownStandingReason::Unconfigured);
    };
    if seller.is_zero() {
        return PublicStanding::Unknown(UnknownStandingReason::IdentityMismatch);
    }

    let mut provider_ids = BTreeSet::new();
    let mut snapshots = Vec::with_capacity(observations.len());
    for observation in observations {
        if !provider_ids.insert(observation.provider_id) {
            return PublicStanding::Unknown(UnknownStandingReason::Malformed);
        }
        match observation.read {
            ProviderRead::Unavailable => {}
            ProviderRead::IdentityMismatch => {
                return PublicStanding::Unknown(UnknownStandingReason::IdentityMismatch);
            }
            ProviderRead::Malformed => {
                return PublicStanding::Unknown(UnknownStandingReason::Malformed);
            }
            ProviderRead::Snapshot(snapshot) => {
                if snapshot.chain_id != config.chain_id
                    || snapshot.registry != config.registry
                    || snapshot.seller != seller
                {
                    return PublicStanding::Unknown(UnknownStandingReason::IdentityMismatch);
                }
                if !snapshot.accepted {
                    return PublicStanding::Unknown(UnknownStandingReason::Unfinalized);
                }
                if snapshot.block_number == 0
                    || snapshot.block_hash.is_zero()
                    || snapshot.last_changed_block > snapshot.block_number
                    || snapshot.observed_at_ms > now_ms
                {
                    return PublicStanding::Unknown(UnknownStandingReason::Malformed);
                }
                if now_ms - snapshot.observed_at_ms > config.maximum_age_ms {
                    return PublicStanding::Unknown(UnknownStandingReason::Stale);
                }
                snapshots.push(snapshot);
            }
        }
    }

    if snapshots.len() < usize::from(config.minimum_provider_quorum) {
        return PublicStanding::Unknown(UnknownStandingReason::Unavailable);
    }

    let first = snapshots[0];
    if snapshots
        .iter()
        .copied()
        .skip(1)
        .any(|snapshot| !first.agrees_with(snapshot))
    {
        return PublicStanding::Unknown(UnknownStandingReason::ConflictingProviders);
    }

    let oldest_observation_ms = snapshots
        .iter()
        .map(|snapshot| snapshot.observed_at_ms)
        .min()
        .unwrap_or(first.observed_at_ms);
    PublicStanding::Verified(VerifiedStanding {
        seller,
        count: first.count,
        last_changed_block: first.last_changed_block,
        block_number: first.block_number,
        block_hash: first.block_hash,
        oldest_observation_ms,
        provider_count: snapshots.len(),
    })
}

/// Explanation of how public evidence compares with this device's ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LocalReconciliation {
    /// Public evidence is unknown, so the private ledger cannot replace it.
    PublicUnknown,
    /// Public standing is verified but this device has no readable ledger.
    LocalUnavailable,
    /// Public registry and device-local count agree.
    Matches,
    /// The public registry has entries absent from this installation.
    LocalBehind {
        /// Number of public entries not represented locally.
        missing_local: u64,
    },
    /// The local ledger has settlements not yet anchored in public standing.
    LocalAhead {
        /// Number of local entries that cannot yet be claimed publicly.
        unanchored_local: u64,
    },
}

/// Reconciles local history without modifying the buyer-visible public value.
#[must_use]
pub fn reconcile_local(public: &PublicStanding, local_settled: Option<u64>) -> LocalReconciliation {
    let PublicStanding::Verified(verified) = public else {
        return LocalReconciliation::PublicUnknown;
    };
    let Some(local) = local_settled else {
        return LocalReconciliation::LocalUnavailable;
    };
    match local.cmp(&verified.count) {
        std::cmp::Ordering::Equal => LocalReconciliation::Matches,
        std::cmp::Ordering::Less => LocalReconciliation::LocalBehind {
            missing_local: verified.count - local,
        },
        std::cmp::Ordering::Greater => LocalReconciliation::LocalAhead {
            unanchored_local: local - verified.count,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 10_000_000;
    const MAXIMUM_AGE_MS: u64 = 5 * 60 * 1_000;

    fn address(byte: u8) -> EvmAddress {
        EvmAddress::from_bytes([byte; 20])
    }

    fn block_hash(byte: u8) -> BlockHash {
        BlockHash::from_bytes([byte; 32])
    }

    fn provider(value: u16) -> ProviderId {
        ProviderId::try_new(value).expect("test provider IDs are non-zero")
    }

    fn config() -> RegistryConfig {
        RegistryConfig::try_new(43_113, address(9), MAXIMUM_AGE_MS, 2)
            .expect("test config is valid")
    }

    fn snapshot(count: u64) -> StandingSnapshot {
        StandingSnapshot {
            chain_id: 43_113,
            registry: address(9),
            seller: address(7),
            count,
            last_changed_block: 490,
            block_number: 500,
            block_hash: block_hash(5),
            observed_at_ms: NOW - 1_000,
            accepted: true,
        }
    }

    fn observation(provider_id: u16, snapshot: StandingSnapshot) -> ProviderObservation {
        ProviderObservation {
            provider_id: provider(provider_id),
            read: ProviderRead::Snapshot(snapshot),
        }
    }

    fn verified(count: u64) -> PublicStanding {
        verify_public_standing(
            Some(&config()),
            address(7),
            &[
                observation(1, snapshot(count)),
                observation(2, snapshot(count)),
            ],
            NOW,
        )
    }

    #[test]
    fn configuration_requires_a_real_registry_and_independent_quorum() {
        assert_eq!(
            RegistryConfig::try_new(0, address(9), 1, 2),
            Err(ConfigError::ZeroChainId)
        );
        assert_eq!(
            RegistryConfig::try_new(43_113, address(0), 1, 2),
            Err(ConfigError::ZeroRegistry)
        );
        assert_eq!(
            RegistryConfig::try_new(43_113, address(9), 0, 2),
            Err(ConfigError::ZeroFreshnessWindow)
        );
        assert_eq!(
            RegistryConfig::try_new(43_113, address(9), 1, 1),
            Err(ConfigError::InsufficientProviderQuorum)
        );
        assert_eq!(ProviderId::try_new(0), Err(ProviderIdError::Zero));
    }

    #[test]
    fn absent_configuration_is_unknown_not_a_fallback_score() {
        let result = verify_public_standing(None, address(7), &[], NOW);
        assert_eq!(
            result,
            PublicStanding::Unknown(UnknownStandingReason::Unconfigured)
        );
        assert_eq!(result.display_value(), "UNKNOWN");
    }

    #[test]
    fn verified_zero_is_distinct_from_unknown() {
        let result = verified(0);
        assert_eq!(result.display_value(), "0");
        assert_eq!(result.unknown_reason(), None);
    }

    #[test]
    fn matching_distinct_providers_verify_the_same_pinned_block() {
        let mut second = snapshot(42);
        second.observed_at_ms = NOW - 2_000;
        let result = verify_public_standing(
            Some(&config()),
            address(7),
            &[observation(11, snapshot(42)), observation(12, second)],
            NOW,
        );
        let PublicStanding::Verified(value) = result else {
            panic!("matching observations must verify")
        };
        assert_eq!(value.seller(), address(7));
        assert_eq!(value.count(), 42);
        assert_eq!(value.last_changed_block(), 490);
        assert_eq!(value.block_number(), 500);
        assert_eq!(value.block_hash(), block_hash(5));
        assert_eq!(value.oldest_observation_ms(), NOW - 2_000);
        assert_eq!(value.provider_count(), 2);
    }

    #[test]
    fn unavailable_provider_is_ignored_only_when_quorum_remains() {
        let unavailable = ProviderObservation {
            provider_id: provider(3),
            read: ProviderRead::Unavailable,
        };
        let with_quorum = verify_public_standing(
            Some(&config()),
            address(7),
            &[
                observation(1, snapshot(8)),
                unavailable,
                observation(2, snapshot(8)),
            ],
            NOW,
        );
        assert!(matches!(with_quorum, PublicStanding::Verified(_)));

        let without_quorum = verify_public_standing(
            Some(&config()),
            address(7),
            &[observation(1, snapshot(8)), unavailable],
            NOW,
        );
        assert_eq!(
            without_quorum,
            PublicStanding::Unknown(UnknownStandingReason::Unavailable)
        );
    }

    #[test]
    fn successful_identity_or_shape_conflict_cannot_hide_as_unavailable() {
        let identity_mismatch = verify_public_standing(
            Some(&config()),
            address(7),
            &[
                observation(1, snapshot(8)),
                observation(2, snapshot(8)),
                ProviderObservation {
                    provider_id: provider(3),
                    read: ProviderRead::IdentityMismatch,
                },
            ],
            NOW,
        );
        assert_eq!(
            identity_mismatch,
            PublicStanding::Unknown(UnknownStandingReason::IdentityMismatch)
        );

        let malformed = verify_public_standing(
            Some(&config()),
            address(7),
            &[
                observation(1, snapshot(8)),
                observation(2, snapshot(8)),
                ProviderObservation {
                    provider_id: provider(3),
                    read: ProviderRead::Malformed,
                },
            ],
            NOW,
        );
        assert_eq!(
            malformed,
            PublicStanding::Unknown(UnknownStandingReason::Malformed)
        );
    }

    #[test]
    fn duplicate_provider_identity_cannot_fake_independent_quorum() {
        let result = verify_public_standing(
            Some(&config()),
            address(7),
            &[observation(1, snapshot(4)), observation(1, snapshot(4))],
            NOW,
        );
        assert_eq!(
            result,
            PublicStanding::Unknown(UnknownStandingReason::Malformed)
        );
    }

    #[test]
    fn wrong_chain_registry_or_seller_is_an_identity_mismatch() {
        for invalid in [
            StandingSnapshot {
                chain_id: 1,
                ..snapshot(4)
            },
            StandingSnapshot {
                registry: address(8),
                ..snapshot(4)
            },
            StandingSnapshot {
                seller: address(6),
                ..snapshot(4)
            },
        ] {
            let result = verify_public_standing(
                Some(&config()),
                address(7),
                &[observation(1, invalid), observation(2, snapshot(4))],
                NOW,
            );
            assert_eq!(
                result,
                PublicStanding::Unknown(UnknownStandingReason::IdentityMismatch)
            );
        }

        let zero_seller = verify_public_standing(Some(&config()), address(0), &[], NOW);
        assert_eq!(
            zero_seller,
            PublicStanding::Unknown(UnknownStandingReason::IdentityMismatch)
        );
    }

    #[test]
    fn stale_observation_is_unknown() {
        let stale = StandingSnapshot {
            observed_at_ms: NOW - MAXIMUM_AGE_MS - 1,
            ..snapshot(5)
        };
        let result = verify_public_standing(
            Some(&config()),
            address(7),
            &[observation(1, stale), observation(2, snapshot(5))],
            NOW,
        );
        assert_eq!(
            result,
            PublicStanding::Unknown(UnknownStandingReason::Stale)
        );
    }

    #[test]
    fn unaccepted_block_is_never_buyer_visible() {
        let unfinalized = StandingSnapshot {
            accepted: false,
            ..snapshot(5)
        };
        let result = verify_public_standing(
            Some(&config()),
            address(7),
            &[observation(1, unfinalized), observation(2, snapshot(5))],
            NOW,
        );
        assert_eq!(
            result,
            PublicStanding::Unknown(UnknownStandingReason::Unfinalized)
        );
    }

    #[test]
    fn malformed_block_metadata_is_unknown() {
        for invalid in [
            StandingSnapshot {
                block_number: 0,
                ..snapshot(5)
            },
            StandingSnapshot {
                block_hash: block_hash(0),
                ..snapshot(5)
            },
            StandingSnapshot {
                last_changed_block: 501,
                ..snapshot(5)
            },
            StandingSnapshot {
                observed_at_ms: NOW + 1,
                ..snapshot(5)
            },
        ] {
            let result = verify_public_standing(
                Some(&config()),
                address(7),
                &[observation(1, invalid), observation(2, snapshot(5))],
                NOW,
            );
            assert_eq!(
                result,
                PublicStanding::Unknown(UnknownStandingReason::Malformed)
            );
        }
    }

    #[test]
    fn provider_disagreement_about_value_or_block_is_unknown() {
        let different_count = snapshot(6);
        let different_hash = StandingSnapshot {
            block_hash: block_hash(6),
            ..snapshot(5)
        };
        let different_height = StandingSnapshot {
            block_number: 501,
            ..snapshot(5)
        };

        for conflicting in [different_count, different_hash, different_height] {
            let result = verify_public_standing(
                Some(&config()),
                address(7),
                &[observation(1, snapshot(5)), observation(2, conflicting)],
                NOW,
            );
            assert_eq!(
                result,
                PublicStanding::Unknown(UnknownStandingReason::ConflictingProviders)
            );
            assert_eq!(result.display_value(), "UNKNOWN");
        }
    }

    #[test]
    fn local_ledger_never_replaces_unknown_public_evidence() {
        let unknown = PublicStanding::Unknown(UnknownStandingReason::Unavailable);
        assert_eq!(
            reconcile_local(&unknown, Some(999)),
            LocalReconciliation::PublicUnknown
        );
        assert_eq!(unknown.display_value(), "UNKNOWN");
    }

    #[test]
    fn local_reconciliation_explains_all_comparisons_without_merging_counts() {
        let public = verified(10);
        assert_eq!(
            reconcile_local(&public, None),
            LocalReconciliation::LocalUnavailable
        );
        assert_eq!(
            reconcile_local(&public, Some(10)),
            LocalReconciliation::Matches
        );
        assert_eq!(
            reconcile_local(&public, Some(7)),
            LocalReconciliation::LocalBehind { missing_local: 3 }
        );
        assert_eq!(
            reconcile_local(&public, Some(14)),
            LocalReconciliation::LocalAhead {
                unanchored_local: 4
            }
        );
        assert_eq!(
            public.display_value(),
            "10",
            "reconciliation must not alter public standing"
        );
    }
}
