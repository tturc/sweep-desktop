use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use sweep_core::{
    apply, db, paths, plan, recover, suggest, undo, walk, Confidence, Item, Lane, Suggestion,
};

#[derive(Parser)]
#[command(name = "sweep", version, about = "A local-first desktop organizer")]
struct Cli {
    #[arg(long, global = true, env = "SWEEP_DB")]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(long = "root")]
        roots: Vec<PathBuf>,
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    Scan {
        #[arg(long)]
        json: bool,
    },
    Board {
        #[arg(long)]
        json: bool,
    },
    Plan {
        #[arg(long)]
        lane: Option<String>,
        #[arg(long, default_value = "high")]
        confidence: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Apply {
        plan: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        suffix: bool,
    },
    Undo {
        #[arg(long)]
        plan: Option<i64>,
    },
    Recover,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or_else(paths::default_db_path);

    match cli.command {
        Command::Init { roots, workspace } => cmd_init(&db_path, roots, workspace),
        Command::Scan { json } => cmd_scan(&db_path, json),
        Command::Board { json } => cmd_board(&db_path, json),
        Command::Plan {
            lane,
            confidence,
            limit,
            out,
        } => cmd_plan(&db_path, lane, confidence, limit, out),
        Command::Apply {
            plan,
            apply,
            suffix,
        } => cmd_apply(&db_path, plan, apply, suffix),
        Command::Undo { plan } => cmd_undo(&db_path, plan),
        Command::Recover => cmd_recover(&db_path),
    }
}

fn cmd_init(db_path: &Path, roots: Vec<PathBuf>, workspace: Option<PathBuf>) -> Result<()> {
    let workspace = paths::ensure_workspace_dir(workspace)?;
    let conn = db::open(db_path)?;
    let workspace_id = db::ensure_workspace(&conn, "default")?;

    for (position, lane) in Lane::all().iter().enumerate() {
        let cap = match lane {
            Lane::Bench => Some(12),
            Lane::Inbox => Some(20),
            _ => None,
        };
        let path = workspace.join(lane.as_str());
        std::fs::create_dir_all(&path)?;
        db::ensure_zone(&conn, workspace_id, *lane, &path, cap, position as i64)?;
    }

    let roots: Vec<(PathBuf, String)> = if roots.is_empty() {
        paths::default_roots()
            .into_iter()
            .map(|(path, kind)| (path, kind.to_string()))
            .collect()
    } else {
        roots
            .into_iter()
            .map(|path| (path, "custom".to_string()))
            .collect()
    };

    for (path, kind) in roots {
        let canonical = paths::canonicalize_root(&path)?;
        let depth = match kind.as_str() {
            "desktop" | "downloads" => Some(1),
            _ => None,
        };
        db::add_root(&conn, workspace_id, &canonical, &kind, depth)?;
        println!(
            "root       {}  ({})",
            paths::display_path(&canonical),
            kind
        );
    }

    println!("workspace  {}", paths::display_path(&workspace));
    println!("db         {}", paths::display_path(db_path));
    Ok(())
}

fn cmd_scan(db_path: &Path, json: bool) -> Result<()> {
    let mut conn = db::open(db_path)?;
    let roots = db::list_roots(&conn)?;
    if roots.is_empty() {
        anyhow::bail!("no roots registered; run `sweep init` first");
    }

    let started = std::time::Instant::now();
    let scan_ts = chrono::Utc::now().timestamp();
    let mut report = Vec::new();

    for root in &roots {
        let items = walk::scan_root(
            Path::new(&root.path),
            root.depth_limit.map(|value| value as usize),
        )?;
        db::replace_items_for_root(&mut conn, root.id, &items, scan_ts)?;
        report.push(serde_json::json!({
            "path": paths::display_path(Path::new(&root.path)),
            "kind": &root.kind,
            "items": items.len(),
        }));
    }

    let elapsed_ms = started.elapsed().as_millis() as u64;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "roots": report,
                "elapsed_ms": elapsed_ms,
            }))?
        );
    } else {
        for row in &report {
            println!(
                "{:>6}  {:<10}  {}",
                row["items"],
                row["kind"].as_str().unwrap_or(""),
                row["path"].as_str().unwrap_or("")
            );
        }
        println!("scanned in {} ms", elapsed_ms);
    }
    Ok(())
}

