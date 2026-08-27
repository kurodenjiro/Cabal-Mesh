/**
 * Exercises the deployed Fuji contracts against the four findings the rewrite
 * was for. Every check runs against the real chain — the local Hardhat suite
 * proves the same rules in isolation, this proves the deployed bytecode.
 *
 * The one rule that cannot be shown here is auto-release, which needs three
 * days of wall clock. That branch is covered locally with time travel.
 *
 * Run: PRIVATE_KEY=... npx hardhat run scripts/verifyFuji.ts --network fuji
 */
import { ethers } from "hardhat";
import * as fs from "fs";
import * as path from "path";

/// Avalanche's public RPC has no pending-block state, so ethers' automatic gas
/// estimation fails; every send carries an explicit limit instead.
const TX = { gasLimit: 600_000 };

/// Small enough that the whole run costs a rounding error of the test balance,
/// large enough to be a real transfer of value rather than a zero.
const PRICE = ethers.parseEther("0.0005");

/// The buyer has to be a second address — the contract refuses self-purchase,
/// and a genuine sale is the whole point of the check. Derived from the
/// deployer's key so it is reproducible without storing a second secret.
async function deriveBuyer() {
  const [deployer] = await ethers.getSigners();
  const seed = ethers.id(`cabalmesh-fuji-verify:${await deployer.getAddress()}`);
  return new ethers.Wallet(seed, ethers.provider);
}

let passed = 0;
let failed = 0;

function ok(label: string, detail = "") {
  passed++;
  console.log(`  PASS  ${label}${detail ? ` — ${detail}` : ""}`);
}

function bad(label: string, detail: string) {
  failed++;
  console.log(`  FAIL  ${label} — ${detail}`);
}

function check(label: string, condition: boolean, detail = "") {
  if (condition) ok(label, detail);
  else bad(label, detail || "expected true");
}

/// Asserts a call reverts with `expected` in its reason. Uses eth_call, so a
/// rejected rule costs no gas to demonstrate.
async function expectRevert(label: string, expected: string, call: () => Promise<unknown>) {
  try {
    await call();
    bad(label, `expected revert "${expected}", call succeeded`);
  } catch (error: any) {
    const reason = error?.reason ?? error?.shortMessage ?? String(error?.message ?? error);
    if (String(reason).includes(expected)) ok(label, `reverted "${expected}"`);
    else bad(label, `expected "${expected}", got "${reason}"`);
  }
}

