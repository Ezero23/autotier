//! Phase 4B：安装级 Session Secret、HMAC Session Hash、有序 Decision Writer。
//!
//! - Secret：32 字节随机值，权限受限文件（无系统钥匙串时的合同回退）。
//! - Hash：HMAC-SHA-256，同 Scope 稳定、跨 Scope 不同。
//! - Writer：单消费者有界队列；Create/Finalize 按入队顺序处理；队列满或
//!   DB 失败不阻塞请求，只增加丢失/失败计数。

use std::collections::HashMap;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::{mpsc, oneshot};

use crate::config::get_app_config_dir;
use crate::database::{lock_conn, AutotierDecisionRow, Database, FinalizeDecisionParams};
use crate::error::AppError;

use super::cost::{
    evaluate_costs, parse_cost_assumptions, price_leg_is_frozen, push_assumption,
    serialize_cost_assumptions, stamp_model_versions, ttl_from_assumptions, PriceLeg, TokenCounts,
    ASSUMPTION_PRICE_FROZEN, ASSUMPTION_PRICE_MISSING, ASSUMPTION_WRITE_PRICE_COMBINED,
};
use super::SessionIdHash;

type HmacSha256 = Hmac<Sha256>;

pub const SESSION_SECRET_LEN: usize = 32;
pub const DECISION_QUEUE_CAP: usize = 4096;
const PENDING_FINALIZE_CAP: usize = 4096;
const PENDING_FINALIZE_TTL: Duration = Duration::from_secs(300);

/// HMAC-SHA-256 hex（小写）。Secret 不得写入日志。
pub fn hash_session_id(session_id: &str, secret: &[u8]) -> SessionIdHash {
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC-SHA-256 accepts a 32-byte install secret");
    mac.update(session_id.as_bytes());
    SessionIdHash(hex_encode(&mac.finalize().into_bytes()))
}

pub fn session_secret_path() -> PathBuf {
    get_app_config_dir().join("autotier").join("session.secret")
}

/// 读取或创建安装级 32 字节 Secret。不把 Secret 写入日志。
pub fn load_or_create_session_secret() -> Result<[u8; SESSION_SECRET_LEN], AppError> {
    let path = session_secret_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    if path.exists() {
        let bytes = fs::read(&path).map_err(|e| AppError::io(&path, e))?;
        if bytes.len() == SESSION_SECRET_LEN {
            let mut secret = [0u8; SESSION_SECRET_LEN];
            secret.copy_from_slice(&bytes);
            return Ok(secret);
        }
        log::warn!("[AutoTier] session secret length invalid, rotating scope");
    }

    let secret = random_secret();
    fs::write(&path, secret).map_err(|e| AppError::io(&path, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::io(&path, e))?;
    }
    Ok(secret)
}

