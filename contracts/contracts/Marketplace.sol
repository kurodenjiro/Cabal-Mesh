// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC721/IERC721.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @notice Asset-backed catalog + atomic settlement. Every listing references
/// an NFT from an allowed collection that the seller actually owns (checked
/// on-chain). Buying atomically locks the buyer's AVAX and pulls the seller's
/// NFT into this contract in one transaction.
///
/// # What changed, and why
///
/// **Settlement was a free option for the buyer.** Both `releaseDeal` and
/// `refundDeal` were buyer-only, so a buyer could take a refund at any moment
/// and the seller had no way to ever force payment. A silent buyer left the
/// token and the funds locked forever — there was no deadline and no third
/// party. Settlement is now asymmetric in the direction the escrow actually
/// needs:
///
/// - `releaseDeal` — the buyer any time, or **anyone** once `autoReleaseAt`
///   passes. The default outcome of a paid deal is that the seller gets paid.
/// - `refundDeal` — the seller only, and only after the buyer has called
///   `requestRefund`. Cancelling requires both sides to agree, so neither
///   holds a unilateral option over the other.
///
/// The asset leg of this trade is already on-chain and atomic, so the escrow
/// window is a cancellation window, not a delivery-dispute window. Treating it
/// as the latter is what produced the one-sided rules.
///
/// **A listing could not be cancelled.** There was no `cancelListing`, so a
/// seller who listed was committed until someone bought. Added.
///
/// **One token could back several live listings.** `createListing` never
/// checked whether the token was already listed, so the same NFT could be
/// listed repeatedly; one sale left the others active and reverting at buy
/// time. A token now has at most one active listing, tracked in
/// `activeListingOf`.
///
/// **The collection was immutable.** `voucher` was an `immutable` address, so
/// replacing the token contract — which the module work requires, since the
/// deployed voucher had an open mint — forced a redeploy of this contract and
/// orphaned every existing listing and deal. Collections are now an
/// issuer-managed allowlist and each listing and deal records the collection
/// it belongs to, so a new token contract is one governance call and old deals
/// keep settling against the contract they were opened against.
contract Marketplace is ReentrancyGuard {
    struct Listing {
        address seller;
        string description;
        uint256 priceWei;
        uint256 tokenId;
        bool active;
        /// The NFT contract this listing's `tokenId` belongs to.
        address collection;
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
        address collection;
        /// After this timestamp, anyone may release the deal to the seller.
        uint64 autoReleaseAt;
        /// Set by the buyer to consent to cancellation. The seller cannot
        /// refund without it.
        bool refundRequested;
    }

    /// The collection used by the legacy `createListing` entry point, and the
    /// one seeded as allowed at deployment.
    IERC721 public immutable voucher;

    /// How long after a buy the buyer keeps the exclusive right to release.
    /// Once it passes, anyone may push the deal to its default outcome so a
    /// silent buyer cannot strand the seller's asset and payment.
    uint256 public immutable releaseWindow;

    /// Controls the collection allowlist.
    address public governor;

    /// Collections that new listings may reference. Removing one stops new
    /// listings; it does not touch listings or deals already open against it.
    mapping(address => bool) public allowedCollections;

    uint256 public nextListingId = 1;
    uint256 public nextDealId = 1;
    mapping(uint256 => Listing) public listings;
    uint256[] public listingIds;
    mapping(uint256 => Deal) public deals;

    /// collection => tokenId => the listing id currently active for it, or 0.
    mapping(address => mapping(uint256 => uint256)) public activeListingOf;

    event ListingCreated(uint256 indexed id, address indexed seller, uint256 indexed tokenId, string description, uint256 priceWei);
    event ListingCancelled(uint256 indexed id, address indexed seller, uint256 indexed tokenId);
    event DealCreated(uint256 indexed dealId, uint256 indexed listingId, address indexed buyer, uint256 tokenId, uint256 amount);
    event DealReleased(uint256 indexed dealId);
    event DealRefunded(uint256 indexed dealId);
    event RefundRequested(uint256 indexed dealId, address indexed buyer);
    event CollectionAllowed(address indexed collection, bool allowed);
    event GovernorTransferred(address indexed previousGovernor, address indexed newGovernor);

    modifier onlyGovernor() {
        require(msg.sender == governor, "Not the governor");
        _;
    }

    constructor(address voucherAddress, uint256 releaseWindowSeconds) {
        require(voucherAddress != address(0), "Zero collection");
        require(releaseWindowSeconds > 0, "Release window required");

        voucher = IERC721(voucherAddress);
        releaseWindow = releaseWindowSeconds;
        governor = msg.sender;
        allowedCollections[voucherAddress] = true;

        emit GovernorTransferred(address(0), msg.sender);
        emit CollectionAllowed(voucherAddress, true);
    }

    // ---- Governance -------------------------------------------------------

    function setCollectionAllowed(address collection, bool allowed) external onlyGovernor {
        require(collection != address(0), "Zero collection");
        allowedCollections[collection] = allowed;
        emit CollectionAllowed(collection, allowed);
    }

    function transferGovernor(address newGovernor) external onlyGovernor {
        require(newGovernor != address(0), "Zero governor");
        address previous = governor;
        governor = newGovernor;
        emit GovernorTransferred(previous, newGovernor);
    }

    // ---- Listing ----------------------------------------------------------

    /// Lists a token from the default collection. Kept for callers that
    /// predate multi-collection support.
    function createListing(string calldata description, uint256 priceWei, uint256 tokenId)
        external
        returns (uint256)
    {
        return _createListing(address(voucher), description, priceWei, tokenId);
    }

    /// Lists a token from any allowed collection.
    function createListingFor(address collection, string calldata description, uint256 priceWei, uint256 tokenId)
        external
        returns (uint256)
    {
        return _createListing(collection, description, priceWei, tokenId);
    }

    function cancelListing(uint256 listingId) external {
        Listing storage l = listings[listingId];
        require(l.active, "Not active");
        require(msg.sender == l.seller, "Only seller");

        l.active = false;
        activeListingOf[l.collection][l.tokenId] = 0;

        emit ListingCancelled(listingId, l.seller, l.tokenId);
    }

    function _createListing(address collection, string calldata description, uint256 priceWei, uint256 tokenId)
        private
        returns (uint256)
    {
        require(allowedCollections[collection], "Collection not allowed");
        require(priceWei > 0, "Price must be > 0");
        require(bytes(description).length > 0, "Description required");
        require(activeListingOf[collection][tokenId] == 0, "Token already listed");

        IERC721 nft = IERC721(collection);
        require(nft.ownerOf(tokenId) == msg.sender, "Not the voucher owner");
        require(
            nft.getApproved(tokenId) == address(this) || nft.isApprovedForAll(msg.sender, address(this)),
            "Approve marketplace first"
        );

        uint256 id = nextListingId++;
        listings[id] = Listing({
            seller: msg.sender,
            description: description,
            priceWei: priceWei,
            tokenId: tokenId,
            active: true,
            collection: collection
        });
        listingIds.push(id);
        activeListingOf[collection][tokenId] = id;

        emit ListingCreated(id, msg.sender, tokenId, description, priceWei);
        return id;
    }

    // ---- Settlement -------------------------------------------------------

    function buy(uint256 listingId) external payable nonReentrant returns (uint256) {
        Listing storage l = listings[listingId];
        require(l.active, "Not active");
        require(msg.sender != l.seller, "Cannot buy your own listing");
        require(msg.value == l.priceWei, "Wrong amount");

        l.active = false;
        activeListingOf[l.collection][l.tokenId] = 0;
        IERC721(l.collection).transferFrom(l.seller, address(this), l.tokenId);

        uint256 dealId = nextDealId++;
        deals[dealId] = Deal({
            buyer: msg.sender,
            seller: l.seller,
            tokenId: l.tokenId,
            amount: msg.value,
            status: DealStatus.Active,
            collection: l.collection,
            autoReleaseAt: uint64(block.timestamp + releaseWindow),
            refundRequested: false
        });

        emit DealCreated(dealId, listingId, msg.sender, l.tokenId, msg.value);
        return dealId;
    }

    /// Pays the seller and hands the token to the buyer. The buyer may call it
    /// at any time; once `autoReleaseAt` passes anyone may, which is what stops
    /// a silent buyer from stranding the seller's asset and payment forever.
    function releaseDeal(uint256 dealId) external nonReentrant {
        Deal storage d = deals[dealId];
        require(d.status == DealStatus.Active, "Not active");
        require(
            msg.sender == d.buyer || block.timestamp >= d.autoReleaseAt,
            "Only buyer before auto-release"
        );

        d.status = DealStatus.Released;
        IERC721(d.collection).transferFrom(address(this), d.buyer, d.tokenId);
        (bool ok, ) = d.seller.call{value: d.amount}("");
        require(ok, "Transfer failed");

        emit DealReleased(dealId);
    }

    /// The buyer's consent to cancel. It moves nothing on its own — it only
    /// unlocks `refundDeal` for the seller.
    function requestRefund(uint256 dealId) external {
        Deal storage d = deals[dealId];
        require(d.status == DealStatus.Active, "Not active");
        require(msg.sender == d.buyer, "Only buyer");

        d.refundRequested = true;
        emit RefundRequested(dealId, msg.sender);
    }

    /// Cancels a deal: AVAX back to the buyer, token back to the seller.
    /// Requires the seller to call it and the buyer to have consented, so
    /// neither side can unwind a settled trade on its own.
    function refundDeal(uint256 dealId) external nonReentrant {
        Deal storage d = deals[dealId];
        require(d.status == DealStatus.Active, "Not active");
        require(msg.sender == d.seller, "Only seller");
        require(d.refundRequested, "Buyer has not requested a refund");

        d.status = DealStatus.Refunded;
        IERC721(d.collection).transferFrom(address(this), d.seller, d.tokenId);
        (bool ok, ) = d.buyer.call{value: d.amount}("");
        require(ok, "Transfer failed");

        emit DealRefunded(dealId);
    }

    // ---- Views ------------------------------------------------------------

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

    /// Same as `getActiveListings`, over a window of the listing history.
    /// `getActiveListings` walks every listing ever created and returns them
    /// all in one response, which stops being callable as the catalog grows;
    /// this is the version a client can keep using.
    function getActiveListingsPaged(uint256 offset, uint256 limit)
        external
        view
        returns (Listing[] memory result, uint256[] memory ids, uint256 nextOffset)
    {
        uint256 total = listingIds.length;
        if (offset >= total || limit == 0) {
            return (new Listing[](0), new uint256[](0), total);
        }

        uint256 end = offset + limit;
        if (end > total) end = total;

        uint256 count;
        for (uint256 i = offset; i < end; i++) {
            if (listings[listingIds[i]].active) count++;
        }

        result = new Listing[](count);
        ids = new uint256[](count);
        uint256 j;
        for (uint256 i = offset; i < end; i++) {
            uint256 id = listingIds[i];
            if (listings[id].active) {
                result[j] = listings[id];
                ids[j] = id;
                j++;
            }
        }

        nextOffset = end;
    }

    function listingCount() external view returns (uint256) {
        return listingIds.length;
    }

    function getDeal(uint256 dealId) external view returns (Deal memory) {
        return deals[dealId];
    }
}