async function main() {
  const deployments = JSON.parse(
    fs.readFileSync(path.join(__dirname, "../deployments/fuji.json"), "utf-8")
  );
  const voucherAddress: string = deployments.voucher.address;
  const marketplaceAddress: string = deployments.marketplace.address;

  const [seller] = await ethers.getSigners();
  const buyer = await deriveBuyer();

  const voucher = await ethers.getContractAt("CabalMeshVoucher", voucherAddress);
  const marketplace = await ethers.getContractAt("Marketplace", marketplaceAddress);

  console.log("Voucher     :", voucherAddress);
  console.log("Marketplace :", marketplaceAddress);
  console.log("Seller      :", seller.address, ethers.formatEther(await ethers.provider.getBalance(seller.address)), "AVAX");
  console.log("Buyer       :", buyer.address, ethers.formatEther(await ethers.provider.getBalance(buyer.address)), "AVAX");

  // --- fund the buyer -----------------------------------------------------
  const buyerBalance = await ethers.provider.getBalance(buyer.address);
  const needed = ethers.parseEther("0.01");
  if (buyerBalance < needed) {
    console.log("\nFunding buyer with 0.01 AVAX…");
    const tx = await seller.sendTransaction({ to: buyer.address, value: needed, gasLimit: 21_000 });
    await tx.wait();
    console.log("  tx", tx.hash);
  }

  // --- deployment shape ---------------------------------------------------
  console.log("\nDeployed configuration");
  check("issuer is the deployer", (await voucher.issuer()) === seller.address);
  check("deployer holds mint rights", await voucher.minters(seller.address));
  check("governor is the deployer", (await marketplace.governor()) === seller.address);
  check(
    "voucher collection is allowed",
    await marketplace.allowedCollections(voucherAddress),
    voucherAddress
  );
  const releaseWindow = await marketplace.releaseWindow();
  check("release window is 3 days", releaseWindow === 259200n, `${releaseWindow}s`);

  // --- finding 4: mint authority -----------------------------------------
  console.log("\nFinding 4 — mint authority");
  await expectRevert(
    "an unauthorized wallet cannot mint",
    "Not an authorized minter",
    () => voucher.connect(buyer).mintVoucher.staticCall("Relay Amplifier", "+18% relay yield")
  );
  await expectRevert(
    "an unauthorized wallet cannot mint to itself via mintTo",
    "Not an authorized minter",
    () => voucher.connect(buyer).mintTo.staticCall(buyer.address, "Relay Amplifier", "+18%")
  );

  const tokenId = await voucher.nextTokenId();
  let tx = await voucher.connect(seller).mintVoucher("Relay Amplifier MK-II", "RADIO · +18% relay yield", TX);
  await tx.wait();
  check("the minter can mint", (await voucher.ownerOf(tokenId)) === seller.address, `token #${tokenId}, tx ${tx.hash}`);

  // --- finding 4b: collection allowlist ----------------------------------
  console.log("\nFinding 4 — collections are swappable without a redeploy");
  const futureCollection = ethers.Wallet.createRandom().address;
  tx = await marketplace.connect(seller).setCollectionAllowed(futureCollection, true, TX);
  await tx.wait();
  check(
    "the governor can allow a new collection",
    await marketplace.allowedCollections(futureCollection),
    `${futureCollection}, tx ${tx.hash}`
  );
  tx = await marketplace.connect(seller).setCollectionAllowed(futureCollection, false, TX);
  await tx.wait();
  check("the governor can disallow it again", !(await marketplace.allowedCollections(futureCollection)));
  await expectRevert(
    "a non-governor cannot change collections",
    "Not the governor",
    () => marketplace.connect(buyer).setCollectionAllowed.staticCall(futureCollection, true)
  );

  // --- listing ------------------------------------------------------------
  console.log("\nFindings 2 and 3 — listing lifecycle");
  tx = await voucher.connect(seller).approve(marketplaceAddress, tokenId, TX);
  await tx.wait();
  tx = await marketplace.connect(seller).createListing("Relay Amplifier MK-II", PRICE, tokenId, TX);
  await tx.wait();
  const firstListingId = (await marketplace.nextListingId()) - 1n;
  check("listing created", (await marketplace.listings(firstListingId)).active, `listing #${firstListingId}, tx ${tx.hash}`);

  await expectRevert(
    "the same token cannot back a second live listing",
    "Token already listed",
    () => marketplace.connect(seller).createListing.staticCall("Duplicate", PRICE, tokenId)
  );

  await expectRevert(
    "a non-seller cannot cancel a listing",
    "Only seller",
    () => marketplace.connect(buyer).cancelListing.staticCall(firstListingId)
  );

  tx = await marketplace.connect(seller).cancelListing(firstListingId, TX);
  await tx.wait();
  check("the seller can cancel", !(await marketplace.listings(firstListingId)).active, `tx ${tx.hash}`);
  check(
    "cancelling frees the token to be listed again",
    (await marketplace.activeListingOf(voucherAddress, tokenId)) === 0n
  );

  tx = await marketplace.connect(seller).createListing("Relay Amplifier MK-II", PRICE, tokenId, TX);
  await tx.wait();
  const listingId = (await marketplace.nextListingId()) - 1n;
  check("the cancelled token relists", (await marketplace.listings(listingId)).active, `listing #${listingId}`);

  // --- finding 1: settlement ---------------------------------------------
  console.log("\nFinding 1 — settlement is no longer a buyer-only option");
  tx = await marketplace.connect(buyer).buy(listingId, { value: PRICE, ...TX });
  await tx.wait();
  const dealId = (await marketplace.nextDealId()) - 1n;
  const deal = await marketplace.getDeal(dealId);
  check("buy locks the token in escrow", (await voucher.ownerOf(tokenId)) === marketplaceAddress, `deal #${dealId}, tx ${tx.hash}`);
  const now = (await ethers.provider.getBlock("latest"))!.timestamp;
  check(
    "the deal carries an auto-release deadline",
    deal.autoReleaseAt > now && deal.autoReleaseAt <= BigInt(now) + releaseWindow + 30n,
    new Date(Number(deal.autoReleaseAt) * 1000).toISOString()
  );

  await expectRevert(
    "the buyer cannot unilaterally refund",
    "Only seller",
    () => marketplace.connect(buyer).refundDeal.staticCall(dealId)
  );
  await expectRevert(
    "the seller cannot refund without buyer consent",
    "Buyer has not requested a refund",
    () => marketplace.connect(seller).refundDeal.staticCall(dealId)
  );
  await expectRevert(
    "the seller cannot release before the deadline",
    "Only buyer before auto-release",
    () => marketplace.connect(seller).releaseDeal.staticCall(dealId)
  );

  const sellerBefore = await ethers.provider.getBalance(seller.address);
  tx = await marketplace.connect(buyer).releaseDeal(dealId, TX);
  await tx.wait();
  const sellerAfter = await ethers.provider.getBalance(seller.address);
  check("release transfers the token to the buyer", (await voucher.ownerOf(tokenId)) === buyer.address, `tx ${tx.hash}`);
  check("release pays the seller", sellerAfter - sellerBefore === PRICE, `+${ethers.formatEther(sellerAfter - sellerBefore)} AVAX`);
  check("the deal is Released", (await marketplace.getDeal(dealId)).status === 2n);

  // --- finding 1: the mutual cancel path ---------------------------------
  console.log("\nFinding 1 — cancelling requires both sides");
  const secondTokenId = await voucher.nextTokenId();
  tx = await voucher.connect(seller).mintVoucher("Ghost Cloak", "CRYPTO · +2 hops", TX);
  await tx.wait();
  tx = await voucher.connect(seller).approve(marketplaceAddress, secondTokenId, TX);
  await tx.wait();
  tx = await marketplace.connect(seller).createListing("Ghost Cloak", PRICE, secondTokenId, TX);
  await tx.wait();
  const secondListingId = (await marketplace.nextListingId()) - 1n;
  tx = await marketplace.connect(buyer).buy(secondListingId, { value: PRICE, ...TX });
  await tx.wait();
  const secondDealId = (await marketplace.nextDealId()) - 1n;

  await expectRevert(
    "a non-buyer cannot request a refund",
    "Only buyer",
    () => marketplace.connect(seller).requestRefund.staticCall(secondDealId)
  );

  tx = await marketplace.connect(buyer).requestRefund(secondDealId, TX);
  await tx.wait();
  check("the buyer can consent to cancellation", (await marketplace.getDeal(secondDealId)).refundRequested, `tx ${tx.hash}`);

  const buyerBefore = await ethers.provider.getBalance(buyer.address);
  tx = await marketplace.connect(seller).refundDeal(secondDealId, TX);
  await tx.wait();
  const buyerAfter = await ethers.provider.getBalance(buyer.address);
  check("refund returns the token to the seller", (await voucher.ownerOf(secondTokenId)) === seller.address, `tx ${tx.hash}`);
  check("refund returns the funds to the buyer", buyerAfter - buyerBefore === PRICE, `+${ethers.formatEther(buyerAfter - buyerBefore)} AVAX`);
  check("the deal is Refunded", (await marketplace.getDeal(secondDealId)).status === 3n);

  // --- catalog views ------------------------------------------------------
  console.log("\nCatalog views");
  const [active] = await marketplace.getActiveListings();
  check("no stale listings survive the run", active.length === 0, `${active.length} active`);
  const [, pagedIds, nextOffset] = await marketplace.getActiveListingsPaged(0, 2);
  check("paged view walks the history", nextOffset === 2n, `${pagedIds.length} active in the first window`);

  console.log(
    `\n${passed} passed, ${failed} failed. Seller balance ${ethers.formatEther(await ethers.provider.getBalance(seller.address))} AVAX.`
  );
  console.log("Not covered here: auto-release after the 3-day window (local test only).");
  if (failed > 0) process.exitCode = 1;
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
