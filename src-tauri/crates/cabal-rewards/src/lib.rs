//! Deterministic relay and gateway reward economics.
//!
//! This crate quotes and settles sender-funded work in integer nAVAX. It does
//! not verify route proofs, own an escrow, submit transactions, or perform
//! I/O. Proof eligibility belongs to the protocol boundary; the eventual
//! contract and app use these same arithmetic rules after eligibility is
//! established.

#![forbid(unsafe_code)]

use std::fmt;
use std::num::{NonZeroU64, NonZeroU8};

/// Version bound into reward authorizations and relay proofs.
pub const POLICY_VERSION: &str = "cabal-rewards-v1";

/// One AVAX expressed in nAVAX.
pub const NAVAX_PER_AVAX: u64 = 1_000_000_000;

/// The v1 billing quantum: retransmission and fragmentation cannot create
/// additional billable bytes inside one acknowledged logical quantum.
pub const BILLING_QUANTUM_BYTES: u64 = 64 * 1024;

/// Maximum logical bytes in one authorization. Longer gateway sessions must
/// open another independently funded window.
pub const MAX_BILLABLE_BYTES: u64 = 1024 * 1024 * 1024;

/// Base work rate for relay and gateway traffic.
pub const RATE_NAVAX_PER_KIB: u64 = 25;

/// Minimum base reward for one fully acknowledged route.
pub const MIN_BASE_ROUTE_REWARD_NAVAX: u64 = 100_000;

/// Maximum base reward before verified module bonuses.
pub const MAX_BASE_ROUTE_REWARD_NAVAX: u64 = 15_000_000;

/// Maximum verified additive module bonus: +100%.
pub const MAX_BONUS_BPS: u16 = 10_000;

/// Maximum work payout, including all eligible relay bonuses.
pub const MAX_ROUTE_WORK_NAVAX: u64 = 30_000_000;

/// Sender-funded cap for reimbursing the successful proof submitter.
pub const SETTLEMENT_GAS_CAP_NAVAX: u64 = 2_000_000;

/// Maximum paid relays in one route.
pub const MAX_RELAY_COUNT: u8 = 3;

/// Shortest sender-authorized proof window.
pub const MIN_AUTHORIZATION_SECONDS: u64 = 2 * 60;

/// Default sender-authorized proof window.
pub const DEFAULT_AUTHORIZATION_SECONDS: u64 = 10 * 60;

/// Longest sender-authorized proof window.
pub const MAX_AUTHORIZATION_SECONDS: u64 = 30 * 60;

const _: () = {
    assert!(MIN_AUTHORIZATION_SECONDS < DEFAULT_AUTHORIZATION_SECONDS);
    assert!(DEFAULT_AUTHORIZATION_SECONDS < MAX_AUTHORIZATION_SECONDS);
};

/// An exact amount in nano-AVAX (10^-9 AVAX).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NAvax(u64);

impl NAvax {
    /// Constructs an amount from its nAVAX representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the integer nAVAX representation.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Zero nAVAX.
    #[must_use]
    pub const fn zero() -> Self {
        Self(0)
    }

    /// Formats an AVAX decimal without floating point or trailing zeroes.
    #[must_use]
    pub fn to_avax_string(self) -> String {
        let whole = self.0 / NAVAX_PER_AVAX;
        let fraction = self.0 % NAVAX_PER_AVAX;
        if fraction == 0 {
            return whole.to_string();
        }
        let fraction = format!("{fraction:09}");
        format!("{whole}.{}", fraction.trim_end_matches('0'))
    }

    fn checked_add(self, other: Self) -> Result<Self, RewardError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(RewardError::ArithmeticOverflow)
    }

    fn checked_sub(self, other: Self) -> Result<Self, RewardError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(RewardError::ArithmeticOverflow)
    }
}

impl fmt::Display for NAvax {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_avax_string())
    }
}

/// Non-zero logical bytes authorized by the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillableBytes(NonZeroU64);

