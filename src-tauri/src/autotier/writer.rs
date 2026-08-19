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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::{mpsc, oneshot};

use crate::config::get_app_config_dir;
use crate::database::{lock_conn, AutotierDecisionRow, Database, FinalizeDecisionParams};
use crate::error::AppError;

use super::SessionIdHash;

type HmacSha256 = Hmac<Sha256>;

pub const SESSION_SECRET_LEN: usize = 32;
pub const DECISION_QUEUE_CAP: usize = 4096;

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
    pub upstream_message_id: Option<String>,
    pub usage_request_id: Option<String>,
    pub actual_input_tokens: Option<i64>,
    pub actual_output_tokens: Option<i64>,
    pub actual_cache_read_tokens: Option<i64>,
    pub actual_cache_write_5m_tokens: Option<i64>,
    pub actual_cache_write_1h_tokens: Option<i64>,
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
    Flush(oneshot::Sender<()>),
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
        tokio::time::timeout(timeout, wait).await.is_ok()
    }
}

fn writer_slot() -> &'static Mutex<Option<(usize, DecisionWriter)>> {
    static SLOT: OnceLock<Mutex<Option<(usize, DecisionWriter)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 按 `Arc<Database>` 身份复用 Writer。测试重建 DB 时自动换新消费者。
pub fn writer_for(db: Arc<Database>) -> DecisionWriter {
    let key = Arc::as_ptr(&db) as usize;
    let mut slot = writer_slot().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((existing, writer)) = slot.as_ref() {
        if *existing == key {
            return writer.clone();
        }
    }
    let writer = DecisionWriter::spawn(db, DECISION_QUEUE_CAP);
    *slot = Some((key, writer.clone()));
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
            actual_cache_write_5m_tokens: Some(cache_creation_tokens),
            status_code: Some(status_code),
            ..Default::default()
        },
    )
}

async fn consumer_loop(
    db: Arc<Database>,
    mut rx: mpsc::Receiver<DecisionEvent>,
    write_failures: Arc<AtomicU64>,
) {
    let mut pending_finalize: HashMap<String, FinalizeEvent> = HashMap::new();
    while let Some(event) = rx.recv().await {
        match event {
            DecisionEvent::Flush(ack) => {
                let _ = ack.send(());
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
    pending: &mut HashMap<String, FinalizeEvent>,
) -> Result<(), AppError> {
    match event {
        DecisionEvent::Create(row) => {
            let id = row.decision_id.clone();
            db.autotier_upsert_decision(&row)?;
            if let Some(finalize) = pending.remove(&id) {
                apply_finalize(db, &finalize)?;
            }
            Ok(())
        }
        DecisionEvent::Finalize(finalize) => match apply_finalize(db, &finalize) {
            Ok(()) => Ok(()),
            Err(e) if is_missing_decision(&e) => {
                pending.insert(finalize.decision_id.clone(), finalize);
                Ok(())
            }
            Err(e) => Err(e),
        },
        DecisionEvent::Flush(_) => Ok(()),
    }
}

fn apply_finalize(db: &Database, event: &FinalizeEvent) -> Result<(), AppError> {
    let params = FinalizeDecisionParams {
        decision_id: &event.decision_id,
        completed_at: event.completed_at,
        actual_outbound_model: event.actual_outbound_model.as_deref(),
        actual_outbound_provider: event.actual_outbound_provider.as_deref(),
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
    fill_baseline_outbound(db, event)
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
    use crate::database::AutotierRoutingConfigDto;
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
    fn load_or_create_secret_is_stable_in_scope() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
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
        let input = ShadowInput {
            decision_id: decision_id.to_string(),
            app_type: AppType::Claude,
            session_id: "sess-xyz".to_string(),
            request_model: "claude-sonnet-4-20250514".to_string(),
            provider_id: "p".to_string(),
        };
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let (row, _) =
            build_shadow_row(&input, &body, &AutotierRoutingConfigDto::default(), secret);
        row
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
    async fn thousand_create_finalize_pairs_are_not_silently_lost() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        let db = Arc::new(Database::init().expect("init db"));
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
        assert!(writer.flush(Duration::from_secs(15)).await);
        assert_eq!(writer.dropped(), 0, "queue dropped events");
        let n = db.autotier_count_decisions().expect("count");
        assert_eq!(n, 1000);
        let row = db.autotier_get_decision("d-0000").unwrap().unwrap();
        assert_eq!(row.usage_request_id.as_deref(), Some("usage-1"));
        assert!(!row.session_id_hash.contains("sess-xyz"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn usage_finalize_sets_complete_without_looking_up_logs() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
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
    fn usage_finalize_skips_ineligible_empty_usage() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
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
    async fn finalize_fills_baseline_and_actual_outbound() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
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
}
