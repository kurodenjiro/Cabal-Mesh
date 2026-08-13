import { expect } from "chai";
import { ethers } from "hardhat";
import { Marketplace, CabalMeshVoucher } from "../typechain-types";
import { HardhatEthersSigner } from "@nomicfoundation/hardhat-ethers/signers";

describe("Marketplace", function () {
    let marketplace: Marketplace;
    let voucher: CabalMeshVoucher;
    // Stands in for the real `RelayRewards` contract — see
    // CabalMeshVoucher.test.ts for why an EOA signer is enough here.
    let rewards: HardhatEthersSigner;
    let seller: HardhatEthersSigner;
    let buyer: HardhatEthersSigner;
    let other: HardhatEthersSigner;
    const price = ethers.parseEther("1.0");

    async function mintAndApprove(): Promise<bigint> {
        await voucher.connect(rewards).mintVoucher(seller.address, "AI Compute Credit", "1 hour Ollama compute", 0, 0, 0);
        const tokenId = 1n;
        await voucher.connect(seller).approve(await marketplace.getAddress(), tokenId);
        return tokenId;
    }

    beforeEach(async function () {
        [rewards, seller, buyer, other] = await ethers.getSigners();

        const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
        voucher = await Voucher.deploy(rewards.address);
        await voucher.waitForDeployment();

        const Marketplace = await ethers.getContractFactory("Marketplace");
        marketplace = await Marketplace.deploy(await voucher.getAddress());
        await marketplace.waitForDeployment();
    });

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
    });

    it("reverts listing a voucher the caller doesn't own", async function () {
        await voucher.connect(rewards).mintVoucher(seller.address, "AI Compute Credit", "desc", 0, 0, 0);
        await voucher.connect(seller).approve(await marketplace.getAddress(), 1n);

        await expect(
            marketplace.connect(other).createListing("desc", price, 1n)
        ).to.be.revertedWith("Not the voucher owner");
    });

    it("reverts listing a voucher not yet approved for the marketplace", async function () {
        await voucher.connect(rewards).mintVoucher(seller.address, "AI Compute Credit", "desc", 0, 0, 0);

        await expect(
            marketplace.connect(seller).createListing("desc", price, 1n)
        ).to.be.revertedWith("Approve marketplace first");
    });

    it("reverts on zero price", async function () {
        const tokenId = await mintAndApprove();
        await expect(
            marketplace.connect(seller).createListing("Item", 0, tokenId)
        ).to.be.revertedWith("Price must be > 0");
    });

    it("buy() atomically locks AVAX and pulls the NFT into escrow", async function () {
        const tokenId = await mintAndApprove();
        await marketplace.connect(seller).createListing("Item", price, tokenId);

        await expect(marketplace.connect(buyer).buy(1, { value: price }))
            .to.emit(marketplace, "DealCreated")
            .withArgs(1, 1, buyer.address, tokenId, price);

        expect(await voucher.ownerOf(tokenId)).to.equal(await marketplace.getAddress());
        const listing = await marketplace.listings(1);
        expect(listing.active).to.equal(false);
    });

    it("reverts buy() with the wrong AVAX amount", async function () {
        const tokenId = await mintAndApprove();
        await marketplace.connect(seller).createListing("Item", price, tokenId);

        await expect(
            marketplace.connect(buyer).buy(1, { value: ethers.parseEther("0.5") })
        ).to.be.revertedWith("Wrong amount");
    });

    it("releaseDeal pays the seller and transfers the NFT to the buyer", async function () {
        const tokenId = await mintAndApprove();
        await marketplace.connect(seller).createListing("Item", price, tokenId);
        await marketplace.connect(buyer).buy(1, { value: price });

        const sellerBalanceBefore = await ethers.provider.getBalance(seller.address);

        await expect(marketplace.connect(buyer).releaseDeal(1))
            .to.emit(marketplace, "DealReleased")
            .withArgs(1);

        expect(await voucher.ownerOf(tokenId)).to.equal(buyer.address);
        const sellerBalanceAfter = await ethers.provider.getBalance(seller.address);
        expect(sellerBalanceAfter - sellerBalanceBefore).to.equal(price);
    });

    it("reverts releaseDeal by a non-buyer", async function () {
        const tokenId = await mintAndApprove();
        await marketplace.connect(seller).createListing("Item", price, tokenId);
        await marketplace.connect(buyer).buy(1, { value: price });

        await expect(
            marketplace.connect(seller).releaseDeal(1)
        ).to.be.revertedWith("Only buyer");
    });

    it("refundDeal returns AVAX to the buyer and the NFT to the seller", async function () {
        const tokenId = await mintAndApprove();
        await marketplace.connect(seller).createListing("Item", price, tokenId);
        await marketplace.connect(buyer).buy(1, { value: price });

        await expect(marketplace.connect(buyer).refundDeal(1))
            .to.emit(marketplace, "DealRefunded")
            .withArgs(1);

        expect(await voucher.ownerOf(tokenId)).to.equal(seller.address);
    });

    it("getActiveListings excludes listings that have been bought", async function () {
        const tokenIdA = await mintAndApprove();
        await marketplace.connect(seller).createListing("Item A", price, tokenIdA);

        await voucher.connect(rewards).mintVoucher(seller.address, "Relay Bandwidth Credit", "500MB", 0, 0, 0);
        const tokenIdB = 2n;
        await voucher.connect(seller).approve(await marketplace.getAddress(), tokenIdB);
        await marketplace.connect(seller).createListing("Item B", price, tokenIdB);

        await marketplace.connect(buyer).buy(1, { value: price });

        const [result, ids] = await marketplace.getActiveListings();
        expect(result.length).to.equal(1);
        expect(ids[0]).to.equal(2n);
        expect(result[0].description).to.equal("Item B");
    });
});
