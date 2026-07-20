# CabalMesh

> **The Zero-Identity Autonomous Layer for Mesh-to-Avalanche Private Intents**

A decentralized, privacy-first infrastructure enabling autonomous AI Agents to negotiate and execute transactions over a physical Mesh Network, settling on the Avalanche C-Chain (Fuji testnet by default).

## 🎯 Philosophy

In this network, you are a **Nobody**. Every trace—from your physical location (IP) and negotiation tactics to your on-chain financial footprint—is erased, leaving only a cryptographically verified result.

## 🏗️ Architecture

### The "Nobody" Stack (Privacy-in-Depth)

1. **The Cloak Layer** (Mesh Networking)
   - ShadowWire + Libp2p for offline peer-to-peer communication
   - mDNS discovery without internet dependency
   - Multi-hop metadata stripping

2. **The Invisible Brain** (Confidential Computation)
   - Ollama AI Agents with "Shark Mode" aggressive negotiation
   - Noir ZK-Circuits for privacy-preserving verification
   - Confidential Compute (FHE/MPC) integration ready

3. **The Settlement Layer** (Avalanche)
   - A minimal on-chain `Escrow` contract (Solidity, deployed to Fuji via Hardhat) locks/releases AVAX for a deal
   - Private Swap for anonymous swaps (interface reserved, not yet integrated)
   - Instant Session keys for sub-second mesh-side agent authority delegation

## 🚀 Quick Start

### Prerequisites

- **Rust** 1.91+ (needed by the `alloy` EVM crate; run `rustup update stable`)
- **Node.js** 18+
- **Ollama** (for AI agent) - [Install](https://ollama.ai)
- **Nargo** (for Noir circuits, optional) - [Install](https://noir-lang.org)

### Installation

```bash
cd cabalmesh
npm install
```

### Deploy the Escrow contract (once, to Fuji testnet)

```bash
cd contracts
npm install
cp .env.example .env   # fill in PRIVATE_KEY of a Fuji-funded test wallet (faucet: https://faucet.avax.network/)
npx hardhat compile
npx hardhat run scripts/deploy.ts --network fuji
```

This writes the deployed address to `contracts/deployments/fuji.json` and the ABI to `src-tauri/abi/Escrow.abi.json` + `src/abi/Escrow.abi.json`. Copy the deployed address into `src-tauri/.env` as `ESCROW_CONTRACT_ADDRESS` (see `src-tauri/.env.example`).

### Run Development Server

```bash
npm run tauri dev
```

This will:
1. Start the Vite dev server (frontend)
2. Initialize the Rust Tauri backend
3. Launch the mesh network with mDNS discovery
4. Open the Nexus UI

## 💻 Usage

### The Nexus Interface

The main UI displays:

- **Central Radar**: Pulsing violet circle representing your mesh presence
- **Peer Dots**: Violet dots appear as nearby nodes are discovered
- **Status Bar**: Shows Internet, Mesh Nodes count, and Privacy level
- **Intent Composer**: Command bar at bottom for entering privacy intents
- **Thought Stream**: Live log of operations on the right side

### Example Intent

```
Buy 10 AVAX under $95 using Shark Mode
```

The system will:
1. Generate a Noir ZK-proof of your balance
2. Negotiate via Ollama AI (localhost:11434)
3. Broadcast encrypted intent to mesh
4. Settle via the on-chain Escrow contract on Avalanche when online

### Going Offline

1. **Disconnect Wi-Fi** - The Internet LED turns red
2. **Post Intent** - Data flows through mesh (Mesh LED stays green)
3. **Reconnect** - Settlement executes on Avalanche

## 🔧 Project Structure

```
cabalmesh/
├── src/                          # React frontend
│   ├── App.tsx                   # Nexus UI
│   ├── styles.css                # Tailwind + custom styles
│   ├── avalanche-settlement.ts   # Read-only Avalanche helper (ethers.js v6)
│   ├── abi/Escrow.abi.json       # Escrow contract ABI (frontend copy)
│   └── main.tsx                  # Entry point
├── src-tauri/                    # Rust backend
│   └── src/
│       ├── mesh.rs               # libp2p mesh networking
│       ├── agent.rs              # Ollama AI integration
│       ├── zk_handler.rs         # Noir ZK proofs
│       ├── blockchain_bridge.rs  # Avalanche identity/RPC/Escrow bridge (alloy)
│       └── lib.rs                # Tauri commands
├── noir-circuit/                 # Noir ZK circuits
│   └── src/
│       └── main.nr               # Bid verification circuit
├── contracts/                    # Hardhat project
│   ├── contracts/Escrow.sol      # On-chain escrow contract
│   ├── scripts/deploy.ts         # Deploys to Fuji, hands off ABI
│   └── test/Escrow.test.ts       # Contract unit tests
└── README.md
```

## 🎨 Key Features

### 1. Offline Intent Execution
Post tasks while completely offline. Local mesh agents relay, negotiate, and sign deals, only hitting Avalanche when an internet gateway is reached.

### 2. Verifiable Aggression
Noir proofs ensure your AI agent followed your "Aggressive" strategy without cheating or leaking your price ceiling.

### 3. Sybil-Resistant ZK-Reputation
Nodes prove honesty via zero-knowledge without revealing interaction history.

### 4. On-Chain Escrow
Deals lock native AVAX in a minimal `Escrow.sol` contract (deposit → release, or depositor/expiry-based refund) instead of being purely simulated.

## 🧪 Testing

### Rust Backend

```bash
cd src-tauri
cargo check  # Verify compilation
cargo test   # Run tests (when added)
```

### Frontend

```bash
npm run build    # Production build
npm run preview  # Preview build
```

### Smart Contract

```bash
cd contracts
npx hardhat test   # Runs against Hardhat's in-memory network, no real funds
```

### Multi-Node Mesh Test

1. Run two instances on different network interfaces
2. Watch mDNS peer discovery in console
3. Send intent from one node
4. Observe Gossipsub propagation

## 🔐 Privacy Layers

| Layer | Technology | Purpose |
|-------|-----------|---------|
| **Physical** | Libp2p + mDNS | Hide IP/location |
| **Negotiation** | Ollama + FHE | Protect strategy |
| **Verification** | Noir ZK | Prove without revealing |
| **Settlement** | On-chain Escrow (Avalanche) | Trustless deal settlement |

## 📦 Dependencies

### Rust
- `libp2p` - P2P networking
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `serde` - Serialization
- `alloy` - Avalanche/EVM signing, RPC, and contract calls

### TypeScript
- `@tauri-apps/api` - Tauri IPC
- `react` - UI framework
- `framer-motion` - Animations
- `ethers` - Read-only Avalanche RPC helper (v6)

### Contracts
- `hardhat` + `@nomicfoundation/hardhat-toolbox` - Solidity compile/test/deploy
- `@openzeppelin/contracts` - `ReentrancyGuard` for the Escrow contract

## 🎯 Use Cases

1. **Hyper-Local Confidential Trade**: P2P marketplaces in disaster zones, festivals, or censored regions
2. **Institutional Execution**: Hide market entry/exit from public order books
3. **Private AI Labor**: Outsource tasks without revealing identities

## 🤝 Contributing

Contributions welcome!

1. Fork the repo
2. Create your feature branch
3. Commit changes
4. Push and open a PR

## 📄 License

MIT License - see LICENSE file

## 🙏 Acknowledgments

- **Avalanche** - C-Chain settlement layer
- **Noir** - Zero-knowledge circuits
- **libp2p** - P2P networking
- **Tauri** - Cross-platform desktop framework
- **Ollama** - Local AI models
