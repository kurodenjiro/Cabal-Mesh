import { ethers } from "hardhat";
import * as fs from "fs";
import * as path from "path";

/// Records the deployment next to the other Fuji addresses. Printing it to
/// stdout alone loses it the moment the terminal scrolls, and an address that
/// only exists in scrollback is an address nobody can verify later.
function record(entry: Record<string, unknown>) {
  const deploymentPath = path.join(__dirname, "../deployments/fuji.json");
  const existing = fs.existsSync(deploymentPath)
    ? JSON.parse(fs.readFileSync(deploymentPath, "utf-8"))
    : {};
  fs.mkdirSync(path.dirname(deploymentPath), { recursive: true });
  fs.writeFileSync(
    deploymentPath,
    JSON.stringify({ ...existing, relaySettlement: entry }, null, 2) + "\n",
  );
}

async function main() {
  const [deployer] = await ethers.getSigners();
  if (!deployer) throw new Error("PRIVATE_KEY is required for Fuji deployment");
  const network = await ethers.provider.getNetwork();
  if (network.chainId !== 43_113n) throw new Error("relay settlement deploys only to Fuji");

  const Settlement = await ethers.getContractFactory("CabalRelaySettlement");
  const settlement = await Settlement.deploy();
  const deployment = settlement.deploymentTransaction();
  if (!deployment) throw new Error("deployment transaction was not created");
  await settlement.waitForDeployment();

  const entry = {
    address: await settlement.getAddress(),
    chainId: Number(network.chainId),
    deployedAt: new Date().toISOString(),
    deployer: deployer.address,
    deploymentTxHash: deployment.hash,
  };
  record(entry);
  console.log(JSON.stringify(entry));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
});
