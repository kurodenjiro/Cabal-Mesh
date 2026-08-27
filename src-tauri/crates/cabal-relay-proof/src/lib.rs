//! EIP-712 proof of requested third-party relay work.
//!
//! Sender authorization, each ordered relay contribution, and the recipient
//! acknowledgement are separate typed wallet signatures. The EIP-712 domain
//! binds signatures to one chain and settlement contract. This crate performs
//! no I/O and owns no replay database; callers provide consumed route and
//! contribution identifiers from authoritative settlement state.

#![forbid(unsafe_code)]

use alloy_primitives::{keccak256, Address, Signature, B256};
use alloy_sol_types::{eip712_domain, sol, Eip712Domain, SolStruct};
use cabal_rewards::{
    quote, BillableBytes, NAvax, RelayCount, RewardQuote, MAX_AUTHORIZATION_SECONDS,
    MIN_AUTHORIZATION_SECONDS, POLICY_VERSION,
};
use std::collections::HashSet;

/// Human-readable EIP-712 signing domain.
pub const DOMAIN_NAME: &str = "CabalMesh Relay Proof";

/// EIP-712 domain version. Policy changes use a different version.
pub const DOMAIN_VERSION: &str = "1";

const PAYLOAD_COMMITMENT_PREFIX: &[u8] = b"CABAL_PAYLOAD_V1\0";
const ROUTE_HASH_PREFIX: &[u8] = b"CABAL_RELAY_ROUTE_V1\0";
const CONTRIBUTION_HASH_PREFIX: &[u8] = b"CABAL_CONTRIBUTIONS_V1\0";

sol! {
    /// Sender-approved route, economics, payload, and lifetime.
    #[derive(Debug, PartialEq, Eq)]
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

    /// One ordered relay's attestation that it forwarded the authorized data.
    #[derive(Debug, PartialEq, Eq)]
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

    /// Recipient receipt over the exact ordered contribution identifiers.
    #[derive(Debug, PartialEq, Eq)]
    struct RecipientAcknowledgement {
        bytes32 authorizationHash;
        bytes32 contributionsHash;
        address recipient;
        bytes32 payloadCommitment;
        uint64 deliveredBytes;
        uint64 receivedAt;
    }
}

/// How recipient-acknowledged bytes determine proof completeness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryMode {
    /// A discrete intent is eligible only when every authorized byte arrives.
    CompletePayload = 0,
    /// A gateway authorization may settle a non-zero acknowledged byte window.
    AcknowledgedByteWindow = 1,
}

impl TryFrom<u8> for DeliveryMode {
    type Error = ProofError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::CompletePayload),
            1 => Ok(Self::AcknowledgedByteWindow),
            _ => Err(ProofError::InvalidDeliveryMode),
        }
    }
}

/// Signed authorization plus the ordered route whose hash it commits to.
#[derive(Debug, Clone)]
pub struct SignedAuthorization {
    message: RelayAuthorization,
    relayers: Box<[Address]>,
    signature: Signature,
}

impl SignedAuthorization {
    /// Creates a transportable signed authorization.
    #[must_use]
    pub fn new(
        message: RelayAuthorization,
        relayers: Box<[Address]>,
        signature: Signature,
    ) -> Self {
        Self {
            message,
            relayers,
            signature,
        }
    }

    /// Typed sender message.
    #[must_use]
    pub const fn message(&self) -> &RelayAuthorization {
        &self.message
    }

    /// Ordered relay wallet addresses.
    #[must_use]
    pub fn relayers(&self) -> &[Address] {
        &self.relayers
    }

    /// ECDSA wallet signature.
    #[must_use]
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }
}

/// One EIP-712-signed relay contribution.
#[derive(Debug, Clone)]
pub struct SignedContribution {
    message: RelayContribution,
    signature: Signature,
}

impl SignedContribution {
    /// Creates a signed contribution.
    #[must_use]
    pub const fn new(message: RelayContribution, signature: Signature) -> Self {
        Self { message, signature }
    }

    /// Typed contribution message.
    #[must_use]
    pub const fn message(&self) -> &RelayContribution {
        &self.message
    }

    /// ECDSA wallet signature.
    #[must_use]
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }
}

/// EIP-712-signed recipient acknowledgement.
#[derive(Debug, Clone)]
pub struct SignedAcknowledgement {
    message: RecipientAcknowledgement,
    signature: Signature,
}

impl SignedAcknowledgement {
    /// Creates a signed acknowledgement.
    #[must_use]
    pub const fn new(message: RecipientAcknowledgement, signature: Signature) -> Self {
        Self { message, signature }
    }

    /// Typed acknowledgement message.
    #[must_use]
    pub const fn message(&self) -> &RecipientAcknowledgement {
        &self.message
    }

    /// ECDSA wallet signature.
    #[must_use]
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }
}

/// Complete evidence submitted for one route.
#[derive(Debug, Clone)]
pub struct RelayProof {
    authorization: SignedAuthorization,
    contributions: Box<[SignedContribution]>,
    acknowledgement: Option<SignedAcknowledgement>,
}

impl RelayProof {
    /// Creates a proof bundle. Verification treats a missing acknowledgement
    /// as ineligible rather than guessing that delivery occurred.
    #[must_use]
    pub fn new(
        authorization: SignedAuthorization,
        contributions: Box<[SignedContribution]>,
        acknowledgement: Option<SignedAcknowledgement>,
    ) -> Self {
        Self {
            authorization,
            contributions,
            acknowledgement,
        }
    }

