# Ollama Auto-Start Integration

## What Changed

The **CabalMesh** app now **automatically manages Ollama** for you! No need to manually run `ollama serve` anymore.

## How It Works

When you start the app (`npm run tauri dev`), it will:

1. **Check** if Ollama is installed on your system
2. **Start** the Ollama service automatically in the background
3. **Pull** the llama2 model if not already downloaded (first launch only)
4. **Verify** the service is healthy and responding
5. **Shut down** Ollama gracefully when you close the app

## First Time Setup

### If Ollama is NOT installed:

The app will show:
```
⚠️  Ollama not found!
📝 Please install from: https://ollama.ai
   Or run: brew install ollama
```

**Install Ollama once:**
```bash
brew install ollama
```

### If Ollama IS installed:

The app will automatically:
```
🔍 Checking Ollama installation...
🤖 Starting Ollama service...
✅ Ollama service started
📥 Pulling model llama2 (this may take a few minutes)...
✅ Model llama2 downloaded successfully
✅ Ollama ready!
✅ Ollama service is healthy
```

## Usage

### Start the App
```bash
npm run tauri dev
```

That's it! No need to run `ollama serve` separately.

### Testing with AI

Enter an intent in the command bar:
```
Buy 10 SOL under $95 using Shark Mode
```

The app will automatically:
- Use the Ollama service running in the background
- Negotiate via llama2 AI agent
- Display results in the Thought Stream

## Technical Details

### New Module: `ollama_manager.rs`

```rust
pub struct OllamaManager {
    process: Arc<Mutex<Option<Child>>>,
    model_name: String,
}
```

**Key Functions:**
- `is_installed()` - Checks if Ollama CLI is available
- `start_service()` - Spawns `ollama serve` as background process
- `pull_model()` - Downloads AI model if missing
- `initialize()` - Combined startup routine
- `health_check()` - Verifies service is responding
- `stop_service()` - Graceful shutdown on app close

### Integration in `lib.rs`

The Ollama manager runs in a separate async task during Tauri app setup:

```rust
tauri::async_runtime::spawn(async move {
    let ollama = OllamaManager::new(Some("llama2".to_string()));
    ollama.initialize().await?;
    // Health check loop
});
```

### Model Configuration

Default model: `llama2`

To use a different model, modify in `lib.rs`:
```rust
OllamaManager::new(Some("llama3".to_string()))
```

## Benefits

✅ **One-Click Launch** - No manual Ollama startup required  
✅ **Auto-Pull Models** - First-time users get the model automatically  
✅ **Clean Shutdown** - Ollama process is killed when app closes  
✅ **Health Monitoring** - Automatic verification that service is responding  
✅ **Better UX** - Users don't need to know Ollama exists

## Troubleshooting

**Issue**: App shows "Ollama not responding"
- **Solution**: Check terminal for errors, ensure port 11434 is available

**Issue**: Model download is slow
- **Solution**: This is normal on first launch (3.8 GB download)

**Issue**: Connection refused
- **Solution**: Wait a few seconds after app start for Ollama to initialize

---

**Status**: ✅ Ollama is now fully integrated into CabalMesh!
