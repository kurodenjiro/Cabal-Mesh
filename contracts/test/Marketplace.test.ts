import { expect } from "chai";
import { ethers } from "hardhat";
import { time } from "@nomicfoundation/hardhat-network-helpers";
import { Marketplace, CabalMeshVoucher } from "../typechain-types";
import { HardhatEthersSigner } from "@nomicfoundation/hardhat-ethers/signers";

const RELEASE_WINDOW = 3 * 24 * 60 * 60; // 3 days, the value deployed to Fuji

describe("Marketplace", function () {
    let marketplace: Marketplace;
    let voucher: CabalMeshVoucher;
    let seller: HardhatEthersSigner;
    let buyer: HardhatEthersSigner;
    let other: HardhatEthersSigner;
    const price = ethers.parseEther("1.0");

    async function mintAndApprove(): Promise<bigint> {
        const tokenId = await voucher.nextTokenId();
        await voucher.connect(seller).mintVoucher("AI Compute Credit", "1 hour Ollama compute");
        await voucher.connect(seller).approve(await marketplace.getAddress(), tokenId);
        return tokenId;
    }

    /// A deal in the Active state, ready for release/refund tests.
    async function openDeal(): Promise<bigint> {
        const tokenId = await mintAndApprove();
        await marketplace.connect(seller).createListing("Item", price, tokenId);
        const listingId = await marketplace.nextListingId() - 1n;
        await marketplace.connect(buyer).buy(listingId, { value: price });
        return await marketplace.nextDealId() - 1n;
    }

    beforeEach(async function () {
        [seller, buyer, other] = await ethers.getSigners();

        const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
        voucher = await Voucher.deploy();
        await voucher.waitForDeployment();
        // The seller is the deployer here, so it already holds mint rights.

        const Marketplace = await ethers.getContractFactory("Marketplace");
        marketplace = await Marketplace.deploy(await voucher.getAddress(), RELEASE_WINDOW);
        await marketplace.waitForDeployment();
    });

    describe("listing", function () {
        it("creates a listing for an owned, approved voucher", async function () {
            const tokenId = await mintAndApprove();

            await expect(marketplace.connect(seller).createListing("1 hour AI compute", price, tokenId))
                .to.emit(marketplace, "ListingCreated")
                .withArgs(1, seller.address, tokenId, "1 hour AI compute", price);

            const listing = await marketplace.listings(1);
            expect(listing.seller).to.equal(seller.address);
            expect(listing.tokenId).to.equal(tokenId);
            expect(listing.priceWei).to.equal(price);
            expect(listing.active).to.equal(true);
            expect(listing.collection).to.equal(await voucher.getAddress());
        });

        it("reverts listing a voucher the caller doesn't own", async function () {
            await voucher.connect(seller).mintVoucher("AI Compute Credit", "desc");
            await voucher.connect(seller).approve(await marketplace.getAddress(), 1n);

            await expect(
                marketplace.connect(other).createListing("desc", price, 1n)
            ).to.be.revertedWith("Not the voucher owner");
        });

        it("reverts listing a voucher not yet approved for the marketplace", async function () {
            await voucher.connect(seller).mintVoucher("AI Compute Credit", "desc");

            await expect(
                marketplace.connect(seller).createListing("desc", price, 1n)
            ).to.be.revertedWith("Approve marketplace first");
        });

        it("accepts a blanket operator approval as well as a per-token one", async function () {
            await voucher.connect(seller).mintVoucher("AI Compute Credit", "desc");
            await voucher.connect(seller).setApprovalForAll(await marketplace.getAddress(), true);

            await expect(marketplace.connect(seller).createListing("desc", price, 1n))
                .to.emit(marketplace, "ListingCreated");
        });

        it("reverts on zero price", async function () {
            const tokenId = await mintAndApprove();
            await expect(
                marketplace.connect(seller).createListing("Item", 0, tokenId)
            ).to.be.revertedWith("Price must be > 0");
        });

        // Finding 3: one token backed several live listings, so one sale left
        // the others active and reverting at buy time.
        it("refuses a second active listing for the same token", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);

            await expect(
                marketplace.connect(seller).createListing("Item again", price, tokenId)
            ).to.be.revertedWith("Token already listed");

            expect(await marketplace.activeListingOf(await voucher.getAddress(), tokenId)).to.equal(1n);
        });

        // Finding 2: a seller who listed was committed until someone bought.
        it("lets the seller cancel a listing and list the token again", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);

            await expect(marketplace.connect(seller).cancelListing(1))
                .to.emit(marketplace, "ListingCancelled")
                .withArgs(1, seller.address, tokenId);

            expect((await marketplace.listings(1)).active).to.equal(false);
            expect(await marketplace.activeListingOf(await voucher.getAddress(), tokenId)).to.equal(0n);

            await expect(marketplace.connect(seller).createListing("Item", price, tokenId))
                .to.emit(marketplace, "ListingCreated");
        });

        it("reverts cancelListing by anyone but the seller", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);

            await expect(marketplace.connect(buyer).cancelListing(1)).to.be.revertedWith("Only seller");
        });

        it("reverts cancelling a listing that is already inactive", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);
            await marketplace.connect(seller).cancelListing(1);

            await expect(marketplace.connect(seller).cancelListing(1)).to.be.revertedWith("Not active");
        });

        it("a cancelled listing cannot be bought", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);
            await marketplace.connect(seller).cancelListing(1);

            await expect(marketplace.connect(buyer).buy(1, { value: price })).to.be.revertedWith("Not active");
        });
    });

    describe("buying", function () {
        it("buy() atomically locks AVAX and pulls the NFT into escrow", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);

            await expect(marketplace.connect(buyer).buy(1, { value: price }))
                .to.emit(marketplace, "DealCreated")
                .withArgs(1, 1, buyer.address, tokenId, price);

            expect(await voucher.ownerOf(tokenId)).to.equal(await marketplace.getAddress());
            const listing = await marketplace.listings(1);
            expect(listing.active).to.equal(false);
            expect(await marketplace.activeListingOf(await voucher.getAddress(), tokenId)).to.equal(0n);
        });

        it("reverts buy() with the wrong AVAX amount", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);

            await expect(
                marketplace.connect(buyer).buy(1, { value: ethers.parseEther("0.5") })
            ).to.be.revertedWith("Wrong amount");
        });

        it("reverts a seller buying their own listing", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);

            await expect(
                marketplace.connect(seller).buy(1, { value: price })
            ).to.be.revertedWith("Cannot buy your own listing");
        });

        it("records an auto-release deadline one release window out", async function () {
            const dealId = await openDeal();
            const deal = await marketplace.getDeal(dealId);
            const now = await time.latest();

            expect(Number(deal.autoReleaseAt)).to.be.closeTo(now + RELEASE_WINDOW, 5);
        });
    });

    describe("release", function () {
        it("releaseDeal pays the seller and transfers the NFT to the buyer", async function () {
            const dealId = await openDeal();
            const tokenId = (await marketplace.getDeal(dealId)).tokenId;
            const sellerBalanceBefore = await ethers.provider.getBalance(seller.address);

            await expect(marketplace.connect(buyer).releaseDeal(dealId))
                .to.emit(marketplace, "DealReleased")
                .withArgs(dealId);

            expect(await voucher.ownerOf(tokenId)).to.equal(buyer.address);
            expect(await ethers.provider.getBalance(seller.address) - sellerBalanceBefore).to.equal(price);
        });

        // Finding 1: with both settlement paths buyer-only, a silent buyer
        // stranded the seller's asset and payment with no deadline at all.
        it("reverts an early release by anyone but the buyer", async function () {
            const dealId = await openDeal();

            await expect(
                marketplace.connect(seller).releaseDeal(dealId)
            ).to.be.revertedWith("Only buyer before auto-release");
            await expect(
                marketplace.connect(other).releaseDeal(dealId)
            ).to.be.revertedWith("Only buyer before auto-release");
        });

        it("lets the seller release once the auto-release deadline passes", async function () {
            const dealId = await openDeal();
            const tokenId = (await marketplace.getDeal(dealId)).tokenId;
            const sellerBalanceBefore = await ethers.provider.getBalance(seller.address);

            await time.increase(RELEASE_WINDOW + 1);

            await expect(marketplace.connect(seller).releaseDeal(dealId))
                .to.emit(marketplace, "DealReleased");

            // The buyer still receives what they paid for; only the liveness
            // of the settlement moved, not its outcome.
            expect(await voucher.ownerOf(tokenId)).to.equal(buyer.address);
            expect(await ethers.provider.getBalance(seller.address)).to.be.greaterThan(sellerBalanceBefore);
        });

        it("lets an uninvolved third party release once the deadline passes", async function () {
            const dealId = await openDeal();
            await time.increase(RELEASE_WINDOW + 1);

            await expect(marketplace.connect(other).releaseDeal(dealId))
                .to.emit(marketplace, "DealReleased");
        });

        it("reverts releasing the same deal twice", async function () {
            const dealId = await openDeal();
            await marketplace.connect(buyer).releaseDeal(dealId);

            await expect(marketplace.connect(buyer).releaseDeal(dealId)).to.be.revertedWith("Not active");
        });
    });

    describe("refund", function () {
        // Finding 1: refundDeal was buyer-only, so a buyer held a free option
        // — take the goods or take the money back, decided after the fact.
        it("reverts a unilateral refund by the buyer", async function () {
            const dealId = await openDeal();

            await expect(marketplace.connect(buyer).refundDeal(dealId)).to.be.revertedWith("Only seller");
        });

        it("reverts a seller refund the buyer has not consented to", async function () {
            const dealId = await openDeal();

            await expect(
                marketplace.connect(seller).refundDeal(dealId)
            ).to.be.revertedWith("Buyer has not requested a refund");
        });

        it("refunds on mutual agreement: buyer requests, seller executes", async function () {
            const dealId = await openDeal();
            const tokenId = (await marketplace.getDeal(dealId)).tokenId;

            await expect(marketplace.connect(buyer).requestRefund(dealId))
                .to.emit(marketplace, "RefundRequested")
                .withArgs(dealId, buyer.address);

            const buyerBalanceBefore = await ethers.provider.getBalance(buyer.address);

            await expect(marketplace.connect(seller).refundDeal(dealId))
                .to.emit(marketplace, "DealRefunded")
                .withArgs(dealId);

            expect(await voucher.ownerOf(tokenId)).to.equal(seller.address);
            expect(await ethers.provider.getBalance(buyer.address) - buyerBalanceBefore).to.equal(price);
        });

        it("reverts requestRefund by anyone but the buyer", async function () {
            const dealId = await openDeal();

            await expect(marketplace.connect(seller).requestRefund(dealId)).to.be.revertedWith("Only buyer");
        });

        // A refund request is consent to cancel, not a veto on payment: the
        // buyer can still change their mind and release.
        it("a refund request does not block the buyer from releasing", async function () {
            const dealId = await openDeal();
            await marketplace.connect(buyer).requestRefund(dealId);

            await expect(marketplace.connect(buyer).releaseDeal(dealId)).to.emit(marketplace, "DealReleased");
        });

        it("reverts refunding a released deal", async function () {
            const dealId = await openDeal();
            await marketplace.connect(buyer).releaseDeal(dealId);

            await expect(marketplace.connect(seller).refundDeal(dealId)).to.be.revertedWith("Not active");
        });

        it("a refunded token can be listed again", async function () {
            const dealId = await openDeal();
            const tokenId = (await marketplace.getDeal(dealId)).tokenId;
            await marketplace.connect(buyer).requestRefund(dealId);
            await marketplace.connect(seller).refundDeal(dealId);

            await voucher.connect(seller).approve(await marketplace.getAddress(), tokenId);
            await expect(marketplace.connect(seller).createListing("Item", price, tokenId))
                .to.emit(marketplace, "ListingCreated");
        });
    });

    // Finding 4: the collection was immutable, so replacing the token contract
    // meant redeploying this one and orphaning every listing and deal.
    describe("collections", function () {
        it("seeds the constructor collection as allowed", async function () {
            expect(await marketplace.allowedCollections(await voucher.getAddress())).to.equal(true);
        });

        it("refuses listings from a collection that is not allowed", async function () {
            const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
            const second = await Voucher.deploy();
            await second.waitForDeployment();
            await second.connect(seller).mintVoucher("Module", "desc");
            await second.connect(seller).approve(await marketplace.getAddress(), 1n);

            await expect(
                marketplace.connect(seller).createListingFor(await second.getAddress(), "desc", price, 1n)
            ).to.be.revertedWith("Collection not allowed");
        });

        it("accepts a new collection after the governor allows it, with no redeploy", async function () {
            const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
            const second = await Voucher.deploy();
            await second.waitForDeployment();
            const secondAddress = await second.getAddress();

            await expect(marketplace.connect(seller).setCollectionAllowed(secondAddress, true))
                .to.emit(marketplace, "CollectionAllowed")
                .withArgs(secondAddress, true);

            await second.connect(seller).mintVoucher("Module", "desc");
            await second.connect(seller).approve(await marketplace.getAddress(), 1n);
            await marketplace.connect(seller).createListingFor(secondAddress, "desc", price, 1n);

            const listing = await marketplace.listings(1);
            expect(listing.collection).to.equal(secondAddress);
        });

        it("settles a deal against the collection it was opened on", async function () {
            const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
            const second = await Voucher.deploy();
            await second.waitForDeployment();
            const secondAddress = await second.getAddress();

            await marketplace.connect(seller).setCollectionAllowed(secondAddress, true);
            await second.connect(seller).mintVoucher("Module", "desc");
            await second.connect(seller).approve(await marketplace.getAddress(), 1n);
            await marketplace.connect(seller).createListingFor(secondAddress, "desc", price, 1n);
            await marketplace.connect(buyer).buy(1, { value: price });

            // Disallowing the collection must not strand a deal already open.
            await marketplace.connect(seller).setCollectionAllowed(secondAddress, false);
            await marketplace.connect(buyer).releaseDeal(1);

            expect(await second.ownerOf(1n)).to.equal(buyer.address);
        });

        it("tracks active listings per collection, not per token id", async function () {
            const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
            const second = await Voucher.deploy();
            await second.waitForDeployment();
            const secondAddress = await second.getAddress();
            await marketplace.connect(seller).setCollectionAllowed(secondAddress, true);

            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);

            // Same token id, different contract — a distinct asset.
            await second.connect(seller).mintVoucher("Module", "desc");
            await second.connect(seller).approve(await marketplace.getAddress(), tokenId);
            await expect(
                marketplace.connect(seller).createListingFor(secondAddress, "desc", price, tokenId)
            ).to.emit(marketplace, "ListingCreated");
        });

        it("reverts governance calls from a non-governor", async function () {
            await expect(
                marketplace.connect(other).setCollectionAllowed(other.address, true)
            ).to.be.revertedWith("Not the governor");
            await expect(
                marketplace.connect(other).transferGovernor(other.address)
            ).to.be.revertedWith("Not the governor");
        });
    });

    describe("views", function () {
        it("getActiveListings excludes listings that have been bought", async function () {
            const tokenIdA = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item A", price, tokenIdA);

            const tokenIdB = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item B", price, tokenIdB);

            await marketplace.connect(buyer).buy(1, { value: price });

            const [result, ids] = await marketplace.getActiveListings();
            expect(result.length).to.equal(1);
            expect(ids[0]).to.equal(2n);
            expect(result[0].description).to.equal("Item B");
        });

        it("getActiveListings excludes cancelled listings", async function () {
            const tokenId = await mintAndApprove();
            await marketplace.connect(seller).createListing("Item", price, tokenId);
            await marketplace.connect(seller).cancelListing(1);

            const [result] = await marketplace.getActiveListings();
            expect(result.length).to.equal(0);
        });

        it("getActiveListingsPaged walks the catalog in windows", async function () {
            for (let i = 0; i < 3; i++) {
                const tokenId = await mintAndApprove();
                await marketplace.connect(seller).createListing(`Item ${i}`, price, tokenId);
            }
            await marketplace.connect(seller).cancelListing(2);

            expect(await marketplace.listingCount()).to.equal(3n);

            const [first, firstIds, nextOffset] = await marketplace.getActiveListingsPaged(0, 2);
            expect(firstIds.map(Number)).to.deep.equal([1]);
            expect(first[0].description).to.equal("Item 0");
            expect(nextOffset).to.equal(2n);

            const [second, secondIds] = await marketplace.getActiveListingsPaged(nextOffset, 2);
            expect(secondIds.map(Number)).to.deep.equal([3]);
            expect(second[0].description).to.equal("Item 2");
        });

        it("getActiveListingsPaged returns empty past the end", async function () {
            const [result, ids, nextOffset] = await marketplace.getActiveListingsPaged(10, 5);
            expect(result.length).to.equal(0);
            expect(ids.length).to.equal(0);
            expect(nextOffset).to.equal(0n);
        });
    });
});
