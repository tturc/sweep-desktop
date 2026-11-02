use crate::db::{RootRow, ZoneRow};
use crate::model::{Confidence, Item, Lane};
use crate::paths;
use crate::suggest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFile {
    pub version: u32,
    pub plan_id: Option<i64>,
    pub workspace: String,
    pub created_at: String,
    pub ops: Vec<PlanOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOp {
    pub seq: i64,
    pub kind: String,
    pub src: String,
    pub dst: String,
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

    for item in items {
        let Some(root) = root_map.get(&item.root_id) else {
            continue;
        };
        let Some(suggestion) = suggest::suggest(item, &root.kind) else {
            continue;
        };
        if suggestion.confidence.rank() < min_confidence.rank() {
            continue;
        }
        if lane_filter.is_none() && suggestion.lane == Lane::Bench {
            continue;
        }
        if let Some(lane) = lane_filter {
            if suggestion.lane != lane {
                continue;
            }
        }
        let Some(zone) = zone_map.get(suggestion.lane.as_str()) else {
            continue;
        };
        let Some(zone_path) = &zone.path else {
            continue;
        };

        let src = Path::new(&root.path).join(&item.rel_path);
        let dst = Path::new(zone_path).join(&item.rel_path);
        if src == dst {
            continue;
        }

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
    }
}