impl BillableBytes {
    /// Exact logical payload bytes before billing-quantum rounding.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl TryFrom<u64> for BillableBytes {
    type Error = RewardError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let bytes = NonZeroU64::new(value).ok_or(RewardError::ZeroBillableBytes)?;
        if bytes.get() > MAX_BILLABLE_BYTES {
            return Err(RewardError::BillableBytesTooLarge {
                provided: value,
                maximum: MAX_BILLABLE_BYTES,
            });
        }
        Ok(Self(bytes))
    }
}

/// A validated count of paid relay contributions in one route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayCount(NonZeroU8);

impl RelayCount {
    /// Number of paid relay contributions.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0.get()
    }
}

impl TryFrom<u8> for RelayCount {
    type Error = RewardError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let count = NonZeroU8::new(value).ok_or(RewardError::InvalidRelayCount {
            provided: value,
            maximum: MAX_RELAY_COUNT,
        })?;
        if count.get() > MAX_RELAY_COUNT {
            return Err(RewardError::InvalidRelayCount {
                provided: value,
                maximum: MAX_RELAY_COUNT,
            });
        }
        Ok(Self(count))
    }
}

/// A verified additive module bonus in basis points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BonusBps(u16);

impl BonusBps {
    /// Additive basis points, where 10,000 means +100%.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for BonusBps {
    type Error = RewardError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value > MAX_BONUS_BPS {
            return Err(RewardError::BonusTooLarge {
                provided: value,
                maximum: MAX_BONUS_BPS,
            });
        }
        Ok(Self(value))
    }
}

/// A deterministic maximum-charge quote that the sender can authorize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardQuote {
    billable_bytes: BillableBytes,
    billed_bytes: u64,
    relay_count: RelayCount,
    base_route_reward: NAvax,
    maximum_work: NAvax,
    settlement_gas_cap: NAvax,
    maximum_charge: NAvax,
}

impl RewardQuote {
    /// Logical bytes signed by the sender.
    #[must_use]
    pub const fn billable_bytes(&self) -> BillableBytes {
        self.billable_bytes
    }

    /// Bytes after rounding up to the 64-KiB billing quantum.
    #[must_use]
    pub const fn billed_bytes(&self) -> u64 {
        self.billed_bytes
    }

    /// Number of eligible paid relays the sender authorized.
    #[must_use]
    pub const fn relay_count(&self) -> RelayCount {
        self.relay_count
    }

    /// Base route reward before equal division and verified bonuses.
    #[must_use]
    pub const fn base_route_reward(&self) -> NAvax {
        self.base_route_reward
    }

    /// Maximum combined relay payout, including bonus headroom.
    #[must_use]
    pub const fn maximum_work(&self) -> NAvax {
        self.maximum_work
    }

    /// Maximum reimbursable settlement gas.
    #[must_use]
    pub const fn settlement_gas_cap(&self) -> NAvax {
        self.settlement_gas_cap
    }

    /// Exact maximum escrow debit the sender authorizes.
    #[must_use]
    pub const fn maximum_charge(&self) -> NAvax {
        self.maximum_charge
    }
}

/// Atomic accounting result after a valid, complete route proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardSettlement {
    relay_payouts: Box<[NAvax]>,
    executor_reimbursement: NAvax,
    uncovered_executor_gas: NAvax,
    sender_refund: NAvax,
}

impl RewardSettlement {
    /// One payout for each eligible relay, in signed route order.
    #[must_use]
    pub fn relay_payouts(&self) -> &[NAvax] {
        &self.relay_payouts
    }

    /// Gas amount reimbursed from the sender-authorized reserve.
    #[must_use]
    pub const fn executor_reimbursement(&self) -> NAvax {
        self.executor_reimbursement
    }

    /// Gas above the reserve, borne by an executor that still submits.
    #[must_use]
    pub const fn uncovered_executor_gas(&self) -> NAvax {
        self.uncovered_executor_gas
    }

    /// Every unused nAVAX returned to the sender's pull-payment balance.
    #[must_use]
    pub const fn sender_refund(&self) -> NAvax {
        self.sender_refund
    }
}

