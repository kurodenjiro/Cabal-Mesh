import { ethers } from "hardhat";
import * as fs from "fs";
import * as path from "path";

/// How long the buyer keeps the exclusive right to release a deal. After it
/// passes anyone may settle the deal to the seller, so a buyer who walks away
/// cannot strand the seller's asset and payment.
///
/// Three days: long enough that a buyer on a phone with an intermittent mesh
/// connection is not raced by an impatient seller, short enough that a stalled
/// deal is not a month-long hostage.
const RELEASE_WINDOW_SECONDS = 3 * 24 * 60 * 60;

/// Avalanche's public RPC refuses `eth_estimateGas` against the pending block
/// ("state not available for pending block"), which is what ethers reaches for
/// when a transaction arrives without a gas limit. Supplying one explicitly
/// keeps the deployment off that path. Unused gas is not charged.
const DEPLOY_OVERRIDES = { gasLimit: 5_000_000 };

function writeAbi(contractName: string) {
  const artifactPath = path.join(
    __dirname,
    `../artifacts/contracts/${contractName}.sol/${contractName}.json`
  );
  const artifact = JSON.parse(fs.readFileSync(artifactPath, "utf-8"));
  const abiJson = JSON.stringify(artifact.abi, null, 2);

  for (const outDir of ["../../src-tauri/abi", "../../src/abi"]) {
    const outPath = path.join(__dirname, outDir, `${contractName}.abi.json`);
    fs.mkdirSync(path.dirname(outPath), { recursive: true });
    fs.writeFileSync(outPath, abiJson);
    console.log("Wrote ABI to", outPath);
  }
}

async function main() {
  const [deployer] = await ethers.getSigners();
  console.log("Deployer:", deployer.address);
  console.log("Balance :", ethers.formatEther(await ethers.provider.getBalance(deployer.address)), "AVAX");

  const Voucher = await ethers.getContractFactory("CabalMeshVoucher");
  const voucher = await Voucher.deploy(DEPLOY_OVERRIDES);
  await voucher.waitForDeployment();
  const voucherAddress = await voucher.getAddress();
  console.log("CabalMeshVoucher deployed to:", voucherAddress);

  const Marketplace = await ethers.getContractFactory("Marketplace");
  const marketplace = await Marketplace.deploy(voucherAddress, RELEASE_WINDOW_SECONDS, DEPLOY_OVERRIDES);
  await marketplace.waitForDeployment();
  const marketplaceAddress = await marketplace.getAddress();
  console.log("Marketplace deployed to:", marketplaceAddress);

  writeAbi("CabalMeshVoucher");
  writeAbi("Marketplace");

  const network = await ethers.provider.getNetwork();
  const deploymentPath = path.join(__dirname, "../deployments/fuji.json");

  let existing: any = {};
  if (fs.existsSync(deploymentPath)) {
    existing = JSON.parse(fs.readFileSync(deploymentPath, "utf-8"));
  }

  const deployedAt = new Date().toISOString();
  const merged = {
    ...(existing.escrow ? { escrow: existing.escrow } : {}),
    voucher: {
      address: voucherAddress,
      chainId: Number(network.chainId),
      deployedAt,
      issuer: deployer.address,
    },
    marketplace: {
      address: marketplaceAddress,
      chainId: Number(network.chainId),
      deployedAt,
      governor: deployer.address,
      releaseWindowSeconds: RELEASE_WINDOW_SECONDS,
      collection: voucherAddress,
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
