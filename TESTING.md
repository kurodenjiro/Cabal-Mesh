# 🧪 CabalMesh - Testing Guide

Follow this guide to verify all 4 layers of the CabalMesh privacy stack.

## 🚀 1. Start the Application

```bash
npm run tauri dev
```
*Wait for the "Nobody" radar interface to appear.*

---

## 🔒 2. Test Privacy & Zero-Knowledge (ZK) Layer

**Action:**
1. Type this intent in the bottom command bar:
   ```
   Buy 10 SOL under $95 using Shark Mode
   ```
2. Press **Enter**.

**Verify in UI:**
- Status changes to: `🔐 Generating Zero-Knowledge Proof...`
- Log appears: `[Noir] Proof Generated ✓`

**Verify in Terminal:**
- You should see: `🔐 Generating Noir ZK-Proof...` followed by checks.

---

## 🤖 3. Test AI Agent Layer (Ollama)

**Action:**
- Watch the status after the ZK step.

**Verify in UI:**
- Status changes to: `🦈 Negotiating with AI Agent...`
- Log appears: `[Arcium] Strategy optimized`
- Log appears: `[Arcium] MPC Negotiation started`

**Verify in Terminal:**
- You should see the AI's JSON response with a strategy (e.g., "Shark Mode Agent will bid aggressively...").

---

## 📡 4. Test Mesh Networking Layer

**Action:**
- Watch the status after the AI step.

**Verify in UI:**
- Status changes to: `📡 Broadcasting to Mesh Network...`
- Log appears: `Intent broadcasted to mesh`

**Verify in Terminal:**
- You should see: `📤 Broadcasting intent to mesh: ...`
- *Note:* If running alone, you may see a warning `⚠️ Note: No peers connected`, which is expected!

---

## ⚡ 5. Test Solana Settlement Layer

**Action:**
- Watch the final step.

**Verify in UI:**
- Status changes to: `✅ Validating on Solana...`
- Log appears: `[SilentSwap] Finalizing on Solana...`

---

## 🔌 6. Test Offline Mode (Optional)

1. Turn off your Wi-Fi / Internet.
2. In the app, verify the **Internet** indicator (top-right) turns **RED**.
3. Submit a new intent.
4. **Result:** The app should still generate the ZK proof and attempt to broadcast to the local mesh (violet dots), even without internet!

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
- **Ollama Error?**
  - Run `ollama serve` manually in a separate terminal to see detailed logs.
- **"Insufficient Peers"?**
  - This is normal for single-node testing. To test peer discovery, run `npm run tauri dev` on a second computer on the same Wi-Fi.
