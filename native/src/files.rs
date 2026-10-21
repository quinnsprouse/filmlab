use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// The newest write asked of each file by `write_atomic_soon`, and the lock its writes take turns on.
static NEWEST: LazyLock<Mutex<HashMap<PathBuf, u64>>> = LazyLock::new(Mutex::default);
static WRITING: Mutex<()> = Mutex::new(());

/// The temporary file `path` is written to before it replaces `path`: hidden, beside it, and unique to this write.
fn temporary(path: &Path) -> io::Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{} has no file name", path.display())))?;
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(path.with_file_name(format!(".{}.{}-{n}.tmp", name.to_string_lossy(), std::process::id())))
}

// why: a crash or power cut during a plain write leaves the file truncated; written whole and flushed to disk under
// another name first, the rename swaps it in at once, so the file is always the old one or the new one
/// Replaces `path` with `bytes`, keeping the permissions it had and writing through a link to the file it names.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let temp = temporary(&target)?;
    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        if let Ok(meta) = fs::metadata(&target) {
            file.set_permissions(meta.permissions())?;
        }
        file.sync_all()?;
        fs::rename(&temp, &target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written?;
    if let Some(dir) = target.parent().and_then(|d| File::open(d).ok()) {
        let _ = dir.sync_all();
    }
    Ok(())
}

// why: flushing a file to disk takes milliseconds on macOS (a full fsync), too long for the UI thread
/// Replaces `path` with `bytes` as `write_atomic` does, on another thread; of writes asked in quick succession only
/// the newest lands.
pub fn write_atomic_soon(path: PathBuf, bytes: Vec<u8>) {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    NEWEST.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), n);
    std::thread::spawn(move || {
        let _turn = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        if NEWEST.lock().unwrap_or_else(|e| e.into_inner()).get(&path) == Some(&n) {
            let _ = write_atomic(&path, &bytes);
        }
    });
}

/// Whether `name` is a file `write_atomic` was writing when the app stopped.
pub fn is_temporary(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(".tmp")
}