    /// Sender authorization and ordered route.
    #[must_use]
    pub const fn authorization(&self) -> &SignedAuthorization {
        &self.authorization
    }

    /// Ordered relay contributions.
    #[must_use]
    pub fn contributions(&self) -> &[SignedContribution] {
        &self.contributions
    }

    /// Recipient acknowledgement, if present.
    #[must_use]
    pub const fn acknowledgement(&self) -> Option<&SignedAcknowledgement> {
        self.acknowledgement.as_ref()
    }
}

/// Authoritative context that is deliberately not trusted from proof bytes.
pub struct VerificationContext<'a> {
    pub chain_id: u64,
    pub settlement_contract: Address,
    pub now: u64,
    pub expected_payload_commitment: B256,
    pub consumed_routes: &'a HashSet<B256>,
    pub consumed_contributions: &'a HashSet<B256>,
}

/// Eligibility returned only after every signature and invariant succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRelayProof {
    route_id: B256,
    contribution_ids: Box<[B256]>,
    relayers: Box<[Address]>,
    delivered_bytes: BillableBytes,
    delivery_mode: DeliveryMode,
    maximum_charge: NAvax,
    reward_quote: RewardQuote,
    expires_at: u64,
}

impl VerifiedRelayProof {
    /// Single-use authorization identifier.
    #[must_use]
    pub const fn route_id(&self) -> B256 {
        self.route_id
    }

    /// Single-use ordered contribution identifiers.
    #[must_use]
    pub fn contribution_ids(&self) -> &[B256] {
        &self.contribution_ids
    }

    /// Ordered eligible relay payout addresses.
    #[must_use]
    pub fn relayers(&self) -> &[Address] {
        &self.relayers
    }

    /// Recipient-acknowledged logical bytes.
    #[must_use]
    pub const fn delivered_bytes(&self) -> BillableBytes {
        self.delivered_bytes
    }

    /// Completeness rule signed by the sender.
    #[must_use]
    pub const fn delivery_mode(&self) -> DeliveryMode {
        self.delivery_mode
    }

    /// Sender-authorized maximum charge.
    #[must_use]
    pub const fn maximum_charge(&self) -> NAvax {
        self.maximum_charge
    }

    /// Policy-derived quote used to divide rewards among the eligible relays.
    #[must_use]
    pub const fn reward_quote(&self) -> &RewardQuote {
        &self.reward_quote
    }

    /// Last accepted proof time.
    #[must_use]
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// Why a relay proof is ineligible for payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProofError {
    #[error("verification context is invalid")]
    InvalidContext,
    #[error("proof policy does not match the active policy")]
    PolicyMismatch,
    #[error("authorization route nonce must be non-zero")]
    InvalidRouteNonce,
    #[error("reward terms do not match the active policy")]
    RewardTermsMismatch,
    #[error("authorization time window is invalid")]
    InvalidAuthorizationWindow,
    #[error("authorization was issued in the future")]
    FutureAuthorization,
    #[error("proof expired before verification")]
    Expired,
    #[error("route length does not match the authorization")]
    RouteLengthMismatch,
    #[error("ordered relay route hash does not match the authorization")]
    RouteHashMismatch,
    #[error("a proof participant is the zero address")]
    ZeroParticipant,
    #[error("sender, relayer, and recipient must use distinct operator wallets")]
    CommonControl,
    #[error("sender signature is invalid")]
    InvalidSenderSignature,
    #[error("proof payload differs from the payload being settled")]
    AlteredPayload,
    #[error("authorization delivery mode is invalid")]
    InvalidDeliveryMode,
    #[error("route authorization was already consumed")]
    RouteAlreadyConsumed,
    #[error("contribution count does not match the signed route")]
    ContributionCountMismatch,
    #[error("contribution {hop} does not match its ordered route position")]
    ContributionRouteMismatch { hop: u8 },
    #[error("contribution {hop} carries inconsistent payload or byte evidence")]
    ContributionEvidenceMismatch { hop: u8 },
    #[error("contribution {hop} has an invalid timestamp")]
    InvalidContributionTime { hop: u8 },
    #[error("contribution {hop} signature is invalid")]
    InvalidRelayerSignature { hop: u8 },
    #[error("contribution {hop} was already consumed")]
    ContributionAlreadyConsumed { hop: u8 },
    #[error("proof repeats a contribution identifier")]
    DuplicateContribution,
    #[error("a discrete payload was not delivered completely")]
    IncompleteDiscretePayload,
    #[error("recipient acknowledgement is required")]
    MissingAcknowledgement,
    #[error("recipient acknowledgement does not match the route evidence")]
    AcknowledgementMismatch,
    #[error("recipient acknowledgement has an invalid timestamp")]
    InvalidAcknowledgementTime,
    #[error("recipient signature is invalid")]
    InvalidRecipientSignature,
    #[error("proof arithmetic or bounded conversion failed")]
    InvalidBoundedValue,
}

/// EIP-712 domain that prevents reuse across chains and settlement contracts.
#[must_use]
pub fn proof_domain(chain_id: u64, settlement_contract: Address) -> Eip712Domain {
    eip712_domain! {
        name: DOMAIN_NAME,
        version: DOMAIN_VERSION,
        chain_id: chain_id,
        verifying_contract: settlement_contract,
    }
}

/// Active reward policy hash carried in every sender authorization.
#[must_use]
pub fn policy_hash() -> B256 {
    keccak256(POLICY_VERSION.as_bytes())
}