/// Why a reward quote or settlement was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RewardError {
    #[error("billable bytes must be greater than zero")]
    ZeroBillableBytes,
    #[error("billable bytes {provided} exceed the {maximum}-byte authorization maximum")]
    BillableBytesTooLarge { provided: u64, maximum: u64 },
    #[error("relay count {provided} is outside 1..={maximum}")]
    InvalidRelayCount { provided: u8, maximum: u8 },
    #[error("bonus {provided} bps exceeds the {maximum} bps maximum")]
    BonusTooLarge { provided: u16, maximum: u16 },
    #[error("received {provided} bonuses for {expected} relays")]
    BonusCountMismatch { provided: usize, expected: u8 },
    #[error("delivered bytes {delivered} exceed the {authorized}-byte authorization")]
    DeliveredBytesExceedAuthorization { delivered: u64, authorized: u64 },
    #[error("available balance {available} AVAX is below required {required} AVAX")]
    InsufficientBalance { available: NAvax, required: NAvax },
    #[error("reward arithmetic overflowed")]
    ArithmeticOverflow,
}

/// Quotes v1 relay or gateway work without consulting a wallet or network.
///
/// # Errors
///
/// Returns [`RewardError::ArithmeticOverflow`] if integer arithmetic cannot
/// represent an intermediate value.
pub fn quote(
    billable_bytes: BillableBytes,
    relay_count: RelayCount,
) -> Result<RewardQuote, RewardError> {
    let (billed_bytes, base_route_reward) = base_reward_for(billable_bytes)?;
    let maximum_work = NAvax::from_raw(
        base_route_reward
            .raw()
            .checked_mul(2)
            .ok_or(RewardError::ArithmeticOverflow)?
            .min(MAX_ROUTE_WORK_NAVAX),
    );
    let settlement_gas_cap = NAvax::from_raw(SETTLEMENT_GAS_CAP_NAVAX);
    let maximum_charge = maximum_work.checked_add(settlement_gas_cap)?;

    Ok(RewardQuote {
        billable_bytes,
        billed_bytes,
        relay_count,
        base_route_reward,
        maximum_work,
        settlement_gas_cap,
        maximum_charge,
    })
}

fn base_reward_for(billable_bytes: BillableBytes) -> Result<(u64, NAvax), RewardError> {
    let quanta = billable_bytes.get().div_ceil(BILLING_QUANTUM_BYTES);
    let billed_bytes = quanta
        .checked_mul(BILLING_QUANTUM_BYTES)
        .ok_or(RewardError::ArithmeticOverflow)?;
    let billed_kib = billed_bytes / 1024;
    let raw_base = billed_kib
        .checked_mul(RATE_NAVAX_PER_KIB)
        .ok_or(RewardError::ArithmeticOverflow)?;
    let base_route_reward =
        NAvax::from_raw(raw_base.clamp(MIN_BASE_ROUTE_REWARD_NAVAX, MAX_BASE_ROUTE_REWARD_NAVAX));
    Ok((billed_bytes, base_route_reward))
}

/// Ensures a wallet can fund the exact maximum before paid broadcast.
///
/// # Errors
///
/// Returns [`RewardError::InsufficientBalance`] without changing state when
/// the available balance is below the quote's maximum charge.
pub fn ensure_fundable(quote: &RewardQuote, available: NAvax) -> Result<(), RewardError> {
    if available < quote.maximum_charge {
        return Err(RewardError::InsufficientBalance {
            available,
            required: quote.maximum_charge,
        });
    }
    Ok(())
}

