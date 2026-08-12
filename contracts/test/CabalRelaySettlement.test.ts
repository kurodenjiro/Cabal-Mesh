import { expect } from "chai";
import { ethers } from "hardhat";
import { time } from "@nomicfoundation/hardhat-network-helpers";
import { CabalRelaySettlement } from "../typechain-types";
import { HardhatEthersSigner } from "@nomicfoundation/hardhat-ethers/signers";

const AUTHORIZATION_TYPES = {
  RelayAuthorization: [
    { name: "policyHash", type: "bytes32" },
    { name: "routeNonce", type: "bytes32" },
    { name: "payloadCommitment", type: "bytes32" },
    { name: "deliveryMode", type: "uint8" },
    { name: "relayRouteHash", type: "bytes32" },
    { name: "sender", type: "address" },
    { name: "recipient", type: "address" },
    { name: "authorizedBytes", type: "uint64" },
    { name: "relayCount", type: "uint8" },
    { name: "maximumChargeNavax", type: "uint64" },
    { name: "issuedAt", type: "uint64" },
    { name: "expiresAt", type: "uint64" },
  ],
};

const CONTRIBUTION_TYPES = {
  RelayContribution: [
    { name: "authorizationHash", type: "bytes32" },
    { name: "hopIndex", type: "uint8" },
    { name: "relayer", type: "address" },
    { name: "ingress", type: "address" },
    { name: "egress", type: "address" },
    { name: "payloadCommitment", type: "bytes32" },
    { name: "deliveredBytes", type: "uint64" },
    { name: "forwardedAt", type: "uint64" },
  ],
};

const ACKNOWLEDGEMENT_TYPES = {
  RecipientAcknowledgement: [
    { name: "authorizationHash", type: "bytes32" },
    { name: "contributionsHash", type: "bytes32" },
    { name: "recipient", type: "address" },
    { name: "payloadCommitment", type: "bytes32" },
    { name: "deliveredBytes", type: "uint64" },
    { name: "receivedAt", type: "uint64" },
  ],
};

const PAYLOAD = ethers.toUtf8Bytes("cabalmesh encrypted intent payload test vector v1");
const AUTHORIZED_BYTES = 100_000n;
const MAXIMUM_CHARGE_NAVAX = 2_200_000n;
const WEI_PER_NAVAX = 1_000_000_000n;

type Fixture = Awaited<ReturnType<typeof deployFixture>>;

async function deployFixture() {
  const [sender, relayer, recipient, executor, outsider] = await ethers.getSigners();
  const Settlement = await ethers.getContractFactory("CabalRelaySettlement");
  const settlement = await Settlement.deploy();
  await settlement.waitForDeployment();
  const network = await ethers.provider.getNetwork();
  const domain = {
    name: "CabalMesh Relay Proof",
    version: "1",
    chainId: network.chainId,
    verifyingContract: await settlement.getAddress(),
  };
  return { settlement, sender, relayer, recipient, executor, outsider, domain };
}

function payloadCommitment(payload: Uint8Array): string {
  return ethers.keccak256(
    ethers.concat([ethers.toUtf8Bytes("CABAL_PAYLOAD_V1\0"), payload]),
  );
}

function relayRouteHash(relayers: string[]): string {
  return ethers.keccak256(
    ethers.concat([
      ethers.toUtf8Bytes("CABAL_RELAY_ROUTE_V1\0"),
      new Uint8Array([relayers.length]),
      ...relayers.map(ethers.getBytes),
    ]),
  );
}

function orderedContributionsHash(ids: string[]): string {
  return ethers.keccak256(
    ethers.concat([
      ethers.toUtf8Bytes("CABAL_CONTRIBUTIONS_V1\0"),
      new Uint8Array([ids.length]),
      ...ids.map(ethers.getBytes),
    ]),
  );
}