/// Commits to the exact logical payload bytes without retaining their content.
#[must_use]
pub fn payload_commitment(payload: &[u8]) -> B256 {
    let mut encoded = Vec::with_capacity(PAYLOAD_COMMITMENT_PREFIX.len() + payload.len());
    encoded.extend_from_slice(PAYLOAD_COMMITMENT_PREFIX);
    encoded.extend_from_slice(payload);
    keccak256(encoded)
}

/// Commits to an ordered relay route.
///
/// # Errors
///
/// Rejects routes outside the policy's 1..=3 relay bound.
pub fn relay_route_hash(relayers: &[Address]) -> Result<B256, ProofError> {
    let count = RelayCount::try_from(
        u8::try_from(relayers.len()).map_err(|_| ProofError::RouteLengthMismatch)?,
    )
    .map_err(|_| ProofError::RouteLengthMismatch)?;
    let mut encoded = Vec::with_capacity(ROUTE_HASH_PREFIX.len() + 1 + relayers.len() * 20);
    encoded.extend_from_slice(ROUTE_HASH_PREFIX);
    encoded.push(count.get());
    for relayer in relayers {
        encoded.extend_from_slice(relayer.as_slice());
    }
    Ok(keccak256(encoded))
}

/// Commits to ordered contribution identifiers for the recipient signature.
///
/// # Errors
///
/// Rejects a list outside the policy's 1..=3 contribution bound.
pub fn ordered_contributions_hash(ids: &[B256]) -> Result<B256, ProofError> {
    RelayCount::try_from(
        u8::try_from(ids.len()).map_err(|_| ProofError::ContributionCountMismatch)?,
    )
    .map_err(|_| ProofError::ContributionCountMismatch)?;
    let mut encoded = Vec::with_capacity(CONTRIBUTION_HASH_PREFIX.len() + 1 + ids.len() * 32);
    encoded.extend_from_slice(CONTRIBUTION_HASH_PREFIX);
    encoded.push(u8::try_from(ids.len()).map_err(|_| ProofError::ContributionCountMismatch)?);
    for id in ids {
        encoded.extend_from_slice(id.as_slice());
    }
    Ok(keccak256(encoded))
}

/// EIP-712 signing hash and single-use route identifier.
#[must_use]
pub fn authorization_hash(message: &RelayAuthorization, domain: &Eip712Domain) -> B256 {
    message.eip712_signing_hash(domain)
}

/// EIP-712 signing hash and single-use contribution identifier.
#[must_use]
pub fn contribution_hash(message: &RelayContribution, domain: &Eip712Domain) -> B256 {
    message.eip712_signing_hash(domain)
}

/// EIP-712 signing hash for the recipient acknowledgement.
#[must_use]
pub fn acknowledgement_hash(message: &RecipientAcknowledgement, domain: &Eip712Domain) -> B256 {
    message.eip712_signing_hash(domain)
}

