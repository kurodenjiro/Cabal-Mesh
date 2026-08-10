# 🧪 CabalMesh - Testing Guide

Follow this guide to verify all 4 layers of the CabalMesh privacy stack (see
the Privacy Layers table in [README.md](README.md)): mesh networking, AI
negotiation, ZK verification, and on-chain settlement on Avalanche.

## 🚀 1. Start the Application

```bash
npm run tauri dev
```

Wait for `✅ System Bootstrap Complete. Mesh Swarm Active.` in the terminal —
the app then opens on the Home screen.

---

## 🔒 2. Test Privacy & Zero-Knowledge (ZK) Layer

**Action:**
1. From Home, open **New** and compose an intent, e.g.:
   ```
   Buy 10 AVAX under $95 using Shark Mode
   ```
2. Broadcast it. This takes you to the **Detail** screen.

**Verify in UI (Detail):**
- The status badge moves from `BROADCAST` toward `NEGOTIATING`.
- The **VERIFICATION LOG** panel fills in as events arrive.

**Verify in Terminal:**
- `🔐 Generating Noir ZK-Proof...` followed by the balance / bid / price
  ceiling lines.

---

## 🤖 3. Test AI Agent Layer (Ollama)

**Action:**
- Watch the Detail screen after broadcasting.

**Verify in Terminal:**
- Ollama-backed negotiation activity from the local agent (`agent.rs`). If no
  Ollama instance is reachable, the terminal logs a warning instead — see
  Troubleshooting below.

---

## 📡 4. Test Mesh Networking Layer

**Verify in Terminal:**
- `intent broadcast` on the sending node.
- On a peer: `peer discovered`, then `📬 Received Intent: ...`.
- Single-node, no peers: `⚠️ Note: No peers connected (Single-Node Mode).
  Intent processed locally.` — this is expected, not an error.

---

## ⚡ 5. Test Avalanche Settlement Layer

**Action:**
- Once negotiation completes, settle the intent from the Detail screen.

**Verify in UI:**
- The app moves to **Settled**, showing the **PROOF** panel with the on-chain
  transaction hash and settlement time.

**Verify in Terminal:**
- `✅ [Bridge] Escrow <id> created. Tx: ...`, then `✅ [Bridge] Escrow <id>
  released. Tx: ...` (or `refunded`, depending on outcome).

---

## 🔌 6. Test Offline Mode (Optional)

1. Turn off your Wi-Fi / Internet.
2. Compose and broadcast a new intent.
3. **Result:** The app should still generate the ZK proof and attempt to
   broadcast to the local mesh, even without internet. The transaction is
   signed offline and queued for mesh relay
   (`📡 [Bridge] Signed offline, queued for mesh relay: ...`).
4. Reconnect — the terminal should log `queued transaction confirmed after
   reconnect` once the queued transaction lands on-chain.

---

## 🧪 7. Multi-Node Simulation (Single Folder!)

You can run two instances from the **same folder** without copying anything.

**Step 1: Start Node A (Default)**
```bash
npm run tauri dev
```

**Step 2: Start Node B (Port 1421)**
Open a **new terminal** in the same folder and run:
```bash
PORT=1421 npm run tauri dev -- --config src-tauri/tauri.node2.conf.json
```

**Verify:**
- Node A runs on port 1420
- Node B runs on port 1421
- They will automatically discover each other! 🟣🟣

---

## ❌ Troubleshooting

- **Stuck on "Generating Proof"?**
  - Restart the app. The background process might have desynced.
  - ZK proving shells out to the `nargo` binary and is desktop-only — it is
    not available on iOS/Android builds.
- **Ollama Error?**
  - Run `ollama serve` manually in a separate terminal to see detailed logs,
    or point the app at a remote instance with `CABALMESH_OLLAMA_URL`.
- **"Insufficient Peers"?**
  - This is normal for single-node testing. To test peer discovery, run
    `npm run tauri dev` on a second machine on the same Wi-Fi, or use the
    multi-node simulation above.
