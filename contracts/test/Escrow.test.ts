import { expect } from "chai";
import { ethers } from "hardhat";
import { time } from "@nomicfoundation/hardhat-toolbox/network-helpers";
import { Escrow } from "../typechain-types";
import { HardhatEthersSigner } from "@nomicfoundation/hardhat-ethers/signers";

describe("Escrow", function () {
  let escrow: Escrow;
  let depositor: HardhatEthersSigner;
  let payee: HardhatEthersSigner;
  let stranger: HardhatEthersSigner;

  beforeEach(async function () {
    [depositor, payee, stranger] = await ethers.getSigners();
    const Escrow = await ethers.getContractFactory("Escrow");
    escrow = await Escrow.deploy();
    await escrow.waitForDeployment();
  });

  it("creates an escrow and releases funds to the payee", async function () {
    const amount = ethers.parseEther("1.0");
    const tx = await escrow.connect(depositor).createEscrow(payee.address, 0, { value: amount });
    await tx.wait();

    const before = await ethers.provider.getBalance(payee.address);
    await expect(escrow.connect(depositor).release(1))
      .to.emit(escrow, "EscrowReleased")
      .withArgs(1, payee.address, amount);
    const after = await ethers.provider.getBalance(payee.address);

    expect(after - before).to.equal(amount);
    const deal = await escrow.getDeal(1);
    expect(deal.status).to.equal(2); // Released
  });

  it("allows the depositor to refund before release", async function () {
    const amount = ethers.parseEther("0.5");
    await escrow.connect(depositor).createEscrow(payee.address, 0, { value: amount });

    const before = await ethers.provider.getBalance(depositor.address);
    const tx = await escrow.connect(depositor).refund(1);
    const receipt = await tx.wait();
    const gasCost = receipt!.gasUsed * receipt!.gasPrice;
    const after = await ethers.provider.getBalance(depositor.address);

    expect(after - before + gasCost).to.equal(amount);
  });

  it("allows a third party to refund after expiry", async function () {
    const amount = ethers.parseEther("0.2");
    const expiry = (await time.latest()) + 60;
    await escrow.connect(depositor).createEscrow(payee.address, expiry, { value: amount });

    await expect(escrow.connect(stranger).refund(1)).to.be.revertedWith("Not authorized or not expired");

    await time.increaseTo(expiry + 1);
    await expect(escrow.connect(stranger).refund(1)).to.emit(escrow, "EscrowRefunded");
  });

  it("reverts release by a non-depositor", async function () {
    await escrow.connect(depositor).createEscrow(payee.address, 0, { value: ethers.parseEther("0.1") });
    await expect(escrow.connect(stranger).release(1)).to.be.revertedWith("Only depositor");
  });

  it("reverts a double release", async function () {
    await escrow.connect(depositor).createEscrow(payee.address, 0, { value: ethers.parseEther("0.1") });
    await escrow.connect(depositor).release(1);
    await expect(escrow.connect(depositor).release(1)).to.be.revertedWith("Not active");
  });

  it("reverts creating an escrow with zero value", async function () {
    await expect(
      escrow.connect(depositor).createEscrow(payee.address, 0, { value: 0 })
    ).to.be.revertedWith("No funds sent");
  });
});