async function signedProof(fixture: Fixture, overrides: {
  sender?: HardhatEthersSigner;
  relayer?: HardhatEthersSigner;
  recipient?: HardhatEthersSigner;
  payloadCommitment?: string;
  contributionPayloadCommitment?: string;
  contributionSigner?: HardhatEthersSigner;
  acknowledgementSigner?: HardhatEthersSigner;
  issuedAt?: bigint;
  expiresAt?: bigint;
} = {}) {
  const sender = overrides.sender ?? fixture.sender;
  const relayer = overrides.relayer ?? fixture.relayer;
  const recipient = overrides.recipient ?? fixture.recipient;
  const now = BigInt(await time.latest());
  const issuedAt = overrides.issuedAt ?? now;
  const expiresAt = overrides.expiresAt ?? issuedAt + 600n;
  const commitment = overrides.payloadCommitment ?? payloadCommitment(PAYLOAD);
  const authorization = {
    policyHash: ethers.keccak256(ethers.toUtf8Bytes("cabal-rewards-v1")),
    routeNonce: ethers.hexlify(new Uint8Array(32).fill(0x42)),
    payloadCommitment: commitment,
    deliveryMode: 0,
    relayRouteHash: relayRouteHash([relayer.address]),
    sender: sender.address,
    recipient: recipient.address,
    authorizedBytes: AUTHORIZED_BYTES,
    relayCount: 1,
    maximumChargeNavax: MAXIMUM_CHARGE_NAVAX,
    issuedAt,
    expiresAt,
  };
  const senderSignature = await sender.signTypedData(
    fixture.domain,
    AUTHORIZATION_TYPES,
    authorization,
  );
  const routeId = ethers.TypedDataEncoder.hash(
    fixture.domain,
    AUTHORIZATION_TYPES,
    authorization,
  );
  const contribution = {
    authorizationHash: routeId,
    hopIndex: 0,
    relayer: relayer.address,
    ingress: sender.address,
    egress: recipient.address,
    payloadCommitment: overrides.contributionPayloadCommitment ?? commitment,
    deliveredBytes: AUTHORIZED_BYTES,
    forwardedAt: issuedAt,
  };
  const contributionSignature = await (
    overrides.contributionSigner ?? relayer
  ).signTypedData(fixture.domain, CONTRIBUTION_TYPES, contribution);
  const contributionId = ethers.TypedDataEncoder.hash(
    fixture.domain,
    CONTRIBUTION_TYPES,
    contribution,
  );
  const acknowledgement = {
    authorizationHash: routeId,
    contributionsHash: orderedContributionsHash([contributionId]),
    recipient: recipient.address,
    payloadCommitment: commitment,
    deliveredBytes: AUTHORIZED_BYTES,
    receivedAt: issuedAt,
  };
  const acknowledgementSignature = await (
    overrides.acknowledgementSigner ?? recipient
  ).signTypedData(fixture.domain, ACKNOWLEDGEMENT_TYPES, acknowledgement);

  return {
    authorization,
    relayers: [relayer.address],
    senderSignature,
    contributions: [contribution],
    contributionSignatures: [contributionSignature],
    acknowledgement,
    acknowledgementSignature,
    routeId,
    contributionId,
  };
}

async function fund(fixture: Fixture, proof: Awaited<ReturnType<typeof signedProof>>) {
  return fixture.settlement.connect(fixture.sender).fundRoute(
    proof.authorization,
    proof.relayers,
    proof.senderSignature,
    { value: MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX },
  );
}

