import { JsonRpcProvider, formatEther, Contract, type Provider } from "ethers";
import escrowAbi from "./abi/Escrow.abi.json";

const DEFAULT_RPC_URL = "https://api.avax-test.network/ext/bc/C/rpc";

/**
 * Read-only, frontend-side view of Avalanche chain state. All signing
 * (identity generation, escrow create/release/refund) happens in the Rust
 * backend, which is the only place the wallet's private key lives.
 */
export class AvalancheSettlement {
    private provider: Provider;

    constructor(rpcUrl: string = DEFAULT_RPC_URL) {
        this.provider = new JsonRpcProvider(rpcUrl);
    }

    async getBalancePrivately(address: string): Promise<string> {
        const balance = await this.provider.getBalance(address);
        return formatEther(balance);
    }

    async monitorTransaction(txHash: string) {
        return this.provider.getTransactionReceipt(txHash);
    }

    async getEscrowStatus(contractAddress: string, escrowId: number) {
        const contract = new Contract(contractAddress, escrowAbi, this.provider);
        return contract.getDeal(escrowId);
    }
}

export default AvalancheSettlement;
