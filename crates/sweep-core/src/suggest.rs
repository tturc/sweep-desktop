use crate::model::{Confidence, Item, Kind, Lane, Reason, Suggestion};

pub fn suggest(item: &Item, root_kind: &str) -> Option<Suggestion> {
    if item.pinned {
        return None;
    }

    let age_days = (chrono::Utc::now().timestamp() - item.mtime).max(0) / 86_400;
    let mut reasons = vec![Reason::Age(age_days), Reason::Kind(item.kind)];
    if item.size > 0 {
        reasons.push(Reason::Size(item.size));
    }
    if item.is_repo {
        reasons.push(Reason::GitRemote(item.has_remote));
    }
    if root_kind == "downloads" {
        reasons.push(Reason::Root(root_kind.to_string()));
    }

    if age_days <= 7 {
        return Some(Suggestion {
            lane: Lane::Bench,
            confidence: Confidence::High,
            reasons,
        });
    }

    match item.kind {
        Kind::Script => Some(Suggestion {
            lane: Lane::Kits,
            confidence: Confidence::High,
            reasons,
        }),
        Kind::Repo => {
            if age_days > 90 {
                Some(Suggestion {
                    lane: Lane::Cold,
                    confidence: if item.has_remote {
                        Confidence::Medium
                    } else {
                        Confidence::Low
                    },
                    reasons,
                })
            } else {
                Some(Suggestion {
                    lane: Lane::Kits,
                    confidence: Confidence::Medium,
                    reasons,
                })
            }
        }
        Kind::Installer | Kind::Shortcut => {
            if age_days > 30 {
                Some(Suggestion {
                    lane: Lane::Shelf,
                    confidence: Confidence::High,
                    reasons,
                })
            } else {
                Some(Suggestion {
                    lane: Lane::Inbox,
                    confidence: Confidence::Low,
                    reasons,
                })
            }
        }
        Kind::Archive => {
            if age_days > 60 {
                Some(Suggestion {
                    lane: Lane::Cold,
                    confidence: Confidence::High,
                    reasons,
                })
            } else {
                Some(Suggestion {
                    lane: Lane::Inbox,
                    confidence: Confidence::Medium,
                    reasons,
                })
            }
        }
        Kind::Doc | Kind::Image | Kind::Video | Kind::Audio => {
            if age_days > 180 {
                Some(Suggestion {
                    lane: Lane::Cold,
                    confidence: Confidence::Medium,
                    reasons,
                })
            } else {
                Some(Suggestion {
                    lane: Lane::Inbox,
                    confidence: Confidence::Low,
                    reasons,
                })
            }
        }
        Kind::Note => {
            if age_days > 90 {
                Some(Suggestion {
                    lane: Lane::Cold,
                    confidence: Confidence::Low,
                    reasons,
                })
            } else {
                Some(Suggestion {
                    lane: Lane::Inbox,
                    confidence: Confidence::Low,
                    reasons,
                })
            }
        }
        Kind::Folder | Kind::Other => Some(Suggestion {
            lane: Lane::Inbox,
            confidence: Confidence::Low,
            reasons,
        }),
    }
}
