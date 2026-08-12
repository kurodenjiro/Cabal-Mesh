import { ethers } from "hardhat";

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

  console.log(JSON.stringify({
    address: await settlement.getAddress(),
    chainId: Number(network.chainId),
    deployer: deployer.address,
    deploymentTxHash: deployment.hash,
  }));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
});
