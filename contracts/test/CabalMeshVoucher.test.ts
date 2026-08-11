import { expect } from "chai";
import { ethers } from "hardhat";
import { CabalMeshVoucher } from "../typechain-types";
import { HardhatEthersSigner } from "@nomicfoundation/hardhat-ethers/signers";

describe("CabalMeshVoucher", function () {
    let voucher: CabalMeshVoucher;
    let owner: HardhatEthersSigner;
    let other: HardhatEthersSigner;
    let third: HardhatEthersSigner;

    beforeEach(async function () {
        [owner, other, third] = await ethers.getSigners();
        const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
        voucher = await Voucher.deploy();
        await voucher.waitForDeployment();
    });

    it("mints a voucher to the caller", async function () {
        await expect(voucher.connect(owner).mintVoucher("AI Compute Credit", "1 hour Ollama compute"))
            .to.emit(voucher, "VoucherMinted")
            .withArgs(1, owner.address, "AI Compute Credit", "1 hour Ollama compute");

        expect(await voucher.ownerOf(1)).to.equal(owner.address);
        const data = await voucher.vouchers(1);
        expect(data.voucherType).to.equal("AI Compute Credit");
        expect(data.description).to.equal("1 hour Ollama compute");
        expect(data.mintedBy).to.equal(owner.address);
    });

    it("reverts minting with an empty voucher type", async function () {
        await expect(
            voucher.connect(owner).mintVoucher("", "some description")
        ).to.be.revertedWith("Voucher type required");
    });

    // Finding 4: mintVoucher was open to every wallet, so any holder could
    // mint themselves a reward-bearing token and pass every later ownership
    // check with it.
    describe("mint authority", function () {
        it("makes the deployer the issuer and a minter", async function () {
            expect(await voucher.issuer()).to.equal(owner.address);
            expect(await voucher.minters(owner.address)).to.equal(true);
            expect(await voucher.minters(other.address)).to.equal(false);
        });

        it("reverts a mint by an unauthorized wallet", async function () {
            await expect(
                voucher.connect(other).mintVoucher("Relay Amplifier", "+18% relay yield")
            ).to.be.revertedWith("Not an authorized minter");

            await expect(
                voucher.connect(other).mintTo(other.address, "Relay Amplifier", "+18% relay yield")
            ).to.be.revertedWith("Not an authorized minter");
        });

        it("lets the issuer grant and revoke minting", async function () {
            await expect(voucher.connect(owner).setMinter(other.address, true))
                .to.emit(voucher, "MinterSet")
                .withArgs(other.address, true);

            await expect(voucher.connect(other).mintVoucher("Module", "desc"))
                .to.emit(voucher, "VoucherMinted");

            await voucher.connect(owner).setMinter(other.address, false);
            await expect(
                voucher.connect(other).mintVoucher("Module", "desc")
            ).to.be.revertedWith("Not an authorized minter");
        });

        it("reverts minter changes from a non-issuer", async function () {
            await expect(
                voucher.connect(other).setMinter(other.address, true)
            ).to.be.revertedWith("Not the issuer");
        });

        it("mints to an earner without giving that earner mint rights", async function () {
            await expect(voucher.connect(owner).mintTo(other.address, "Standing", "settled 10 intents"))
                .to.emit(voucher, "VoucherMinted")
                .withArgs(1, other.address, "Standing", "settled 10 intents");

            expect(await voucher.ownerOf(1)).to.equal(other.address);
            // Provenance stays with the authority, not the recipient.
            expect((await voucher.vouchers(1)).mintedBy).to.equal(owner.address);
            expect(await voucher.minters(other.address)).to.equal(false);
        });

        it("reverts minting to the zero address", async function () {
            await expect(
                voucher.connect(owner).mintTo(ethers.ZeroAddress, "Module", "desc")
            ).to.be.revertedWith("Zero recipient");
        });

        it("transfers issuer authority", async function () {
            await expect(voucher.connect(owner).transferIssuer(third.address))
                .to.emit(voucher, "IssuerTransferred")
                .withArgs(owner.address, third.address);

            expect(await voucher.issuer()).to.equal(third.address);
            await expect(
                voucher.connect(owner).setMinter(other.address, true)
            ).to.be.revertedWith("Not the issuer");
            await expect(voucher.connect(third).setMinter(other.address, true)).to.emit(voucher, "MinterSet");
        });

        it("reverts transferring issuer authority to the zero address", async function () {
            await expect(
                voucher.connect(owner).transferIssuer(ethers.ZeroAddress)
            ).to.be.revertedWith("Zero issuer");
        });

        // Holding a token is still proof of possession — it is just no longer
        // proof that the token was legitimately issued.
        it("does not let a token holder mint", async function () {
            await voucher.connect(owner).mintTo(other.address, "Module", "desc");

            await expect(
                voucher.connect(other).mintVoucher("Module", "another one for me")
            ).to.be.revertedWith("Not an authorized minter");
        });
    });

    it("allows the owner to redeem (burn) their voucher", async function () {
        await voucher.connect(owner).mintVoucher("Relay Bandwidth Credit", "500MB");

        await expect(voucher.connect(owner).redeemVoucher(1))
            .to.emit(voucher, "VoucherRedeemed")
            .withArgs(1, owner.address, "Relay Bandwidth Credit");

        await expect(voucher.ownerOf(1)).to.be.reverted;
    });

    it("reverts redemption by a non-owner", async function () {
        await voucher.connect(owner).mintVoucher("Relay Bandwidth Credit", "500MB");

        await expect(
            voucher.connect(other).redeemVoucher(1)
        ).to.be.revertedWith("Not the owner");
    });

    it("reverts redeeming the same voucher twice", async function () {
        await voucher.connect(owner).mintVoucher("Relay Bandwidth Credit", "500MB");
        await voucher.connect(owner).redeemVoucher(1);

        await expect(
            voucher.connect(owner).redeemVoucher(1)
        ).to.be.reverted;
    });
});
