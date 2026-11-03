use crate::model::{Item, Kind, Lane, Safety};
use crate::plan::PlanOp;
use crate::walk::ScannedItem;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct RootRow {
    pub id: i64,
    pub path: String,
    pub kind: String,
    pub depth_limit: Option<i64>,
}

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS workspace (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS root (
            id INTEGER PRIMARY KEY,
            workspace_id INTEGER NOT NULL REFERENCES workspace(id),
            path TEXT NOT NULL,
            kind TEXT NOT NULL,
            depth_limit INTEGER,
            enabled INTEGER NOT NULL DEFAULT 1,
            UNIQUE(workspace_id, path)
        );

        CREATE TABLE IF NOT EXISTS zone (
            id INTEGER PRIMARY KEY,
            workspace_id INTEGER NOT NULL REFERENCES workspace(id),
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            position INTEGER NOT NULL,
            cap INTEGER,
            path TEXT,
            UNIQUE(workspace_id, name)
        );

        CREATE TABLE IF NOT EXISTS item (
            id INTEGER PRIMARY KEY,
            root_id INTEGER NOT NULL REFERENCES root(id),
            rel_path TEXT NOT NULL,
            name TEXT NOT NULL,
            ext TEXT,
            kind TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            ctime INTEGER,
            is_repo INTEGER NOT NULL DEFAULT 0,
            has_remote INTEGER NOT NULL DEFAULT 0,
            is_hidden INTEGER NOT NULL DEFAULT 0,
            is_placeholder INTEGER NOT NULL DEFAULT 0,
            zone_id INTEGER REFERENCES zone(id),
            pinned INTEGER NOT NULL DEFAULT 0,
            review_by INTEGER,
            safety TEXT NOT NULL DEFAULT 'safe',
            safety_reason TEXT,
            last_seen_scan INTEGER NOT NULL,
            UNIQUE(root_id, rel_path)
        );

        CREATE INDEX IF NOT EXISTS idx_item_root ON item(root_id);
        CREATE INDEX IF NOT EXISTS idx_item_kind ON item(kind);

        CREATE TABLE IF NOT EXISTS plan (
            id INTEGER PRIMARY KEY,
            created_at INTEGER NOT NULL,
            status TEXT NOT NULL,
            ops_json TEXT NOT NULL,
            note TEXT
        );

        CREATE TABLE IF NOT EXISTS op (
            id INTEGER PRIMARY KEY,
            plan_id INTEGER NOT NULL REFERENCES plan(id),
            seq INTEGER NOT NULL,
            kind TEXT NOT NULL,
            src TEXT NOT NULL,
            dst TEXT,
            dst_actual TEXT,
            state TEXT NOT NULL,
            error TEXT
        );
        "#,
    )?;
    ensure_column(
        conn,
        "item",
        "safety",
        "safety TEXT NOT NULL DEFAULT 'safe'",
    )?;
    ensure_column(conn, "item", "safety_reason", "safety_reason TEXT")?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !names.iter().any(|name| name == column) {
        conn.execute(
            &format!("ALTER TABLE {} ADD COLUMN {}", table, definition),
            [],
        )?;
    }
    Ok(())
}

pub fn ensure_workspace(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO workspace (name) VALUES (?1)",
        [name],
    )?;
    let id = conn.query_row(
        "SELECT id FROM workspace WHERE name = ?1",
        [name],
        |row| row.get(0),
    )?;
    Ok(id)
}

pub fn ensure_zone(
    conn: &Connection,
    workspace_id: i64,
    lane: Lane,
    path: &Path,
    cap: Option<i64>,
    position: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO zone (workspace_id, name, kind, position, cap, path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            workspace_id,
            lane.as_str(),
            lane.as_str(),
            position,
            cap,
            path.to_string_lossy()
        ],
    )?;
    let id = conn.query_row(
        "SELECT id FROM zone WHERE workspace_id = ?1 AND name = ?2",
        params![workspace_id, lane.as_str()],
        |row| row.get(0),
    )?;
    Ok(id)
}