fn random_secret() -> [u8; SESSION_SECRET_LEN] {
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut out = [0u8; SESSION_SECRET_LEN];
    out[..16].copy_from_slice(a.as_bytes());
    out[16..].copy_from_slice(b.as_bytes());
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Finalize 入队载荷（拥有所有权，供跨 await 传递）。
///
/// Phase 4C：Forwarder 完成后回填 Baseline/Actual。DAO Finalize 目前只
/// COALESCE 更新 `actual_outbound_*`，Baseline 由 Writer 同事务外补写
///（`None` 不覆盖 Create 时的空值以外的已有值）。
#[derive(Debug, Clone, Default)]
pub struct FinalizeEvent {
    pub decision_id: String,
    pub completed_at: i64,
    pub baseline_outbound_model: Option<String>,
    pub baseline_outbound_provider: Option<String>,
    pub actual_outbound_model: Option<String>,
    pub actual_outbound_provider: Option<String>,
    pub vision_fallback_applied: Option<bool>,
    pub vision_describe_input_tokens: Option<i64>,
    pub vision_describe_output_tokens: Option<i64>,
    pub upstream_message_id: Option<String>,
    pub usage_request_id: Option<String>,
    pub actual_input_tokens: Option<i64>,
    pub actual_output_tokens: Option<i64>,
    pub actual_cache_read_tokens: Option<i64>,
    pub actual_cache_write_5m_tokens: Option<i64>,
    pub actual_cache_write_1h_tokens: Option<i64>,
    /// 上游合并的 cache creation；Writer 按请求 TTL 归因到 5m/1h/unknown。
    pub cache_creation_tokens: Option<i64>,
    pub actual_cost_usd: Option<String>,
    pub status_code: Option<i64>,
    pub outcome: Option<String>,
    pub retry_count: Option<i32>,
    pub fallback_count: Option<i32>,
    pub error_code: Option<String>,
}

#[allow(clippy::large_enum_variant)] // Create carries a full decision row; Finalize is smaller.
pub enum DecisionEvent {
    Create(AutotierDecisionRow),
    Finalize(FinalizeEvent),
    Flush(oneshot::Sender<bool>),
}

#[derive(Clone)]
pub struct DecisionWriter {
    tx: mpsc::Sender<DecisionEvent>,
    dropped: Arc<AtomicU64>,
    write_failures: Arc<AtomicU64>,
}

impl DecisionWriter {
    pub fn spawn(db: Arc<Database>, capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let write_failures = Arc::new(AtomicU64::new(0));
        let failures = write_failures.clone();
        tokio::spawn(async move {
            consumer_loop(db, rx, failures).await;
        });
        Self {
            tx,
            dropped,
            write_failures,
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn write_failures(&self) -> u64 {
        self.write_failures.load(Ordering::Relaxed)
    }

    /// 非阻塞入队。队列满或已关闭时计数并返回 false，不阻塞调用方。
    pub fn try_enqueue(&self, event: DecisionEvent) -> bool {
        match self.tx.try_send(event) {
            Ok(()) => true,
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                log::warn!("[AutoTier] decision queue full or closed; observation dropped");
                false
            }
        }
    }

    pub async fn flush(&self, timeout: Duration) -> bool {
        let (ack, wait) = oneshot::channel();
        if tokio::time::timeout(timeout, self.tx.send(DecisionEvent::Flush(ack)))
            .await
            .is_err()
        {
            return false;
        }
        matches!(tokio::time::timeout(timeout, wait).await, Ok(Ok(true)))
    }
}

type WriterSlot = Mutex<Option<(Weak<Database>, DecisionWriter)>>;

fn writer_slot() -> &'static WriterSlot {
    static SLOT: OnceLock<WriterSlot> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 按 `Arc<Database>` 身份复用 Writer。使用 Weak 做 ptr_eq，避免数据库释放后
/// 新 Arc 恰好复用同一地址而错误复用已关闭的 Writer channel。
pub fn writer_for(db: Arc<Database>) -> DecisionWriter {
    let mut slot = writer_slot().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((existing, writer)) = slot.as_ref() {
        let current = Arc::downgrade(&db);
        if existing.ptr_eq(&current) {
            return writer.clone();
        }
    }
    let writer = DecisionWriter::spawn(db.clone(), DECISION_QUEUE_CAP);
    *slot = Some((Arc::downgrade(&db), writer.clone()));
    writer
}

pub fn enqueue_create(db: Arc<Database>, row: AutotierDecisionRow) -> bool {
    writer_for(db).try_enqueue(DecisionEvent::Create(row))
}

pub fn enqueue_finalize(db: Arc<Database>, event: FinalizeEvent) -> bool {
    writer_for(db).try_enqueue(DecisionEvent::Finalize(event))
}

/// Phase 5A：按 `decision_id` 回填 Usage Logger 的最终 ID 与 token。
///
/// 无 `message_id` 且无计费 token 时不写（合法无 Usage，保持 `is_complete=false`）。
/// 不反查 `proxy_request_logs`。
#[allow(clippy::too_many_arguments)]
pub fn enqueue_usage_finalize(
    db: Arc<Database>,
    decision_id: String,
    upstream_message_id: Option<String>,
    usage_request_id: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    status_code: i64,
) -> bool {
    let eligible = upstream_message_id.is_some()
        || input_tokens > 0
        || output_tokens > 0
        || cache_read_tokens > 0
        || cache_creation_tokens > 0;
    if !eligible {
        return false;
    }
    enqueue_finalize(
        db,
        FinalizeEvent {
            decision_id,
            completed_at: chrono::Utc::now().timestamp_millis(),
            upstream_message_id,
            usage_request_id: Some(usage_request_id),
            actual_input_tokens: Some(input_tokens),
            actual_output_tokens: Some(output_tokens),
            actual_cache_read_tokens: Some(cache_read_tokens),
            cache_creation_tokens: Some(cache_creation_tokens),
            status_code: Some(status_code),
            ..Default::default()
        },
    )
}

struct PendingFinalize {
    event: FinalizeEvent,
    queued_at: Instant,
}

fn evict_stale_pending(pending: &mut HashMap<String, PendingFinalize>) {
    let now = Instant::now();
    let stale = pending
        .iter()
        .filter(|(_, item)| now.duration_since(item.queued_at) > PENDING_FINALIZE_TTL)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in stale {
        pending.remove(&id);
        log::warn!("[AutoTier] pending Finalize expired; observation dropped");
    }

    while pending.len() >= PENDING_FINALIZE_CAP {
        let oldest = pending
            .iter()
            .min_by_key(|(_, item)| item.queued_at)
            .map(|(id, _)| id.clone());
        let Some(id) = oldest else { break };
        pending.remove(&id);
        log::warn!("[AutoTier] pending Finalize capacity reached; observation dropped");
    }
}

async fn consumer_loop(
    db: Arc<Database>,
    mut rx: mpsc::Receiver<DecisionEvent>,
    write_failures: Arc<AtomicU64>,
) {
    let mut pending_finalize: HashMap<String, PendingFinalize> = HashMap::new();
    while let Some(event) = rx.recv().await {
        evict_stale_pending(&mut pending_finalize);
        match event {
            DecisionEvent::Flush(ack) => {
                let _ = ack.send(pending_finalize.is_empty());
            }
            other => {
                let db = db.clone();
                let failures = write_failures.clone();
                let result = catch_unwind(AssertUnwindSafe(|| {
                    process_event(&db, other, &mut pending_finalize)
                }));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        failures.fetch_add(1, Ordering::Relaxed);
                        log::warn!("[AutoTier] decision writer failed: {e}");
                    }
                    Err(_) => {
                        failures.fetch_add(1, Ordering::Relaxed);
                        log::warn!("[AutoTier] decision writer panicked; consumer continues");
                    }
                }
            }
        }
    }
}