describe("CabalRelaySettlement", function () {
  it("quotes and requires the exact maximum before accepting a paid route", async function () {
    const fixture = await deployFixture();
    const proof = await signedProof(fixture);
    const quote = await fixture.settlement.quote(AUTHORIZED_BYTES, 1);

    expect(quote.billedBytes).to.equal(131_072n);
    expect(quote.baseRouteRewardNavax).to.equal(100_000n);
    expect(quote.maximumWorkNavax).to.equal(200_000n);
    expect(quote.settlementGasCapNavax).to.equal(2_000_000n);
    expect(quote.maximumChargeNavax).to.equal(MAXIMUM_CHARGE_NAVAX);

    await expect(
      fixture.settlement.connect(fixture.sender).fundRoute(
        proof.authorization,
        proof.relayers,
        proof.senderSignature,
        { value: MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX - 1n },
      ),
    ).to.be.revertedWith("Wrong escrow amount");

    await expect(fund(fixture, proof))
      .to.emit(fixture.settlement, "RouteFunded")
      .withArgs(
        proof.routeId,
        fixture.sender.address,
        fixture.recipient.address,
        MAXIMUM_CHARGE_NAVAX,
        proof.authorization.expiresAt,
      );
    expect(await fixture.settlement.activeLiabilityWei()).to.equal(
      MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX,
    );
    expect(await fixture.settlement.settledRelayEarningsNavax(fixture.relayer.address)).to.equal(0);
    expect(await fixture.settlement.solvent()).to.equal(true);
  });

  it("settles one genuine sender-relayer-recipient proof exactly once and records a transaction", async function () {
    const fixture = await deployFixture();
    const proof = await signedProof(fixture);
    expect(await fixture.settlement.authorizationHash(proof.authorization)).to.equal(proof.routeId);
    expect(await fixture.settlement.relayRouteHash(proof.relayers)).to.equal(
      proof.authorization.relayRouteHash,
    );
    expect(await fixture.settlement.contributionHash(proof.contributions[0])).to.equal(
      proof.contributionId,
    );
    expect(await fixture.settlement.orderedContributionsHash([proof.contributionId])).to.equal(
      proof.acknowledgement.contributionsHash,
    );
    await fund(fixture, proof);

    const transaction = await fixture.settlement.connect(fixture.executor).settle(proof);
    expect(transaction.hash).to.match(/^0x[0-9a-f]{64}$/);
    const receipt = await transaction.wait();
    expect(receipt?.status).to.equal(1);
    const settledLog = receipt?.logs
      .map((log) => {
        try {
          return fixture.settlement.interface.parseLog(log);
        } catch {
          return null;
        }
      })
      .find((log) => log?.name === "RouteSettled");
    expect(settledLog).not.to.equal(undefined);
    const settled = settledLog!.args;
    expect(settled.workPaidNavax).to.equal(100_000n);
    expect(settled.executorPaidNavax).to.be.greaterThan(0n);
    expect(settled.executorPaidNavax).to.be.at.most(2_000_000n);
    expect(
      settled.workPaidNavax + settled.executorPaidNavax + settled.senderRefundNavax,
    ).to.equal(MAXIMUM_CHARGE_NAVAX);
    // The contract meters entry-to-accounting gas plus the versioned 50,000
    // overhead. It cannot reimburse more than the transaction plus that
    // overhead (rounded up from wei to nAVAX), nor more than the policy cap.
    const upperMeteredNavax =
      ((receipt?.gasUsed ?? 0n) + 50_000n) * (receipt?.gasPrice ?? 0n) / WEI_PER_NAVAX + 1n;
    expect(settled.executorPaidNavax).to.be.at.most(upperMeteredNavax);

    expect(await fixture.settlement.settledRelayEarningsNavax(fixture.relayer.address)).to.equal(
      100_000n,
    );
    expect(await fixture.settlement.withdrawableWei(fixture.relayer.address)).to.equal(
      100_000n * WEI_PER_NAVAX,
    );
    expect(await fixture.settlement.consumedRoutes(proof.routeId)).to.equal(true);
    expect(await fixture.settlement.consumedContributions(proof.contributionId)).to.equal(true);
    expect(await fixture.settlement.activeLiabilityWei()).to.equal(0);
    expect(await fixture.settlement.totalCreditsWei()).to.equal(
      MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX,
    );
    expect(await fixture.settlement.solvent()).to.equal(true);

    await expect(fixture.settlement.connect(fixture.executor).settle(proof)).to.be.revertedWith(
      "Route not active",
    );
    expect(await fixture.settlement.settledRelayEarningsNavax(fixture.relayer.address)).to.equal(
      100_000n,
    );
  });

  it("rejects self-relay and common-control routes before funds become pending", async function () {
    const fixture = await deployFixture();
    const selfRelay = await signedProof(fixture, { relayer: fixture.sender });
    await expect(
      fixture.settlement.connect(fixture.sender).fundRoute(
        selfRelay.authorization,
        selfRelay.relayers,
        selfRelay.senderSignature,
        { value: MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX },
      ),
    ).to.be.revertedWith("Common control");

    const sameRecipient = await signedProof(fixture, { recipient: fixture.sender });
    await expect(
      fixture.settlement.connect(fixture.sender).fundRoute(
        sameRecipient.authorization,
        sameRecipient.relayers,
        sameRecipient.senderSignature,
        { value: MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX },
      ),
    ).to.be.revertedWith("Common control");
    expect(await ethers.provider.getBalance(await fixture.settlement.getAddress())).to.equal(0);
  });

  it("pays nothing for missing receipt, altered payload, or invalid signatures", async function () {
    const fixture = await deployFixture();
    const proof = await signedProof(fixture);
    await fund(fixture, proof);

    const missingReceipt = {
      ...proof,
      acknowledgement: {
        ...proof.acknowledgement,
        recipient: ethers.ZeroAddress,
      },
      acknowledgementSignature: "0x" + "00".repeat(65),
    };
    await expect(
      fixture.settlement.connect(fixture.executor).settle(missingReceipt),
    ).to.be.reverted;

    const altered = await signedProof(fixture, {
      contributionPayloadCommitment: payloadCommitment(ethers.toUtf8Bytes("altered")),
    });
    altered.authorization = proof.authorization;
    altered.senderSignature = proof.senderSignature;
    altered.routeId = proof.routeId;
    altered.contributions[0].authorizationHash = proof.routeId;
    await expect(fixture.settlement.connect(fixture.executor).settle(altered)).to.be.revertedWith(
      "Contribution evidence mismatch",
    );

    const invalidRelay = await signedProof(fixture, { contributionSigner: fixture.outsider });
    invalidRelay.authorization = proof.authorization;
    invalidRelay.senderSignature = proof.senderSignature;
    invalidRelay.routeId = proof.routeId;
    invalidRelay.contributions[0].authorizationHash = proof.routeId;
    await expect(
      fixture.settlement.connect(fixture.executor).settle(invalidRelay),
    ).to.be.revertedWith("Invalid relay signature");

    expect((await fixture.settlement.routes(proof.routeId)).state).to.equal(1);
    expect(await fixture.settlement.settledRelayEarningsNavax(fixture.relayer.address)).to.equal(0);
    expect(await fixture.settlement.activeLiabilityWei()).to.equal(
      MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX,
    );
  });

  it("rejects invalid sender and recipient signatures without consuming evidence", async function () {
    const fixture = await deployFixture();
    const invalidSender = await signedProof(fixture);
    invalidSender.senderSignature = await fixture.outsider.signTypedData(
      fixture.domain,
      AUTHORIZATION_TYPES,
      invalidSender.authorization,
    );
    await expect(
      fixture.settlement.connect(fixture.sender).fundRoute(
        invalidSender.authorization,
        invalidSender.relayers,
        invalidSender.senderSignature,
        { value: MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX },
      ),
    ).to.be.revertedWith("Invalid sender signature");

    const proof = await signedProof(fixture);
    await fund(fixture, proof);
    proof.acknowledgementSignature = await fixture.outsider.signTypedData(
      fixture.domain,
      ACKNOWLEDGEMENT_TYPES,
      proof.acknowledgement,
    );
    await expect(fixture.settlement.connect(fixture.executor).settle(proof)).to.be.revertedWith(
      "Invalid recipient signature",
    );
    expect(await fixture.settlement.consumedRoutes(proof.routeId)).to.equal(false);
    expect(await fixture.settlement.consumedContributions(proof.contributionId)).to.equal(false);
    expect(await fixture.settlement.settledRelayEarningsNavax(fixture.relayer.address)).to.equal(0);
  });

  it("refunds the complete pending escrow after an expired or receipt-less route", async function () {
    const fixture = await deployFixture();
    const now = BigInt(await time.latest());
    const proof = await signedProof(fixture, { issuedAt: now, expiresAt: now + 120n });
    await fund(fixture, proof);
    await time.increaseTo(proof.authorization.expiresAt + 1n);

    await expect(fixture.settlement.connect(fixture.executor).settle(proof)).to.be.revertedWith(
      "Authorization expired",
    );
    await expect(fixture.settlement.connect(fixture.outsider).expireRoute(proof.routeId))
      .to.emit(fixture.settlement, "RouteExpired")
      .withArgs(proof.routeId, fixture.sender.address, MAXIMUM_CHARGE_NAVAX);

    expect(await fixture.settlement.withdrawableWei(fixture.sender.address)).to.equal(
      MAXIMUM_CHARGE_NAVAX * WEI_PER_NAVAX,
    );
    expect(await fixture.settlement.settledRelayEarningsNavax(fixture.relayer.address)).to.equal(0);
    expect(await fixture.settlement.activeLiabilityWei()).to.equal(0);
    expect(await fixture.settlement.solvent()).to.equal(true);
  });

  it("withdraws only accepted credits and preserves the solvency invariant", async function () {
    const fixture = await deployFixture();
    const proof = await signedProof(fixture);
    await fund(fixture, proof);
    await fixture.settlement.connect(fixture.executor).settle(proof);

    const credit = await fixture.settlement.withdrawableWei(fixture.relayer.address);
    const contractBalanceBefore = await ethers.provider.getBalance(
      await fixture.settlement.getAddress(),
    );
    await expect(fixture.settlement.connect(fixture.relayer).withdraw())
      .to.emit(fixture.settlement, "CreditWithdrawn")
      .withArgs(fixture.relayer.address, credit);
    expect(await fixture.settlement.withdrawableWei(fixture.relayer.address)).to.equal(0);
    expect(await ethers.provider.getBalance(await fixture.settlement.getAddress())).to.equal(
      contractBalanceBefore - credit,
    );
    expect(await fixture.settlement.solvent()).to.equal(true);
  });
});
