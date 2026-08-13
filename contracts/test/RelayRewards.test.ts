import { expect } from "chai";
import { ethers } from "hardhat";
import { RelayRewards, CabalMeshVoucher } from "../typechain-types";
import { HardhatEthersSigner } from "@nomicfoundation/hardhat-ethers/signers";

describe("RelayRewards", function () {
    let rewards: RelayRewards;
    let voucher: CabalMeshVoucher;
    let deployer: HardhatEthersSigner;
    let sender: HardhatEthersSigner;
    let gateway: HardhatEthersSigner;
    let other: HardhatEthersSigner;
    const fee = ethers.parseEther("0.01");

    // Deploys in the only order that works: RelayRewards first (its address
    // is what CabalMeshVoucher restricts minting to), then the voucher, then
    // one call wiring RelayRewards to it — see both contracts' doc comments
    // for why the circular reference has to be broken this way.
    beforeEach(async function () {
        [deployer, sender, gateway, other] = await ethers.getSigners();

        const Rewards = await ethers.getContractFactory("RelayRewards");
        rewards = await Rewards.deploy();
        await rewards.waitForDeployment();

        const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
        voucher = await Voucher.deploy(await rewards.getAddress());
        await voucher.waitForDeployment();

        await rewards.connect(deployer).setVoucher(await voucher.getAddress());
    });

    it("pays the named gateway the attached fee", async function () {
        const before = await ethers.provider.getBalance(gateway.address);

        await expect(rewards.connect(sender).recordGatewayRelay(gateway.address, { value: fee }))
            .to.emit(rewards, "GatewayRelayPaid")
            .withArgs(gateway.address, fee, fee);

        const after = await ethers.provider.getBalance(gateway.address);
        expect(after - before).to.equal(fee);
    });

    it("tracks cumulative relayed value per gateway", async function () {
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: fee });
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: fee });

        expect(await rewards.relayedWei(gateway.address)).to.equal(fee * 2n);
    });

    it("reverts with no fee attached", async function () {
        await expect(rewards.connect(sender).recordGatewayRelay(gateway.address)).to.be.revertedWith("No fee attached");
    });

    it("reverts with no gateway named", async function () {
        await expect(
            rewards.connect(sender).recordGatewayRelay(ethers.ZeroAddress, { value: fee })
        ).to.be.revertedWith("Gateway required");
    });

    it("mints a module once cumulative relayed value crosses the milestone", async function () {
        const milestone = await rewards.MILESTONE_WEI();

        // Just under the milestone: no module yet.
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: milestone - 1n });
        expect(await voucher.balanceOf(gateway.address)).to.equal(0);

        // The payment that crosses it mints exactly one module, to the
        // gateway, atomically with the payout.
        await expect(rewards.connect(sender).recordGatewayRelay(gateway.address, { value: 1n }))
            .to.emit(rewards, "GatewayMilestoneReached")
            .withArgs(gateway.address, 1n, 1n);

        expect(await voucher.balanceOf(gateway.address)).to.equal(1);
        expect(await voucher.ownerOf(1)).to.equal(gateway.address);

        const data = await voucher.vouchers(1);
        expect(data.voucherType).to.equal("Gateway License");
        expect(data.slot).to.equal(await rewards.SLOT_POWER());
        expect(data.effectBps).to.equal(await rewards.GATEWAY_MODULE_EFFECT_BPS());
    });

    it("does not mint a second module for the same milestone", async function () {
        const milestone = await rewards.MILESTONE_WEI();
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: milestone });
        expect(await voucher.balanceOf(gateway.address)).to.equal(1);

        // A tiny payment that doesn't reach the *next* milestone must not
        // mint again.
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: 1n });
        expect(await voucher.balanceOf(gateway.address)).to.equal(1);
    });

    it("mints a second module on crossing the second milestone", async function () {
        const milestone = await rewards.MILESTONE_WEI();
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: milestone * 2n });

        expect(await voucher.balanceOf(gateway.address)).to.equal(2);
        expect(await rewards.milestonesClaimed(gateway.address)).to.equal(2);
    });

    it("caps mints per call and finishes the rest on the next payment", async function () {
        const milestone = await rewards.MILESTONE_WEI();
        const cap = await rewards.MAX_MILESTONES_PER_CALL();

        // Cross far more milestones than the cap in one payment.
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: milestone * (cap + 5n) });
        expect(await voucher.balanceOf(gateway.address)).to.equal(cap);
        expect(await rewards.milestonesClaimed(gateway.address)).to.equal(cap);

        // Nothing was lost — the next payment picks up exactly where the
        // cap left off, even with no new value needed to cross a milestone.
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: 1n });
        expect(await voucher.balanceOf(gateway.address)).to.equal(cap + 5n);
    });

    it("keeps gateways' cumulative totals and milestones independent", async function () {
        const milestone = await rewards.MILESTONE_WEI();
        await rewards.connect(sender).recordGatewayRelay(gateway.address, { value: milestone });
        await rewards.connect(sender).recordGatewayRelay(other.address, { value: 1n });

        expect(await voucher.balanceOf(gateway.address)).to.equal(1);
        expect(await voucher.balanceOf(other.address)).to.equal(0);
        expect(await rewards.relayedWei(other.address)).to.equal(1n);
    });

    describe("setVoucher", function () {
        it("reverts being called a second time", async function () {
            await expect(rewards.connect(deployer).setVoucher(await voucher.getAddress())).to.be.revertedWith(
                "Voucher already set"
            );
        });

        it("reverts when called by anyone other than the deployer", async function () {
            const Rewards = await ethers.getContractFactory("RelayRewards");
            const fresh = await Rewards.deploy();
            await fresh.waitForDeployment();

            await expect(fresh.connect(other).setVoucher(await voucher.getAddress())).to.be.revertedWithCustomError(
                fresh,
                "OwnableUnauthorizedAccount"
            );
        });

        it("payments before the voucher is wired still pay the gateway, just without minting", async function () {
            const Rewards = await ethers.getContractFactory("RelayRewards");
            const fresh = await Rewards.deploy();
            await fresh.waitForDeployment();
            const milestone = await fresh.MILESTONE_WEI();

            await expect(fresh.connect(sender).recordGatewayRelay(gateway.address, { value: milestone })).to.not.be
                .reverted;
            expect(await fresh.relayedWei(gateway.address)).to.equal(milestone);
        });
    });
});