fn process_event(
    db: &Database,
    event: DecisionEvent,
    pending: &mut HashMap<String, PendingFinalize>,
) -> Result<(), AppError> {
    match event {
        DecisionEvent::Create(row) => {
            let id = row.decision_id.clone();
            db.autotier_upsert_decision(&row)?;
            if let Some(pending_finalize) = pending.remove(&id) {
                if let Err(error) = apply_finalize(db, &pending_finalize.event) {
                    pending.insert(id, pending_finalize);
                    return Err(error);
                }
            }
            Ok(())
        }
        DecisionEvent::Finalize(finalize) => match apply_finalize(db, &finalize) {
            Ok(()) => Ok(()),
            Err(e) if is_missing_decision(&e) => {
                pending.insert(
                    finalize.decision_id.clone(),
                    PendingFinalize {
                        event: finalize,
                        queued_at: Instant::now(),
                    },
                );
                Ok(())
            }
            Err(e) => Err(e),
        },
        DecisionEvent::Flush(_) => Ok(()),
    }
}

fn apply_finalize(db: &Database, event: &FinalizeEvent) -> Result<(), AppError> {
    let (event, cost) = enrich_usage_costs(db, event)?;
    let params = FinalizeDecisionParams {
        decision_id: &event.decision_id,
        completed_at: event.completed_at,
        actual_outbound_model: event.actual_outbound_model.as_deref(),
        actual_outbound_provider: event.actual_outbound_provider.as_deref(),
        vision_fallback_applied: event.vision_fallback_applied,
        vision_describe_input_tokens: event.vision_describe_input_tokens,
        vision_describe_output_tokens: event.vision_describe_output_tokens,
        upstream_message_id: event.upstream_message_id.as_deref(),
        usage_request_id: event.usage_request_id.as_deref(),
        actual_input_tokens: event.actual_input_tokens,
        actual_output_tokens: event.actual_output_tokens,
        actual_cache_read_tokens: event.actual_cache_read_tokens,
        actual_cache_write_5m_tokens: event.actual_cache_write_5m_tokens,
        actual_cache_write_1h_tokens: event.actual_cache_write_1h_tokens,
        actual_cost_usd: event.actual_cost_usd.as_deref(),
        status_code: event.status_code,
        outcome: event.outcome.as_deref(),
        retry_count: event.retry_count,
        fallback_count: event.fallback_count,
        error_code: event.error_code.as_deref(),
    };
    db.autotier_finalize_decision(&params)?;
    fill_baseline_outbound(db, &event)?;
    persist_candidate_costs(db, &event.decision_id, &cost)
}

