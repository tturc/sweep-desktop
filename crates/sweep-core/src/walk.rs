use crate::classify::{classify, has_git_remote};
use crate::model::{Kind, Safety};
use crate::safety;
use anyhow::Result;
use std::path::Path;
use walkdir::{DirEntry, WalkDir};

const SKIP_NAMES: &[&str] = &["desktop.ini", "Thumbs.db", ".DS_Store", "$RECYCLE.BIN"];

pub struct ScannedItem {
    pub rel_path: String,
    pub name: String,
    pub ext: Option<String>,
    pub kind: Kind,
    pub size: u64,
    pub mtime: i64,
    pub ctime: Option<i64>,
    pub is_repo: bool,
    pub has_remote: bool,
    pub is_hidden: bool,
    pub is_placeholder: bool,
    pub safety: Safety,
    pub safety_reason: Option<String>,
}

pub fn scan_root(root: &Path, depth_limit: Option<usize>) -> Result<Vec<ScannedItem>> {
    let mut out = Vec::new();
    let mut walker = WalkDir::new(root).follow_links(false);
    if let Some(depth) = depth_limit {
        walker = walker.max_depth(depth);
    }

    for entry in walker.into_iter().filter_entry(|entry| !is_skipped(entry)) {
        let entry = entry?;
        if entry.path() == root {
            continue;
        }

        let metadata = entry.metadata()?;
        let is_dir = metadata.is_dir();
        let is_repo = is_dir && entry.path().join(".git").is_dir();
        let has_remote = is_repo && has_git_remote(entry.path());
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        let kind = classify(entry.path(), is_dir, is_repo);
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        let ctime = metadata
            .created()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64);

        let assessment = safety::assess(entry.path(), &name, is_dir, is_repo);

        out.push(ScannedItem {
            rel_path: rel.to_string_lossy().to_string(),
            name: name.clone(),
            ext,
            kind,
            size: if is_dir { 0 } else { metadata.len() },
            mtime,
            ctime,
            is_repo,
            has_remote,
            is_hidden: name.starts_with('.'),
            is_placeholder: false,
            safety: assessment.safety,
            safety_reason: assessment.reason,
        });
    }

    Ok(out)
}

fn is_skipped(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    if SKIP_NAMES
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
    {
        return true;
    }
    entry.file_type().is_dir() && name == ".git"
}
