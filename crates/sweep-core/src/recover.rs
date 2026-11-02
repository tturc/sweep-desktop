use crate::db;
use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn run(conn: &mut Connection) -> Result<()> {
    let plans = db::list_plans(conn)?;
    let mut fixed = 0;

    for plan in plans {
        if !["applying", "applied", "partial"].contains(&plan.status.as_str()) {
            continue;
        }
        let ops = db::list_ops(conn, plan.id)?;
        for op in ops {
            if op.state != "pending" && op.state != "undoing" {
                continue;
            }

            let src = Path::new(&op.src);
            let dst_text = op
                .dst_actual
                .as_deref()
                .or(op.dst.as_deref())
                .unwrap_or("");
            let dst = Path::new(dst_text);
            let src_exists = src.exists();
            let dst_exists = dst.exists();

            match (op.state.as_str(), src_exists, dst_exists) {
                ("pending", true, false) => {
                    println!("pending, not applied: {}", src.display());
                }
                ("pending", false, true) => {
                    let actual = dst.to_string_lossy().to_string();
                    db::update_op_state(conn, op.id, "done", Some(&actual), None)?;
                    fixed += 1;
                }
                ("undoing", true, false) => {
                    db::update_op_state(conn, op.id, "undone", None, None)?;
                    fixed += 1;
                }
                ("undoing", false, true) => {
                    db::update_op_state(conn, op.id, "done", None, None)?;
                    fixed += 1;
                }
                (_, true, true) => {
                    println!(
                        "ambiguous, both exist: {} / {}",
                        src.display(),
                        dst.display()
                    );
                }
                (_, false, false) => {
                    println!(
                        "lost, neither exists: {} / {}",
                        src.display(),
                        dst.display()
                    );
                }
                _ => {}
            }
        }
    }

    println!("recovered {} ops", fixed);
    Ok(())
}