/// Verifies one complete proof without performing I/O or mutating replay state.
///
/// # Errors
///
/// Returns a specific [`ProofError`] for every failed signature, route,
/// evidence, replay, timing, common-control, or economics invariant.
pub fn verify(
    proof: &RelayProof,
    context: &VerificationContext<'_>,
) -> Result<VerifiedRelayProof, ProofError> {
    if context.chain_id == 0 || context.settlement_contract == Address::ZERO {
        return Err(ProofError::InvalidContext);
    }

    let authorization = proof.authorization.message();
    if authorization.policyHash != policy_hash() {
        return Err(ProofError::PolicyMismatch);
    }
    if authorization.routeNonce == B256::ZERO {
        return Err(ProofError::InvalidRouteNonce);
    }
    if authorization.sender == Address::ZERO || authorization.recipient == Address::ZERO {
        return Err(ProofError::ZeroParticipant);
    }

    let duration = authorization
        .expiresAt
        .checked_sub(authorization.issuedAt)
        .ok_or(ProofError::InvalidAuthorizationWindow)?;
    if !(MIN_AUTHORIZATION_SECONDS..=MAX_AUTHORIZATION_SECONDS).contains(&duration) {
        return Err(ProofError::InvalidAuthorizationWindow);
    }
    if authorization.issuedAt > context.now {
        return Err(ProofError::FutureAuthorization);
    }
    if context.now > authorization.expiresAt {
        return Err(ProofError::Expired);
    }

    let relay_count = RelayCount::try_from(authorization.relayCount)
        .map_err(|_| ProofError::RouteLengthMismatch)?;
    if proof.authorization.relayers().len() != usize::from(relay_count.get()) {
        return Err(ProofError::RouteLengthMismatch);
    }
    if relay_route_hash(proof.authorization.relayers())? != authorization.relayRouteHash {
        return Err(ProofError::RouteHashMismatch);
    }
    validate_distinct_participants(
        authorization.sender,
        proof.authorization.relayers(),
        authorization.recipient,
    )?;

    let authorized_bytes = BillableBytes::try_from(authorization.authorizedBytes)
        .map_err(|_| ProofError::InvalidBoundedValue)?;
    let delivery_mode = DeliveryMode::try_from(authorization.deliveryMode)?;
    let reward_quote =
        quote(authorized_bytes, relay_count).map_err(|_| ProofError::InvalidBoundedValue)?;
    if reward_quote.maximum_charge().raw() != authorization.maximumChargeNavax {
        return Err(ProofError::RewardTermsMismatch);
    }

    if authorization.payloadCommitment != context.expected_payload_commitment {
        return Err(ProofError::AlteredPayload);
    }

    let domain = proof_domain(context.chain_id, context.settlement_contract);
    let route_id = authorization_hash(authorization, &domain);
    if !signature_matches(
        proof.authorization.signature(),
        &route_id,
        authorization.sender,
    ) {
        return Err(ProofError::InvalidSenderSignature);
    }
    if context.consumed_routes.contains(&route_id) {
        return Err(ProofError::RouteAlreadyConsumed);
    }

    if proof.contributions().len() != usize::from(relay_count.get()) {
        return Err(ProofError::ContributionCountMismatch);
    }

    let mut contribution_ids = Vec::with_capacity(proof.contributions().len());
    let mut seen = HashSet::with_capacity(proof.contributions().len());
    let mut delivered_bytes = None;
    let mut last_forwarded_at = authorization.issuedAt;

    for (index, signed) in proof.contributions().iter().enumerate() {
        let hop = u8::try_from(index).map_err(|_| ProofError::ContributionCountMismatch)?;
        let contribution = signed.message();
        let expected_relayer = proof.authorization.relayers()[index];
        let expected_ingress = if index == 0 {
            authorization.sender
        } else {
            proof.authorization.relayers()[index - 1]
        };
        let expected_egress = if index + 1 == proof.authorization.relayers().len() {
            authorization.recipient
        } else {
            proof.authorization.relayers()[index + 1]
        };

        if contribution.authorizationHash != route_id
            || contribution.hopIndex != hop
            || contribution.relayer != expected_relayer
            || contribution.ingress != expected_ingress
            || contribution.egress != expected_egress
        {
            return Err(ProofError::ContributionRouteMismatch { hop });
        }
        let bytes = BillableBytes::try_from(contribution.deliveredBytes)
            .map_err(|_| ProofError::ContributionEvidenceMismatch { hop })?;
        if contribution.payloadCommitment != authorization.payloadCommitment
            || bytes.get() > authorized_bytes.get()
            || delivered_bytes.is_some_and(|previous: BillableBytes| previous != bytes)
        {
            return Err(ProofError::ContributionEvidenceMismatch { hop });
        }
        if contribution.forwardedAt < last_forwarded_at
            || contribution.forwardedAt > context.now
            || contribution.forwardedAt > authorization.expiresAt
        {
            return Err(ProofError::InvalidContributionTime { hop });
        }

        let id = contribution_hash(contribution, &domain);
        if !signature_matches(signed.signature(), &id, expected_relayer) {
            return Err(ProofError::InvalidRelayerSignature { hop });
        }
        if context.consumed_contributions.contains(&id) {
            return Err(ProofError::ContributionAlreadyConsumed { hop });
        }
        if !seen.insert(id) {
            return Err(ProofError::DuplicateContribution);
        }

        delivered_bytes = Some(bytes);
        last_forwarded_at = contribution.forwardedAt;
        contribution_ids.push(id);
    }

    let acknowledgement = proof
        .acknowledgement()
        .ok_or(ProofError::MissingAcknowledgement)?;
    let acknowledgement_message = acknowledgement.message();
    let delivered_bytes = delivered_bytes.ok_or(ProofError::ContributionCountMismatch)?;
    if delivery_mode == DeliveryMode::CompletePayload && delivered_bytes != authorized_bytes {
        return Err(ProofError::IncompleteDiscretePayload);
    }
    if acknowledgement_message.authorizationHash != route_id
        || acknowledgement_message.contributionsHash
            != ordered_contributions_hash(&contribution_ids)?
        || acknowledgement_message.recipient != authorization.recipient
        || acknowledgement_message.payloadCommitment != authorization.payloadCommitment
        || acknowledgement_message.deliveredBytes != delivered_bytes.get()
    {
        return Err(ProofError::AcknowledgementMismatch);
    }
    if acknowledgement_message.receivedAt < last_forwarded_at
        || acknowledgement_message.receivedAt > context.now
        || acknowledgement_message.receivedAt > authorization.expiresAt
    {
        return Err(ProofError::InvalidAcknowledgementTime);
    }

    let acknowledgement_id = acknowledgement_hash(acknowledgement_message, &domain);
    if !signature_matches(
        acknowledgement.signature(),
        &acknowledgement_id,
        authorization.recipient,
    ) {
        return Err(ProofError::InvalidRecipientSignature);
    }

    Ok(VerifiedRelayProof {
        route_id,
        contribution_ids: contribution_ids.into_boxed_slice(),
        relayers: proof.authorization.relayers.clone(),
        delivered_bytes,
        delivery_mode,
        maximum_charge: NAvax::from_raw(authorization.maximumChargeNavax),
        reward_quote,
        expires_at: authorization.expiresAt,
    })
}

fn validate_distinct_participants(
    sender: Address,
    relayers: &[Address],
    recipient: Address,
) -> Result<(), ProofError> {
    let mut participants = HashSet::with_capacity(relayers.len() + 2);
    if !participants.insert(sender) || !participants.insert(recipient) {
        return Err(ProofError::CommonControl);
    }
    for relayer in relayers {
        if *relayer == Address::ZERO {
            return Err(ProofError::ZeroParticipant);
        }
        if !participants.insert(*relayer) {
            return Err(ProofError::CommonControl);
        }
    }
    Ok(())
}

