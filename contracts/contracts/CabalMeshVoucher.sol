// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";

/// @notice Redeemable digital vouchers (e.g. AI compute credit, relay bandwidth
/// credit). Redeeming requires being the current owner and burns the token.
///
/// # Why minting is no longer open to everyone
///
/// The first version let any wallet call `mintVoucher` and mint into its own
/// name, and documented that as the proof-of-possession check. That is only
/// sound while a voucher is worth nothing: the moment a token carries a reward
/// — a relay multiplier, a gateway licence — an open mint is a mint button for
/// free money, and every downstream check that trusts "this address owns an
/// authentic module" is checking a claim the holder wrote themselves.
///
/// Minting is therefore gated on an issuer-managed minter set. Ownership still
/// proves possession; it no longer proves authenticity, so authenticity is
/// established at issue time instead.
///
/// Note that this contract still has no on-chain notion of slot, rarity or
/// effect, and no soulbound tokens. Those belong to the module metadata work
/// and are deliberately not smuggled in here.
contract CabalMeshVoucher is ERC721 {
    struct VoucherData {
        string voucherType;
        string description;
        address mintedBy;
    }

    uint256 public nextTokenId = 1;
    mapping(uint256 => VoucherData) public vouchers;

    /// The address that controls the minter set. Set to the deployer, and
    /// transferable exactly once per call so a compromised issuer can be
    /// rotated without redeploying the token.
    address public issuer;

    /// Addresses allowed to mint. The issuer is one by default; anything else
    /// is an explicit grant.
    mapping(address => bool) public minters;

    event VoucherMinted(uint256 indexed tokenId, address indexed owner, string voucherType, string description);
    event VoucherRedeemed(uint256 indexed tokenId, address indexed redeemer, string voucherType);
    event MinterSet(address indexed minter, bool allowed);
    event IssuerTransferred(address indexed previousIssuer, address indexed newIssuer);

    modifier onlyIssuer() {
        require(msg.sender == issuer, "Not the issuer");
        _;
    }

    modifier onlyMinter() {
        require(minters[msg.sender], "Not an authorized minter");
        _;
    }

    constructor() ERC721("CabalMesh Voucher", "CMV") {
        issuer = msg.sender;
        minters[msg.sender] = true;
        emit IssuerTransferred(address(0), msg.sender);
        emit MinterSet(msg.sender, true);
    }

    /// Mints to the caller. Same signature as before, but now restricted —
    /// an unauthorized caller reverts instead of minting itself a reward.
    function mintVoucher(string calldata voucherType, string calldata description)
        external
        onlyMinter
        returns (uint256)
    {
        return _mintVoucher(msg.sender, voucherType, description);
    }

    /// Mints to someone else. This is the shape a milestone reward actually
    /// needs: the authority issues, the earner receives, and the earner never
    /// holds mint rights.
    function mintTo(address to, string calldata voucherType, string calldata description)
        external
        onlyMinter
        returns (uint256)
    {
        require(to != address(0), "Zero recipient");
        return _mintVoucher(to, voucherType, description);
    }

    function setMinter(address minter, bool allowed) external onlyIssuer {
        require(minter != address(0), "Zero minter");
        minters[minter] = allowed;
        emit MinterSet(minter, allowed);
    }

    function transferIssuer(address newIssuer) external onlyIssuer {
        require(newIssuer != address(0), "Zero issuer");
        address previous = issuer;
        issuer = newIssuer;
        emit IssuerTransferred(previous, newIssuer);
    }

    function redeemVoucher(uint256 tokenId) external {
        require(ownerOf(tokenId) == msg.sender, "Not the owner");

        string memory vType = vouchers[tokenId].voucherType;
        _burn(tokenId);

        emit VoucherRedeemed(tokenId, msg.sender, vType);
    }

    function _mintVoucher(address to, string calldata voucherType, string calldata description)
        private
        returns (uint256)
    {
        require(bytes(voucherType).length > 0, "Voucher type required");

        uint256 tokenId = nextTokenId++;
        _safeMint(to, tokenId);
        vouchers[tokenId] = VoucherData({
            voucherType: voucherType,
            description: description,
            mintedBy: msg.sender
        });

        emit VoucherMinted(tokenId, to, voucherType, description);
        return tokenId;
    }
}
