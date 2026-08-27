import { expect } from "chai";
import { ethers } from "hardhat";
import { time } from "@nomicfoundation/hardhat-network-helpers";

describe("CabalStandingRegistry", function () {
  async function deployFixture() {
    const [admin, source, corrector, seller, otherSeller, other, secondSource] =
      await ethers.getSigners();
    const Registry = await ethers.getContractFactory("CabalStandingRegistry");
    const registry = await Registry.deploy(admin.address, source.address, corrector.address);
    await registry.waitForDeployment();
    return { registry, admin, source, corrector, seller, otherSeller, other, secondSource };
  }

  const settlementId = (label: string) => ethers.id(`settlement:${label}`);
  const evidenceHash = (label: string) => ethers.id(`evidence:${label}`);
  const reasonHash = (label: string) => ethers.id(`reversal:${label}`);

  async function credit(
    registry: any,
    source: any,
    seller: string,
    label: string,
  ) {
    const sourceId = settlementId(label);
    const recordId = await registry.recordIdFor(source.address, sourceId);
    await registry.connect(source).recordSettlement(sourceId, seller, evidenceHash(label));
    return { sourceId, recordId };
  }

  it("keeps deployed bytecode below the EVM contract-size limit", async function () {
    const { registry } = await deployFixture();
    const runtimeCode = await ethers.provider.getCode(await registry.getAddress());
    expect((runtimeCode.length - 2) / 2).to.be.lessThanOrEqual(24_576);
  });

  it("separates delayed administration, settlement sources, and correction authority", async function () {
    const { registry, admin, source, corrector, seller } = await deployFixture();
    expect(await registry.defaultAdmin()).to.equal(admin.address);
    expect(await registry.defaultAdminDelay()).to.equal(2 * 24 * 60 * 60);
    expect(await registry.hasRole(await registry.SOURCE_ROLE(), source.address)).to.equal(true);
    expect(await registry.hasRole(await registry.CORRECTOR_ROLE(), corrector.address)).to.equal(true);
    expect(await registry.hasRole(await registry.SOURCE_ROLE(), seller.address)).to.equal(false);
  });

  it("enforces the two-step delay before a new default admin can accept", async function () {
    const { registry, admin, other } = await deployFixture();
    await registry.connect(admin).beginDefaultAdminTransfer(other.address);
    const [, acceptSchedule] = await registry.pendingDefaultAdmin();
    await expect(registry.connect(other).acceptDefaultAdminTransfer())
      .to.be.revertedWithCustomError(registry, "AccessControlEnforcedDefaultAdminDelay")
      .withArgs(acceptSchedule);
    await time.increase(2 * 24 * 60 * 60);
    await registry.connect(other).acceptDefaultAdminTransfer();
    expect(await registry.defaultAdmin()).to.equal(other.address);
  });

  it("credits a completed settlement and exposes independently derivable evidence", async function () {
    const { registry, source, seller } = await deployFixture();
    const sourceId = settlementId("complete-1");
    const evidence = evidenceHash("complete-1");
    const recordId = await registry.recordIdFor(source.address, sourceId);

    const tx = await registry.connect(source).recordSettlement(sourceId, seller.address, evidence);
    const receipt = await tx.wait();
    await expect(tx)
      .to.emit(registry, "StandingCredited")
      .withArgs(recordId, source.address, seller.address, sourceId, evidence, 1);

    expect(await registry.standingOf(seller.address)).to.deep.equal([1n, BigInt(receipt.blockNumber)]);
    const record = await registry.settlementRecord(recordId);
    expect(record.source).to.equal(source.address);
    expect(record.seller).to.equal(seller.address);
    expect(record.sourceSettlementId).to.equal(sourceId);
    expect(record.evidenceHash).to.equal(evidence);
    expect(record.recordedAtBlock).to.equal(receipt.blockNumber);
    expect(record.reversedAtBlock).to.equal(0);
    expect(record.reversalReasonHash).to.equal(ethers.ZeroHash);
    expect(record.active).to.equal(true);
  });

  it("rejects credits from an untrusted wallet", async function () {
    const { registry, other, seller } = await deployFixture();
    const role = await registry.SOURCE_ROLE();
    await expect(
      registry.connect(other).recordSettlement(
        settlementId("forged"),
        seller.address,
        evidenceHash("forged"),
      ),
    )
      .to.be.revertedWithCustomError(registry, "AccessControlUnauthorizedAccount")
      .withArgs(other.address, role);
  });

  it("rejects zero seller, source ID, and evidence commitments", async function () {
    const { registry, source, seller } = await deployFixture();
    await expect(
      registry.connect(source).recordSettlement(
        settlementId("zero-seller"),
        ethers.ZeroAddress,
        evidenceHash("zero-seller"),
      ),
    ).to.be.revertedWithCustomError(registry, "ZeroSeller");
    await expect(
      registry.connect(source).recordSettlement(ethers.ZeroHash, seller.address, evidenceHash("id")),
    ).to.be.revertedWithCustomError(registry, "InvalidSourceSettlementId");
    await expect(
      registry.connect(source).recordSettlement(settlementId("evidence"), seller.address, ethers.ZeroHash),
    ).to.be.revertedWithCustomError(registry, "InvalidEvidenceHash");
  });

  it("counts a source settlement exactly once", async function () {
    const { registry, source, seller } = await deployFixture();
    const { sourceId, recordId } = await credit(registry, source, seller.address, "once");
    await expect(
      registry.connect(source).recordSettlement(sourceId, seller.address, evidenceHash("once")),
    ).to.be.revertedWithCustomError(registry, "DuplicateSettlement").withArgs(recordId);
    expect((await registry.standingOf(seller.address))[0]).to.equal(1);
  });

  it("namespaces identical source IDs across independent authorized sources", async function () {
    const { registry, admin, source, secondSource, seller } = await deployFixture();
    await registry.connect(admin).grantRole(await registry.SOURCE_ROLE(), secondSource.address);
    const sourceId = settlementId("shared-id");
    const firstId = await registry.recordIdFor(source.address, sourceId);
    const secondId = await registry.recordIdFor(secondSource.address, sourceId);
    expect(firstId).not.to.equal(secondId);

    await registry.connect(source).recordSettlement(sourceId, seller.address, evidenceHash("first"));
    await registry.connect(secondSource).recordSettlement(sourceId, seller.address, evidenceHash("second"));
    expect((await registry.standingOf(seller.address))[0]).to.equal(2);
  });

  it("keeps seller wallets isolated", async function () {
    const { registry, source, seller, otherSeller } = await deployFixture();
    await credit(registry, source, seller.address, "seller-a-1");
    await credit(registry, source, seller.address, "seller-a-2");
    await credit(registry, source, otherSeller.address, "seller-b-1");
    expect((await registry.standingOf(seller.address))[0]).to.equal(2);
    expect((await registry.standingOf(otherSeller.address))[0]).to.equal(1);
  });

  it("lets the active original source remove a reversed settlement exactly once", async function () {
    const { registry, source, seller } = await deployFixture();
    const { recordId } = await credit(registry, source, seller.address, "reversed");
    const reason = reasonHash("charge-refund");
    const tx = await registry.connect(source).reverseSettlement(recordId, reason);
    const receipt = await tx.wait();
    await expect(tx)
      .to.emit(registry, "StandingReversed")
      .withArgs(recordId, seller.address, source.address, reason, 0);

    expect(await registry.standingOf(seller.address)).to.deep.equal([0n, BigInt(receipt.blockNumber)]);
    const record = await registry.settlementRecord(recordId);
    expect(record.active).to.equal(false);
    expect(record.reversedAtBlock).to.equal(receipt.blockNumber);
    expect(record.reversalReasonHash).to.equal(reason);
    await expect(registry.connect(source).reverseSettlement(recordId, reason))
      .to.be.revertedWithCustomError(registry, "SettlementAlreadyReversed")
      .withArgs(recordId);
  });

  it("lets an independent corrector apply an authoritative refund", async function () {
    const { registry, source, corrector, seller } = await deployFixture();
    const { recordId } = await credit(registry, source, seller.address, "refund");
    await expect(registry.connect(corrector).reverseSettlement(recordId, reasonHash("refund")))
      .to.emit(registry, "StandingReversed")
      .withArgs(recordId, seller.address, corrector.address, reasonHash("refund"), 0);
  });

  it("rejects unknown records, empty reasons, and unauthorized reversals", async function () {
    const { registry, source, seller, other } = await deployFixture();
    const unknown = ethers.id("unknown-record");
    await expect(registry.connect(source).reverseSettlement(unknown, reasonHash("unknown")))
      .to.be.revertedWithCustomError(registry, "UnknownSettlement")
      .withArgs(unknown);

    const { recordId } = await credit(registry, source, seller.address, "protected");
    await expect(registry.connect(source).reverseSettlement(recordId, ethers.ZeroHash))
      .to.be.revertedWithCustomError(registry, "InvalidReversalReason");
    await expect(registry.connect(other).reverseSettlement(recordId, reasonHash("forged")))
      .to.be.revertedWithCustomError(registry, "UnauthorizedReversal")
      .withArgs(other.address, recordId);
  });

  it("requires the original source to remain authorized but preserves correction liveness", async function () {
    const { registry, admin, source, corrector, seller } = await deployFixture();
    const { recordId } = await credit(registry, source, seller.address, "compromised-source");
    await registry.connect(admin).revokeRole(await registry.SOURCE_ROLE(), source.address);

    await expect(registry.connect(source).reverseSettlement(recordId, reasonHash("source")))
      .to.be.revertedWithCustomError(registry, "UnauthorizedReversal")
      .withArgs(source.address, recordId);
    await registry.connect(corrector).reverseSettlement(recordId, reasonHash("corrector"));
    expect((await registry.standingOf(seller.address))[0]).to.equal(0);
  });

  it("retains immutable credit evidence after a correction", async function () {
    const { registry, source, corrector, seller } = await deployFixture();
    const sourceId = settlementId("history");
    const evidence = evidenceHash("history");
    const recordId = await registry.recordIdFor(source.address, sourceId);
    await registry.connect(source).recordSettlement(sourceId, seller.address, evidence);
    const before = await registry.settlementRecord(recordId);
    await registry.connect(corrector).reverseSettlement(recordId, reasonHash("history"));
    const after = await registry.settlementRecord(recordId);

    expect(after.source).to.equal(before.source);
    expect(after.seller).to.equal(before.seller);
    expect(after.sourceSettlementId).to.equal(before.sourceSettlementId);
    expect(after.evidenceHash).to.equal(before.evidenceHash);
    expect(after.recordedAtBlock).to.equal(before.recordedAtBlock);
    expect(after.active).to.equal(false);
  });

  it("rejects zero source and correction authority at deployment", async function () {
    const [admin, source] = await ethers.getSigners();
    const Registry = await ethers.getContractFactory("CabalStandingRegistry");
    await expect(Registry.deploy(admin.address, ethers.ZeroAddress, source.address))
      .to.be.revertedWithCustomError(Registry, "ZeroSource");
    await expect(Registry.deploy(admin.address, source.address, ethers.ZeroAddress))
      .to.be.revertedWithCustomError(Registry, "ZeroCorrector");
  });
});
