//! AutoTier Shadow 的有界会话状态。
//!
//! 这里的 key 只接受已经经过安装级 HMAC 的 `SessionIdHash`。状态仅存在代理
//! 进程内，不把原始 Session ID、Prompt 或请求头写入数据库，也不跨进程持久化。

use std::collections::{HashMap, VecDeque};
use std::sync::{RwLock, RwLockWriteGuard};

use super::{RoutingSessionState, SessionIdHash};

/// 由应用类型、Provider 和 HMAC Session Hash 组成的内存索引。
///
/// 包含 `provider_id` 的原因：缓存连续性是 Provider 级属性——同一客户端会话
/// 切换 Provider（或 failover）后，上游 KV cache 不复存在，旧槽位推荐不能再
/// 作为保护依据。与基座 `GeminiShadowKey(provider_id, session_id)` 的先例一致。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoutingSessionKey {
    app_type: String,
    provider_id: String,
    session_hash: SessionIdHash,
}

impl RoutingSessionKey {
    /// 创建只包含非敏感派生值的 key。
    pub fn new(
        app_type: impl Into<String>,
        provider_id: impl Into<String>,
        session_hash: SessionIdHash,
    ) -> Self {
        Self {
            app_type: app_type.into(),
            provider_id: provider_id.into(),
            session_hash,
        }
    }

    #[cfg(test)]
    fn app_type(&self) -> &str {
        &self.app_type
    }

    #[cfg(test)]
    fn provider_id(&self) -> &str {
        &self.provider_id
    }
}

#[derive(Debug, Default)]
struct RoutingSessionStoreInner {
    sessions: HashMap<RoutingSessionKey, RoutingSessionState>,
    lru_order: VecDeque<RoutingSessionKey>,
}

/// 线程安全、有界的 Shadow 会话状态存储。
///
/// 借鉴上游 Shadow store 的显式容量和 LRU 淘汰，但只保留 AutoTier 的
/// `RoutingSessionState`。代理重启时状态自然清空，属于安全降级：真实请求不受影响，
/// 下一轮 Shadow 只会从默认状态重新开始。
#[derive(Debug)]
pub struct RoutingSessionStore {
    max_sessions: usize,
    inner: RwLock<RoutingSessionStoreInner>,
}

impl Default for RoutingSessionStore {
    fn default() -> Self {
        Self::with_limits(2_000)
    }
}

impl RoutingSessionStore {
    pub fn with_limits(max_sessions: usize) -> Self {
        Self {
            max_sessions: max_sessions.max(1),
            inner: RwLock::new(RoutingSessionStoreInner::default()),
        }
    }

    /// 读取状态；命中时刷新 LRU 顺序。
    pub fn get(&self, key: &RoutingSessionKey) -> Option<RoutingSessionState> {
        let mut inner = self.write_inner();
        let state = inner.sessions.get(key).cloned();
        if state.is_some() {
            Self::touch(&mut inner.lru_order, key);
        }
        state
    }

    /// 在同一把锁内读取旧状态、计算并提交新状态。
    ///
    /// 回调只应执行纯内存计算；不要在锁内做数据库、网络或异步操作。
    pub fn update_with<T, F>(&self, key: RoutingSessionKey, f: F) -> T
    where
        F: FnOnce(&RoutingSessionState) -> (RoutingSessionState, T),
    {
        let mut inner = self.write_inner();
        let current = inner.sessions.get(&key).cloned().unwrap_or_default();
        let (next, result) = f(&current);
        inner.sessions.insert(key.clone(), next);
        Self::touch(&mut inner.lru_order, &key);
        Self::prune(&mut inner, self.max_sessions);
        result
    }

    /// 仅当 key 已存在时更新（响应反馈写回）。
    ///
    /// 与 `update_with` 的区别：不存在时不插入。Usage Finalize 阶段无法区分
    /// “生成的 UUID 会话”与“真实会话”，靠“请求路径只为真实会话建条目”这一
    /// 事实保证反馈不会为匿名请求创建垃圾条目。返回是否命中。
    pub fn update_existing_with<F>(&self, key: &RoutingSessionKey, f: F) -> bool
    where
        F: FnOnce(&RoutingSessionState) -> RoutingSessionState,
    {
        let mut inner = self.write_inner();
        let Some(current) = inner.sessions.get(key).cloned() else {
            return false;
        };
        inner.sessions.insert(key.clone(), f(&current));
        Self::touch(&mut inner.lru_order, key);
        true
    }

