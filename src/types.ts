export interface Peer {
    id: string;
    address: string;
    timestamp: number;
}

export interface ThoughtLog {
    message: string;
    timestamp: number;
    type: "noir" | "confidential-compute" | "private-swap" | "mesh" | "agent";
}

export interface MeshEvent {
    type: string;
    peer_id?: string;
    address?: string;
    intent?: any;
}

export type ViewState =
    | "nexus"
    | "negotiation"
    | "integrity"
    | "report"
    | "config"
    | "provider"
    | "service-creator"
    | "delegation"
    | "wallet-cabinet"
    | "identity-init"
    | "wallet"
    | "archives"
    | "escrow"
    | "compute"
    | "notification";