struct CostPersist {
    low: Option<String>,
    base: Option<String>,
    high: Option<String>,
    assumptions_json: Option<String>,
}

/// 在 DAO 写入前按已冻结快照计算成本；缺行则跳过（Create 尚未落库）。
fn enrich_usage_costs(
    db: &Database,
    event: &FinalizeEvent,
) -> Result<(FinalizeEvent, CostPersist), AppError> {
    let mut event = event.clone();
    let skip = CostPersist {
        low: None,
        base: None,
        high: None,
        assumptions_json: None,
    };
    let Some(row) = db.autotier_get_decision(&event.decision_id)? else {
        return Ok((event, skip));
    };

    let mut doc = parse_cost_assumptions(&row.cost_assumptions_json);
    stamp_model_versions(&mut doc);

    if !price_leg_is_frozen(doc.baseline.as_ref()) {
        let model = event
            .actual_outbound_model
            .as_deref()
            .or(row.actual_outbound_model.as_deref())
            .filter(|s| !s.is_empty())
            .unwrap_or(row.client_requested_model.as_str());
        let provider = event
            .actual_outbound_provider
            .as_deref()
            .or(row.actual_outbound_provider.as_deref())
            .or(row.initial_selected_provider.as_deref());
        if let Some(leg) = lookup_price_leg(db, provider, model)? {
            doc.baseline = Some(leg);
            push_assumption(&mut doc, ASSUMPTION_PRICE_FROZEN);
            push_assumption(&mut doc, ASSUMPTION_WRITE_PRICE_COMBINED);
        } else {
            push_assumption(&mut doc, ASSUMPTION_PRICE_MISSING);
        }
    }

    if !price_leg_is_frozen(doc.candidate.as_ref()) {
        if let Some(model) = row.candidate_model.as_deref().filter(|s| !s.is_empty()) {
            if let Some(leg) = lookup_price_leg(db, row.candidate_provider.as_deref(), model)? {
                doc.candidate = Some(leg);
            }
        }
    }

    let usage_like = event.actual_input_tokens.is_some()
        || event.actual_output_tokens.is_some()
        || event.actual_cache_read_tokens.is_some()
        || event.cache_creation_tokens.is_some();

    if !usage_like {
        return Ok((
            event,
            CostPersist {
                low: None,
                base: None,
                high: None,
                assumptions_json: Some(serialize_cost_assumptions(&doc)),
            },
        ));
    }

    // 历史行已有实际成本：不得用 live 价重写金额或快照。
    if row.actual_cost_usd.is_some() {
        return Ok((event, skip));
    }

    let ttl = ttl_from_assumptions(&doc);
    let tokens = TokenCounts {
        input: event.actual_input_tokens.unwrap_or(0),
        output: event.actual_output_tokens.unwrap_or(0),
        cache_read: event.actual_cache_read_tokens.unwrap_or(0),
        cache_creation: event.cache_creation_tokens.unwrap_or(0),
        retry_count: event.retry_count.unwrap_or(row.retry_count),
        fallback_count: event.fallback_count.unwrap_or(row.fallback_count),
    };
    let inclusive = crate::services::sql_helpers::is_cache_inclusive_app(&row.app_type);
    let baseline_rates = doc.baseline.as_ref().and_then(PriceLeg::rates);
    let candidate_rates = doc.candidate.as_ref().and_then(PriceLeg::rates);
    let outcome = evaluate_costs(
        tokens,
        ttl,
        baseline_rates.as_ref(),
        candidate_rates.as_ref(),
        inclusive,
        doc,
    );

    event.actual_cache_write_5m_tokens = outcome.write_5m;
    event.actual_cache_write_1h_tokens = outcome.write_1h;
    event.actual_cost_usd = outcome.actual_usd.clone();
    Ok((
        event,
        CostPersist {
            low: outcome.candidate.low_usd,
            base: outcome.candidate.base_usd,
            high: outcome.candidate.high_usd,
            assumptions_json: Some(serialize_cost_assumptions(&outcome.assumptions)),
        },
    ))
}

