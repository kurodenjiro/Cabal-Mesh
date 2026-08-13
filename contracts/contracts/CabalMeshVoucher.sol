// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";

/// @notice Redeemable digital vouchers (AI compute credit, relay bandwidth
/// credit) and node modules (RADIO/CRYPTO/POWER items that boost relay
/// yield). Redeeming requires being the current owner and burns the token.
///
/// @dev Minting is restricted to `rewardsContract` — see
/// docs/intent-chat-and-modules-design.md, decision 0. The original version
/// of this contract let `mintVoucher` be called by anyone, for any
/// `voucherType`, for free: a module claiming "+18% relay yield" was
/// indistinguishable from one a caller invented on the spot by calling the
/// contract directly, no app involved. Restricting minting to a single
/// contract address — not an admin key — keeps the fix trustless: a module
/// is minted only as the verified, atomic side effect of something
/// `rewardsContract` already checked on-chain (see `RelayRewards.sol`),
/// never by a party's own say-so, off-chain or on.
contract CabalMeshVoucher is ERC721 {
    struct VoucherData {
        string voucherType;
        string description;
        address mintedBy;
        /// 0 = RADIO, 1 = CRYPTO, 2 = POWER, 3 = SOULBOUND (Standing Badge —
        /// earned, not tradable; the marketplace/equip layer enforces the
        /// non-tradable part, this is just the label).
        uint8 slot;
        /// 0 = COMMON, 1 = UNCOMMON, 2 = RARE, 3 = LEGENDARY.
        uint8 rarity;
        /// The module's effect in basis points (1800 = +18%). Zero for
        /// vouchers that aren't modules (AI compute credit, etc.) — those
        /// redeem for something off-chain and carry no on-chain multiplier.
        uint16 effectBps;
    }

    /// Who may mint. Immutable and checked in the constructor rather than
    /// left settable, so the access-control fix cannot itself be
    /// reintroduced as a mutable, reassignable admin field later.
    address public immutable rewardsContract;

    uint256 public nextTokenId = 1;
    mapping(uint256 => VoucherData) public vouchers;

    event VoucherMinted(
        uint256 indexed tokenId,
        address indexed owner,
        string voucherType,
        string description,
        uint8 slot,
        uint8 rarity,
        uint16 effectBps
    );
    event VoucherRedeemed(uint256 indexed tokenId, address indexed redeemer, string voucherType);

    constructor(address rewardsContractAddress) ERC721("CabalMesh Voucher", "CMV") {
        require(rewardsContractAddress != address(0), "Rewards contract required");
        rewardsContract = rewardsContractAddress;
    }

    modifier onlyRewardsContract() {
        require(msg.sender == rewardsContract, "Only the rewards contract may mint");
        _;
    }

    /// Mints a voucher or module to `to`. Restricted — see the contract-level
    /// doc comment for why this is a contract address, not an admin key.
    function mintVoucher(
        address to,
        string calldata voucherType,
        string calldata description,
        uint8 slot,
        uint8 rarity,
        uint16 effectBps
    ) external onlyRewardsContract returns (uint256) {
        require(bytes(voucherType).length > 0, "Voucher type required");
        require(to != address(0), "Recipient required");

        uint256 tokenId = nextTokenId++;
        _safeMint(to, tokenId);
        vouchers[tokenId] = VoucherData({
            voucherType: voucherType,
            description: description,
            mintedBy: to,
            slot: slot,
            rarity: rarity,
            effectBps: effectBps
        });

        emit VoucherMinted(tokenId, to, voucherType, description, slot, rarity, effectBps);
        return tokenId;
    }

    function redeemVoucher(uint256 tokenId) external {
        require(ownerOf(tokenId) == msg.sender, "Not the owner");

        string memory vType = vouchers[tokenId].voucherType;
        _burn(tokenId);

        emit VoucherRedeemed(tokenId, msg.sender, vType);
    }
}