    pub fn len(&self) -> usize {
        self.write_inner().sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn write_inner(&self) -> RwLockWriteGuard<'_, RoutingSessionStoreInner> {
        self.inner.write().unwrap_or_else(|poisoned| {
            log::warn!("[AutoTier] recovering poisoned routing session store lock");
            poisoned.into_inner()
        })
    }

    fn touch(order: &mut VecDeque<RoutingSessionKey>, key: &RoutingSessionKey) {
        if let Some(pos) = order.iter().position(|existing| existing == key) {
            order.remove(pos);
        }
        order.push_back(key.clone());
    }

    fn prune(inner: &mut RoutingSessionStoreInner, max_sessions: usize) {
        while inner.sessions.len() > max_sessions {
            let Some(oldest) = inner.lru_order.pop_front() else {
                break;
            };
            inner.sessions.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autotier::ModelSlot;

    fn key(app: &str, provider: &str, hash: &str) -> RoutingSessionKey {
        RoutingSessionKey::new(app, provider, SessionIdHash(hash.to_string()))
    }

    #[test]
    fn missing_state_defaults_without_raw_session_data() {
        let store = RoutingSessionStore::with_limits(4);
        let key = key("claude", "provider-a", "hmac-session-1");

        assert_eq!(store.get(&key), None);
        store.update_with(key.clone(), |current| {
            assert_eq!(current, &RoutingSessionState::default());
            let mut next = current.clone();
            next.last_recommended_slot = Some(ModelSlot::Mid);
            (next, ())
        });

        let state = store.get(&key).expect("stored state");
        assert_eq!(state.last_recommended_slot, Some(ModelSlot::Mid));
        assert_eq!(key.app_type(), "claude");
        assert_eq!(key.provider_id(), "provider-a");
        assert!(!format!("{key:?}").contains("raw-session"));
    }

    #[test]
    fn app_type_provider_and_session_hash_are_isolated() {
        let store = RoutingSessionStore::with_limits(4);
        let claude = key("claude", "provider-a", "same-hash");
        let codex = key("codex", "provider-a", "same-hash");
        let other_provider = key("claude", "provider-b", "same-hash");
        let other_session = key("claude", "provider-a", "other-hash");

        store.update_with(claude.clone(), |state| {
            let mut next = state.clone();
            next.session_request_count = 3;
            (next, ())
        });

        assert_eq!(store.get(&codex), None);
        assert_eq!(store.get(&other_provider), None);
        assert_eq!(store.get(&other_session), None);
        assert_eq!(store.get(&claude).unwrap().session_request_count, 3);
    }

    #[test]
    fn evicts_least_recently_used_session() {
        let store = RoutingSessionStore::with_limits(2);
        let first = key("claude", "provider-a", "first");
        let second = key("claude", "provider-a", "second");
        let third = key("claude", "provider-a", "third");

        store.update_with(first.clone(), |state| (state.clone(), ()));
        store.update_with(second.clone(), |state| (state.clone(), ()));
        assert!(store.get(&first).is_some()); // refresh first; second is now oldest
        store.update_with(third.clone(), |state| (state.clone(), ()));

        assert!(store.get(&first).is_some());
        assert!(store.get(&second).is_none());
        assert!(store.get(&third).is_some());
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn update_existing_only_touches_present_keys() {
        let store = RoutingSessionStore::with_limits(4);
        let present = key("claude", "provider-a", "present");
        let missing = key("claude", "provider-a", "missing");

        // 未命中：不插入、不污染 LRU
        let inserted = store.update_existing_with(&missing, |state| {
            let mut next = state.clone();
            next.last_cache_read_tokens = 500;
            next
        });
        assert!(!inserted);
        assert_eq!(store.len(), 0);

        // 命中：在已有状态上更新
        store.update_with(present.clone(), |state| {
            let mut next = state.clone();
            next.session_request_count = 7;
            (next, ())
        });
        let updated = store.update_existing_with(&present, |state| {
            let mut next = state.clone();
            next.last_cache_read_tokens = 500;
            next.last_cache_read_at = Some(1_700_000_000_000);
            next
        });
        assert!(updated);
        let state = store.get(&present).unwrap();
        assert_eq!(state.session_request_count, 7);
        assert_eq!(state.last_cache_read_tokens, 500);
    }

    #[test]
    fn zero_capacity_is_clamped_and_store_remains_bounded() {
        let store = RoutingSessionStore::with_limits(0);
        let first = key("claude", "provider-a", "first");
        let second = key("claude", "provider-a", "second");

        store.update_with(first, |state| (state.clone(), ()));
        store.update_with(second.clone(), |state| (state.clone(), ()));

        assert_eq!(store.len(), 1);
        assert!(store.get(&second).is_some());
    }
}