/// Settles a complete, eligible route after proof verification.
///
/// Base reward is divided equally with integer division. Each verified bonus
/// is applied to that share and rounded down; every remainder returns to the
/// sender. Gas reimbursement is capped, and an executor that submits above
/// the cap bears the exact excess.
///
/// # Errors
///
/// Returns [`RewardError::BonusCountMismatch`] unless there is exactly one
/// verified bonus for every eligible relay,
/// [`RewardError::DeliveredBytesExceedAuthorization`] if a receipt claims more
/// than the sender funded, or
/// [`RewardError::ArithmeticOverflow`] if checked accounting fails.
pub fn settle_complete_route(
    quote: &RewardQuote,
    delivered_bytes: BillableBytes,
    bonuses: &[BonusBps],
    measured_executor_gas: NAvax,
) -> Result<RewardSettlement, RewardError> {
    if delivered_bytes.get() > quote.billable_bytes.get() {
        return Err(RewardError::DeliveredBytesExceedAuthorization {
            delivered: delivered_bytes.get(),
            authorized: quote.billable_bytes.get(),
        });
    }
    if bonuses.len() != usize::from(quote.relay_count.get()) {
        return Err(RewardError::BonusCountMismatch {
            provided: bonuses.len(),
            expected: quote.relay_count.get(),
        });
    }

    let (_, delivered_base_reward) = base_reward_for(delivered_bytes)?;
    let base_share = delivered_base_reward.raw() / u64::from(quote.relay_count.get());
    let relay_payouts = bonuses
        .iter()
        .map(|bonus| {
            let multiplier = 10_000_u64 + u64::from(bonus.get());
            base_share
                .checked_mul(multiplier)
                .and_then(|scaled| scaled.checked_div(10_000))
                .map(NAvax::from_raw)
                .ok_or(RewardError::ArithmeticOverflow)
        })
        .collect::<Result<Box<[_]>, _>>()?;

    let paid_work = relay_payouts
        .iter()
        .try_fold(NAvax::zero(), |total, payout| total.checked_add(*payout))?;
    if paid_work > quote.maximum_work {
        return Err(RewardError::ArithmeticOverflow);
    }

    let executor_reimbursement = NAvax::from_raw(
        measured_executor_gas
            .raw()
            .min(quote.settlement_gas_cap.raw()),
    );
    let uncovered_executor_gas = measured_executor_gas.checked_sub(executor_reimbursement)?;
    let spent = paid_work.checked_add(executor_reimbursement)?;
    let sender_refund = quote.maximum_charge.checked_sub(spent)?;

    Ok(RewardSettlement {
        relay_payouts,
        executor_reimbursement,
        uncovered_executor_gas,
        sender_refund,
    })
}