fn signature_matches(signature: &Signature, hash: &B256, expected: Address) -> bool {
    if signature.normalize_s().is_some() {
        return false;
    }
    signature
        .recover_address_from_prehash(hash)
        .is_ok_and(|recovered| recovered == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256};
    use alloy_signer::{Signer, SignerSync};
    use alloy_signer_local::PrivateKeySigner;
    use cabal_rewards::{settle_complete_route, BonusBps, DEFAULT_AUTHORIZATION_SECONDS};

    const CHAIN_ID: u64 = 43_113;
    const ISSUED_AT: u64 = 1_800_000_000;
    const NOW: u64 = ISSUED_AT + 5 * 60;
    const AUTHORIZED_BYTES: u64 = 100_000;
    const PARTIAL_BYTES: u64 = 60_000;
    const SENDER_KEY: u8 = 1;
    const RECIPIENT_KEY: u8 = 10;
    const PAYLOAD: &[u8] = b"cabalmesh encrypted intent payload test vector v1";

    fn settlement_contract() -> Address {
        Address::repeat_byte(0x99)
    }

    fn signer(seed: u8) -> PrivateKeySigner {
        let mut private_key = [0_u8; 32];
        private_key[31] = seed;
        PrivateKeySigner::from_slice(&private_key)
            .expect("test vector uses a non-zero secp256k1 private key")
    }

    fn sign(signer: &PrivateKeySigner, hash: B256) -> Signature {
        signer
            .sign_hash_sync(&hash)
            .expect("deterministic test vector must sign")
    }

    fn valid_proof(relay_keys: &[u8]) -> RelayProof {
        signed_proof(
            relay_keys,
            DeliveryMode::CompletePayload as u8,
            AUTHORIZED_BYTES,
        )
    }

    fn signed_proof(relay_keys: &[u8], delivery_mode: u8, delivered_bytes: u64) -> RelayProof {
        let sender = signer(SENDER_KEY);
        let recipient = signer(RECIPIENT_KEY);
        let relay_signers = relay_keys.iter().copied().map(signer).collect::<Vec<_>>();
        let relayers = relay_signers
            .iter()
            .map(Signer::address)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let relay_count = RelayCount::try_from(
            u8::try_from(relayers.len()).expect("test route contains at most three relays"),
        )
        .expect("test route count is policy-valid");
        let authorized_bytes =
            BillableBytes::try_from(AUTHORIZED_BYTES).expect("test bytes are policy-valid");
        let reward_quote = quote(authorized_bytes, relay_count).expect("test quote must succeed");
        let domain = proof_domain(CHAIN_ID, settlement_contract());
        let payload = payload_commitment(PAYLOAD);

        let authorization_message = RelayAuthorization {
            policyHash: policy_hash(),
            routeNonce: B256::repeat_byte(0x42),
            payloadCommitment: payload,
            deliveryMode: delivery_mode,
            relayRouteHash: relay_route_hash(&relayers).expect("test route must hash"),
            sender: sender.address(),
            recipient: recipient.address(),
            authorizedBytes: AUTHORIZED_BYTES,
            relayCount: relay_count.get(),
            maximumChargeNavax: reward_quote.maximum_charge().raw(),
            issuedAt: ISSUED_AT,
            expiresAt: ISSUED_AT + DEFAULT_AUTHORIZATION_SECONDS,
        };
        let route_id = authorization_hash(&authorization_message, &domain);
        let authorization = SignedAuthorization::new(
            authorization_message,
            relayers.clone(),
            sign(&sender, route_id),
        );

        let mut contribution_ids = Vec::with_capacity(relay_signers.len());
        let contributions = relay_signers
            .iter()
            .enumerate()
            .map(|(index, relay_signer)| {
                let forwarded_at =
                    ISSUED_AT + 60 * (u64::try_from(index).expect("test index fits u64") + 1);
                let message = RelayContribution {
                    authorizationHash: route_id,
                    hopIndex: u8::try_from(index).expect("test index fits u8"),
                    relayer: relay_signer.address(),
                    ingress: if index == 0 {
                        sender.address()
                    } else {
                        relayers[index - 1]
                    },
                    egress: if index + 1 == relayers.len() {
                        recipient.address()
                    } else {
                        relayers[index + 1]
                    },
                    payloadCommitment: payload,
                    deliveredBytes: delivered_bytes,
                    forwardedAt: forwarded_at,
                };
                let id = contribution_hash(&message, &domain);
                contribution_ids.push(id);
                SignedContribution::new(message, sign(relay_signer, id))
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let received_at = ISSUED_AT
            + 60 * (u64::try_from(relay_signers.len()).expect("test length fits u64") + 1);
        let acknowledgement_message = RecipientAcknowledgement {
            authorizationHash: route_id,
            contributionsHash: ordered_contributions_hash(&contribution_ids)
                .expect("test contributions must hash"),
            recipient: recipient.address(),
            payloadCommitment: payload,
            deliveredBytes: delivered_bytes,
            receivedAt: received_at,
        };
        let acknowledgement_id = acknowledgement_hash(&acknowledgement_message, &domain);
        let acknowledgement = SignedAcknowledgement::new(
            acknowledgement_message,
            sign(&recipient, acknowledgement_id),
        );

        RelayProof::new(authorization, contributions, Some(acknowledgement))
    }

    fn verify_with_state(
        proof: &RelayProof,
        now: u64,
        expected_payload: B256,
        consumed_routes: &HashSet<B256>,
        consumed_contributions: &HashSet<B256>,
    ) -> Result<VerifiedRelayProof, ProofError> {
        verify(
            proof,
            &VerificationContext {
                chain_id: CHAIN_ID,
                settlement_contract: settlement_contract(),
                now,
                expected_payload_commitment: expected_payload,
                consumed_routes,
                consumed_contributions,
            },
        )
    }

    fn verify_fresh(proof: &RelayProof) -> Result<VerifiedRelayProof, ProofError> {
        verify_with_state(
            proof,
            NOW,
            payload_commitment(PAYLOAD),
            &HashSet::new(),
            &HashSet::new(),
        )
    }

    #[test]
    fn canonical_eip712_type_strings_are_stable() {
        assert_eq!(
            RelayAuthorization::eip712_root_type().as_ref(),
            "RelayAuthorization(bytes32 policyHash,bytes32 routeNonce,bytes32 payloadCommitment,uint8 deliveryMode,bytes32 relayRouteHash,address sender,address recipient,uint64 authorizedBytes,uint8 relayCount,uint64 maximumChargeNavax,uint64 issuedAt,uint64 expiresAt)"
        );
        assert_eq!(
            RelayContribution::eip712_root_type().as_ref(),
            "RelayContribution(bytes32 authorizationHash,uint8 hopIndex,address relayer,address ingress,address egress,bytes32 payloadCommitment,uint64 deliveredBytes,uint64 forwardedAt)"
        );
        assert_eq!(
            RecipientAcknowledgement::eip712_root_type().as_ref(),
            "RecipientAcknowledgement(bytes32 authorizationHash,bytes32 contributionsHash,address recipient,bytes32 payloadCommitment,uint64 deliveredBytes,uint64 receivedAt)"
        );
    }

    #[test]
    fn deterministic_three_node_route_verifies_and_settles_one_relay() {
        let proof = valid_proof(&[2]);
        let domain = proof_domain(CHAIN_ID, settlement_contract());
        let contribution_id = contribution_hash(proof.contributions[0].message(), &domain);

        assert_eq!(
            signer(SENDER_KEY).address(),
            address!("0x7e5f4552091a69125d5dfcb7b8c2659029395bdf")
        );
        assert_eq!(
            signer(2).address(),
            address!("0x2b5ad5c4795c026514f8317c7a215e218dccd6cf")
        );
        assert_eq!(
            signer(RECIPIENT_KEY).address(),
            address!("0x4cceba2d7d2b4fdce4304d3e09a1fea9fbeb1528")
        );
        assert_eq!(
            ordered_contributions_hash(&[contribution_id]).expect("fixture list must hash"),
            b256!("0x4c0dae468e63953940bc7e3ae9336684bbc368b4065762d280fd19d5964d3e04")
        );

        assert_eq!(
            policy_hash(),
            b256!("0x7d3821fdcb04674be80351b9825999ac97df54c20641a2385b8358417c3fe715")
        );
        assert_eq!(
            payload_commitment(PAYLOAD),
            b256!("0x3d4e45347523184a29acfdeb1d303b18024c39d66b0dadffa328d200537eabde")
        );
        assert_eq!(
            relay_route_hash(proof.authorization.relayers()).expect("fixture route must hash"),
            b256!("0x2de7af3f987af09739b79eb9552a2a47f16fbe81073c6cbdab153789424634fe")
        );
        assert_eq!(
            authorization_hash(proof.authorization.message(), &domain),
            b256!("0xecfc329182f65e5e88e1c7fbb590e7d9211dac8e56d1c80474366c38140a80a1")
        );
        assert_eq!(
            contribution_id,
            b256!("0xffa46f8d9747fc79416946ed100014aa2fb1c85ded231f5905714ac8c2aa2919")
        );
        assert_eq!(
            acknowledgement_hash(
                proof
                    .acknowledgement()
                    .expect("fixture acknowledgement")
                    .message(),
                &domain,
            ),
            b256!("0xaf71c0faf912001a2fa5a2a0afba72978f84f1977ae3692b76a84451775ade56")
        );

        let verified = verify_fresh(&proof).expect("valid sender-relay-recipient vector");
        let settlement = settle_complete_route(
            verified.reward_quote(),
            verified.delivered_bytes(),
            &[BonusBps::default()],
            NAvax::zero(),
        )
        .expect("verified route must settle");

        assert_eq!(verified.relayers(), &[signer(2).address()]);
        assert_eq!(verified.contribution_ids().len(), 1);
        assert_eq!(verified.contribution_ids(), &[contribution_id]);
        assert_eq!(verified.delivered_bytes().get(), AUTHORIZED_BYTES);
        assert_eq!(verified.delivery_mode(), DeliveryMode::CompletePayload);
        assert_eq!(
            verified.maximum_charge(),
            verified.reward_quote().maximum_charge()
        );
        assert_eq!(settlement.relay_payouts().len(), 1);
        assert!(settlement.relay_payouts()[0] > NAvax::zero());
    }

    #[test]
    fn ordered_multi_relay_route_verifies_and_divides_reward_once_per_hop() {
        let proof = valid_proof(&[2, 3, 4]);

        let verified = verify_fresh(&proof).expect("valid three-relay vector");
        let settlement = settle_complete_route(
            verified.reward_quote(),
            verified.delivered_bytes(),
            &[BonusBps::default(); 3],
            NAvax::zero(),
        )
        .expect("verified multi-relay route must settle");

        assert_eq!(verified.relayers().len(), 3);
        assert_eq!(verified.contribution_ids().len(), 3);
        assert_eq!(settlement.relay_payouts().len(), 3);
        assert!(verified
            .contribution_ids()
            .windows(2)
            .all(|ids| ids[0] != ids[1]));
        assert!(settlement
            .relay_payouts()
            .windows(2)
            .all(|payouts| payouts[0] == payouts[1]));
    }

    #[test]
    fn acknowledged_gateway_window_can_settle_partial_bytes() {
        let proof = signed_proof(
            &[2],
            DeliveryMode::AcknowledgedByteWindow as u8,
            PARTIAL_BYTES,
        );

        let verified = verify_fresh(&proof).expect("valid partial gateway window");
        let settlement = settle_complete_route(
            verified.reward_quote(),
            verified.delivered_bytes(),
            &[BonusBps::default()],
            NAvax::zero(),
        )
        .expect("acknowledged gateway bytes must settle");

        assert_eq!(
            verified.delivery_mode(),
            DeliveryMode::AcknowledgedByteWindow
        );
        assert_eq!(verified.delivered_bytes().get(), PARTIAL_BYTES);
        assert!(settlement.relay_payouts()[0] > NAvax::zero());
    }

    #[test]
    fn partial_discrete_payload_and_unknown_delivery_mode_are_rejected() {
        let incomplete = signed_proof(&[2], DeliveryMode::CompletePayload as u8, PARTIAL_BYTES);
        assert_eq!(
            verify_fresh(&incomplete),
            Err(ProofError::IncompleteDiscretePayload)
        );

        let unknown_mode = signed_proof(&[2], 2, AUTHORIZED_BYTES);
        assert_eq!(
            verify_fresh(&unknown_mode),
            Err(ProofError::InvalidDeliveryMode)
        );
    }

    #[test]
    fn missing_recipient_acknowledgement_is_rejected() {
        let mut proof = valid_proof(&[2]);
        proof.acknowledgement = None;

        assert_eq!(
            verify_fresh(&proof),
            Err(ProofError::MissingAcknowledgement)
        );
    }

    #[test]
    fn invalid_sender_relayer_and_recipient_signatures_are_rejected() {
        let domain = proof_domain(CHAIN_ID, settlement_contract());
        let wrong_signer = signer(9);

        let mut sender_proof = valid_proof(&[2]);
        let sender_hash = authorization_hash(sender_proof.authorization.message(), &domain);
        sender_proof.authorization.signature = sign(&wrong_signer, sender_hash);
        assert_eq!(
            verify_fresh(&sender_proof),
            Err(ProofError::InvalidSenderSignature)
        );

        let mut relay_proof = valid_proof(&[2]);
        let relay_hash = contribution_hash(relay_proof.contributions[0].message(), &domain);
        relay_proof.contributions[0].signature = sign(&wrong_signer, relay_hash);
        assert_eq!(
            verify_fresh(&relay_proof),
            Err(ProofError::InvalidRelayerSignature { hop: 0 })
        );

        let mut recipient_proof = valid_proof(&[2]);
        let acknowledgement_hash = acknowledgement_hash(
            recipient_proof
                .acknowledgement
                .as_ref()
                .expect("fixture has acknowledgement")
                .message(),
            &domain,
        );
        recipient_proof
            .acknowledgement
            .as_mut()
            .expect("fixture has acknowledgement")
            .signature = sign(&wrong_signer, acknowledgement_hash);
        assert_eq!(
            verify_fresh(&recipient_proof),
            Err(ProofError::InvalidRecipientSignature)
        );
    }

    #[test]
    fn changed_payload_bytes_are_rejected() {
        let proof = valid_proof(&[2]);

        assert_eq!(
            verify_with_state(
                &proof,
                NOW,
                payload_commitment(b"different encrypted payload"),
                &HashSet::new(),
                &HashSet::new(),
            ),
            Err(ProofError::AlteredPayload)
        );
    }

    #[test]
    fn expired_proof_is_rejected() {
        let proof = valid_proof(&[2]);

        assert_eq!(
            verify_with_state(
                &proof,
                ISSUED_AT + DEFAULT_AUTHORIZATION_SECONDS + 1,
                payload_commitment(PAYLOAD),
                &HashSet::new(),
                &HashSet::new(),
            ),
            Err(ProofError::Expired)
        );
    }

    #[test]
    fn replayed_route_and_duplicate_contribution_submission_are_rejected() {
        let proof = valid_proof(&[2]);
        let domain = proof_domain(CHAIN_ID, settlement_contract());
        let route_id = authorization_hash(proof.authorization.message(), &domain);
        let contribution_id = contribution_hash(proof.contributions[0].message(), &domain);

        assert_eq!(
            verify_with_state(
                &proof,
                NOW,
                payload_commitment(PAYLOAD),
                &HashSet::from([route_id]),
                &HashSet::new(),
            ),
            Err(ProofError::RouteAlreadyConsumed)
        );
        assert_eq!(
            verify_with_state(
                &proof,
                NOW,
                payload_commitment(PAYLOAD),
                &HashSet::new(),
                &HashSet::from([contribution_id]),
            ),
            Err(ProofError::ContributionAlreadyConsumed { hop: 0 })
        );
    }

    #[test]
    fn same_wallet_in_multiple_route_roles_is_rejected() {
        let mut sender_relay = valid_proof(&[2]);
        sender_relay.authorization.relayers[0] = signer(SENDER_KEY).address();
        sender_relay.authorization.message.relayRouteHash =
            relay_route_hash(sender_relay.authorization.relayers())
                .expect("one-hop route must hash");
        assert_eq!(verify_fresh(&sender_relay), Err(ProofError::CommonControl));

        let mut sender_recipient = valid_proof(&[2]);
        sender_recipient.authorization.message.recipient = signer(SENDER_KEY).address();
        assert_eq!(
            verify_fresh(&sender_recipient),
            Err(ProofError::CommonControl)
        );
    }

    #[test]
    fn altered_route_and_reward_terms_are_rejected() {
        let mut route_proof = valid_proof(&[2, 3]);
        route_proof.authorization.relayers.swap(0, 1);
        assert_eq!(
            verify_fresh(&route_proof),
            Err(ProofError::RouteHashMismatch)
        );

        let mut reward_proof = valid_proof(&[2]);
        reward_proof.authorization.message.maximumChargeNavax += 1;
        assert_eq!(
            verify_fresh(&reward_proof),
            Err(ProofError::RewardTermsMismatch)
        );
    }

    #[test]
    fn contribution_route_payload_bytes_and_time_are_bound() {
        let mut route_proof = valid_proof(&[2]);
        route_proof.contributions[0].message.ingress = signer(7).address();
        assert_eq!(
            verify_fresh(&route_proof),
            Err(ProofError::ContributionRouteMismatch { hop: 0 })
        );

        let mut payload_proof = valid_proof(&[2]);
        payload_proof.contributions[0].message.payloadCommitment = B256::repeat_byte(0x55);
        assert_eq!(
            verify_fresh(&payload_proof),
            Err(ProofError::ContributionEvidenceMismatch { hop: 0 })
        );

        let mut bytes_proof = valid_proof(&[2]);
        bytes_proof.contributions[0].message.deliveredBytes = AUTHORIZED_BYTES + 1;
        assert_eq!(
            verify_fresh(&bytes_proof),
            Err(ProofError::ContributionEvidenceMismatch { hop: 0 })
        );

        let mut time_proof = valid_proof(&[2]);
        time_proof.contributions[0].message.forwardedAt = NOW + 1;
        assert_eq!(
            verify_fresh(&time_proof),
            Err(ProofError::InvalidContributionTime { hop: 0 })
        );
    }

    #[test]
    fn acknowledgement_evidence_and_time_are_bound() {
        let mut evidence_proof = valid_proof(&[2]);
        evidence_proof
            .acknowledgement
            .as_mut()
            .expect("fixture has acknowledgement")
            .message
            .contributionsHash = B256::repeat_byte(0x77);
        assert_eq!(
            verify_fresh(&evidence_proof),
            Err(ProofError::AcknowledgementMismatch)
        );

        let mut time_proof = valid_proof(&[2]);
        time_proof
            .acknowledgement
            .as_mut()
            .expect("fixture has acknowledgement")
            .message
            .receivedAt = NOW + 1;
        assert_eq!(
            verify_fresh(&time_proof),
            Err(ProofError::InvalidAcknowledgementTime)
        );
    }

    #[test]
    fn signatures_cannot_cross_chain_or_settlement_contract_domains() {
        let proof = valid_proof(&[2]);
        let empty = HashSet::new();

        let wrong_chain = verify(
            &proof,
            &VerificationContext {
                chain_id: CHAIN_ID + 1,
                settlement_contract: settlement_contract(),
                now: NOW,
                expected_payload_commitment: payload_commitment(PAYLOAD),
                consumed_routes: &empty,
                consumed_contributions: &empty,
            },
        );
        assert_eq!(wrong_chain, Err(ProofError::InvalidSenderSignature));

        let wrong_contract = verify(
            &proof,
            &VerificationContext {
                chain_id: CHAIN_ID,
                settlement_contract: Address::repeat_byte(0x98),
                now: NOW,
                expected_payload_commitment: payload_commitment(PAYLOAD),
                consumed_routes: &empty,
                consumed_contributions: &empty,
            },
        );
        assert_eq!(wrong_contract, Err(ProofError::InvalidSenderSignature));
    }

    #[test]
    fn invalid_authorization_windows_and_future_issue_time_are_rejected() {
        let mut zero_nonce = valid_proof(&[2]);
        zero_nonce.authorization.message.routeNonce = B256::ZERO;
        assert_eq!(
            verify_fresh(&zero_nonce),
            Err(ProofError::InvalidRouteNonce)
        );

        let mut short_window = valid_proof(&[2]);
        short_window.authorization.message.expiresAt = ISSUED_AT + MIN_AUTHORIZATION_SECONDS - 1;
        assert_eq!(
            verify_fresh(&short_window),
            Err(ProofError::InvalidAuthorizationWindow)
        );

        let mut future = valid_proof(&[2]);
        future.authorization.message.issuedAt = NOW + 1;
        future.authorization.message.expiresAt = NOW + 1 + DEFAULT_AUTHORIZATION_SECONDS;
        assert_eq!(verify_fresh(&future), Err(ProofError::FutureAuthorization));
    }

    #[test]
    fn route_length_contribution_count_and_zero_participants_are_rejected() {
        let mut route_length = valid_proof(&[2]);
        route_length.authorization.message.relayCount = 2;
        assert_eq!(
            verify_fresh(&route_length),
            Err(ProofError::RouteLengthMismatch)
        );

        let mut contribution_count = valid_proof(&[2, 3]);
        contribution_count.contributions = contribution_count.contributions[..1]
            .to_vec()
            .into_boxed_slice();
        assert_eq!(
            verify_fresh(&contribution_count),
            Err(ProofError::ContributionCountMismatch)
        );

        let mut zero_relayer = valid_proof(&[2]);
        zero_relayer.authorization.relayers[0] = Address::ZERO;
        zero_relayer.authorization.message.relayRouteHash =
            relay_route_hash(zero_relayer.authorization.relayers())
                .expect("one-hop route must hash");
        assert_eq!(
            verify_fresh(&zero_relayer),
            Err(ProofError::ZeroParticipant)
        );
    }
}
