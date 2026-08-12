import { ethers } from "hardhat";
import type { Log, LogDescription } from "ethers";

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

const AUTHORIZED_BYTES = 100_000n;
const WEI_PER_NAVAX = 1_000_000_000n;

function configuredSeed(): Uint8Array {
  const configured = process.env.PRIVATE_KEY;
  if (!configured) throw new Error("PRIVATE_KEY is required for the Fuji smoke route");
  const normalized = configured.startsWith("0x") ? configured : `0x${configured}`;
  const bytes = ethers.getBytes(normalized);
  if (bytes.length !== 32) throw new Error("PRIVATE_KEY must contain exactly 32 bytes");
  return bytes;
}

function derivedOperator(seed: Uint8Array, index: number) {
  return ethers.HDNodeWallet.fromSeed(seed).derivePath(`m/44'/60'/0'/0/${index}`);
}

function payloadCommitment(payload: Uint8Array): string {
  return ethers.keccak256(
    ethers.concat([ethers.toUtf8Bytes("CABAL_PAYLOAD_V1\0"), payload]),
  );
}

function relayRouteHash(relayer: string): string {
  return ethers.keccak256(
    ethers.concat([
      ethers.toUtf8Bytes("CABAL_RELAY_ROUTE_V1\0"),
      new Uint8Array([1]),
      ethers.getBytes(relayer),
    ]),
  );
}

function orderedContributionsHash(contributionId: string): string {
  return ethers.keccak256(
    ethers.concat([
      ethers.toUtf8Bytes("CABAL_CONTRIBUTIONS_V1\0"),
      new Uint8Array([1]),
      ethers.getBytes(contributionId),
    ]),
  );
}