fn lookup_price_leg(
    db: &Database,
    provider_id: Option<&str>,
    model_id: &str,
) -> Result<Option<PriceLeg>, AppError> {
    if model_id.is_empty() {
        return Ok(None);
    }
    let conn = lock_conn!(db.conn);
    let Some((input, output, cache_read, cache_creation)) =
        crate::services::usage_stats::find_provider_model_pricing_row(
            &conn,
            provider_id,
            model_id,
        )?
    else {
        return Ok(None);
    };
    Ok(Some(PriceLeg {
        provider_id: provider_id.map(str::to_string),
        model_id: model_id.to_string(),
        price_source: if provider_id.is_some() {
            "provider_snapshot_or_global_fallback".to_string()
        } else {
            "builtin_global".to_string()
        },
        price_observed_at: chrono::Utc::now().timestamp_millis(),
        input_per_million: input,
        output_per_million: output,
        cache_read_per_million: cache_read,
        cache_write_5m_per_million: cache_creation.clone(),
        cache_write_1h_per_million: cache_creation,
    }))
}

fn persist_candidate_costs(
    db: &Database,
    decision_id: &str,
    cost: &CostPersist,
) -> Result<(), AppError> {
    if cost.low.is_none()
        && cost.base.is_none()
        && cost.high.is_none()
        && cost.assumptions_json.is_none()
    {
        return Ok(());
    }
    let conn = lock_conn!(db.conn);
    conn.execute(
        "UPDATE autotier_routing_decisions SET
            candidate_cost_low_usd = COALESCE(?2, candidate_cost_low_usd),
            candidate_cost_base_usd = COALESCE(?3, candidate_cost_base_usd),
            candidate_cost_high_usd = COALESCE(?4, candidate_cost_high_usd),
            cost_assumptions_json = COALESCE(?5, cost_assumptions_json)
         WHERE decision_id = ?1",
        rusqlite::params![
            decision_id,
            cost.low.as_deref(),
            cost.base.as_deref(),
            cost.high.as_deref(),
            cost.assumptions_json.as_deref(),
        ],
    )
    .map_err(|e| AppError::Database(format!("candidate cost update failed: {e}")))?;
    Ok(())
}

/// DAO Finalize 不更新 Baseline；Shadow 下 Baseline == Actual，须在 Forwarder 后回填。
fn fill_baseline_outbound(db: &Database, event: &FinalizeEvent) -> Result<(), AppError> {
    if event.baseline_outbound_model.is_none() && event.baseline_outbound_provider.is_none() {
        return Ok(());
    }
    let conn = lock_conn!(db.conn);
    conn.execute(
        "UPDATE autotier_routing_decisions SET
            baseline_outbound_model = COALESCE(?2, baseline_outbound_model),
            baseline_outbound_provider = COALESCE(?3, baseline_outbound_provider)
         WHERE decision_id = ?1",
        rusqlite::params![
            event.decision_id,
            event.baseline_outbound_model.as_deref(),
            event.baseline_outbound_provider.as_deref(),
        ],
    )
    .map_err(|e| AppError::Database(format!("baseline outbound update failed: {e}")))?;
    Ok(())
}

