use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

const REPO_OWNER: &str = "RickDeckardWebsim";
const REPO_NAME:  &str = "Voxlink";
#[cfg(target_os = "windows")]
const ASSET_NAME: &str = "voxlink-windows.zip";
#[cfg(target_os = "windows")]
const EXE_NAME:   &str = "voxlink.exe";

#[cfg(target_os = "linux")]
const ASSET_NAME: &str = "voxlink-linux.zip";
#[cfg(target_os = "linux")]
const EXE_NAME:   &str = "voxlink";

// ── Public types ──────────────────────────────────────────────────────────────

pub struct ReleaseInfo {
    pub version:    String,
    pub notes:      String,
    pub asset_url:  String,
    pub asset_size: u64,
}

pub enum UpdaterEvent {
    // Check results
    CheckStarted,
    UpdateAvailable(ReleaseInfo),
    AlreadyUpToDate,
    CheckFailed(String),
    // Download / install progress
    Phase(String),
    DownloadProgress { downloaded: u64, total: u64 },
    Finished,
    Failed(String),
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn check_for_updates(tx: mpsc::Sender<UpdaterEvent>) {
    thread::spawn(move || {
        let _ = tx.send(UpdaterEvent::CheckStarted);
        match fetch_release_info() {
            Ok(None)       => { let _ = tx.send(UpdaterEvent::AlreadyUpToDate); }
            Ok(Some(info)) => { let _ = tx.send(UpdaterEvent::UpdateAvailable(info)); }
            Err(e)         => { let _ = tx.send(UpdaterEvent::CheckFailed(e)); }
        }
    });
}

/// Download, extract, and replace the binary, sending progress events throughout.
/// `asset_url` and `asset_size` come from the `ReleaseInfo` returned by `check_for_updates`.
pub fn run_update(asset_url: String, asset_size: u64, tx: mpsc::Sender<UpdaterEvent>) {
    thread::spawn(move || {
        let _ = tx.send(UpdaterEvent::Phase("Downloading...".to_string()));
        let zip_path = match download_asset(&asset_url, asset_size, &tx) {
            Ok(p)  => p,
            Err(e) => { let _ = tx.send(UpdaterEvent::Failed(e)); return; }
        };

        let _ = tx.send(UpdaterEvent::Phase("Extracting...".to_string()));
        let new_exe = match extract_exe(&zip_path) {
            Ok(p)  => p,
            Err(e) => { let _ = tx.send(UpdaterEvent::Failed(e)); return; }
        };

        let _ = tx.send(UpdaterEvent::Phase("Installing — restarting shortly...".to_string()));
        if let Err(e) = replace_and_restart(&new_exe) {
            let _ = tx.send(UpdaterEvent::Failed(e));
        }
        // If replace_and_restart succeeds on Windows it calls process::exit(0),
        // so this line is only reached on non-Windows or on failure paths.
        let _ = tx.send(UpdaterEvent::Finished);
    });
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("voxlink-updater/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

fn fetch_release_info() -> Result<Option<ReleaseInfo>, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        REPO_OWNER, REPO_NAME
    );

    let client = http_client()?;
    let resp = client.get(&url).send().map_err(|e| e.to_string())?;

    if resp.status().as_u16() == 404 {
        return Ok(None); // No releases published yet
    }
    if !resp.status().is_success() {
        return Err(format!("GitHub API returned HTTP {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;

    let tag     = json["tag_name"].as_str().unwrap_or("0.0.0");
    let version = tag.trim_start_matches('v').to_string();
    let notes   = json["body"].as_str().unwrap_or("No release notes available.").to_string();

    // Only update if the release is actually newer.
    match self_update::version::bump_is_greater(env!("CARGO_PKG_VERSION"), &version) {
        Ok(true) => {}
        _        => return Ok(None),
    }

    let assets = match json["assets"].as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return Ok(None),
    };

    let asset = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(ASSET_NAME))
        .ok_or_else(|| format!("Asset '{}' not found in release", ASSET_NAME))?;

    let asset_url  = asset["browser_download_url"]
        .as_str()
        .ok_or("Missing download URL")?
        .to_string();
    let asset_size = asset["size"].as_u64().unwrap_or(0);

    Ok(Some(ReleaseInfo { version, notes, asset_url, asset_size }))
}

fn download_asset(
    url: &str,
    size_hint: u64,
    tx: &mpsc::Sender<UpdaterEvent>,
) -> Result<PathBuf, String> {
    let client   = http_client()?;
    let mut resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .map_err(|e| e.to_string())?;

    let total    = resp.content_length().unwrap_or(size_hint);
    let out_path = std::env::temp_dir().join("voxlink_update.zip");
    let mut file = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;

    let mut downloaded = 0u64;
    let mut buf        = [0u8; 65536]; // 64 KB chunks

    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        downloaded += n as u64;
        let _ = tx.send(UpdaterEvent::DownloadProgress { downloaded, total });
    }

    Ok(out_path)
}

fn extract_exe(zip_path: &Path) -> Result<PathBuf, String> {
    use std::io::Read;

    let file    = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut arc = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    if arc.len() == 0 {
        return Err("Zip archive is empty".to_string());
    }

    // Read the exe bytes into memory so ZipFile's borrow of `arc` ends before
    // we write to disk.  Our workflow puts exactly one file in the zip.
    let bytes: Vec<u8> = {
        let mut entry = arc.by_index(0).map_err(|e| e.to_string())?;
        let mut buf   = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        buf
    };

    let out_name = if cfg!(target_os = "windows") { "voxlink_staged.exe" } else { "voxlink_staged" };
    let out_path = std::env::temp_dir().join(out_name);
    std::fs::write(&out_path, &bytes).map_err(|e| e.to_string())?;
    Ok(out_path)
}

// ── Platform-specific replacement ─────────────────────────────────────────────

#[cfg(windows)]
fn replace_and_restart(new_exe: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let bat     = std::env::temp_dir().join("voxlink_update.bat");

    // The batch script waits for the current process to exit, copies the new
    // binary over the old path, launches it, then deletes itself.
    let script = format!(
        "@echo off\r\n\
         timeout /t 2 /nobreak >nul\r\n\
         copy /Y \"{new}\" \"{cur}\"\r\n\
         start \"\" \"{cur}\"\r\n\
         del \"%~f0\"\r\n",
        new = new_exe.display(),
        cur = current.display(),
    );
    std::fs::write(&bat, script).map_err(|e| e.to_string())?;

    std::process::Command::new("cmd")
        .args(["/C", bat.to_str().unwrap_or("")])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("Failed to launch update script: {}", e))?;

    std::process::exit(0);
}

#[cfg(not(windows))]
fn replace_and_restart(new_exe: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;

    // On non-Windows the running binary can be overwritten directly.
    std::fs::copy(new_exe, &current).map_err(|e| e.to_string())?;

    // Make it executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&current).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&current, perms).map_err(|e| e.to_string())?;
    }

    std::process::Command::new(&current)
        .spawn()
        .map_err(|e| e.to_string())?;

    std::process::exit(0);
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Format a byte count as a human-readable string (e.g. "3.6 MB", "512 KB").
pub fn format_bytes(bytes: u64) -> String {
    const MB: u64 = 1_048_576;
    const KB: u64 = 1_024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
