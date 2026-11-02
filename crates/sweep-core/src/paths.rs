use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn default_roots() -> Vec<(PathBuf, &'static str)> {
    let mut roots = Vec::new();
    if let Some(path) = dirs::desktop_dir() {
        roots.push((path, "desktop"));
    }
    if let Some(path) = dirs::download_dir() {
        roots.push((path, "downloads"));
    }
    roots
}

pub fn default_workspace() -> PathBuf {
    #[cfg(windows)]
    {
        return PathBuf::from(r"C:\work");
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Sweep")
    }
}

pub fn default_db_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sweep")
        .join("sweep.db")
}

pub fn ensure_workspace_dir(preferred: Option<PathBuf>) -> Result<PathBuf> {
    let candidate = preferred.unwrap_or_else(default_workspace);
    match std::fs::create_dir_all(&candidate) {
        Ok(()) => Ok(candidate),
        Err(err) => {
            #[cfg(windows)]
            {
                if candidate == PathBuf::from(r"C:\work") {
                    let fallback = dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("work");
                    std::fs::create_dir_all(&fallback)?;
                    return Ok(fallback);
                }
            }
            Err(err.into())
        }
    }
}

pub fn canonicalize_root(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).with_context(|| format!("cannot resolve root {}", path.display()))
}

pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}
