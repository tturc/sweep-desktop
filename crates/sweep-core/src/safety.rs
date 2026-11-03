use crate::model::Safety;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub safety: Safety,
    pub reason: Option<String>,
}

impl Assessment {
    fn safe() -> Self {
        Assessment {
            safety: Safety::Safe,
            reason: None,
        }
    }

    fn review(reason: impl Into<String>) -> Self {
        Assessment {
            safety: Safety::Review,
            reason: Some(reason.into()),
        }
    }

    fn blocked(reason: impl Into<String>) -> Self {
        Assessment {
            safety: Safety::Blocked,
            reason: Some(reason.into()),
        }
    }
}

/// Classify one scanned item. Directory inspection is top-level only: a
/// marker or a credential in the folder is enough to hold the whole folder.
pub fn assess(path: &Path, name: &str, is_dir: bool, is_repo: bool) -> Assessment {
    if let Some(reason) = reparse_reason(path) {
        return Assessment::blocked(reason);
    }

    let lower = name.to_ascii_lowercase();
    if let Some(reason) = sensitive_name(&lower) {
        return Assessment::blocked(reason);
    }

    if is_dir {
        if let Some(outcome) = inspect_dir(path) {
            return outcome;
        }
    }

    if is_repo {
        return Assessment::review("git repository; moving it breaks IDE recent paths");
    }

    if lower.contains("key") {
        return Assessment::review("name looks key-like; held for a human decision");
    }

    Assessment::safe()
}

fn sensitive_name(name: &str) -> Option<String> {
    let blocked = name.starts_with("id_rsa")
        || name.starts_with("id_dsa")
        || name.starts_with("id_ecdsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("gitaccess")
        || name.starts_with("masterkey")
        || name.contains("secret")
        || name.ends_with(".pem")
        || name.ends_with(".ppk")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".kdbx")
        || name.ends_with(".cryptomator");

    if blocked {
        Some("credential or vault material; never auto-moved".to_string())
    } else {
        None
    }
}

fn inspect_dir(path: &Path) -> Option<Assessment> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return Some(Assessment::review("could not inspect folder contents")),
    };

    let mut names = Vec::new();
    let mut child_dirs = Vec::new();
    for entry in entries.flatten() {
        let child_path = entry.path();
        let child_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            child_dirs.push(child_path);
        }
        names.push(child_name);
    }

    let has = |needle: &str| names.iter().any(|name| name == needle);

    // Blocked markers win over every review rule. A vault that is also a git
    // repo is still a vault first.
    if has(".obsidian") {
        return Some(Assessment::blocked(
            "Obsidian vault; moving it breaks the vault path",
        ));
    }
    if names.iter().any(|name| {
        name == "masterkey.cryptomator"
            || name == "vault.cryptomator"
            || name.ends_with(".cryptomator")
    }) {
        return Some(Assessment::blocked(
            "Cryptomator vault; moving it breaks the vault and sync",
        ));
    }
    if has(".tmp.driveupload") || has(".tmp.drivedownload") {
        return Some(Assessment::blocked(
            "Google Drive sync folder; moving it breaks the sync",
        ));
    }
    if has(".dropbox") {
        return Some(Assessment::blocked(
            "Dropbox sync folder; moving it breaks the sync",
        ));
    }
    if has(".icloud") {
        return Some(Assessment::blocked(
            "iCloud sync folder; moving it breaks the sync",
        ));
    }
    if names.iter().any(|name| sensitive_name(name).is_some()) {
        return Some(Assessment::blocked(
            "folder contains credential or vault material; never auto-moved",
        ));
    }

    for child in &child_dirs {
        if child.join(".obsidian").is_dir() {
            return Some(Assessment::blocked(
                "contains an Obsidian vault; moving it breaks the vault path",
            ));
        }
    }

    if path.join(".git").exists() {
        return Some(Assessment::review(
            "git repository; moving it breaks IDE recent paths",
        ));
    }
    for child in &child_dirs {
        if child.join(".git").exists() {
            return Some(Assessment::review(
                "contains a git repository; moving it breaks IDE recent paths",
            ));
        }
    }

    if path.join("project.godot").is_file() {
        return Some(Assessment::review(
            "Godot project; moving it breaks recent-project paths",
        ));
    }
    for child in &child_dirs {
        if child.join("project.godot").is_file() {
            return Some(Assessment::review(
                "contains a Godot project; moving it breaks recent-project paths",
            ));
        }
    }

    let has_exe = names.iter().any(|name| name.ends_with(".exe"));
    if has_exe {
        return Some(Assessment::review(
            "contains an executable; moving it may break shortcuts or config",
        ));
    }

    None
}

fn reparse_reason(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_attributes() & 0x400 != 0 => Some(
                "reparse point (junction, symlink, or cloud placeholder); not moved".to_string(),
            ),
            Ok(_) => None,
            Err(_) => Some("could not inspect path attributes; held for review".to_string()),
        }
    }

    #[cfg(not(windows))]
    {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => Some("symlink; not moved".to_string()),
            Ok(_) => None,
            Err(_) => Some("could not inspect path attributes; held for review".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sweep-safety-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn plain_notes_are_safe() {
        let dir = fixture("notes");
        let path = dir.join("notes.txt");
        std::fs::write(&path, "hello").unwrap();
        let result = assess(&path, "notes.txt", false, false);
        assert_eq!(result.safety, Safety::Safe);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn private_keys_are_blocked() {
        let dir = fixture("key");
        let path = dir.join("id_rsa");
        std::fs::write(&path, "private").unwrap();
        let result = assess(&path, "id_rsa", false, false);
        assert_eq!(result.safety, Safety::Blocked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn obsidian_folders_are_blocked() {
        let dir = fixture("vault");
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        let result = assess(&dir, "vault", true, false);
        assert_eq!(result.safety, Safety::Blocked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_obsidian_vaults_are_blocked() {
        let dir = fixture("nested-vault");
        std::fs::create_dir_all(dir.join("Vault").join(".obsidian")).unwrap();
        let result = assess(&dir, "Obsidian", true, false);
        assert_eq!(result.safety, Safety::Blocked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_folders_are_blocked() {
        let dir = fixture("keys");
        std::fs::write(dir.join("id_rsa"), "private").unwrap();
        let result = assess(&dir, "Keys", true, false);
        assert_eq!(result.safety, Safety::Blocked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_repos_are_review() {
        let dir = fixture("repo");
        let result = assess(&dir, "repo", true, true);
        assert_eq!(result.safety, Safety::Review);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn godot_projects_are_review() {
        let dir = fixture("godot");
        std::fs::write(dir.join("project.godot"), "").unwrap();
        let result = assess(&dir, "game", true, false);
        assert_eq!(result.safety, Safety::Review);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn portable_apps_are_review() {
        let dir = fixture("portable");
        std::fs::write(dir.join("tool.exe"), "").unwrap();
        std::fs::write(dir.join("tool.ini"), "").unwrap();
        let result = assess(&dir, "tool", true, false);
        assert_eq!(result.safety, Safety::Review);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ambiguous_key_names_are_review() {
        let dir = fixture("keyboard");
        let path = dir.join("Keyboard shortcuts.pdf");
        std::fs::write(&path, "").unwrap();
        let result = assess(&path, "Keyboard shortcuts.pdf", false, false);
        assert_eq!(result.safety, Safety::Review);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
