// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/access/Ownable.sol";

interface ICabalMeshVoucher {
    function mintVoucher(
        address to,
        string calldata voucherType,
        string calldata description,
        uint8 slot,
        uint8 rarity,
        uint16 effectBps
    ) external returns (uint256);
}

/// @notice Pays a gateway for relaying a transaction to the chain, and mints
/// a POWER module once a gateway's cumulative relayed value crosses a
/// milestone. See docs/intent-chat-and-modules-design.md, decisions 0, 3
/// and 4 for the reasoning; this contract is the mechanism those decisions
/// describe.
///
/// @dev Scoped to **gateway** relaying only, deliberately. BLE mesh relaying
/// is flood-based — a packet is copied by whichever neighbours a thinned
/// fanout selects, with no routing table and no single node identifiable as
/// "the" relay for a given hop (`cabal-ble/src/router.rs`). A gateway is
/// different: it submits an already-signed transaction to the chain itself,
/// so the transaction's own signer (`msg.sender` here) is already
/// attributable on-chain with no new protocol needed. Rewarding BLE flood
/// relay needs an attributable per-hop receipt first — out of scope here.
///
/// @dev The fee is sender-paid, not treasury emission: `msg.value` comes
/// from whoever calls `recordGatewayRelay`, which in the intended flow is
/// the *original sender's own offline-signed transaction* — the gateway's
/// only job is broadcasting bytes it did not sign, so `msg.sender` on
/// execution is the sender, and `gateway` is who the sender named as having
/// carried it. Farming this by a gateway paying itself costs the same real
/// AVAX moving between two real addresses that self-dealing already costs
/// anywhere else in this app; see decision 4 for why that is judged
/// sufficient for a first version.
contract RelayRewards is Ownable {
    ICabalMeshVoucher public voucher;

    /// Cumulative wei this gateway has been paid for relaying.
    mapping(address => uint256) public relayedWei;
    /// How many milestone modules this gateway has already been minted, so
    /// crossing the same threshold twice does not mint twice.
    mapping(address => uint256) public milestonesClaimed;

    /// Cumulative relayed value that earns one module. A placeholder
    /// worth revisiting once real relay volume exists to calibrate against
    /// — nothing about the mechanism depends on this specific number.
    uint256 public constant MILESTONE_WEI = 1 ether;

    /// Modules minted in a single call is capped so one unusually large
    /// payment cannot make gas cost unbounded. Nothing is lost by the cap —
    /// `milestonesClaimed` persists, so any milestones past it mint on the
    /// gateway's next payment instead of this one.
    uint256 public constant MAX_MILESTONES_PER_CALL = 20;

    uint8 public constant SLOT_POWER = 2;
    uint8 public constant RARITY_UNCOMMON = 1;
    uint16 public constant GATEWAY_MODULE_EFFECT_BPS = 500; // +5%

    event GatewayRelayPaid(address indexed gateway, uint256 amount, uint256 cumulative);
    event GatewayMilestoneReached(address indexed gateway, uint256 milestoneNumber, uint256 tokenId);

    constructor() Ownable(msg.sender) {}

    /// One-time wiring to the voucher contract this rewards contract is
    /// allowed to mint on. Not a constructor argument: `CabalMeshVoucher`'s
    /// own constructor takes *this* contract's address (to restrict minting
    /// to it), so this contract necessarily deploys first, with nothing yet
    /// to point at. Callable once, by the deployer only — after that this
    /// behaves exactly as if it had been immutable from construction.
    function setVoucher(address voucherAddress) external onlyOwner {
        require(address(voucher) == address(0), "Voucher already set");
        require(voucherAddress != address(0), "Voucher required");
        voucher = ICabalMeshVoucher(voucherAddress);
    }

    /// Pays `gateway` the attached fee for relaying a transaction, and mints
    /// a module if this payment crosses a milestone. `payable` and
    /// forwarding `msg.value` in full — this contract holds no funds of its
    /// own between calls, so there is nothing here for `onlyOwner` to ever
    /// need to sweep.
    function recordGatewayRelay(address gateway) external payable {
        require(msg.value > 0, "No fee attached");
        require(gateway != address(0), "Gateway required");

        uint256 cumulative = relayedWei[gateway] + msg.value;
        relayedWei[gateway] = cumulative;

        (bool ok, ) = gateway.call{value: msg.value}("");
        require(ok, "Payout failed");

        emit GatewayRelayPaid(gateway, msg.value, cumulative);

        if (address(voucher) == address(0)) {
            return;
        }

        uint256 earned = cumulative / MILESTONE_WEI;
        uint256 claimed = milestonesClaimed[gateway];
        if (earned <= claimed) {
            return;
        }

        // Award every milestone this payment crossed, not just one — a
        // single large payment can cross several at once, and undercounting
        // here would be an under-payment the gateway has no way to notice.
        uint256 toMint = earned - claimed;
        if (toMint > MAX_MILESTONES_PER_CALL) {
            toMint = MAX_MILESTONES_PER_CALL;
        }

        for (uint256 i = 0; i < toMint; i++) {
            claimed++;
            uint256 tokenId = voucher.mintVoucher(
                gateway,
                "Gateway License",
                "Earned by relaying settled transactions as a gateway",
                SLOT_POWER,
                RARITY_UNCOMMON,
                GATEWAY_MODULE_EFFECT_BPS
            );
            emit GatewayMilestoneReached(gateway, claimed, tokenId);
        }
        milestonesClaimed[gateway] = claimed;
    }
}
