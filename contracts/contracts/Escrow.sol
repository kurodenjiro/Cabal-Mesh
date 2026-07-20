// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @notice Minimal multi-deal escrow: a depositor locks native AVAX for a payee,
/// then either the depositor releases it, or either party unwinds it (depositor
/// any time, anyone after an optional expiry).
contract Escrow is ReentrancyGuard {
    enum Status {
        None,
        Active,
        Released,
        Refunded
    }

    struct Deal {
        address depositor;
        address payee;
        uint256 amount;
        uint256 expiry; // unix timestamp; 0 = no expiry
        Status status;
    }

    uint256 public nextEscrowId = 1;
    mapping(uint256 => Deal) public deals;

    event EscrowCreated(
        uint256 indexed escrowId,
        address indexed depositor,
        address indexed payee,
        uint256 amount,
        uint256 expiry
    );
    event EscrowReleased(uint256 indexed escrowId, address indexed payee, uint256 amount);
    event EscrowRefunded(uint256 indexed escrowId, address indexed depositor, uint256 amount);

    function createEscrow(address payee, uint256 expiry) external payable returns (uint256) {
        require(msg.value > 0, "No funds sent");
        require(payee != address(0), "Invalid payee");
        require(expiry == 0 || expiry > block.timestamp, "Invalid expiry");

        uint256 id = nextEscrowId++;
        deals[id] = Deal({
            depositor: msg.sender,
            payee: payee,
            amount: msg.value,
            expiry: expiry,
            status: Status.Active
        });

        emit EscrowCreated(id, msg.sender, payee, msg.value, expiry);
        return id;
    }

    function release(uint256 escrowId) external nonReentrant {
        Deal storage d = deals[escrowId];
        require(d.status == Status.Active, "Not active");
        require(msg.sender == d.depositor, "Only depositor");

        d.status = Status.Released;
        uint256 amount = d.amount;
        (bool ok, ) = d.payee.call{value: amount}("");
        require(ok, "Transfer failed");

        emit EscrowReleased(escrowId, d.payee, amount);
    }

    function refund(uint256 escrowId) external nonReentrant {
        Deal storage d = deals[escrowId];
        require(d.status == Status.Active, "Not active");
        require(
            msg.sender == d.depositor || (d.expiry != 0 && block.timestamp >= d.expiry),
            "Not authorized or not expired"
        );

        d.status = Status.Refunded;
        uint256 amount = d.amount;
        (bool ok, ) = d.depositor.call{value: amount}("");
        require(ok, "Transfer failed");

        emit EscrowRefunded(escrowId, d.depositor, amount);
    }

    function getDeal(uint256 escrowId) external view returns (Deal memory) {
        return deals[escrowId];
    }
}