pub fn add_root(
    conn: &Connection,
    workspace_id: i64,
    path: &Path,
    kind: &str,
    depth_limit: Option<i64>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO root (workspace_id, path, kind, depth_limit, enabled)
         VALUES (?1, ?2, ?3, ?4, 1)
         ON CONFLICT(workspace_id, path) DO UPDATE SET
            kind = excluded.kind,
            depth_limit = excluded.depth_limit,
            enabled = 1",
        params![workspace_id, path.to_string_lossy(), kind, depth_limit],
    )?;
    let id = conn.query_row(
        "SELECT id FROM root WHERE workspace_id = ?1 AND path = ?2",
        params![workspace_id, path.to_string_lossy()],
        |row| row.get(0),
    )?;
    Ok(id)
}

pub fn list_roots(conn: &Connection) -> Result<Vec<RootRow>> {
    let mut stmt =
        conn.prepare("SELECT id, path, kind, depth_limit FROM root WHERE enabled = 1 ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok(RootRow {
            id: row.get(0)?,
            path: row.get(1)?,
            kind: row.get(2)?,
            depth_limit: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn remove_root(conn: &mut Connection, root_id: i64) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM item WHERE root_id = ?1", [root_id])?;
    tx.execute("DELETE FROM root WHERE id = ?1", [root_id])?;
    tx.commit()?;
    Ok(())
}

pub fn replace_items_for_root(
    conn: &mut Connection,
    root_id: i64,
    items: &[ScannedItem],
    scan_ts: i64,
) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM item WHERE root_id = ?1", [root_id])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO item
             (root_id, rel_path, name, ext, kind, size, mtime, ctime,
              is_repo, has_remote, is_hidden, is_placeholder, safety, safety_reason, last_seen_scan)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;
        for item in items {
            stmt.execute(params![
                root_id,
                &item.rel_path,
                &item.name,
                item.ext.as_deref(),
                item.kind.as_str(),
                item.size as i64,
                item.mtime,
                item.ctime,
                item.is_repo as i64,
                item.has_remote as i64,
                item.is_hidden as i64,
                item.is_placeholder as i64,
                item.safety.as_str(),
                item.safety_reason.as_deref(),
                scan_ts,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn list_items(conn: &Connection) -> Result<Vec<Item>> {
    let mut stmt = conn.prepare(
        "SELECT id, root_id, rel_path, name, ext, kind, size, mtime, ctime,
                is_repo, has_remote, is_hidden, is_placeholder, pinned, review_by,
                safety, safety_reason
         FROM item",
    )?;
    let rows = stmt.query_map([], |row| {
        let kind_text: String = row.get(5)?;
        let safety_text: String = row.get(15)?;
        Ok(Item {
            id: row.get(0)?,
            root_id: row.get(1)?,
            rel_path: row.get(2)?,
            name: row.get(3)?,
            ext: row.get(4)?,
            kind: Kind::from_str(&kind_text),
            size: row.get::<_, i64>(6)? as u64,
            mtime: row.get(7)?,
            ctime: row.get(8)?,
            is_repo: row.get::<_, i64>(9)? != 0,
            has_remote: row.get::<_, i64>(10)? != 0,
            is_hidden: row.get::<_, i64>(11)? != 0,
            is_placeholder: row.get::<_, i64>(12)? != 0,
            pinned: row.get::<_, i64>(13)? != 0,
            review_by: row.get(14)?,
            safety: Safety::from_str(&safety_text),
            safety_reason: row.get(16)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}


pub struct ZoneRow {
    pub id: i64,
    pub name: String,
    pub path: Option<String>,
    pub cap: Option<i64>,
}

pub struct PlanRow {
    pub id: i64,
    pub created_at: i64,
    pub status: String,
    pub ops_json: String,
    pub note: Option<String>,
}

pub struct OpRow {
    pub id: i64,
    pub plan_id: i64,
    pub seq: i64,
    pub kind: String,
    pub src: String,
    pub dst: Option<String>,
    pub dst_actual: Option<String>,
    pub state: String,
    pub error: Option<String>,
}

pub fn list_zones(conn: &Connection) -> Result<Vec<ZoneRow>> {
    let mut stmt = conn.prepare("SELECT id, name, path, cap FROM zone ORDER BY position")?;
    let rows = stmt.query_map([], |row| {
        Ok(ZoneRow {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            cap: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn insert_plan(conn: &Connection, ops_json: &str, status: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO plan (created_at, status, ops_json, note) VALUES (?1, ?2, ?3, NULL)",
        params![chrono::Utc::now().timestamp(), status, ops_json],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_plan_json(conn: &Connection, plan_id: i64, ops_json: &str) -> Result<()> {
    conn.execute(
        "UPDATE plan SET ops_json = ?1 WHERE id = ?2",
        params![ops_json, plan_id],
    )?;
    Ok(())
}

pub fn insert_ops(conn: &Connection, plan_id: i64, ops: &[PlanOp]) -> Result<()> {
    for op in ops {
        conn.execute(
            "INSERT INTO op (plan_id, seq, kind, src, dst, state)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            params![plan_id, op.seq, &op.kind, &op.src, &op.dst],
        )?;
    }
    Ok(())
}

pub fn list_ops(conn: &Connection, plan_id: i64) -> Result<Vec<OpRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, plan_id, seq, kind, src, dst, dst_actual, state, error
         FROM op WHERE plan_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map([plan_id], |row| {
        Ok(OpRow {
            id: row.get(0)?,
            plan_id: row.get(1)?,
            seq: row.get(2)?,
            kind: row.get(3)?,
            src: row.get(4)?,
            dst: row.get(5)?,
            dst_actual: row.get(6)?,
            state: row.get(7)?,
            error: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn update_op_state(
    conn: &Connection,
    op_id: i64,
    state: &str,
    dst_actual: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE op SET state = ?1, dst_actual = COALESCE(?2, dst_actual), error = ?3 WHERE id = ?4",
        params![state, dst_actual, error, op_id],
    )?;
    Ok(())
}

pub fn update_plan_status(conn: &Connection, plan_id: i64, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE plan SET status = ?1 WHERE id = ?2",
        params![status, plan_id],
    )?;
    Ok(())
}

pub fn get_plan(conn: &Connection, plan_id: i64) -> Result<Option<PlanRow>> {
    let mut stmt =
        conn.prepare("SELECT id, created_at, status, ops_json, note FROM plan WHERE id = ?1")?;
    let mut rows = stmt.query_map([plan_id], plan_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn last_plan(conn: &Connection) -> Result<Option<PlanRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, status, ops_json, note FROM plan ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], plan_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn last_applied_plan(conn: &Connection) -> Result<Option<PlanRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, status, ops_json, note FROM plan
         WHERE status IN ('applied', 'partial')
         ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], plan_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list_plans(conn: &Connection) -> Result<Vec<PlanRow>> {
    let mut stmt =
        conn.prepare("SELECT id, created_at, status, ops_json, note FROM plan ORDER BY id")?;
    let rows = stmt.query_map([], plan_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn plan_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlanRow> {
    Ok(PlanRow {
        id: row.get(0)?,
        created_at: row.get(1)?,
        status: row.get(2)?,
        ops_json: row.get(3)?,
        note: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn last_applied_plan_ignores_a_newer_draft() {
        let dir = std::env::temp_dir().join(format!(
            "sweep-db-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("sweep.db");
        let conn = open(&db_path).unwrap();
        let applied = insert_plan(&conn, "{}", "applied").unwrap();
        let _draft = insert_plan(&conn, "{}", "draft").unwrap();

        let picked = last_applied_plan(&conn).unwrap().unwrap();
        assert_eq!(picked.id, applied);

        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
