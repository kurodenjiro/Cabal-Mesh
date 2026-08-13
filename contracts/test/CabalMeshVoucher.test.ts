import { expect } from "chai";
import { ethers } from "hardhat";
import { CabalMeshVoucher } from "../typechain-types";
import { HardhatEthersSigner } from "@nomicfoundation/hardhat-ethers/signers";

describe("CabalMeshVoucher", function () {
    let voucher: CabalMeshVoucher;
    let rewards: HardhatEthersSigner;
    let recipient: HardhatEthersSigner;
    let other: HardhatEthersSigner;

    // `rewards` stands in for the real `RelayRewards` contract in these
    // tests — an EOA signer is enough to prove the access-control boundary
    // without deploying the whole reward mechanism; RelayRewards.test.ts
    // covers the real contract-to-contract call.
    beforeEach(async function () {
        [rewards, recipient, other] = await ethers.getSigners();
        const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
        voucher = await Voucher.deploy(rewards.address);
        await voucher.waitForDeployment();
    });

    it("mints a voucher to the named recipient when called by the rewards contract", async function () {
        await expect(
            voucher
                .connect(rewards)
                .mintVoucher(recipient.address, "AI Compute Credit", "1 hour Ollama compute", 0, 0, 0)
        )
            .to.emit(voucher, "VoucherMinted")
            .withArgs(1, recipient.address, "AI Compute Credit", "1 hour Ollama compute", 0, 0, 0);

        expect(await voucher.ownerOf(1)).to.equal(recipient.address);
        const data = await voucher.vouchers(1);
        expect(data.voucherType).to.equal("AI Compute Credit");
        expect(data.description).to.equal("1 hour Ollama compute");
        expect(data.mintedBy).to.equal(recipient.address);
    });

    it("stores slot, rarity and effect for a module", async function () {
        await voucher.connect(rewards).mintVoucher(recipient.address, "Relay Amplifier MK-II", "RADIO module", 0, 2, 1800);

        const data = await voucher.vouchers(1);
        expect(data.slot).to.equal(0);
        expect(data.rarity).to.equal(2);
        expect(data.effectBps).to.equal(1800);
    });

    it("reverts minting from anyone other than the rewards contract", async function () {
        // This is the fix for the vulnerability decision 0 in
        // docs/intent-chat-and-modules-design.md records: the original
        // contract let any caller mint any voucher to themselves, for free.
        await expect(
            voucher.connect(recipient).mintVoucher(recipient.address, "Free Module", "self-minted", 0, 3, 9999)
        ).to.be.revertedWith("Only the rewards contract may mint");

        await expect(
            voucher.connect(other).mintVoucher(recipient.address, "Free Module", "self-minted", 0, 3, 9999)
        ).to.be.revertedWith("Only the rewards contract may mint");
    });

    it("reverts deployment with no rewards contract address", async function () {
        const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
        await expect(Voucher.deploy(ethers.ZeroAddress)).to.be.revertedWith("Rewards contract required");
    });

    it("reverts minting with an empty voucher type", async function () {
        await expect(
            voucher.connect(rewards).mintVoucher(recipient.address, "", "some description", 0, 0, 0)
        ).to.be.revertedWith("Voucher type required");
    });

    it("reverts minting to the zero address", async function () {
        await expect(
            voucher.connect(rewards).mintVoucher(ethers.ZeroAddress, "AI Compute Credit", "desc", 0, 0, 0)
        ).to.be.revertedWith("Recipient required");
    });

    it("allows the owner to redeem (burn) their voucher", async function () {
        await voucher.connect(rewards).mintVoucher(recipient.address, "Relay Bandwidth Credit", "500MB", 0, 0, 0);

        await expect(voucher.connect(recipient).redeemVoucher(1))
            .to.emit(voucher, "VoucherRedeemed")
            .withArgs(1, recipient.address, "Relay Bandwidth Credit");

        await expect(voucher.ownerOf(1)).to.be.reverted;
    });

    it("reverts redemption by a non-owner", async function () {
        await voucher.connect(rewards).mintVoucher(recipient.address, "Relay Bandwidth Credit", "500MB", 0, 0, 0);

        await expect(voucher.connect(other).redeemVoucher(1)).to.be.revertedWith("Not the owner");
    });

    it("reverts redeeming the same voucher twice", async function () {
        await voucher.connect(rewards).mintVoucher(recipient.address, "Relay Bandwidth Credit", "500MB", 0, 0, 0);
        await voucher.connect(recipient).redeemVoucher(1);

        await expect(voucher.connect(recipient).redeemVoucher(1)).to.be.reverted;
    });
});
