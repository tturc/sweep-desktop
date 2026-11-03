use crate::db::{RootRow, ZoneRow};
use crate::model::{Confidence, Item, Lane, Safety};
use crate::paths;
use crate::safety;
use crate::suggest;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFile {
    pub version: u32,
    pub plan_id: Option<i64>,
    pub workspace: String,
    pub created_at: String,
    pub ops: Vec<PlanOp>,
    #[serde(default)]
    pub held: Vec<HeldItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOp {
    pub seq: i64,
    pub kind: String,
    pub src: String,
    pub dst: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeldItem {
    pub name: String,
    pub path: String,
    pub lane: String,
    pub safety: Safety,
    pub reason: String,
}

pub fn build(
    roots: &[RootRow],
    zones: &[ZoneRow],
    items: &[Item],
    lane_filter: Option<Lane>,
    min_confidence: Confidence,
    limit: usize,
) -> PlanFile {
    let root_map: HashMap<i64, &RootRow> = roots.iter().map(|root| (root.id, root)).collect();
    let zone_map: HashMap<&str, &ZoneRow> = zones
        .iter()
        .map(|zone| (zone.name.as_str(), zone))
        .collect();
    let mut ops = Vec::new();
    let mut held = Vec::new();
    let mut used: HashSet<String> = HashSet::new();

    for item in items {
        // Plans move top-level items only. A nested file moves with its folder,
        // never as a second op that would race the parent.
        if Path::new(&item.rel_path).components().count() > 1 {
            continue;
        }

        let Some(root) = root_map.get(&item.root_id) else {
            continue;
        };
        let Some(suggestion) = suggest::suggest(item, &root.kind) else {
            continue;
        };
        if let Some(lane) = lane_filter {
            if suggestion.lane != lane {
                continue;
            }
        }

        let src = Path::new(&root.path).join(&item.rel_path);
        let assessment = safety::assess(&src, &item.name, src.is_dir(), item.is_repo);
        if !assessment.safety.is_safe() {
            held.push(HeldItem {
                name: item.name.clone(),
                path: paths::display_path(&src),
                lane: suggestion.lane.as_str().to_string(),
                safety: assessment.safety,
                reason: assessment
                    .reason
                    .unwrap_or_else(|| "held for a human decision".to_string()),
            });
            continue;
        }

        if suggestion.confidence.rank() < min_confidence.rank() {
            continue;
        }
        if lane_filter.is_none() && suggestion.lane == Lane::Bench {
            continue;
        }
        let Some(zone) = zone_map.get(suggestion.lane.as_str()) else {
            continue;
        };
        let Some(zone_path) = &zone.path else {
            continue;
        };

        let dst = Path::new(zone_path).join(&item.rel_path);
        if src == dst {
            continue;
        }
        let dst = unique_dst(dst, &mut used);

        ops.push(PlanOp {
            seq: ops.len() as i64 + 1,
            kind: "move".to_string(),
            src: paths::display_path(&src),
            dst: paths::display_path(&dst),
            reason: suggestion
                .reasons
                .iter()
                .map(|reason| reason.label())
                .collect::<Vec<_>>()
                .join(", "),
        });

        if ops.len() >= limit {
            break;
        }
    }

    PlanFile {
        version: 1,
        plan_id: None,
        workspace: "default".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ops,
        held,
    }
}

fn unique_dst(path: PathBuf, used: &mut HashSet<String>) -> PathBuf {
    let key = path_key(&path);
    if !path.exists() && !used.contains(&key) {
        used.insert(key);
        return path;
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value))
        .unwrap_or_default();

    for index in 2..1000 {
        let candidate = parent.join(format!("{} ({}){}", stem, index, extension));
        let candidate_key = path_key(&candidate);
        if !candidate.exists() && !used.contains(&candidate_key) {
            used.insert(candidate_key);
            return candidate;
        }
    }

    used.insert(key);
    path
}

fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        text.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        text.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Kind, Safety};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sweep-plan-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item(root_id: i64, rel_path: &str, name: &str, kind: Kind, mtime: i64) -> Item {
        Item {
            id: 1,
            root_id,
            rel_path: rel_path.to_string(),
            name: name.to_string(),
            ext: None,
            kind,
            size: 10,
            mtime,
            ctime: None,
            is_repo: false,
            has_remote: false,
            is_hidden: false,
            is_placeholder: false,
            pinned: false,
            review_by: None,
            safety: Safety::Safe,
            safety_reason: None,
        }
    }

    #[test]
    fn unique_dst_suffixes_duplicates() {
        let dir = fixture("dedupe");
        let mut used = HashSet::new();
        let first = unique_dst(dir.join("report.txt"), &mut used);
        let second = unique_dst(dir.join("report.txt"), &mut used);
        assert_eq!(first, dir.join("report.txt"));
        assert_eq!(second, dir.join("report (2).txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocked_items_are_held_and_never_planned() {
        let dir = fixture("held");
        let root = dir.join("Desktop");
        let kits = dir.join("Sweep").join("kits");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&kits).unwrap();
        std::fs::create_dir_all(root.join("Vault").join(".obsidian")).unwrap();
        std::fs::write(root.join("run.ps1"), "echo hi").unwrap();

        let old = chrono::Utc::now().timestamp() - 10 * 86_400;
        let roots = vec![RootRow {
            id: 1,
            path: root.to_string_lossy().to_string(),
            kind: "desktop".to_string(),
            depth_limit: Some(1),
        }];
        let zones = vec![ZoneRow {
            id: 1,
            name: "kits".to_string(),
            path: Some(kits.to_string_lossy().to_string()),
            cap: None,
        }];
        let items = vec![
            item(1, "run.ps1", "run.ps1", Kind::Script, old),
            item(1, "Vault", "Vault", Kind::Folder, old),
        ];

        let plan = build(
            &roots,
            &zones,
            &items,
            None,
            Confidence::High,
            50,
        );
        assert_eq!(plan.ops.len(), 1);
        assert!(plan.ops[0].src.ends_with("run.ps1"));
        assert!(plan
            .held
            .iter()
            .any(|held| held.name == "Vault" && held.safety == Safety::Blocked));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
