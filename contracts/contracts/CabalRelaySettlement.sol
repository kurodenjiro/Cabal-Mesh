// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @title CabalMesh sender-funded relay settlement
/// @notice Verifies cabal-rewards-v1 three-party evidence and converts one
///         prefunded route into pull-payment credits. There is no governor and
///         no path that can withdraw active escrow.
contract CabalRelaySettlement is EIP712, ReentrancyGuard {
    uint256 public constant WEI_PER_NAVAX = 1_000_000_000;
    uint64 public constant MIN_AUTHORIZATION_SECONDS = 120;
    uint64 public constant MAX_AUTHORIZATION_SECONDS = 1_800;
    uint64 public constant MAX_BILLABLE_BYTES = 1_073_741_824;
    uint8 public constant MAX_RELAY_COUNT = 3;
    uint64 public constant SETTLEMENT_GAS_CAP_NAVAX = 2_000_000;
    uint256 public constant SETTLEMENT_OVERHEAD_GAS = 50_000;
    bytes32 public constant POLICY_HASH = keccak256("cabal-rewards-v1");

    bytes32 private constant AUTHORIZATION_TYPEHASH = keccak256(
        "RelayAuthorization(bytes32 policyHash,bytes32 routeNonce,bytes32 payloadCommitment,uint8 deliveryMode,bytes32 relayRouteHash,address sender,address recipient,uint64 authorizedBytes,uint8 relayCount,uint64 maximumChargeNavax,uint64 issuedAt,uint64 expiresAt)"
    );
    bytes32 private constant CONTRIBUTION_TYPEHASH = keccak256(
        "RelayContribution(bytes32 authorizationHash,uint8 hopIndex,address relayer,address ingress,address egress,bytes32 payloadCommitment,uint64 deliveredBytes,uint64 forwardedAt)"
    );
    bytes32 private constant ACKNOWLEDGEMENT_TYPEHASH = keccak256(
        "RecipientAcknowledgement(bytes32 authorizationHash,bytes32 contributionsHash,address recipient,bytes32 payloadCommitment,uint64 deliveredBytes,uint64 receivedAt)"
    );

    bytes private constant ROUTE_HASH_PREFIX = "CABAL_RELAY_ROUTE_V1\x00";
    bytes private constant CONTRIBUTIONS_HASH_PREFIX = "CABAL_CONTRIBUTIONS_V1\x00";

    enum RouteState {
        None,
        Active,
        Settled,
        Expired
    }

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

    struct Route {
        address sender;
        address recipient;
        bytes32 payloadCommitment;
        uint64 authorizedBytes;
        uint64 maximumChargeNavax;
        uint64 expiresAt;
        RouteState state;
    }

    mapping(bytes32 routeId => Route route) public routes;
    mapping(bytes32 routeId => bool consumed) public consumedRoutes;
    mapping(bytes32 contributionId => bool consumed) public consumedContributions;
    mapping(address account => uint256 amountWei) public withdrawableWei;
    mapping(address relayer => uint256 amountNavax) public settledRelayEarningsNavax;

    uint256 public activeLiabilityWei;
    uint256 public totalCreditsWei;

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
    event RouteExpired(bytes32 indexed routeId, address indexed sender, uint64 refundNavax);
    event CreditWithdrawn(address indexed account, uint256 amountWei);

    constructor() EIP712("CabalMesh Relay Proof", "1") {}

    /// @notice Exact policy quote. This is also recomputed during funding and
    ///         settlement; a caller-provided maximum is never trusted.
    function quote(uint64 authorizedBytes, uint8 relayCount)
        public
        pure
        returns (
            uint64 billedBytes,
            uint64 baseRouteRewardNavax,
            uint64 maximumWorkNavax,
            uint64 settlementGasCapNavax,
            uint64 maximumChargeNavax
        )
    {
        require(authorizedBytes > 0 && authorizedBytes <= MAX_BILLABLE_BYTES, "Invalid bytes");
        require(relayCount > 0 && relayCount <= MAX_RELAY_COUNT, "Invalid relay count");

        uint64 quanta = (authorizedBytes + 65_535) / 65_536;
        billedBytes = quanta * 65_536;
        uint64 calculated = (billedBytes / 1_024) * 25;
        baseRouteRewardNavax = calculated < 100_000 ? 100_000 : calculated;
        if (baseRouteRewardNavax > 15_000_000) baseRouteRewardNavax = 15_000_000;
        maximumWorkNavax = baseRouteRewardNavax * 2;
        if (maximumWorkNavax > 30_000_000) maximumWorkNavax = 30_000_000;
        settlementGasCapNavax = SETTLEMENT_GAS_CAP_NAVAX;
        maximumChargeNavax = maximumWorkNavax + settlementGasCapNavax;
    }

    function authorizationHash(RelayAuthorization calldata authorization)
        public
        view
        returns (bytes32)
    {
        return _hashTypedDataV4(
            keccak256(
                abi.encode(
                    AUTHORIZATION_TYPEHASH,
                    authorization.policyHash,
                    authorization.routeNonce,
                    authorization.payloadCommitment,
                    authorization.deliveryMode,
                    authorization.relayRouteHash,
                    authorization.sender,
                    authorization.recipient,
                    authorization.authorizedBytes,
                    authorization.relayCount,
                    authorization.maximumChargeNavax,
                    authorization.issuedAt,
                    authorization.expiresAt
                )
            )
        );
    }

    function contributionHash(RelayContribution calldata contribution)
        public
        view
        returns (bytes32)
    {
        return _hashTypedDataV4(
            keccak256(
                abi.encode(
                    CONTRIBUTION_TYPEHASH,
                    contribution.authorizationHash,
                    contribution.hopIndex,
                    contribution.relayer,
                    contribution.ingress,
                    contribution.egress,
                    contribution.payloadCommitment,
                    contribution.deliveredBytes,
                    contribution.forwardedAt
                )
            )
        );
    }

    function acknowledgementHash(RecipientAcknowledgement calldata acknowledgement)
        public
        view
        returns (bytes32)
    {
        return _hashTypedDataV4(
            keccak256(
                abi.encode(
                    ACKNOWLEDGEMENT_TYPEHASH,
                    acknowledgement.authorizationHash,
                    acknowledgement.contributionsHash,
                    acknowledgement.recipient,
                    acknowledgement.payloadCommitment,
                    acknowledgement.deliveredBytes,
                    acknowledgement.receivedAt
                )
            )
        );
    }

    function relayRouteHash(address[] calldata relayers) public pure returns (bytes32) {
        require(relayers.length > 0 && relayers.length <= MAX_RELAY_COUNT, "Invalid relay count");
        bytes memory encoded = abi.encodePacked(ROUTE_HASH_PREFIX, bytes1(uint8(relayers.length)));
        for (uint256 index = 0; index < relayers.length; index++) {
            encoded = abi.encodePacked(encoded, relayers[index]);
        }
        return keccak256(encoded);
    }

    function orderedContributionsHash(bytes32[] memory ids) public pure returns (bytes32) {
        require(ids.length > 0 && ids.length <= MAX_RELAY_COUNT, "Invalid contribution count");
        bytes memory encoded = abi.encodePacked(
            CONTRIBUTIONS_HASH_PREFIX,
            bytes1(uint8(ids.length))
        );
        for (uint256 index = 0; index < ids.length; index++) {
            encoded = abi.encodePacked(encoded, ids[index]);
        }
        return keccak256(encoded);
    }

    /// @notice Locks exactly the sender-authorized maximum before any paid
    ///         route may be broadcast. Wallet transaction gas is separate.
    function fundRoute(
        RelayAuthorization calldata authorization,
        address[] calldata relayers,
        bytes calldata senderSignature
    ) external payable returns (bytes32 routeId) {
        routeId = _validateAuthorization(authorization, relayers, senderSignature);
        require(msg.sender == authorization.sender, "Only sender funds");
        require(routes[routeId].state == RouteState.None, "Route already funded");

        uint256 escrowWei = uint256(authorization.maximumChargeNavax) * WEI_PER_NAVAX;
        require(msg.value == escrowWei, "Wrong escrow amount");

        routes[routeId] = Route({
            sender: authorization.sender,
            recipient: authorization.recipient,
            payloadCommitment: authorization.payloadCommitment,
            authorizedBytes: authorization.authorizedBytes,
            maximumChargeNavax: authorization.maximumChargeNavax,
            expiresAt: authorization.expiresAt,
            state: RouteState.Active
        });
        activeLiabilityWei += escrowWei;

        emit RouteFunded(
            routeId,
            authorization.sender,
            authorization.recipient,
            authorization.maximumChargeNavax,
            authorization.expiresAt
        );
    }

    /// @notice Settles only a complete signed bundle. Any failure reverts all
    ///         replay markers and credits, leaving the funded route active.
    function settle(RelayProof calldata proof) external returns (bytes32 routeId) {
        uint256 gasAtEntry = gasleft();
        routeId = _validateAuthorization(
            proof.authorization,
            proof.relayers,
            proof.senderSignature
        );
        Route storage route = routes[routeId];
        require(route.state == RouteState.Active, "Route not active");
        require(!consumedRoutes[routeId], "Route consumed");
        require(block.timestamp <= proof.authorization.expiresAt, "Proof expired");
        require(proof.contributions.length == proof.relayers.length, "Contribution count mismatch");
        require(proof.contributionSignatures.length == proof.relayers.length, "Signature count mismatch");

        (
            bytes32[] memory contributionIds,
            uint64 deliveredBytes,
            uint64 lastForwardedAt
        ) = _validateContributions(
            proof.authorization,
            proof.relayers,
            proof.contributions,
            proof.contributionSignatures
        );
        _validateAcknowledgement(
            proof.authorization,
            contributionIds,
            deliveredBytes,
            lastForwardedAt,
            proof.acknowledgement,
            proof.acknowledgementSignature
        );

        route.state = RouteState.Settled;
        consumedRoutes[routeId] = true;
        for (uint256 index = 0; index < contributionIds.length; index++) {
            consumedContributions[contributionIds[index]] = true;
        }

        uint64 workPaidNavax = _creditRelayWork(routeId, proof.relayers, deliveredBytes);
        uint64 executorPaidNavax = _meteredExecutorNavax(gasAtEntry);
        uint64 senderRefundNavax = proof.authorization.maximumChargeNavax
            - workPaidNavax
            - executorPaidNavax;

        _credit(msg.sender, uint256(executorPaidNavax) * WEI_PER_NAVAX);
        _credit(proof.authorization.sender, uint256(senderRefundNavax) * WEI_PER_NAVAX);
        activeLiabilityWei -= uint256(proof.authorization.maximumChargeNavax) * WEI_PER_NAVAX;

        emit RouteSettled(
            routeId,
            msg.sender,
            deliveredBytes,
            workPaidNavax,
            executorPaidNavax,
            senderRefundNavax
        );
    }

    /// @notice Converts an unfulfilled route into a full sender credit. The
    ///         caller receives no keeper reward and pays this transaction gas.
    function expireRoute(bytes32 routeId) external {
        Route storage route = routes[routeId];
        require(route.state == RouteState.Active, "Route not active");
        require(block.timestamp > route.expiresAt, "Route not expired");

        route.state = RouteState.Expired;
        uint256 refundWei = uint256(route.maximumChargeNavax) * WEI_PER_NAVAX;
        activeLiabilityWei -= refundWei;
        _credit(route.sender, refundWei);

        emit RouteExpired(routeId, route.sender, route.maximumChargeNavax);
    }

    function withdraw() external nonReentrant {
        uint256 amountWei = withdrawableWei[msg.sender];
        require(amountWei > 0, "No credit");
        withdrawableWei[msg.sender] = 0;
        totalCreditsWei -= amountWei;
        (bool sent, ) = msg.sender.call{value: amountWei}("");
        require(sent, "Transfer failed");
        emit CreditWithdrawn(msg.sender, amountWei);
    }

    function solvent() external view returns (bool) {
        return activeLiabilityWei + totalCreditsWei <= address(this).balance;
    }

    function _validateAuthorization(
        RelayAuthorization calldata authorization,
        address[] calldata relayers,
        bytes calldata senderSignature
    ) private view returns (bytes32 routeId) {
        require(authorization.policyHash == POLICY_HASH, "Wrong policy");
        require(authorization.routeNonce != bytes32(0), "Zero nonce");
        require(authorization.sender != address(0) && authorization.recipient != address(0), "Zero participant");
        require(authorization.deliveryMode <= 1, "Invalid delivery mode");
        require(authorization.relayCount == relayers.length, "Route length mismatch");
        require(authorization.relayRouteHash == relayRouteHash(relayers), "Route hash mismatch");

        require(authorization.expiresAt >= authorization.issuedAt, "Invalid authorization window");
        uint64 duration = authorization.expiresAt - authorization.issuedAt;
        require(
            duration >= MIN_AUTHORIZATION_SECONDS && duration <= MAX_AUTHORIZATION_SECONDS,
            "Invalid authorization window"
        );
        require(authorization.issuedAt <= block.timestamp, "Future authorization");
        require(block.timestamp <= authorization.expiresAt, "Authorization expired");
        _validateDistinct(authorization.sender, relayers, authorization.recipient);

        (, , , , uint64 expectedMaximum) = quote(
            authorization.authorizedBytes,
            authorization.relayCount
        );
        require(authorization.maximumChargeNavax == expectedMaximum, "Wrong reward terms");

        routeId = authorizationHash(authorization);
        require(ECDSA.recover(routeId, senderSignature) == authorization.sender, "Invalid sender signature");
    }

    function _validateContributions(
        RelayAuthorization calldata authorization,
        address[] calldata relayers,
        RelayContribution[] calldata contributions,
        bytes[] calldata signatures
    ) private view returns (bytes32[] memory ids, uint64 deliveredBytes, uint64 lastForwardedAt) {
        ids = new bytes32[](contributions.length);
        lastForwardedAt = authorization.issuedAt;
        bytes32 routeId = authorizationHash(authorization);

        for (uint256 index = 0; index < contributions.length; index++) {
            RelayContribution calldata contribution = contributions[index];
            address expectedIngress = index == 0 ? authorization.sender : relayers[index - 1];
            address expectedEgress = index + 1 == relayers.length
                ? authorization.recipient
                : relayers[index + 1];
            require(
                contribution.authorizationHash == routeId
                    && contribution.hopIndex == index
                    && contribution.relayer == relayers[index]
                    && contribution.ingress == expectedIngress
                    && contribution.egress == expectedEgress,
                "Contribution route mismatch"
            );
            require(
                contribution.payloadCommitment == authorization.payloadCommitment
                    && contribution.deliveredBytes > 0
                    && contribution.deliveredBytes <= authorization.authorizedBytes,
                "Contribution evidence mismatch"
            );
            if (index == 0) {
                deliveredBytes = contribution.deliveredBytes;
            } else {
                require(contribution.deliveredBytes == deliveredBytes, "Contribution bytes mismatch");
            }
            require(
                contribution.forwardedAt >= lastForwardedAt
                    && contribution.forwardedAt <= block.timestamp
                    && contribution.forwardedAt <= authorization.expiresAt,
                "Invalid contribution time"
            );

            bytes32 id = contributionHash(contribution);
            require(!consumedContributions[id], "Contribution consumed");
            for (uint256 earlier = 0; earlier < index; earlier++) {
                require(ids[earlier] != id, "Duplicate contribution");
            }
            require(ECDSA.recover(id, signatures[index]) == relayers[index], "Invalid relay signature");
            ids[index] = id;
            lastForwardedAt = contribution.forwardedAt;
        }

        if (authorization.deliveryMode == 0) {
            require(deliveredBytes == authorization.authorizedBytes, "Incomplete payload");
        }
    }

    function _validateAcknowledgement(
        RelayAuthorization calldata authorization,
        bytes32[] memory contributionIds,
        uint64 deliveredBytes,
        uint64 lastForwardedAt,
        RecipientAcknowledgement calldata acknowledgement,
        bytes calldata signature
    ) private view {
        require(
            acknowledgement.authorizationHash == authorizationHash(authorization)
                && acknowledgement.contributionsHash == orderedContributionsHash(contributionIds)
                && acknowledgement.recipient == authorization.recipient
                && acknowledgement.payloadCommitment == authorization.payloadCommitment
                && acknowledgement.deliveredBytes == deliveredBytes,
            "Acknowledgement mismatch"
        );
        require(
            acknowledgement.receivedAt >= lastForwardedAt
                && acknowledgement.receivedAt <= block.timestamp
                && acknowledgement.receivedAt <= authorization.expiresAt,
            "Invalid acknowledgement time"
        );
        bytes32 acknowledgementId = acknowledgementHash(acknowledgement);
        require(
            ECDSA.recover(acknowledgementId, signature) == authorization.recipient,
            "Invalid recipient signature"
        );
    }

    function _validateDistinct(address sender, address[] calldata relayers, address recipient)
        private
        pure
    {
        require(sender != recipient, "Common control");
        for (uint256 index = 0; index < relayers.length; index++) {
            address relayer = relayers[index];
            require(relayer != address(0) && relayer != sender && relayer != recipient, "Common control");
            for (uint256 earlier = 0; earlier < index; earlier++) {
                require(relayers[earlier] != relayer, "Common control");
            }
        }
    }

    function _creditRelayWork(
        bytes32 routeId,
        address[] calldata relayers,
        uint64 deliveredBytes
    )
        private
        returns (uint64 totalPaidNavax)
    {
        (, uint64 deliveredBase, , , ) = quote(deliveredBytes, uint8(relayers.length));
        uint64 perRelay = deliveredBase / uint64(relayers.length);
        for (uint256 index = 0; index < relayers.length; index++) {
            settledRelayEarningsNavax[relayers[index]] += perRelay;
            _credit(relayers[index], uint256(perRelay) * WEI_PER_NAVAX);
            totalPaidNavax += perRelay;
            emit RelayRewardCredited(routeId, relayers[index], perRelay);
        }
    }

    function _meteredExecutorNavax(uint256 gasAtEntry) private view returns (uint64) {
        uint256 meteredWei = (gasAtEntry - gasleft() + SETTLEMENT_OVERHEAD_GAS) * tx.gasprice;
        uint256 meteredNavax = (meteredWei + WEI_PER_NAVAX - 1) / WEI_PER_NAVAX;
        if (meteredNavax > SETTLEMENT_GAS_CAP_NAVAX) return SETTLEMENT_GAS_CAP_NAVAX;
        return uint64(meteredNavax);
    }

    function _credit(address account, uint256 amountWei) private {
        if (amountWei == 0) return;
        withdrawableWei[account] += amountWei;
        totalCreditsWei += amountWei;
    }
}
