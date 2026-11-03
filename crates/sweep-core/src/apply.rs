use crate::db::{self, OpRow, RootRow, ZoneRow};
use crate::plan::PlanFile;
use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub fn run(conn: &mut Connection, plan: &PlanFile, do_apply: bool, suffix: bool) -> Result<()> {
    let roots = db::list_roots(conn)?;
    let zones = db::list_zones(conn)?;
    let (plan_id, ops) = ensure_plan(conn, plan)?;
    validate(&roots, &zones, &ops, suffix)?;

    if !do_apply {
        println!("dry run: {} ops validated; re-run with --apply", ops.len());
        return Ok(());
    }

    let mut applied = 0;
    // Test hook for the crash harness: slow each op so a kill lands mid-apply.
    let delay_ms = std::env::var("SWEEP_APPLY_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    for op in &ops {
        if op.state != "pending" {
            continue;
        }

        let src = PathBuf::from(&op.src);
        let planned = PathBuf::from(op.dst.as_deref().unwrap_or(""));
        if src.as_os_str().is_empty() || planned.as_os_str().is_empty() {
            bail!("op {} has an empty path", op.seq);
        }
        if let Some(parent) = planned.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let actual = if planned.exists() && suffix {
            unique_path(&planned)
        } else {
            planned.clone()
        };
        if actual.exists() {
            bail!("destination exists: {}", actual.display());
        }

        db::update_op_state(conn, op.id, "pending", None, None)?;
        db::update_plan_status(conn, plan_id, "applying")?;

        match std::fs::rename(&src, &actual) {
            Ok(()) => {
                let actual_text = actual.to_string_lossy().to_string();
                db::update_op_state(conn, op.id, "done", Some(&actual_text), None)?;
                applied += 1;
                if delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }
            Err(err) => {
                db::update_op_state(conn, op.id, "failed", None, Some(&err.to_string()))?;
                db::update_plan_status(conn, plan_id, "partial")?;
                return Err(err).with_context(|| {
                    format!(
                        "op {} failed: {} -> {}",
                        op.seq,
                        src.display(),
                        actual.display()
                    )
                });
            }
        }
    }

    db::update_plan_status(conn, plan_id, "applied")?;
    println!("applied {} ops", applied);
    Ok(())
}

fn validate(roots: &[RootRow], zones: &[ZoneRow], ops: &[OpRow], suffix: bool) -> Result<()> {
    for op in ops {
        if op.state != "pending" {
            continue;
        }

        let src = PathBuf::from(&op.src);
        let dst = PathBuf::from(op.dst.as_deref().unwrap_or(""));
        if !src.exists() {
            bail!("source missing: {}", src.display());
        }

        let src_canon = src
            .canonicalize()
            .with_context(|| format!("canonicalize {}", src.display()))?;
        if !roots.iter().any(|root| {
            comparable(&src_canon).starts_with(comparable(Path::new(&root.path)))
        }) {
            bail!("source outside registered roots: {}", src.display());
        }

        let dst_parent = dst
            .parent()
            .ok_or_else(|| anyhow::anyhow!("destination has no parent: {}", dst.display()))?;
        // Validation must not touch the filesystem. The destination may not
        // exist yet, so containment is checked on the lexical path and the
        // volume is probed from the nearest existing ancestor.
        let dst_parent_norm = comparable(&lexical_normalize(dst_parent));

        let inside_zone = zones
            .iter()
            .filter_map(|zone| zone.path.as_deref())
            .any(|zone_path| {
                let zone_norm = comparable(
                    &std::fs::canonicalize(Path::new(zone_path))
                        .unwrap_or_else(|_| lexical_normalize(Path::new(zone_path))),
                );
                dst_parent_norm.starts_with(&zone_norm)
            });
        if !inside_zone {
            bail!("destination outside zones: {}", dst.display());
        }

        if dst.exists() && !suffix {
            bail!("destination exists: {}", dst.display());
        }
        let dst_probe = existing_ancestor(&dst_parent_norm);
        let dst_probe_canon = comparable(
            &std::fs::canonicalize(&dst_probe).unwrap_or(dst_probe),
        );
        if !same_volume(&comparable(&src_canon), &dst_probe_canon) {
            bail!("cross-volume move not supported: {}", dst.display());
        }
    }
    Ok(())
}

