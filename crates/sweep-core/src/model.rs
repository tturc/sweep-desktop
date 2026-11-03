use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Script,
    Shortcut,
    Installer,
    Archive,
    Note,
    Doc,
    Image,
    Video,
    Audio,
    Repo,
    Folder,
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Script => "script",
            Kind::Shortcut => "shortcut",
            Kind::Installer => "installer",
            Kind::Archive => "archive",
            Kind::Note => "note",
            Kind::Doc => "doc",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Repo => "repo",
            Kind::Folder => "folder",
            Kind::Other => "other",
        }
    }

    pub fn from_str(value: &str) -> Kind {
        match value {
            "script" => Kind::Script,
            "shortcut" => Kind::Shortcut,
            "installer" => Kind::Installer,
            "archive" => Kind::Archive,
            "note" => Kind::Note,
            "doc" => Kind::Doc,
            "image" => Kind::Image,
            "video" => Kind::Video,
            "audio" => Kind::Audio,
            "repo" => Kind::Repo,
            "folder" => Kind::Folder,
            _ => Kind::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    Bench,
    Kits,
    Shelf,
    Cold,
    Inbox,
}

impl Lane {
    pub fn as_str(self) -> &'static str {
        match self {
            Lane::Bench => "bench",
            Lane::Kits => "kits",
            Lane::Shelf => "shelf",
            Lane::Cold => "cold",
            Lane::Inbox => "inbox",
        }
    }

    pub fn all() -> [Lane; 5] {
        [Lane::Bench, Lane::Kits, Lane::Shelf, Lane::Cold, Lane::Inbox]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
    Safe,
    Review,
    Blocked,
}

impl Default for Safety {
    fn default() -> Self {
        Safety::Safe
    }
}

impl Safety {
    pub fn as_str(self) -> &'static str {
        match self {
            Safety::Safe => "safe",
            Safety::Review => "review",
            Safety::Blocked => "blocked",
        }
    }

    pub fn is_safe(self) -> bool {
        matches!(self, Safety::Safe)
    }

    pub fn from_str(value: &str) -> Safety {
        match value {
            "review" => Safety::Review,
            "blocked" => Safety::Blocked,
            _ => Safety::Safe,
        }
    }
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Confidence::High => 3,
            Confidence::Medium => 2,
            Confidence::Low => 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "code", content = "value", rename_all = "snake_case")]
pub enum Reason {
    Age(i64),
    Kind(Kind),
    Size(u64),
    GitRemote(bool),
    Root(String),
    NamePattern(String),
}

impl Reason {
    pub fn label(&self) -> String {
        match self {
            Reason::Age(days) => format!("{} days untouched", days),
            Reason::Kind(kind) => kind.as_str().to_string(),
            Reason::Size(bytes) => format_size(*bytes),
            Reason::GitRemote(true) => "git remote present".to_string(),
            Reason::GitRemote(false) => "no git remote".to_string(),
            Reason::Root(root) => format!("{} root", root),
            Reason::NamePattern(pattern) => pattern.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub root_id: i64,
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
    pub pinned: bool,
    pub review_by: Option<i64>,
    #[serde(default)]
    pub safety: Safety,
    #[serde(default)]
    pub safety_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub lane: Lane,
    pub confidence: Confidence,
    pub reasons: Vec<Reason>,
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}
