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
        if !roots
            .iter()
            .any(|root| src_canon.starts_with(Path::new(&root.path)))
        {
            bail!("source outside registered roots: {}", src.display());
        }

        let dst_parent = dst
            .parent()
            .ok_or_else(|| anyhow::anyhow!("destination has no parent: {}", dst.display()))?;
        std::fs::create_dir_all(dst_parent)?;
        let dst_parent_canon = dst_parent.canonicalize()?;

        let inside_zone = zones
            .iter()
            .filter_map(|zone| zone.path.as_deref())
            .any(|zone_path| {
                let zone_canon = Path::new(zone_path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(zone_path));
                dst_parent_canon.starts_with(&zone_canon)
            });
        if !inside_zone {
            bail!("destination outside zones: {}", dst.display());
        }

        if dst.exists() && !suffix {
            bail!("destination exists: {}", dst.display());
        }
        if !same_volume(&src_canon, &dst_parent_canon) {
            bail!("cross-volume move not supported: {}", dst.display());
        }
    }
    Ok(())
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