/// Refunds an unfulfilled, failed, partial, or expired route in full.
///
/// The expiry caller pays its own transaction gas. No relay or executor can
/// claim route funds without a complete proof.
#[must_use]
pub fn refund_unfulfilled(quote: &RewardQuote) -> RewardSettlement {
    RewardSettlement {
        relay_payouts: Box::new([]),
        executor_reimbursement: NAvax::zero(),
        uncovered_executor_gas: NAvax::zero(),
        sender_refund: quote.maximum_charge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn bytes(value: u64) -> BillableBytes {
        BillableBytes::try_from(value).unwrap()
    }

    fn relays(value: u8) -> RelayCount {
        RelayCount::try_from(value).unwrap()
    }

    fn bonus(value: u16) -> BonusBps {
        BonusBps::try_from(value).unwrap()
    }

    #[test]
    fn four_kib_intent_uses_the_minimum_and_has_a_bounded_maximum_charge() {
        let quote = quote(bytes(4 * 1024), relays(1)).unwrap();

        assert_eq!(quote.billed_bytes(), 64 * 1024);
        assert_eq!(quote.base_route_reward(), NAvax::from_raw(100_000));
        assert_eq!(quote.maximum_work(), NAvax::from_raw(200_000));
        assert_eq!(quote.settlement_gas_cap(), NAvax::from_raw(2_000_000));
        assert_eq!(quote.maximum_charge(), NAvax::from_raw(2_200_000));
        assert_eq!(quote.maximum_charge().to_avax_string(), "0.0022");
    }

    #[test]
    fn one_gib_authorization_cannot_exceed_the_absolute_route_cap() {
        let quote = quote(bytes(MAX_BILLABLE_BYTES), relays(3)).unwrap();

        assert_eq!(
            quote.base_route_reward(),
            NAvax::from_raw(MAX_BASE_ROUTE_REWARD_NAVAX)
        );
        assert_eq!(quote.maximum_work(), NAvax::from_raw(MAX_ROUTE_WORK_NAVAX));
        assert_eq!(quote.maximum_charge(), NAvax::from_raw(32_000_000));
        assert_eq!(quote.maximum_charge().to_avax_string(), "0.032");
    }

    #[test]
    fn authorization_windows_are_bounded_and_ordered() {
        assert_eq!(MIN_AUTHORIZATION_SECONDS, 120);
        assert_eq!(DEFAULT_AUTHORIZATION_SECONDS, 600);
        assert_eq!(MAX_AUTHORIZATION_SECONDS, 1_800);
    }

    #[test]
    fn navax_formatting_is_exact_and_trims_only_fractional_zeroes() {
        assert_eq!(NAvax::zero().to_avax_string(), "0");
        assert_eq!(NAvax::from_raw(1).to_avax_string(), "0.000000001");
        assert_eq!(NAvax::from_raw(12_445_696).to_avax_string(), "0.012445696");
        assert_eq!(NAvax::from_raw(2_000_000_000).to_avax_string(), "2");
    }

    #[test]
    fn four_hundred_twelve_mib_matches_the_home_screen_economic_example() {
        let quote = quote(bytes(412 * 1024 * 1024), relays(1)).unwrap();
        let settlement = settle_complete_route(
            &quote,
            bytes(412 * 1024 * 1024),
            &[bonus(1_800)],
            NAvax::from_raw(1_250_000),
        )
        .unwrap();

        assert_eq!(quote.base_route_reward(), NAvax::from_raw(10_547_200));
        assert_eq!(quote.maximum_charge(), NAvax::from_raw(23_094_400));
        assert_eq!(settlement.relay_payouts(), &[NAvax::from_raw(12_445_696)]);
        assert_eq!(
            settlement.executor_reimbursement(),
            NAvax::from_raw(1_250_000)
        );
        assert_eq!(settlement.sender_refund(), NAvax::from_raw(9_398_704));
    }

    #[test]
    fn executor_gas_above_the_cap_never_increases_sender_spend() {
        let quote = quote(bytes(4 * 1024), relays(1)).unwrap();
        let settlement = settle_complete_route(
            &quote,
            bytes(4 * 1024),
            &[bonus(0)],
            NAvax::from_raw(3_000_000),
        )
        .unwrap();

        assert_eq!(
            settlement.executor_reimbursement(),
            NAvax::from_raw(2_000_000)
        );
        assert_eq!(
            settlement.uncovered_executor_gas(),
            NAvax::from_raw(1_000_000)
        );
        assert_eq!(settlement.sender_refund(), NAvax::from_raw(100_000));
    }

    #[test]
    fn insufficient_balance_reports_the_exact_maximum_without_authorizing() {
        let quote = quote(bytes(4 * 1024), relays(1)).unwrap();

        assert_eq!(
            ensure_fundable(&quote, NAvax::from_raw(2_199_999)),
            Err(RewardError::InsufficientBalance {
                available: NAvax::from_raw(2_199_999),
                required: NAvax::from_raw(2_200_000),
            })
        );
        assert_eq!(ensure_fundable(&quote, quote.maximum_charge()), Ok(()));
    }

    #[test]
    fn partial_failed_and_expired_routes_refund_every_authorized_navax() {
        let quote = quote(bytes(412 * 1024 * 1024), relays(2)).unwrap();
        let settlement = refund_unfulfilled(&quote);

        assert!(settlement.relay_payouts().is_empty());
        assert_eq!(settlement.executor_reimbursement(), NAvax::zero());
        assert_eq!(settlement.sender_refund(), quote.maximum_charge());
    }

    #[test]
    fn three_relay_division_rounds_down_and_returns_every_remainder() {
        let quote = quote(bytes(4 * 1024), relays(3)).unwrap();
        let settlement = settle_complete_route(
            &quote,
            bytes(4 * 1024),
            &[bonus(0), bonus(0), bonus(0)],
            NAvax::zero(),
        )
        .unwrap();

        assert_eq!(
            settlement.relay_payouts(),
            &[
                NAvax::from_raw(33_333),
                NAvax::from_raw(33_333),
                NAvax::from_raw(33_333)
            ]
        );
        assert_eq!(settlement.sender_refund(), NAvax::from_raw(2_100_001));
    }

    #[test]
    fn invalid_counts_and_unbounded_bonuses_are_rejected_at_construction() {
        assert_eq!(
            BillableBytes::try_from(0),
            Err(RewardError::ZeroBillableBytes)
        );
        assert_eq!(
            BillableBytes::try_from(MAX_BILLABLE_BYTES + 1),
            Err(RewardError::BillableBytesTooLarge {
                provided: MAX_BILLABLE_BYTES + 1,
                maximum: MAX_BILLABLE_BYTES
            })
        );
        assert_eq!(
            RelayCount::try_from(0),
            Err(RewardError::InvalidRelayCount {
                provided: 0,
                maximum: 3
            })
        );
        assert_eq!(
            RelayCount::try_from(4),
            Err(RewardError::InvalidRelayCount {
                provided: 4,
                maximum: 3
            })
        );
        assert_eq!(
            BonusBps::try_from(10_001),
            Err(RewardError::BonusTooLarge {
                provided: 10_001,
                maximum: 10_000
            })
        );
    }

    #[test]
    fn settlement_requires_one_verified_bonus_value_per_relay() {
        let quote = quote(bytes(4 * 1024), relays(2)).unwrap();

        assert_eq!(
            settle_complete_route(&quote, bytes(4 * 1024), &[bonus(0)], NAvax::zero()),
            Err(RewardError::BonusCountMismatch {
                provided: 1,
                expected: 2
            })
        );
    }

    #[test]
    fn acknowledged_gateway_bytes_cannot_exceed_the_sender_authorization() {
        let quote = quote(bytes(64 * 1024), relays(1)).unwrap();

        assert_eq!(
            settle_complete_route(&quote, bytes(128 * 1024), &[bonus(0)], NAvax::zero()),
            Err(RewardError::DeliveredBytesExceedAuthorization {
                delivered: 128 * 1024,
                authorized: 64 * 1024
            })
        );
    }

    #[test]
    fn unused_gateway_authorization_is_returned_to_the_sender() {
        let quote = quote(bytes(412 * 1024 * 1024), relays(1)).unwrap();
        let settlement =
            settle_complete_route(&quote, bytes(64 * 1024 * 1024), &[bonus(0)], NAvax::zero())
                .unwrap();

        assert_eq!(settlement.relay_payouts(), &[NAvax::from_raw(1_638_400)]);
        assert_eq!(settlement.sender_refund(), NAvax::from_raw(21_456_000));
    }

    proptest! {
        #[test]
        fn every_successful_settlement_conserves_the_authorized_maximum(
            logical_bytes in 1_u64..=MAX_BILLABLE_BYTES,
            delivered_seed in any::<u64>(),
            relay_count in 1_u8..=MAX_RELAY_COUNT,
            bonus_values in prop::collection::vec(0_u16..=MAX_BONUS_BPS, 3),
            gas in any::<u64>(),
        ) {
            let quote = quote(bytes(logical_bytes), relays(relay_count)).unwrap();
            let delivered = 1 + delivered_seed % logical_bytes;
            let bonuses = bonus_values[..usize::from(relay_count)]
                .iter()
                .copied()
                .map(bonus)
                .collect::<Vec<_>>();
            let settlement = settle_complete_route(
                &quote,
                bytes(delivered),
                &bonuses,
                NAvax::from_raw(gas),
            )
            .unwrap();
            let paid = settlement
                .relay_payouts()
                .iter()
                .map(|amount| amount.raw())
                .sum::<u64>();
            let accounted = paid
                .checked_add(settlement.executor_reimbursement().raw())
                .and_then(|subtotal| subtotal.checked_add(settlement.sender_refund().raw()))
                .unwrap();

            prop_assert_eq!(accounted, quote.maximum_charge().raw());
        }
    }
}
