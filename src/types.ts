export interface Peer {
    id: string;
    address: string;
    timestamp: number;
    /** The peer's real AVAX wallet address, learned from its "presence"
     * broadcast — not known at discovery time, only once received. */
    walletAddress?: string;
}

export interface ThoughtLog {
    message: string;
    timestamp: number;
    type: "noir" | "confidential-compute" | "private-swap" | "mesh" | "agent";
}

export interface AssetListingView {
    id: number;
    seller: string;
    description: string;
    price_wei: string;
    price_avax: string;
    token_id: number;
}

export interface VoucherView {
    token_id: number;
    voucher_type: string;
    description: string;
    owner: string;
    /** Address that originally minted this voucher — used to distinguish
     * "bought from someone else" from "minted it myself and still hold it". */
    minted_by: string;
}

/** A Marketplace deal's real on-chain status — the "someone is currently
 * transacting on this listing" signal. */
export interface DealView {
    deal_id: number;
    buyer: string;
    seller: string;
    token_id: number;
    amount_avax: string;
    status: "active" | "released" | "refunded" | "none";
    role: "buyer" | "seller";
}

export interface MatchResult {
    listing_id: number;
    seller: string;
    description: string;
    price_avax: string;
    price_wei: string;
    token_id: number;
    reason: string;
}

export interface MeshEvent {
    type: string;
    peer_id?: string;
    address?: string;
    intent?: any;
    queue_id?: string;
    raw_tx_hex?: string;
    summary?: string;
    status?: string;
    tx_hash?: string;
    token_id?: number;
    text?: string;
    signature?: string;
    signer_address?: string;
}

/** Result of an action that normally hits the chain directly: either it went
 * through immediately, or the RPC was unreachable and it was signed offline
 * and queued for mesh relay instead. */
export type TxResult =
    | { kind: "confirmed"; id: number }
    | { kind: "queued"; queueId: string };

export interface QueuedTx {
    id: string;
    raw_tx_hex: string;
    summary: string;
    created_at: string;
    status: "queued" | "confirmed" | "failed";
    tx_hash: string | null;
}

/** A piece of content (e.g. a book page) committed to by its seller: a real
 * EIP-191 signature over the exact text, verifiable by recovering the
 * signer's address — used in place of a literal ZK proof (no nargo/Noir
 * available on this machine). */
export interface ContentRecord {
    token_id: number;
    text: string;
    fingerprint: string;
    signature: string;
    signer_address: string;
}

export type ViewState =
    | "nexus"
    | "config"
    | "service-creator"
    | "delegation"
    | "wallet-cabinet"
    | "escrow"
    | "redeem";