fn comparable(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", rest));
        }
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }

    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

fn existing_ancestor(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    while !current.exists() {
        if !current.pop() {
            break;
        }
    }
    current
}

fn ensure_plan(conn: &mut Connection, plan: &PlanFile) -> Result<(i64, Vec<OpRow>)> {
    if let Some(id) = plan.plan_id {
        if db::get_plan(conn, id)?.is_some() {
            return Ok((id, db::list_ops(conn, id)?));
        }
    }

    let plan_id = db::insert_plan(conn, "{}", "draft")?;
    db::insert_ops(conn, plan_id, &plan.ops)?;
    let mut stored = plan.clone();
    stored.plan_id = Some(plan_id);
    db::update_plan_json(conn, plan_id, &serde_json::to_string(&stored)?)?;
    Ok((plan_id, db::list_ops(conn, plan_id)?))
}

fn unique_path(path: &Path) -> PathBuf {
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
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[cfg(windows)]
fn same_volume(first: &Path, second: &Path) -> bool {
    first.components().next() == second.components().next()
}

#[cfg(not(windows))]
fn same_volume(first: &Path, second: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(first), std::fs::metadata(second)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Lane;
    use crate::plan::{PlanFile, PlanOp};
    use crate::undo;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sweep-apply-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn setup(dir: &Path) -> (Connection, PathBuf, PathBuf) {
        let root = dir.join("Desktop");
        let zone = dir.join("Sweep").join("kits");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&zone).unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let zone = std::fs::canonicalize(&zone).unwrap();
        let db_path = dir.join("sweep.db");
        let conn = db::open(&db_path).unwrap();
        let workspace_id = db::ensure_workspace(&conn, "default").unwrap();
        db::ensure_zone(&conn, workspace_id, Lane::Kits, &zone, None, 0).unwrap();
        db::add_root(&conn, workspace_id, &root, "desktop", Some(1)).unwrap();
        (conn, root, zone)
    }

    fn plan(src: &Path, dst: &Path) -> PlanFile {
        PlanFile {
            version: 1,
            plan_id: None,
            workspace: "default".to_string(),
            created_at: "test".to_string(),
            ops: vec![PlanOp {
                seq: 1,
                kind: "move".to_string(),
                src: src.to_string_lossy().to_string(),
                dst: dst.to_string_lossy().to_string(),
                reason: "test".to_string(),
            }],
            held: vec![],
        }
    }

    #[test]
    fn dry_run_does_not_create_the_destination_parent() {
        let dir = fixture("dry-run");
        let (mut conn, root, zone) = setup(&dir);
        let src = root.join("notes.txt");
        std::fs::write(&src, "hello").unwrap();
        let dst = zone.join("new-parent").join("notes.txt");

        run(&mut conn, &plan(&src, &dst), false, false).unwrap();

        assert!(!zone.join("new-parent").exists());
        assert!(src.exists());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_targets_the_applied_plan_not_a_newer_draft() {
        let dir = fixture("undo");
        let (mut conn, root, zone) = setup(&dir);
        let src = root.join("notes.txt");
        let dst = zone.join("notes.txt");
        std::fs::write(&src, "hello").unwrap();

        run(&mut conn, &plan(&src, &dst), true, true).unwrap();
        assert!(dst.exists());
        assert!(!src.exists());

        // A preview after the apply creates a newer draft. Undo must skip it.
        db::insert_plan(&conn, "{}", "draft").unwrap();
        undo::run(&mut conn, None).unwrap();

        assert!(src.exists());
        assert!(!dst.exists());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