fn cmd_board(db_path: &Path, json: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let roots = db::list_roots(&conn)?;
    if roots.is_empty() {
        anyhow::bail!("no roots registered; run `sweep init` first");
    }

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
    suggestions.truncate(10);

    if json {
        let lane_json: serde_json::Map<String, serde_json::Value> = lanes
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::json!(value)))
            .collect();
        let suggestion_json: Vec<serde_json::Value> = suggestions
            .iter()
            .map(|(item, suggestion)| {
                serde_json::json!({
                    "name": &item.name,
                    "kind": item.kind.as_str(),
                    "age_days": age_days(item),
                    "lane": suggestion.lane.as_str(),
                    "confidence": suggestion.confidence.as_str(),
                    "reasons": suggestion.reasons.iter().map(|reason| reason.label()).collect::<Vec<_>>(),
                })
            })
            .collect();

        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "roots": roots.iter().map(|root| serde_json::json!({
                    "path": paths::display_path(Path::new(&root.path)),
                    "kind": &root.kind,
                })).collect::<Vec<_>>(),
                "lanes": lane_json,
                "suggestions": suggestion_json,
            }))?
        );
    } else {
        println!("{:<8} {:>6}", "lane", "count");
        for lane in Lane::all() {
            println!(
                "{:<8} {:>6}",
                lane.as_str(),
                lanes.get(lane.as_str()).copied().unwrap_or(0)
            );
        }
        println!("\ntop suggestions:");
        for (item, suggestion) in &suggestions {
            println!(
                "  {:<32} {:<10} {:>4}d  -> {:<6} ({})",
                truncate(&item.name, 32),
                item.kind.as_str(),
                age_days(item),
                suggestion.lane.as_str(),
                suggestion.confidence.as_str()
            );
        }
    }
    Ok(())
}

fn cmd_plan(
    db_path: &Path,
    lane: Option<String>,
    confidence: String,
    limit: usize,
    out: Option<PathBuf>,
) -> Result<()> {
    let conn = db::open(db_path)?;
    let roots = db::list_roots(&conn)?;
    let zones = db::list_zones(&conn)?;
    if roots.is_empty() {
        bail!("no roots registered; run `sweep init` first");
    }
    let items = db::list_items(&conn)?;

    let lane = match lane.as_deref() {
        None => None,
        Some("bench") => Some(Lane::Bench),
        Some("kits") => Some(Lane::Kits),
        Some("shelf") => Some(Lane::Shelf),
        Some("cold") => Some(Lane::Cold),
        Some("inbox") => Some(Lane::Inbox),
        Some(other) => bail!("unknown lane: {}", other),
    };
    let confidence = match confidence.as_str() {
        "high" => Confidence::High,
        "medium" => Confidence::Medium,
        "low" => Confidence::Low,
        other => bail!("unknown confidence: {}", other),
    };

    let mut plan_file = plan::build(&roots, &zones, &items, lane, confidence, limit);
    if plan_file.ops.is_empty() {
        bail!("no ops matched");
    }

    let plan_id = db::insert_plan(&conn, "{}", "draft")?;
    plan_file.plan_id = Some(plan_id);
    db::update_plan_json(&conn, plan_id, &serde_json::to_string(&plan_file)?)?;
    db::insert_ops(&conn, plan_id, &plan_file.ops)?;

    let json = serde_json::to_string_pretty(&plan_file)?;
    if let Some(path) = out {
        std::fs::write(&path, &json)?;
        println!(
            "wrote {} ({} ops, plan {})",
            path.display(),
            plan_file.ops.len(),
            plan_id
        );
    } else {
        println!("{}", json);
    }
    Ok(())
}

fn cmd_apply(db_path: &Path, plan_path: PathBuf, do_apply: bool, suffix: bool) -> Result<()> {
    let mut conn = db::open(db_path)?;
    let text = std::fs::read_to_string(&plan_path)?;
    let plan_file: plan::PlanFile = serde_json::from_str(&text)?;
    apply::run(&mut conn, &plan_file, do_apply, suffix)
}

fn cmd_undo(db_path: &Path, plan_id: Option<i64>) -> Result<()> {
    let mut conn = db::open(db_path)?;
    undo::run(&mut conn, plan_id)
}

fn cmd_recover(db_path: &Path) -> Result<()> {
    let mut conn = db::open(db_path)?;
    recover::run(&mut conn)
}

fn age_days(item: &Item) -> i64 {
    (chrono::Utc::now().timestamp() - item.mtime).max(0) / 86_400
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_string()
    } else {
        let mut out: String = value.chars().take(limit.saturating_sub(3)).collect();
        out.push_str("...");
        out
    }
}
