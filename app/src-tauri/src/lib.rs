use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use sweep_core::{
    apply as apply_mod, db, paths, plan as plan_mod, recover as recover_mod, suggest,
    undo as undo_mod, walk, Confidence, Item, Lane, Suggestion,
};

#[derive(Serialize)]
struct RootView {
    id: i64,
    path: String,
    kind: String,
    items: usize,
}

#[derive(Serialize)]
struct SuggestionView {
    name: String,
    kind: String,
    age_days: i64,
    lane: String,
    confidence: String,
    reasons: Vec<String>,
}

#[derive(Serialize)]
struct BoardView {
    roots: Vec<RootView>,
    lanes: HashMap<String, i64>,
    suggestions: Vec<SuggestionView>,
    last_plan_status: Option<String>,
}

#[derive(Serialize)]
struct DefaultRoot {
    path: String,
    kind: String,
    registered: bool,
}

#[tauri::command]
fn get_board() -> Result<BoardView, String> {
    board_impl().map_err(|error| error.to_string())
}

#[tauri::command]
fn list_default_roots() -> Result<Vec<DefaultRoot>, String> {
    list_default_roots_impl().map_err(|error| error.to_string())
}

#[tauri::command]
fn add_root(path: String, kind: Option<String>) -> Result<BoardView, String> {
    add_root_impl(path, kind).map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_root(id: i64) -> Result<BoardView, String> {
    remove_root_impl(id).map_err(|error| error.to_string())
}

#[tauri::command]
fn scan_roots() -> Result<BoardView, String> {
    scan_impl()
        .and_then(|_| board_impl())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_plan(lane: Option<String>) -> Result<plan_mod::PlanFile, String> {
    plan_impl(lane).map_err(|error| error.to_string())
}

#[tauri::command]
fn apply_plan(plan: plan_mod::PlanFile) -> Result<String, String> {
    apply_impl(plan).map_err(|error| error.to_string())
}

#[tauri::command]
fn undo_last() -> Result<String, String> {
    undo_impl().map_err(|error| error.to_string())
}

#[tauri::command]
fn recover_plans() -> Result<String, String> {
    recover_impl().map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_board,
            list_default_roots,
            add_root,
            remove_root,
            scan_roots,
            create_plan,
            apply_plan,
            undo_last,
            recover_plans
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn board_impl() -> anyhow::Result<BoardView> {
    let conn = db::open(&paths::default_db_path())?;
    let roots = db::list_roots(&conn)?;
    let root_kind: HashMap<i64, String> = roots
        .iter()
        .map(|root| (root.id, root.kind.clone()))
        .collect();
    let items = db::list_items(&conn)?;
    let mut lanes: HashMap<String, i64> = Lane::all()
        .iter()
        .map(|lane| (lane.as_str().to_string(), 0))
        .collect();
    let mut suggestions: Vec<(&Item, Suggestion)> = Vec::new();

    for item in &items {
        let kind = root_kind
            .get(&item.root_id)
            .map(|value| value.as_str())
            .unwrap_or("custom");
        if let Some(suggestion) = suggest::suggest(item, kind) {
            *lanes
                .entry(suggestion.lane.as_str().to_string())
                .or_insert(0) += 1;
            suggestions.push((item, suggestion));
        } else {
            *lanes.entry(Lane::Inbox.as_str().to_string()).or_insert(0) += 1;
        }
    }

    suggestions.sort_by(|a, b| {
        b.1.confidence
            .rank()
            .cmp(&a.1.confidence.rank())
            .then_with(|| age_days(b.0).cmp(&age_days(a.0)))
    });

    let suggestion_views = suggestions
        .into_iter()
        .map(|(item, suggestion)| SuggestionView {
            name: item.name.clone(),
            kind: item.kind.as_str().to_string(),
            age_days: age_days(item),
            lane: suggestion.lane.as_str().to_string(),
            confidence: suggestion.confidence.as_str().to_string(),
            reasons: suggestion
                .reasons
                .iter()
                .map(|reason| reason.label())
                .collect(),
        })
        .collect();

    let root_views = roots
        .iter()
        .map(|root| RootView {
            id: root.id,
            path: paths::display_path(Path::new(&root.path)),
            kind: root.kind.clone(),
            items: items.iter().filter(|item| item.root_id == root.id).count(),
        })
        .collect();

    Ok(BoardView {
        roots: root_views,
        lanes,
        suggestions: suggestion_views,
        last_plan_status: db::last_plan(&conn)?.map(|plan| plan.status),
    })
}

fn scan_impl() -> anyhow::Result<()> {
    let mut conn = db::open(&paths::default_db_path())?;
    let roots = db::list_roots(&conn)?;
    if roots.is_empty() {
        anyhow::bail!("no roots registered; run `sweep init` first");
    }
    let scan_ts = chrono::Utc::now().timestamp();
    for root in &roots {
        let items = walk::scan_root(
            Path::new(&root.path),
            root.depth_limit.map(|value| value as usize),
        )?;
        db::replace_items_for_root(&mut conn, root.id, &items, scan_ts)?;
    }
    Ok(())
}

fn plan_impl(lane: Option<String>) -> anyhow::Result<plan_mod::PlanFile> {
    let conn = db::open(&paths::default_db_path())?;
    let roots = db::list_roots(&conn)?;
    let zones = db::list_zones(&conn)?;
    if roots.is_empty() {
        anyhow::bail!("no roots registered; run `sweep init` first");
    }
    let items = db::list_items(&conn)?;
    let lane = match lane.as_deref() {
        None => None,
        Some("bench") => Some(Lane::Bench),
        Some("kits") => Some(Lane::Kits),
        Some("shelf") => Some(Lane::Shelf),
        Some("cold") => Some(Lane::Cold),
        Some("inbox") => Some(Lane::Inbox),
        Some(other) => anyhow::bail!("unknown lane: {}", other),
    };
    let lane_selected = lane.is_some();
    let min_confidence = if lane_selected {
        Confidence::Low
    } else {
        Confidence::High
    };
    let limit = if lane_selected { 500 } else { 50 };
    let mut plan = plan_mod::build(&roots, &zones, &items, lane, min_confidence, limit);
    if plan.ops.is_empty() {
        anyhow::bail!("no ops matched");
    }
    let plan_id = db::insert_plan(&conn, "{}", "draft")?;
    plan.plan_id = Some(plan_id);
    db::update_plan_json(&conn, plan_id, &serde_json::to_string(&plan)?)?;
    db::insert_ops(&conn, plan_id, &plan.ops)?;
    Ok(plan)
}

fn apply_impl(plan: plan_mod::PlanFile) -> anyhow::Result<String> {
    let mut conn = db::open(&paths::default_db_path())?;
    let count = plan.ops.len();
    apply_mod::run(&mut conn, &plan, true, false)?;
    scan_impl()?;
    Ok(format!("applied {} ops", count))
}

fn undo_impl() -> anyhow::Result<String> {
    let mut conn = db::open(&paths::default_db_path())?;
    undo_mod::run(&mut conn, None)?;
    scan_impl()?;
    Ok("undone".to_string())
}

fn recover_impl() -> anyhow::Result<String> {
    let mut conn = db::open(&paths::default_db_path())?;
    recover_mod::run(&mut conn)?;
    scan_impl()?;
    Ok("recovered".to_string())
}

fn list_default_roots_impl() -> anyhow::Result<Vec<DefaultRoot>> {
    let conn = db::open(&paths::default_db_path())?;
    let registered: Vec<String> = db::list_roots(&conn)?
        .into_iter()
        .map(|root| paths::display_path(Path::new(&root.path)))
        .collect();
    let mut out = Vec::new();
    for (path, kind) in paths::default_roots() {
        let canonical = path.canonicalize().unwrap_or(path);
        let display = paths::display_path(&canonical);
        out.push(DefaultRoot {
            registered: registered.iter().any(|item| item == &display),
            path: display,
            kind: kind.to_string(),
        });
    }
    Ok(out)
}

fn add_root_impl(path: String, kind: Option<String>) -> anyhow::Result<BoardView> {
    let conn = db::open(&paths::default_db_path())?;
    let workspace = paths::ensure_workspace_dir(None)?;
    let workspace_id = db::ensure_workspace(&conn, "default")?;
    for (position, lane) in Lane::all().iter().enumerate() {
        let cap = match lane {
            Lane::Bench => Some(12),
            Lane::Inbox => Some(20),
            _ => None,
        };
        let zone_path = workspace.join(lane.as_str());
        std::fs::create_dir_all(&zone_path)?;
        db::ensure_zone(&conn, workspace_id, *lane, &zone_path, cap, position as i64)?;
    }
    let canonical = paths::canonicalize_root(Path::new(&path))?;
    let kind = kind.unwrap_or_else(|| "custom".to_string());
    db::add_root(&conn, workspace_id, &canonical, &kind, Some(1))?;
    drop(conn);
    scan_impl()?;
    board_impl()
}

fn remove_root_impl(id: i64) -> anyhow::Result<BoardView> {
    let mut conn = db::open(&paths::default_db_path())?;
    db::remove_root(&mut conn, id)?;
    drop(conn);
    board_impl()
}

fn age_days(item: &Item) -> i64 {
    (chrono::Utc::now().timestamp() - item.mtime).max(0) / 86_400
}