async function main() {
  const address = process.env.RELAY_SETTLEMENT_ADDRESS;
  if (!address) throw new Error("RELAY_SETTLEMENT_ADDRESS is required");
  const settlementAddress = ethers.getAddress(address);
  const network = await ethers.provider.getNetwork();
  if (network.chainId !== 43_113n) throw new Error("relay smoke route runs only on Fuji");

  const [sender] = await ethers.getSigners();
  if (!sender) throw new Error("configured Fuji sender is unavailable");
  const seed = configuredSeed();
  const relayer = derivedOperator(seed, 1);
  const recipient = derivedOperator(seed, 2);
  if (
    new Set([sender.address, relayer.address, recipient.address].map((value) => value.toLowerCase()))
      .size !== 3
  ) {
    throw new Error("smoke route requires three distinct operator identities");
  }

  const settlement = await ethers.getContractAt(
    "CabalRelaySettlement",
    settlementAddress,
    sender,
  );
  const code = await ethers.provider.getCode(settlementAddress);
  if (code === "0x") throw new Error("relay settlement address has no deployed bytecode");

  const quote = await settlement.quote(AUTHORIZED_BYTES, 1);
  const maximumChargeNavax = quote.maximumChargeNavax;
  const block = await ethers.provider.getBlock("latest");
  if (!block) throw new Error("Fuji latest block is unavailable");
  const issuedAt = BigInt(block.timestamp);
  const payload = ethers.toUtf8Bytes("cabalmesh fuji relay smoke v1");
  const commitment = payloadCommitment(payload);
  const domain = {
    name: "CabalMesh Relay Proof",
    version: "1",
    chainId: network.chainId,
    verifyingContract: settlementAddress,
  };
  const authorization = {
    policyHash: ethers.keccak256(ethers.toUtf8Bytes("cabal-rewards-v1")),
    routeNonce: ethers.hexlify(ethers.randomBytes(32)),
    payloadCommitment: commitment,
    deliveryMode: 0,
    relayRouteHash: relayRouteHash(relayer.address),
    sender: sender.address,
    recipient: recipient.address,
    authorizedBytes: AUTHORIZED_BYTES,
    relayCount: 1,
    maximumChargeNavax,
    issuedAt,
    expiresAt: issuedAt + 600n,
  };
  const senderSignature = await sender.signTypedData(
    domain,
    AUTHORIZATION_TYPES,
    authorization,
  );
  const routeId = ethers.TypedDataEncoder.hash(
    domain,
    AUTHORIZATION_TYPES,
    authorization,
  );
  if ((await settlement.authorizationHash(authorization)) !== routeId) {
    throw new Error("contract and harness disagree on the authorization hash");
  }

  const fundingTransaction = await settlement.fundRoute(
    authorization,
    [relayer.address],
    senderSignature,
    { value: maximumChargeNavax * WEI_PER_NAVAX },
  );
  const fundingReceipt = await fundingTransaction.wait();
  if (!fundingReceipt || fundingReceipt.status !== 1) {
    throw new Error("Fuji funding transaction was not accepted");
  }

  const contribution = {
    authorizationHash: routeId,
    hopIndex: 0,
    relayer: relayer.address,
    ingress: sender.address,
    egress: recipient.address,
    payloadCommitment: commitment,
    deliveredBytes: AUTHORIZED_BYTES,
    forwardedAt: issuedAt,
  };
  const contributionSignature = await relayer.signTypedData(
    domain,
    CONTRIBUTION_TYPES,
    contribution,
  );
  const contributionId = ethers.TypedDataEncoder.hash(
    domain,
    CONTRIBUTION_TYPES,
    contribution,
  );
  const acknowledgement = {
    authorizationHash: routeId,
    contributionsHash: orderedContributionsHash(contributionId),
    recipient: recipient.address,
    payloadCommitment: commitment,
    deliveredBytes: AUTHORIZED_BYTES,
    receivedAt: issuedAt,
  };
  const acknowledgementSignature = await recipient.signTypedData(
    domain,
    ACKNOWLEDGEMENT_TYPES,
    acknowledgement,
  );
  const proof = {
    authorization,
    relayers: [relayer.address],
    senderSignature,
    contributions: [contribution],
    contributionSignatures: [contributionSignature],
    acknowledgement,
    acknowledgementSignature,
  };

  const settlementTransaction = await settlement.settle(proof);
  const settlementReceipt = await settlementTransaction.wait();
  if (!settlementReceipt || settlementReceipt.status !== 1) {
    throw new Error("Fuji settlement transaction was not accepted");
  }
  const parsedLogs = (settlementReceipt.logs as Log[]).flatMap(
    (log: Log): LogDescription[] => {
      try {
        const parsed = settlement.interface.parseLog(log);
        return parsed ? [parsed] : [];
      } catch {
        return [];
      }
    },
  );
  const settled = parsedLogs.filter(
    (log: LogDescription) => log.name === "RouteSettled",
  );
  const rewards = parsedLogs.filter(
    (log: LogDescription) => log.name === "RelayRewardCredited",
  );
  if (settled.length !== 1 || rewards.length !== 1) {
    throw new Error("settlement must emit exactly one route and one relay reward record");
  }
  if (
    rewards[0].args.routeId !== routeId ||
    rewards[0].args.relayer.toLowerCase() !== relayer.address.toLowerCase()
  ) {
    throw new Error("settled reward does not belong to the signed route relayer");
  }
  const rewardNavax = rewards[0].args.amountNavax as bigint;
  const recordedEarnings = await settlement.settledRelayEarningsNavax(relayer.address);
  const withdrawableWei = await settlement.withdrawableWei(relayer.address);
  if (
    rewardNavax <= 0n ||
    recordedEarnings !== rewardNavax ||
    withdrawableWei !== rewardNavax * WEI_PER_NAVAX
  ) {
    throw new Error("accepted relay reward accounting is inconsistent");
  }
  if (!(await settlement.solvent())) throw new Error("settlement contract is insolvent");

  console.log(
    JSON.stringify({
      contract: settlementAddress,
      chainId: Number(network.chainId),
      sender: sender.address,
      relayer: relayer.address,
      recipient: recipient.address,
      routeId,
      fundingTransactionHash: fundingTransaction.hash,
      fundingBlock: fundingReceipt.blockNumber,
      settlementTransactionHash: settlementTransaction.hash,
      settlementBlock: settlementReceipt.blockNumber,
      relayRewardNavax: rewardNavax.toString(),
      relayWithdrawableWei: withdrawableWei.toString(),
    }),
  );
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : "relay settlement smoke failed");
  process.exitCode = 1;
});
