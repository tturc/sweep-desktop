use crate::model::Kind;
use std::path::Path;

pub fn classify(path: &Path, is_dir: bool, is_repo: bool) -> Kind {
    if is_dir {
        return if is_repo { Kind::Repo } else { Kind::Folder };
    }

    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "ps1" | "sh" | "bat" | "cmd" | "py" | "js" | "ts" | "rb" | "lua" => Kind::Script,
        "lnk" | "url" | "desktop" => Kind::Shortcut,
        "exe" | "msi" | "msix" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" => Kind::Installer,
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" => Kind::Archive,
        "md" | "txt" | "rtf" => Kind::Note,
        "pdf" | "docx" | "xlsx" | "pptx" | "csv" => Kind::Doc,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" => Kind::Image,
        "mp4" | "mov" | "mkv" | "webm" | "avi" => Kind::Video,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => Kind::Audio,
        _ => Kind::Other,
    }
}

pub fn has_git_remote(path: &Path) -> bool {
    let config = path.join(".git").join("config");
    let Ok(text) = std::fs::read_to_string(config) else {
        return false;
    };
    text.lines()
        .any(|line| line.trim_start().starts_with("[remote "))
}
