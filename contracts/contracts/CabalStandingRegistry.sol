// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {AccessControlDefaultAdminRules} from
    "@openzeppelin/contracts/access/extensions/AccessControlDefaultAdminRules.sol";

/// @notice Canonical public standing for marketplace seller wallets.
///
/// Standing is the lifetime net number of completed settlements that remain
/// active. An authorized settlement source credits a seller only after final
/// completion. That source, or an independent corrector, removes the credit
/// when the settlement is reversed or refunded. The event history remains
/// available so buyers do not have to trust the seller's local database.
contract CabalStandingRegistry is AccessControlDefaultAdminRules {
    uint48 public constant ADMIN_TRANSFER_DELAY = 2 days;
    bytes32 public constant SOURCE_ROLE = keccak256("CABAL_STANDING_SOURCE_ROLE");
    bytes32 public constant CORRECTOR_ROLE = keccak256("CABAL_STANDING_CORRECTOR_ROLE");

    struct SettlementRecord {
        address source;
        address seller;
        bytes32 sourceSettlementId;
        bytes32 evidenceHash;
        uint256 recordedAtBlock;
        uint256 reversedAtBlock;
        bytes32 reversalReasonHash;
        bool active;
    }

    error ZeroSource();
    error ZeroCorrector();
    error ZeroSeller();
    error InvalidSourceSettlementId();
    error InvalidEvidenceHash();
    error InvalidReversalReason();
    error DuplicateSettlement(bytes32 recordId);
    error UnknownSettlement(bytes32 recordId);
    error SettlementAlreadyReversed(bytes32 recordId);
    error UnauthorizedReversal(address account, bytes32 recordId);
    error StandingOverflow(address seller);
    error StandingInvariantViolation(address seller);

    mapping(bytes32 recordId => SettlementRecord record) private _records;
    mapping(address seller => uint64 count) private _activeSettlements;
    mapping(address seller => uint256 blockNumber) public lastChangedBlock;

    event StandingCredited(
        bytes32 indexed recordId,
        address indexed source,
        address indexed seller,
        bytes32 sourceSettlementId,
        bytes32 evidenceHash,
        uint64 newStanding
    );
    event StandingReversed(
        bytes32 indexed recordId,
        address indexed seller,
        address indexed reversedBy,
        bytes32 reasonHash,
        uint64 newStanding
    );

    constructor(address initialAdmin, address initialSource, address initialCorrector)
        AccessControlDefaultAdminRules(ADMIN_TRANSFER_DELAY, initialAdmin)
    {
        if (initialSource == address(0)) revert ZeroSource();
        if (initialCorrector == address(0)) revert ZeroCorrector();
        _grantRole(SOURCE_ROLE, initialSource);
        _grantRole(CORRECTOR_ROLE, initialCorrector);
    }

    /// @notice Credits one finally completed settlement exactly once.
    /// @param sourceSettlementId Identifier assigned by the authorized source.
    /// It is namespaced by `msg.sender`, so independent sources cannot collide.
    /// @param seller Marketplace seller wallet credited by that settlement.
    /// @param evidenceHash Commitment to private/off-chain settlement evidence;
    /// it must not contain the payload, amount, peer identity, or other PII.
    function recordSettlement(bytes32 sourceSettlementId, address seller, bytes32 evidenceHash)
        external
        onlyRole(SOURCE_ROLE)
        returns (bytes32 recordId)
    {
        if (seller == address(0)) revert ZeroSeller();
        if (sourceSettlementId == bytes32(0)) revert InvalidSourceSettlementId();
        if (evidenceHash == bytes32(0)) revert InvalidEvidenceHash();

        recordId = recordIdFor(msg.sender, sourceSettlementId);
        if (_records[recordId].source != address(0)) revert DuplicateSettlement(recordId);

        uint64 current = _activeSettlements[seller];
        if (current == type(uint64).max) revert StandingOverflow(seller);
        uint64 next = current + 1;

        _records[recordId] = SettlementRecord({
            source: msg.sender,
            seller: seller,
            sourceSettlementId: sourceSettlementId,
            evidenceHash: evidenceHash,
            recordedAtBlock: block.number,
            reversedAtBlock: 0,
            reversalReasonHash: bytes32(0),
            active: true
        });
        _activeSettlements[seller] = next;
        lastChangedBlock[seller] = block.number;

        emit StandingCredited(
            recordId, msg.sender, seller, sourceSettlementId, evidenceHash, next
        );
    }

    /// @notice Removes a previously credited settlement after reversal/refund.
    /// The active source may reverse its own record. A CORRECTOR_ROLE may do so
    /// even after a compromised source has had its role revoked.
    function reverseSettlement(bytes32 recordId, bytes32 reasonHash) external {
        SettlementRecord storage record = _records[recordId];
        if (record.source == address(0)) revert UnknownSettlement(recordId);
        if (!record.active) revert SettlementAlreadyReversed(recordId);
        if (reasonHash == bytes32(0)) revert InvalidReversalReason();

        bool activeOriginalSource =
            msg.sender == record.source && hasRole(SOURCE_ROLE, msg.sender);
        if (!activeOriginalSource && !hasRole(CORRECTOR_ROLE, msg.sender)) {
            revert UnauthorizedReversal(msg.sender, recordId);
        }

        uint64 current = _activeSettlements[record.seller];
        if (current == 0) revert StandingInvariantViolation(record.seller);
        uint64 next = current - 1;

        record.active = false;
        record.reversedAtBlock = block.number;
        record.reversalReasonHash = reasonHash;
        _activeSettlements[record.seller] = next;
        lastChangedBlock[record.seller] = block.number;

        emit StandingReversed(recordId, record.seller, msg.sender, reasonHash, next);
    }

    /// @notice Current canonical count and its last mutation block.
    function standingOf(address seller) external view returns (uint64 count, uint256 changedAtBlock) {
        return (_activeSettlements[seller], lastChangedBlock[seller]);
    }

    /// @notice Immutable credit data plus append-only reversal data.
    function settlementRecord(bytes32 recordId)
        external
        view
        returns (SettlementRecord memory)
    {
        return _records[recordId];
    }

    /// @notice Source-namespaced public identifier used by events and lookups.
    function recordIdFor(address source, bytes32 sourceSettlementId)
        public
        pure
        returns (bytes32)
    {
        return keccak256(abi.encode(source, sourceSettlementId));
    }
}
