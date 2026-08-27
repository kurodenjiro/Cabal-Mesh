/**
 * Reads the deployed relay settlement's state back off Fuji and reclaims any
 * route left pending, so a failed run does not quietly strand escrow.
 *
 * Run: PRIVATE_KEY=... RELAY_SETTLEMENT_ADDRESS=0x… \
 *        npx hardhat run scripts/verifyRelaySettlementFuji.ts --network fuji
 */
import { ethers } from "hardhat";
import type { Log } from "ethers";

const WEI_PER_NAVAX = 1_000_000_000n;

/// Deployment block of the contract under test; nothing relevant precedes it.
const FROM_BLOCK = 57_716_000;

async function main() {
  const address = process.env.RELAY_SETTLEMENT_ADDRESS;
  if (!address) throw new Error("RELAY_SETTLEMENT_ADDRESS is required");
  const settlementAddress = ethers.getAddress(address);
  const network = await ethers.provider.getNetwork();
  if (network.chainId !== 43_113n) throw new Error("this check runs only on Fuji");

  const [sender] = await ethers.getSigners();
  const settlement = await ethers.getContractAt("CabalRelaySettlement", settlementAddress, sender);

  const funded = await settlement.queryFilter(settlement.filters.RouteFunded(), FROM_BLOCK, "latest");
  const settled = await settlement.queryFilter(settlement.filters.RouteSettled(), FROM_BLOCK, "latest");
  const rewards = await settlement.queryFilter(settlement.filters.RelayRewardCredited(), FROM_BLOCK, "latest");
  const expired = await settlement.queryFilter(settlement.filters.RouteExpired(), FROM_BLOCK, "latest");

  console.log("Contract  :", settlementAddress);
  console.log("Funded    :", funded.length);
  console.log("Settled   :", settled.length);
  console.log("Rewarded  :", rewards.length);
  console.log("Expired   :", expired.length);

  for (const event of settled) {
    const { routeId, deliveredBytes, workPaidNavax, executorPaidNavax, senderRefundNavax } = event.args;
    console.log(
      `  settled ${routeId.slice(0, 12)}… bytes=${deliveredBytes} work=${workPaidNavax} executor=${executorPaidNavax} refund=${senderRefundNavax}`
    );
  }
  for (const event of rewards) {
    console.log(`  reward  ${event.args.relayer} ${event.args.amountNavax} nAVAX`);
  }

  // Anything funded but neither settled nor expired is still holding escrow.
  const terminal = new Set([...settled, ...expired].map((event) => event.args.routeId as string));
  const pending = funded.filter((event) => !terminal.has(event.args.routeId as string));

  const now = (await ethers.provider.getBlock("latest"))!.timestamp;
  for (const event of pending) {
    const routeId = event.args.routeId as string;
    const expiresAt = Number(event.args.expiresAt);
    if (now < expiresAt) {
      console.log(`  pending ${routeId.slice(0, 12)}… not expirable for ${expiresAt - now}s`);
      continue;
    }
    const tx = await settlement.expireRoute(routeId);
    await tx.wait();
    console.log(`  expired ${routeId.slice(0, 12)}… reclaimed, tx ${tx.hash}`);
  }

  const senderCredit = await settlement.withdrawableWei(sender.address);
  if (senderCredit > 0n) {
    const tx = await settlement.withdraw();
    const receipt = await tx.wait();
    const drawn = (receipt!.logs as Log[]).length;
    console.log(`  withdrew ${ethers.formatEther(senderCredit)} AVAX (${drawn} logs), tx ${tx.hash}`);
  }

  const held = await ethers.provider.getBalance(settlementAddress);
  const liability = await settlement.activeLiabilityWei();
  const credits = await settlement.totalCreditsWei();
  console.log("\nHeld      :", ethers.formatEther(held), "AVAX");
  console.log("Liability :", ethers.formatEther(liability), "AVAX");
  console.log("Credits   :", ethers.formatEther(credits), "AVAX");
  console.log("Solvent   :", await settlement.solvent());
  if (held < liability + credits) throw new Error("contract holds less than it owes");
  console.log("Sender    :", ethers.formatEther(await ethers.provider.getBalance(sender.address)), "AVAX");

  const relayerCredits = await Promise.all(
    [...new Set(rewards.map((event) => event.args.relayer as string))].map(
      async (relayer) => [relayer, await settlement.withdrawableWei(relayer)] as const
    )
  );
  for (const [relayer, credit] of relayerCredits) {
    const earned = await settlement.settledRelayEarningsNavax(relayer);
    console.log(
      `Relayer   : ${relayer} earned ${earned} nAVAX, withdrawable ${credit} wei` +
        (credit === earned * WEI_PER_NAVAX ? " (consistent)" : " (INCONSISTENT)")
    );
    if (credit !== earned * WEI_PER_NAVAX) throw new Error("relay earnings and credit disagree");
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
});
