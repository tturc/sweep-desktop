use crate::db;
use anyhow::{bail, Result};
use rusqlite::Connection;
use std::path::PathBuf;

pub fn run(conn: &mut Connection, plan_id: Option<i64>) -> Result<()> {
    let plan = match plan_id {
        Some(id) => db::get_plan(conn, id)?.ok_or_else(|| anyhow::anyhow!("plan {} not found", id))?,
        None => db::last_plan(conn)?.ok_or_else(|| anyhow::anyhow!("no plans to undo"))?,
    };
    let ops = db::list_ops(conn, plan.id)?;
    let mut undone = 0;

    for op in ops.iter().rev() {
        if op.state != "done" {
            continue;
        }

        let src = PathBuf::from(&op.src);
        let dst = PathBuf::from(
            op.dst_actual
                .as_deref()
                .or(op.dst.as_deref())
                .unwrap_or(""),
        );
        if dst.as_os_str().is_empty() {
            bail!("undo: op {} has no destination", op.seq);
        }
        if !dst.exists() {
            bail!("undo: destination missing: {}", dst.display());
        }
        if src.exists() {
            bail!("undo: source already exists: {}", src.display());
        }
        if let Some(parent) = src.parent() {
            std::fs::create_dir_all(parent)?;
        }

        db::update_op_state(conn, op.id, "undoing", None, None)?;
        match std::fs::rename(&dst, &src) {
            Ok(()) => {
                db::update_op_state(conn, op.id, "undone", None, None)?;
                undone += 1;
            }
            Err(err) => {
                db::update_op_state(conn, op.id, "failed", None, Some(&err.to_string()))?;
                db::update_plan_status(conn, plan.id, "partial")?;
                return Err(err.into());
            }
        }
    }

    if undone > 0 {
        db::update_plan_status(conn, plan.id, "undone")?;
    }
    println!("undid {} ops", undone);
    Ok(())
}
