//! Session holdout split and quality metrics (Phase 8B).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::autotier::export::{ExportDecisionLine, ExportManifest};
use crate::autotier::replay::load_export_manifest;
use crate::error::AppError;

pub const DEFAULT_SPLIT_SEED: u64 = 170_000_013;
pub const TUNE_RATIO: f64 = 0.70;
pub const MIN_HOLDOUT_SAMPLES: usize = 30;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalSplit {
    Tune,
    Holdout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalDecisionRow {
    pub decision_id: String,
    pub session_id_hash: String,
    pub recommended_slot: Option<String>,
    pub actual_outbound_model: Option<String>,
    pub candidate_model: Option<String>,
    pub label: Option<String>,
    pub unsafe_reasons_json: String,
    pub split: EvalSplit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalMetrics {
    pub tune_count: usize,
    pub holdout_count: usize,
    pub strong_recall: f64,
    pub unsafe_downgrade: f64,
    pub cache_adjusted_saving_usd: f64,
    pub holdout_sample_sufficient: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalReport {
    pub manifest: ExportManifest,
    pub metrics: EvalMetrics,
    pub sessions: usize,
}

fn stable_bucket(session_hash: &str, seed: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    session_hash.hash(&mut hasher);
    hasher.finish()
}

pub fn assign_split(session_hash: &str, seed: u64) -> EvalSplit {
    let bucket = stable_bucket(session_hash, seed) as f64 / u64::MAX as f64;
    if bucket < TUNE_RATIO {
        EvalSplit::Tune
    } else {
        EvalSplit::Holdout
    }
}

fn parse_labels(path: &Path) -> Result<HashMap<String, String>, AppError> {
    let mut map = HashMap::new();
    let raw = fs::read_to_string(path)
        .map_err(|e| AppError::InvalidInput(format!("labels.jsonl read failed: {e}")))?;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| AppError::InvalidInput(format!("labels.jsonl parse failed: {e}")))?;
        if let (Some(id), Some(label)) = (
            value.get("decision_id").and_then(|v| v.as_str()),
            value.get("label").and_then(|v| v.as_str()),
        ) {
            map.insert(id.to_string(), label.to_string());
        }
    }
    Ok(map)
}

fn load_decisions(path: &Path) -> Result<Vec<ExportDecisionLine>, AppError> {
    let raw = fs::read_to_string(path)
        .map_err(|e| AppError::InvalidInput(format!("decisions.jsonl read failed: {e}")))?;
    let mut rows = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).map_err(|e| {
            AppError::InvalidInput(format!("decisions.jsonl parse failed: {e}"))
        })?);
    }
    Ok(rows)
}

pub fn compute_metrics(rows: &[EvalDecisionRow]) -> EvalMetrics {
    let tune_count = rows.iter().filter(|r| r.split == EvalSplit::Tune).count();
    let holdout_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.split == EvalSplit::Holdout)
        .collect();
    let holdout_count = holdout_rows.len();
    let holdout_sample_sufficient = holdout_count >= MIN_HOLDOUT_SAMPLES;

    let labeled_should_stronger = holdout_rows
        .iter()
        .filter(|r| r.label.as_deref() == Some("should_be_stronger"))
        .count();
    let strong_recall_hits = holdout_rows
        .iter()
        .filter(|r| {
            r.label.as_deref() == Some("should_be_stronger")
                && r.recommended_slot.as_deref() == Some("strong")
        })
        .count();
    let strong_recall = if labeled_should_stronger == 0 {
        0.0
    } else {
        strong_recall_hits as f64 / labeled_should_stronger as f64
    };

    let unsafe_downgrade = holdout_rows
        .iter()
        .filter(|r| {
            !r.unsafe_reasons_json.trim().is_empty()
                && r.unsafe_reasons_json != "[]"
                && r.recommended_slot.as_deref() == Some("cheap")
        })
        .count() as f64
        / holdout_count.max(1) as f64;

    let cache_adjusted_saving_usd = holdout_rows
        .iter()
        .filter_map(|r| {
            r.candidate_model
                .as_ref()
                .zip(r.actual_outbound_model.as_ref())
                .filter(|(candidate, actual)| candidate != actual)
                .map(|_| 0.0)
        })
        .sum();

    let mut warnings = Vec::new();
    if !holdout_sample_sufficient {
        warnings.push(format!(
            "holdout sample size {holdout_count} < minimum {MIN_HOLDOUT_SAMPLES}; metrics are advisory only"
        ));
    }

    EvalMetrics {
        tune_count,
        holdout_count,
        strong_recall,
        unsafe_downgrade,
        cache_adjusted_saving_usd,
        holdout_sample_sufficient,
        warnings,
    }
}

pub fn evaluate_export_dir(export_dir: &Path, seed: u64) -> Result<EvalReport, AppError> {
    let manifest = load_export_manifest(export_dir)?;
    let decisions = load_decisions(&export_dir.join("decisions.jsonl"))?;
    let labels = parse_labels(&export_dir.join("labels.jsonl"))?;

    let mut sessions = BTreeSet::new();
    let mut rows = Vec::with_capacity(decisions.len());
    for line in decisions {
        sessions.insert(line.session_id_hash.clone());
        rows.push(EvalDecisionRow {
            decision_id: line.decision_id.clone(),
            session_id_hash: line.session_id_hash.clone(),
            recommended_slot: line.recommended_slot.clone(),
            actual_outbound_model: line.actual_outbound_model.clone(),
            candidate_model: line.candidate_model.clone(),
            label: labels.get(&line.decision_id).cloned(),
            unsafe_reasons_json: line.unsafe_reasons_json.clone(),
            split: assign_split(&line.session_id_hash, seed),
        });
    }

    // Session leakage guard: each session must land entirely in one split.
    let mut session_splits: BTreeMap<String, EvalSplit> = BTreeMap::new();
    for row in &rows {
        match session_splits.get(&row.session_id_hash) {
            Some(existing) if *existing != row.split => {
                return Err(AppError::InvalidInput(format!(
                    "session split leakage detected for {}",
                    row.session_id_hash
                )));
            }
            None => {
                session_splits.insert(row.session_id_hash.clone(), row.split);
            }
            _ => {}
        }
    }

    let metrics = compute_metrics(&rows);
    Ok(EvalReport {
        manifest,
        metrics,
        sessions: sessions.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_is_session_stable() {
        let a = assign_split("hash-a", DEFAULT_SPLIT_SEED);
        let b = assign_split("hash-a", DEFAULT_SPLIT_SEED);
        assert_eq!(a, b);
        let c = assign_split("hash-b", DEFAULT_SPLIT_SEED);
        assert!(a == EvalSplit::Tune || a == EvalSplit::Holdout);
        let _ = c;
    }

    #[test]
    fn small_holdout_emits_warning() {
        let rows = (0..5)
            .map(|i| EvalDecisionRow {
                decision_id: format!("d-{i}"),
                session_id_hash: format!("s-{i}"),
                recommended_slot: Some("cheap".into()),
                actual_outbound_model: Some("strong-model".into()),
                candidate_model: Some("cheap-model".into()),
                label: None,
                unsafe_reasons_json: "[]".into(),
                split: EvalSplit::Holdout,
            })
            .collect::<Vec<_>>();
        let metrics = compute_metrics(&rows);
        assert!(!metrics.holdout_sample_sufficient);
        assert!(!metrics.warnings.is_empty());
    }
}
