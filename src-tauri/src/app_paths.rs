use std::fs;
use std::path::PathBuf;

/// Directory holding this instance's on-disk state.
///
/// CABALMESH_DATA_DIR lets multiple isolated instances run side by side on one
/// machine (e.g. for a local 2-node mesh test) without sharing a wallet. On iOS
/// `dirs::data_dir()` resolves inside the app sandbox container.
pub fn data_dir() -> PathBuf {
    let dir = match std::env::var("CABALMESH_DATA_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cabalmesh"),
    };
    let _ = fs::create_dir_all(&dir);
    dir
}
