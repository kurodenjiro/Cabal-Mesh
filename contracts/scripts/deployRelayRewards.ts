import { ethers } from "hardhat";
import * as fs from "fs";
import * as path from "path";

function writeAbi(contractName: string) {
  const artifactPath = path.join(__dirname, `../artifacts/contracts/${contractName}.sol/${contractName}.json`);
  const artifact = JSON.parse(fs.readFileSync(artifactPath, "utf-8"));
  const abiJson = JSON.stringify(artifact.abi, null, 2);

  for (const outDir of ["../../src-tauri/abi", "../../src/abi"]) {
    const outPath = path.join(__dirname, outDir, `${contractName}.abi.json`);
    fs.mkdirSync(path.dirname(outPath), { recursive: true });
    fs.writeFileSync(outPath, abiJson);
    console.log("Wrote ABI to", outPath);
  }
}

/// Deploys the fix for docs/intent-chat-and-modules-design.md decision 0:
/// RelayRewards first (its address is what the new CabalMeshVoucher
/// restricts minting to), then the voucher, then one call wiring
/// RelayRewards to it. See both contracts' doc comments for why the order
/// can't be the other way around.
///
/// Deliberately does NOT touch the old `voucher` entry in deployments/fuji.json
/// in place — it writes a new `voucherV2` / `relayRewards` entry instead, so
/// the record of the vulnerable original deployment (0xaEa0F4...) is not
/// silently lost. Whoever points network_config.rs at a real address decides
/// that separately; this script only deploys and records.
async function main() {
  const RelayRewards = await ethers.getContractFactory("RelayRewards");
  const relayRewards = await RelayRewards.deploy();
  await relayRewards.waitForDeployment();
  const relayRewardsAddress = await relayRewards.getAddress();
  console.log("RelayRewards deployed to:", relayRewardsAddress);

  const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
  const voucher = await Voucher.deploy(relayRewardsAddress);
  await voucher.waitForDeployment();
  const voucherAddress = await voucher.getAddress();
  console.log("CabalMeshVoucher (v2, access-controlled) deployed to:", voucherAddress);

  const wireTx = await relayRewards.setVoucher(voucherAddress);
  await wireTx.wait();
  console.log("RelayRewards wired to voucher:", voucherAddress);

  writeAbi("CabalMeshVoucher");
  writeAbi("RelayRewards");

  const network = await ethers.provider.getNetwork();
  const deploymentPath = path.join(__dirname, "../deployments/fuji.json");

  let existing: Record<string, unknown> = {};
  if (fs.existsSync(deploymentPath)) {
    existing = JSON.parse(fs.readFileSync(deploymentPath, "utf-8"));
  }

  const merged = {
    ...existing,
    // Deliberately not overwriting `voucher` — that key stays the historical
    // record of the vulnerable original deployment (see decision 0).
    voucherV2: { address: voucherAddress, chainId: Number(network.chainId), deployedAt: new Date().toISOString() },
    relayRewards: {
      address: relayRewardsAddress,
      chainId: Number(network.chainId),
      deployedAt: new Date().toISOString(),
    },
  };

  fs.mkdirSync(path.dirname(deploymentPath), { recursive: true });
  fs.writeFileSync(deploymentPath, JSON.stringify(merged, null, 2));
  console.log("Wrote deployment info to", deploymentPath);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