fn is_missing_decision(err: &AppError) -> bool {
    err.to_string().contains("not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::autotier::{build_shadow_row, ShadowInput};
    use crate::database::{lock_conn, AutotierRoutingConfigDto};
    use crate::error::AppError;
    use serde_json::json;
    use sha2::Digest;

    const SCOPE_A: [u8; 32] = [0x11; 32];
    const SCOPE_B: [u8; 32] = [0x22; 32];

    fn sha256_hex(input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        hex_encode(&hasher.finalize())
    }

    #[test]
    fn hmac_same_scope_is_stable() {
        let a = hash_session_id("sess-xyz", &SCOPE_A);
        let b = hash_session_id("sess-xyz", &SCOPE_A);
        assert_eq!(a.0, b.0);
        assert_ne!(a.0, "sess-xyz");
        assert!(!a.0.contains("sess-xyz"));
    }

    #[test]
    fn hmac_cross_scope_differs() {
        let a = hash_session_id("sess-xyz", &SCOPE_A);
        let b = hash_session_id("sess-xyz", &SCOPE_B);
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn hmac_replaces_unsalted_sha256() {
        let hmac = hash_session_id("sess-xyz", &SCOPE_A);
        assert_ne!(hmac.0, sha256_hex("sess-xyz"));
    }

    #[test]
    #[serial_test::serial]
    fn load_or_create_secret_is_stable_in_scope() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let first = load_or_create_session_secret().unwrap();
        let second = load_or_create_session_secret().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(session_secret_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    fn sample_row(decision_id: &str, secret: &[u8]) -> AutotierDecisionRow {
        sample_row_with(
            decision_id,
            secret,
            "claude-sonnet-4-20250514",
            json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        )
    }

    fn sample_row_with(
        decision_id: &str,
        secret: &[u8],
        model: &str,
        body: serde_json::Value,
    ) -> AutotierDecisionRow {
        let input = ShadowInput {
            decision_id: decision_id.to_string(),
            app_type: AppType::Claude,
            session_id: "sess-xyz".to_string(),
            request_model: model.to_string(),
            provider_id: "p".to_string(),
        };
        let (row, _) =
            build_shadow_row(&input, &body, &AutotierRoutingConfigDto::default(), secret);
        row
    }

    fn set_model_input_price(db: &Database, model_id: &str, price: &str) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "UPDATE model_pricing SET input_cost_per_million = ?2 WHERE model_id = ?1",
            rusqlite::params![model_id, price],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn queue_full_does_not_block() {
        let (tx, _rx) = mpsc::channel(1);
        let writer = DecisionWriter {
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
            write_failures: Arc::new(AtomicU64::new(0)),
        };
        let row = sample_row("fill", &SCOPE_A);
        assert!(writer.try_enqueue(DecisionEvent::Create(row.clone())));
        assert!(!writer.try_enqueue(DecisionEvent::Create(row)));
        assert_eq!(writer.dropped(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn thousand_create_finalize_pairs_are_not_silently_lost() {
        // This test verifies queue ordering and losslessness, not filesystem throughput.
        // An in-memory database keeps the assertion deterministic on Windows CI.
        let db = Arc::new(Database::memory().expect("init in-memory db"));
        let writer = DecisionWriter::spawn(db.clone(), DECISION_QUEUE_CAP);

        let mut joins = Vec::with_capacity(1000);
        for i in 0..1000 {
            let writer = writer.clone();
            joins.push(tokio::spawn(async move {
                let id = format!("d-{i:04}");
                let row = sample_row(&id, &SCOPE_A);
                assert!(writer.try_enqueue(DecisionEvent::Create(row)));
                assert!(writer.try_enqueue(DecisionEvent::Finalize(FinalizeEvent {
                    decision_id: id,
                    completed_at: 1,
                    usage_request_id: Some("usage-1".into()),
                    ..Default::default()
                })));
            }));
        }
        for join in joins {
            join.await.expect("task");
        }
        // Windows CI 的 SQLite + debug test binary 处理 2,000 个事件可能超过 15s；
        // 这里验证的是不丢事件，不是把磁盘吞吐当作产品 SLO。
        assert!(writer.flush(Duration::from_secs(120)).await);
        assert_eq!(writer.dropped(), 0, "queue dropped events");
        let n = db.autotier_count_decisions().expect("count");
        assert_eq!(n, 1000);
        let row = db.autotier_get_decision("d-0000").unwrap().unwrap();
        assert_eq!(row.usage_request_id.as_deref(), Some("usage-1"));
        assert!(!row.session_id_hash.contains("sess-xyz"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn usage_finalize_sets_complete_without_looking_up_logs() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));
        let row = sample_row("d-usage", &SCOPE_A);
        assert!(enqueue_create(db.clone(), row));
        assert!(enqueue_usage_finalize(
            db.clone(),
            "d-usage".into(),
            Some("msg_abc".into()),
            "session:msg_abc".into(),
            10,
            5,
            0,
            0,
            200,
        ));
        assert!(writer_for(db.clone()).flush(Duration::from_secs(5)).await);
        let got = db.autotier_get_decision("d-usage").unwrap().unwrap();
        assert_eq!(got.upstream_message_id.as_deref(), Some("msg_abc"));
        assert_eq!(got.usage_request_id.as_deref(), Some("session:msg_abc"));
        assert_eq!(got.actual_input_tokens, Some(10));
        assert_eq!(got.actual_output_tokens, Some(5));
        assert!(got.is_complete);
    }

    #[test]
    #[serial_test::serial]
    fn usage_finalize_skips_ineligible_empty_usage() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));
        assert!(!enqueue_usage_finalize(
            db,
            "d-skip".into(),
            None,
            "uuid-fallback".into(),
            0,
            0,
            0,
            0,
            500,
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn finalize_fills_baseline_and_actual_outbound() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));
        let writer = DecisionWriter::spawn(db.clone(), DECISION_QUEUE_CAP);
        let row = sample_row("d-out", &SCOPE_A);
        assert!(writer.try_enqueue(DecisionEvent::Create(row)));
        assert!(writer.try_enqueue(DecisionEvent::Finalize(FinalizeEvent {
            decision_id: "d-out".into(),
            completed_at: 2,
            baseline_outbound_model: Some("claude-sonnet-4-20250514".into()),
            baseline_outbound_provider: Some("p-real".into()),
            actual_outbound_model: Some("claude-sonnet-4-20250514".into()),
            actual_outbound_provider: Some("p-real".into()),
            status_code: Some(200),
            outcome: Some("success".into()),
            ..Default::default()
        })));
        assert!(writer.flush(Duration::from_secs(5)).await);
        let got = db.autotier_get_decision("d-out").unwrap().unwrap();
        assert_eq!(
            got.baseline_outbound_model.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
        assert_eq!(got.actual_outbound_model, got.baseline_outbound_model);
        assert_eq!(got.baseline_outbound_provider.as_deref(), Some("p-real"));
        assert_eq!(got.actual_outbound_provider, got.baseline_outbound_provider);
        assert_eq!(got.status_code, Some(200));
        assert_eq!(got.outcome.as_deref(), Some("success"));
        assert!(got.completed_at.is_some());
        assert!(!got.is_complete);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn usage_unknown_ttl_does_not_fill_5m_or_1h() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));
        let row = sample_row("d-ttl-unknown", &SCOPE_A);
        assert!(enqueue_create(db.clone(), row));
        assert!(enqueue_usage_finalize(
            db.clone(),
            "d-ttl-unknown".into(),
            Some("msg_u".into()),
            "session:msg_u".into(),
            1000,
            500,
            200,
            100,
            200,
        ));
        assert!(writer_for(db.clone()).flush(Duration::from_secs(5)).await);
        let got = db.autotier_get_decision("d-ttl-unknown").unwrap().unwrap();
        assert_eq!(got.actual_cache_write_5m_tokens, None);
        assert_eq!(got.actual_cache_write_1h_tokens, None);
        assert!(got.actual_cost_usd.is_some(), "combined write still priced");
        let doc: serde_json::Value = serde_json::from_str(&got.cost_assumptions_json).unwrap();
        assert_eq!(doc["cache_write_ttl"], "unknown");
        assert_eq!(doc["breakdown_coverage"], "partial");
        assert!(got.candidate_cost_high_usd.is_some());
        assert_ne!(got.candidate_cost_high_usd, got.actual_cost_usd);
        assert_ne!(got.candidate_cost_low_usd, got.candidate_cost_high_usd);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn usage_5m_and_1h_ttl_are_attributed_separately() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));

        let body_5m = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "s", "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": "hi"}]
        });
        assert!(enqueue_create(
            db.clone(),
            sample_row_with("d-ttl-5m", &SCOPE_A, "claude-sonnet-4-20250514", body_5m),
        ));
        assert!(enqueue_usage_finalize(
            db.clone(),
            "d-ttl-5m".into(),
            Some("msg5".into()),
            "session:msg5".into(),
            10,
            5,
            0,
            40,
            200,
        ));

        let body_1h = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "s", "cache_control": {"type": "ephemeral", "ttl": "1h"}}],
            "messages": [{"role": "user", "content": "hi"}]
        });
        assert!(enqueue_create(
            db.clone(),
            sample_row_with("d-ttl-1h", &SCOPE_A, "claude-sonnet-4-20250514", body_1h),
        ));
        assert!(enqueue_usage_finalize(
            db.clone(),
            "d-ttl-1h".into(),
            Some("msg1".into()),
            "session:msg1".into(),
            10,
            5,
            0,
            40,
            200,
        ));
        assert!(writer_for(db.clone()).flush(Duration::from_secs(5)).await);

        let got5 = db.autotier_get_decision("d-ttl-5m").unwrap().unwrap();
        assert_eq!(got5.actual_cache_write_5m_tokens, Some(40));
        assert_eq!(got5.actual_cache_write_1h_tokens, None);
        let doc5: serde_json::Value = serde_json::from_str(&got5.cost_assumptions_json).unwrap();
        assert_eq!(doc5["cache_write_ttl"], "5m");

        let got1 = db.autotier_get_decision("d-ttl-1h").unwrap().unwrap();
        assert_eq!(got1.actual_cache_write_5m_tokens, None);
        assert_eq!(got1.actual_cache_write_1h_tokens, Some(40));
        let doc1: serde_json::Value = serde_json::from_str(&got1.cost_assumptions_json).unwrap();
        assert_eq!(doc1["cache_write_ttl"], "1h");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn historical_price_update_does_not_rewrite_decision_cost() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));
        let row = sample_row("d-freeze", &SCOPE_A);
        assert!(enqueue_create(db.clone(), row));
        assert!(enqueue_finalize(
            db.clone(),
            FinalizeEvent {
                decision_id: "d-freeze".into(),
                completed_at: 1,
                actual_outbound_model: Some("claude-sonnet-4-20250514".into()),
                actual_outbound_provider: Some("p".into()),
                ..Default::default()
            }
        ));
        assert!(writer_for(db.clone()).flush(Duration::from_secs(5)).await);
        let before = db.autotier_get_decision("d-freeze").unwrap().unwrap();
        let snap_before: serde_json::Value =
            serde_json::from_str(&before.cost_assumptions_json).unwrap();
        let frozen_input = snap_before["baseline"]["input_per_million"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!frozen_input.is_empty());

        set_model_input_price(&db, "claude-sonnet-4-20250514", "99").unwrap();

        assert!(enqueue_usage_finalize(
            db.clone(),
            "d-freeze".into(),
            Some("msg_f".into()),
            "session:msg_f".into(),
            1000,
            0,
            0,
            0,
            200,
        ));
        assert!(writer_for(db.clone()).flush(Duration::from_secs(5)).await);
        let after = db.autotier_get_decision("d-freeze").unwrap().unwrap();
        let snap_after: serde_json::Value =
            serde_json::from_str(&after.cost_assumptions_json).unwrap();
        assert_eq!(
            snap_after["baseline"]["input_per_million"].as_str(),
            Some(frozen_input.as_str())
        );
        let cost = after.actual_cost_usd.expect("priced from frozen snapshot");
        // 1000 * 3 / 1M = 0.003，不是 99/million。
        assert_eq!(cost, "0.003");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial_test::serial]
    async fn missing_price_leaves_actual_cost_unset() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AUTOTIER_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));
        let body = json!({
            "model": "no-such-autotier-model-xyz",
            "messages": [{"role": "user", "content": "hi"}]
        });
        assert!(enqueue_create(
            db.clone(),
            sample_row_with("d-noprice", &SCOPE_A, "no-such-autotier-model-xyz", body),
        ));
        assert!(enqueue_usage_finalize(
            db.clone(),
            "d-noprice".into(),
            Some("msg_n".into()),
            "session:msg_n".into(),
            1000,
            500,
            0,
            0,
            200,
        ));
        assert!(writer_for(db.clone()).flush(Duration::from_secs(5)).await);
        let got = db.autotier_get_decision("d-noprice").unwrap().unwrap();
        assert_eq!(got.actual_cost_usd, None);
        assert_eq!(got.candidate_cost_low_usd, None);
        let doc: serde_json::Value = serde_json::from_str(&got.cost_assumptions_json).unwrap();
        let assumptions = doc["assumptions"].as_array().unwrap();
        assert!(assumptions
            .iter()
            .any(|v| v.as_str() == Some("PRICE_MISSING")));
    }
}
