// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC721/IERC721.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @notice Asset-backed catalog + atomic settlement. Every listing must
/// reference a CabalMeshVoucher tokenId the seller actually owns (checked
/// on-chain — no one can list an asset they don't hold). Buying atomically
/// locks the buyer's AVAX and pulls the seller's NFT into this contract in a
/// single transaction, so neither side can back out or double-sell once
/// matched; releasing/refunding moves both the funds and the asset together.
contract Marketplace is ReentrancyGuard {
    struct Listing {
        address seller;
        string description;
        uint256 priceWei;
        uint256 tokenId;
        bool active;
    }

    enum DealStatus {
        None,
        Active,
        Released,
        Refunded
    }

    struct Deal {
        address buyer;
        address seller;
        uint256 tokenId;
        uint256 amount;
        DealStatus status;
    }

    IERC721 public immutable voucher;

    uint256 public nextListingId = 1;
    uint256 public nextDealId = 1;
    mapping(uint256 => Listing) public listings;
    uint256[] public listingIds;
    mapping(uint256 => Deal) public deals;

    event ListingCreated(uint256 indexed id, address indexed seller, uint256 indexed tokenId, string description, uint256 priceWei);
    event DealCreated(uint256 indexed dealId, uint256 indexed listingId, address indexed buyer, uint256 tokenId, uint256 amount);
    event DealReleased(uint256 indexed dealId);
    event DealRefunded(uint256 indexed dealId);

    constructor(address voucherAddress) {
        voucher = IERC721(voucherAddress);
    }

    function createListing(string calldata description, uint256 priceWei, uint256 tokenId) external returns (uint256) {
        require(priceWei > 0, "Price must be > 0");
        require(bytes(description).length > 0, "Description required");
        require(voucher.ownerOf(tokenId) == msg.sender, "Not the voucher owner");
        require(voucher.getApproved(tokenId) == address(this), "Approve marketplace first");

        uint256 id = nextListingId++;
        listings[id] = Listing({
            seller: msg.sender,
            description: description,
            priceWei: priceWei,
            tokenId: tokenId,
            active: true
        });
        listingIds.push(id);

        emit ListingCreated(id, msg.sender, tokenId, description, priceWei);
        return id;
    }

    function buy(uint256 listingId) external payable nonReentrant returns (uint256) {
        Listing storage l = listings[listingId];
        require(l.active, "Not active");
        require(msg.sender != l.seller, "Cannot buy your own listing");
        require(msg.value == l.priceWei, "Wrong amount");

        l.active = false;
        voucher.transferFrom(l.seller, address(this), l.tokenId);

        uint256 dealId = nextDealId++;
        deals[dealId] = Deal({
            buyer: msg.sender,
            seller: l.seller,
            tokenId: l.tokenId,
            amount: msg.value,
            status: DealStatus.Active
        });

        emit DealCreated(dealId, listingId, msg.sender, l.tokenId, msg.value);
        return dealId;
    }

    function releaseDeal(uint256 dealId) external {
        Deal storage d = deals[dealId];
        require(d.status == DealStatus.Active, "Not active");
        require(msg.sender == d.buyer, "Only buyer");

        d.status = DealStatus.Released;
        voucher.transferFrom(address(this), d.buyer, d.tokenId);
        (bool ok, ) = d.seller.call{value: d.amount}("");
        require(ok, "Transfer failed");

        emit DealReleased(dealId);
    }

    function refundDeal(uint256 dealId) external {
        Deal storage d = deals[dealId];
        require(d.status == DealStatus.Active, "Not active");
        require(msg.sender == d.buyer, "Only buyer");

        d.status = DealStatus.Refunded;
        voucher.transferFrom(address(this), d.seller, d.tokenId);
        (bool ok, ) = d.buyer.call{value: d.amount}("");
        require(ok, "Transfer failed");

        emit DealRefunded(dealId);
    }

    function getActiveListings() external view returns (Listing[] memory result, uint256[] memory ids) {
        uint256 count;
        for (uint256 i = 0; i < listingIds.length; i++) {
            if (listings[listingIds[i]].active) count++;
        }

        result = new Listing[](count);
        ids = new uint256[](count);
        uint256 j;
        for (uint256 i = 0; i < listingIds.length; i++) {
            uint256 id = listingIds[i];
            if (listings[id].active) {
                result[j] = listings[id];
                ids[j] = id;
                j++;
            }
        }
    }

    function getDeal(uint256 dealId) external view returns (Deal memory) {
        return deals[dealId];
    }
}
