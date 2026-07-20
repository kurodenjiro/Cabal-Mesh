import { ethers } from "hardhat";
import * as fs from "fs";
import * as path from "path";

async function main() {
  const Escrow = await ethers.getContractFactory("Escrow");
  const escrow = await Escrow.deploy();
  await escrow.waitForDeployment();
  const address = await escrow.getAddress();
  console.log("Escrow deployed to:", address);

  const artifactPath = path.join(
    __dirname,
    "../artifacts/contracts/Escrow.sol/Escrow.json"
  );
  const artifact = JSON.parse(fs.readFileSync(artifactPath, "utf-8"));
  const abiJson = JSON.stringify(artifact.abi, null, 2);

  for (const outDir of ["../../src-tauri/abi", "../../src/abi"]) {
    const outPath = path.join(__dirname, outDir, "Escrow.abi.json");
    fs.mkdirSync(path.dirname(outPath), { recursive: true });
    fs.writeFileSync(outPath, abiJson);
    console.log("Wrote ABI to", outPath);
  }

  const network = await ethers.provider.getNetwork();
  const deploymentPath = path.join(__dirname, "../deployments/fuji.json");
  fs.mkdirSync(path.dirname(deploymentPath), { recursive: true });
  fs.writeFileSync(
    deploymentPath,
    JSON.stringify(
      { address, chainId: Number(network.chainId), deployedAt: new Date().toISOString() },
      null,
      2
    )
  );
  console.log("Wrote deployment info to", deploymentPath);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
