use crate::model::{Item, Kind, Lane};
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
              is_repo, has_remote, is_hidden, is_placeholder, last_seen_scan)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                is_repo, has_remote, is_hidden, is_placeholder, pinned, review_by
         FROM item",
    )?;
    let rows = stmt.query_map([], |row| {
        let kind_text: String = row.get(5)?;
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
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
