//! Phase 6A：AutoTier Mode / Slot / Retention 命令层。
//!
//! v0.1 只读写 Off/Shadow。不暴露 Live Command。响应不含 Provider Key。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::autotier::{
    CACHE_STATS_VERSION, CAPABILITY_TABLE_VERSION, CLASSIFIER_VERSION, COST_MODEL_VERSION,
    FEATURE_VERSION, POLICY_VERSION,
};
use crate::database::{lock_conn, AutotierProviderSlotDto, AutotierRoutingConfigDto, Database};
use crate::error::AppError;
use crate::store::AppState;

/// v0.1 允许的 retention_days。
pub const RETENTION_DAYS_ALLOWED: &[i32] = &[7, 14, 30, 90];

const REQUIRED_SLOTS: [&str; 3] = ["cheap", "mid", "strong"];
const OPTIONAL_SLOTS: [&str; 2] = ["long_context", "background"];
const CAPABILITY_STATUSES: [&str; 6] = [
    "unknown", "declared", "probed", "verified", "stale", "failed",
];
const LIVE_MODES: [&str; 5] = [
    "canary_live",
    "full_live",
    "forced_cheap",
    "forced_mid",
    "forced_strong",
];

/// 配置读出视图：单行 DB 配置 + 当日版本常量。不含 Key。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutotierRoutingConfigView {
    pub mode: String,
    pub retention_days: i32,
    pub raw_prompt_opt_in: bool,
    pub classifier_version: String,
    pub feature_version: String,
    pub policy_version: String,
    pub capability_table_version: String,
    pub cost_model_version: String,
    pub cache_stats_version: String,
    pub updated_at: i64,
    /// 若存储值为 Live 并已降级，记录原值；否则为 null。
    pub degraded_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveRoutingConfigInput {
    pub mode: String,
    pub retention_days: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredSlotsStatus {
    pub provider_id: String,
    pub complete: bool,
    pub present: Vec<String>,
    pub missing: Vec<String>,
}

#[tauri::command]
pub fn autotier_get_routing_config(
    state: State<'_, AppState>,
) -> Result<AutotierRoutingConfigView, String> {
    load_routing_config(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_save_routing_config(
    state: State<'_, AppState>,
    input: SaveRoutingConfigInput,
) -> Result<AutotierRoutingConfigView, String> {
    save_routing_config(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_list_provider_slots(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<AutotierProviderSlotDto>, String> {
    list_provider_slots(&state.db, &provider_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_upsert_provider_slot(
    state: State<'_, AppState>,
    slot: AutotierProviderSlotDto,
) -> Result<AutotierProviderSlotDto, String> {
    upsert_provider_slot(&state.db, slot).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_delete_provider_slot(
    state: State<'_, AppState>,
    provider_id: String,
    slot: String,
) -> Result<u64, String> {
    delete_provider_slot(&state.db, &provider_id, &slot).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_required_slots_status(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<RequiredSlotsStatus, String> {
    required_slots_status(&state.db, &provider_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_clear_decisions(state: State<'_, AppState>) -> Result<(), String> {
    clear_decisions(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autotier_prune_decisions(
    state: State<'_, AppState>,
    retention_days: Option<i32>,
) -> Result<u64, String> {
    prune_decisions(&state.db, retention_days).map_err(|e| e.to_string())
}

pub(crate) fn load_routing_config(db: &Database) -> Result<AutotierRoutingConfigView, AppError> {
    let mut config = db.autotier_get_config()?;
    let degraded_from = normalize_stored_mode(&mut config);
    if degraded_from.is_some() {
        config.updated_at = chrono::Utc::now().timestamp_millis();
        db.autotier_set_config(&config)?;
        log::warn!(
            "[AutoTier] v0.1 live/unknown mode degraded: {:?}",
            degraded_from
        );
    }
    Ok(config_view(config, degraded_from))
}

pub(crate) fn save_routing_config(
    db: &Database,
    input: SaveRoutingConfigInput,
) -> Result<AutotierRoutingConfigView, AppError> {
    let (mode, degraded_from) = resolve_save_mode(&input.mode)?;
    validate_retention_days(input.retention_days)?;

    let mut config = db.autotier_get_config()?;
    config.mode = mode.to_string();
    config.retention_days = input.retention_days;
    config.raw_prompt_opt_in = false;
    config.classifier_version = CLASSIFIER_VERSION.to_string();
    config.feature_version = FEATURE_VERSION.to_string();
    config.policy_version = POLICY_VERSION.to_string();
    config.updated_at = chrono::Utc::now().timestamp_millis();

    if config.mode == "shadow"
        && (config.classifier_version.is_empty()
            || config.feature_version.is_empty()
            || config.policy_version.is_empty())
    {
        return Err(AppError::InvalidInput(
            "cannot enable shadow with empty version stamps".into(),
        ));
    }

    db.autotier_set_config(&config)?;
    if let Some(from) = degraded_from.as_ref() {
        log::warn!("[AutoTier] live mode {from} degraded to shadow on save");
    }
    Ok(config_view(config, degraded_from))
}

pub(crate) fn list_provider_slots(
    db: &Database,
    provider_id: &str,
) -> Result<Vec<AutotierProviderSlotDto>, AppError> {
    if provider_id.trim().is_empty() {
        return Err(AppError::InvalidInput("provider_id is required".into()));
    }
    db.autotier_get_slots(provider_id)
}

pub(crate) fn upsert_provider_slot(
    db: &Database,
    mut slot: AutotierProviderSlotDto,
) -> Result<AutotierProviderSlotDto, AppError> {
    slot.provider_id = slot.provider_id.trim().to_string();
    slot.slot = slot.slot.trim().to_ascii_lowercase();
    slot.model_id = slot.model_id.trim().to_string();
    slot.capability_status = slot.capability_status.trim().to_ascii_lowercase();
    if slot.provider_id.is_empty() {
        return Err(AppError::InvalidInput("provider_id is required".into()));
    }
    if slot.model_id.is_empty() {
        return Err(AppError::InvalidInput("model_id is required".into()));
    }
    validate_slot_name(&slot.slot)?;
    validate_capability_status(&slot.capability_status)?;
    let now = chrono::Utc::now().timestamp_millis();
    if slot.created_at == 0 {
        slot.created_at = now;
    }
    slot.updated_at = now;
    db.autotier_upsert_slot(&slot)?;
    db.autotier_get_slot(&slot.provider_id, &slot.slot)?
        .ok_or_else(|| AppError::Database("slot upsert did not persist".into()))
}

pub(crate) fn delete_provider_slot(
    db: &Database,
    provider_id: &str,
    slot: &str,
) -> Result<u64, AppError> {
    if provider_id.trim().is_empty() {
        return Err(AppError::InvalidInput("provider_id is required".into()));
    }
    let slot = slot.trim().to_ascii_lowercase();
    validate_slot_name(&slot)?;
    let conn = lock_conn!(db.conn);
    let n = conn
        .execute(
            "DELETE FROM autotier_provider_slots WHERE provider_id = ?1 AND slot = ?2",
            rusqlite::params![provider_id, slot],
        )
        .map_err(|e| AppError::Database(format!("delete provider slot failed: {e}")))?;
    Ok(n as u64)
}

pub(crate) fn required_slots_status(
    db: &Database,
    provider_id: &str,
) -> Result<RequiredSlotsStatus, AppError> {
    if provider_id.trim().is_empty() {
        return Err(AppError::InvalidInput("provider_id is required".into()));
    }
    let slots = db.autotier_get_slots(provider_id)?;
    let present: Vec<String> = REQUIRED_SLOTS
        .iter()
        .filter(|name| slots.iter().any(|s| s.slot == **name))
        .map(|s| (*s).to_string())
        .collect();
    let missing: Vec<String> = REQUIRED_SLOTS
        .iter()
        .filter(|name| !present.iter().any(|p| p == *name))
        .map(|s| (*s).to_string())
        .collect();
    Ok(RequiredSlotsStatus {
        provider_id: provider_id.to_string(),
        complete: missing.is_empty(),
        present,
        missing,
    })
}

pub(crate) fn clear_decisions(db: &Database) -> Result<(), AppError> {
    db.autotier_clear_all_labels()?;
    db.autotier_clear_all_decisions()
}

pub(crate) fn prune_decisions(db: &Database, retention_days: Option<i32>) -> Result<u64, AppError> {
    let days = match retention_days {
        Some(days) => {
            validate_retention_days(days)?;
            days
        }
        None => {
            let config = db.autotier_get_config()?;
            validate_retention_days(config.retention_days)?;
            config.retention_days
        }
    };
    db.autotier_prune_decisions(days)
}

fn config_view(
    config: AutotierRoutingConfigDto,
    degraded_from: Option<String>,
) -> AutotierRoutingConfigView {
    AutotierRoutingConfigView {
        mode: config.mode,
        retention_days: config.retention_days,
        raw_prompt_opt_in: false,
        classifier_version: config.classifier_version,
        feature_version: config.feature_version,
        policy_version: config.policy_version,
        capability_table_version: CAPABILITY_TABLE_VERSION.to_string(),
        cost_model_version: COST_MODEL_VERSION.to_string(),
        cache_stats_version: CACHE_STATS_VERSION.to_string(),
        updated_at: config.updated_at,
        degraded_from,
    }
}

fn normalize_mode(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().replace('-', "_")
}

fn is_live_mode(mode: &str) -> bool {
    LIVE_MODES.contains(&mode)
}

/// 读路径：Live → Shadow；未知 → Off。返回被降级的原值。
fn normalize_stored_mode(config: &mut AutotierRoutingConfigDto) -> Option<String> {
    let mode = normalize_mode(&config.mode);
    if mode == "off" || mode == "shadow" {
        config.mode = mode;
        return None;
    }
    if is_live_mode(&mode) {
        config.mode = "shadow".into();
        return Some(mode);
    }
    let original = config.mode.clone();
    config.mode = "off".into();
    Some(original)
}

fn resolve_save_mode(raw: &str) -> Result<(&'static str, Option<String>), AppError> {
    let mode = normalize_mode(raw);
    match mode.as_str() {
        "off" => Ok(("off", None)),
        "shadow" => Ok(("shadow", None)),
        live if is_live_mode(live) => Ok(("shadow", Some(live.to_string()))),
        other => Err(AppError::InvalidInput(format!(
            "illegal routing mode: {other}"
        ))),
    }
}

fn validate_retention_days(days: i32) -> Result<(), AppError> {
    if RETENTION_DAYS_ALLOWED.contains(&days) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "retention_days {days} is not in {:?}",
            RETENTION_DAYS_ALLOWED
        )))
    }
}

fn validate_slot_name(slot: &str) -> Result<(), AppError> {
    if REQUIRED_SLOTS.contains(&slot) || OPTIONAL_SLOTS.contains(&slot) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!("illegal slot: {slot}")))
    }
}

fn validate_capability_status(status: &str) -> Result<(), AppError> {
    if CAPABILITY_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "illegal capability_status: {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_has_secret(value: &serde_json::Value) -> bool {
        let encoded = value.to_string().to_ascii_lowercase();
        ["api_key", "apikey", "authorization", "\"secret\"", "sk-ant-"]
            .iter()
            .any(|needle| encoded.contains(needle))
    }

    fn db() -> Database {
        Database::memory().expect("memory db")
    }

    fn sample_slot(provider: &str, slot: &str, model: &str) -> AutotierProviderSlotDto {
        AutotierProviderSlotDto {
            provider_id: provider.into(),
            slot: slot.into(),
            model_id: model.into(),
            capability_status: "unknown".into(),
            supports_tools: Some(true),
            supports_streaming: Some(true),
            supports_vision: Some(false),
            context_limit: Some(200_000),
            api_format: Some("anthropic".into()),
            pricing_source: Some("builtin".into()),
            capability_source: Some("manual".into()),
            verified_at: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn get_config_defaults_to_shadow_without_keys() {
        let db = db();
        let view = load_routing_config(&db).unwrap();
        assert_eq!(view.mode, "shadow");
        assert!(!view.raw_prompt_opt_in);
        assert_eq!(view.capability_table_version, CAPABILITY_TABLE_VERSION);
        let json = serde_json::to_value(&view).unwrap();
        assert!(!json_has_secret(&json));
    }

    #[test]
    fn save_off_and_shadow_roundtrip() {
        let db = db();
        let off = save_routing_config(
            &db,
            SaveRoutingConfigInput {
                mode: "off".into(),
                retention_days: 14,
            },
        )
        .unwrap();
        assert_eq!(off.mode, "off");
        assert_eq!(off.retention_days, 14);

        let shadow = save_routing_config(
            &db,
            SaveRoutingConfigInput {
                mode: "Shadow".into(),
                retention_days: 30,
            },
        )
        .unwrap();
        assert_eq!(shadow.mode, "shadow");
        assert_eq!(shadow.classifier_version, CLASSIFIER_VERSION);
    }

    #[test]
    fn illegal_mode_is_rejected_on_save() {
        let db = db();
        let err = save_routing_config(
            &db,
            SaveRoutingConfigInput {
                mode: "banana".into(),
                retention_days: 30,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("illegal routing mode"));
        assert_eq!(load_routing_config(&db).unwrap().mode, "shadow");
    }

    #[test]
    fn live_mode_is_degraded_to_shadow_and_not_exposed() {
        let db = db();
        let view = save_routing_config(
            &db,
            SaveRoutingConfigInput {
                mode: "full_live".into(),
                retention_days: 30,
            },
        )
        .unwrap();
        assert_eq!(view.mode, "shadow");
        assert_eq!(view.degraded_from.as_deref(), Some("full_live"));
        assert_eq!(db.autotier_get_config().unwrap().mode, "shadow");
    }

    #[test]
    fn stored_live_config_is_degraded_on_read() {
        let db = db();
        let mut config = db.autotier_get_config().unwrap();
        config.mode = "canary_live".into();
        db.autotier_set_config(&config).unwrap();
        let view = load_routing_config(&db).unwrap();
        assert_eq!(view.mode, "shadow");
        assert_eq!(view.degraded_from.as_deref(), Some("canary_live"));
        assert_eq!(db.autotier_get_config().unwrap().mode, "shadow");
    }

    #[test]
    fn illegal_retention_is_rejected() {
        let db = db();
        let err = save_routing_config(
            &db,
            SaveRoutingConfigInput {
                mode: "shadow".into(),
                retention_days: 11,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("retention_days"));
    }

    #[test]
    fn slot_crud_and_required_status() {
        let db = db();
        let cheap = upsert_provider_slot(&db, sample_slot("p1", "cheap", "claude-haiku")).unwrap();
        assert_eq!(cheap.slot, "cheap");
        assert!(cheap.created_at > 0);
        upsert_provider_slot(&db, sample_slot("p1", "mid", "claude-sonnet")).unwrap();
        let status = required_slots_status(&db, "p1").unwrap();
        assert!(!status.complete);
        assert_eq!(status.missing, vec!["strong".to_string()]);

        upsert_provider_slot(&db, sample_slot("p1", "strong", "claude-opus")).unwrap();
        let status = required_slots_status(&db, "p1").unwrap();
        assert!(status.complete);
        assert!(status.missing.is_empty());

        let listed = list_provider_slots(&db, "p1").unwrap();
        assert_eq!(listed.len(), 3);
        let json = serde_json::to_value(&listed).unwrap();
        assert!(!json_has_secret(&json));

        assert_eq!(delete_provider_slot(&db, "p1", "mid").unwrap(), 1);
        assert_eq!(list_provider_slots(&db, "p1").unwrap().len(), 2);
        assert!(!required_slots_status(&db, "p1").unwrap().complete);
    }

    #[test]
    fn illegal_slot_and_capability_are_rejected() {
        let db = db();
        let mut bad = sample_slot("p1", "ultra", "m");
        assert!(upsert_provider_slot(&db, bad.clone()).is_err());
        bad.slot = "cheap".into();
        bad.capability_status = "live-ready".into();
        assert!(upsert_provider_slot(&db, bad).is_err());
    }

    #[test]
    fn responses_never_include_provider_keys() {
        let db = db();
        upsert_provider_slot(&db, sample_slot("p-key", "cheap", "claude-haiku")).unwrap();
        let slots = serde_json::to_value(list_provider_slots(&db, "p-key").unwrap()).unwrap();
        let config = serde_json::to_value(load_routing_config(&db).unwrap()).unwrap();
        assert!(!json_has_secret(&slots));
        assert!(!json_has_secret(&config));
        assert!(slots[0].get("api_key").is_none());
        assert!(config.get("api_key").is_none());
    }

    #[test]
    fn no_live_command_surface() {
        let src = include_str!("autotier.rs");
        for forbidden in [
            "autotier_set_live",
            "autotier_enable_live",
            "autotier_enable_canary",
            "autotier_set_canary",
        ] {
            assert!(
                !src.contains(&format!("pub fn {forbidden}")),
                "must not expose {forbidden}"
            );
        }
    }

    #[test]
    fn clear_and_prune_use_validated_retention() {
        let db = db();
        assert!(prune_decisions(&db, Some(11)).is_err());
        assert_eq!(prune_decisions(&db, Some(30)).unwrap(), 0);
        clear_decisions(&db).unwrap();
    }
}
