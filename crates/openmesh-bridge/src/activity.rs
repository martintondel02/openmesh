//! Bounded activity log for write API calls (AI-NATIVE.md §2).
//!
//! Every mutating REST request is recorded with its source attribution:
//! `X-OpenMesh-Agent: <name>` → `agent:<name>`, `X-OpenMesh-Client: web|cli` → that value, and
//! otherwise `user`. Entries never contain request bodies, message text, tokens or PSKs.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{Value, json};

use openmesh_core::util;

/// Ring buffer capacity required by the specification.
pub const ACTIVITY_CAPACITY: usize = 256;

#[derive(Clone, Debug, Serialize)]
pub struct ActivityEntry {
    pub id: u64,
    pub at_ms: u64,
    pub source: String,
    pub method: String,
    pub path: String,
    pub summary: String,
    pub ok: bool,
    pub error: Option<String>,
}

impl ActivityEntry {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Thread-safe bounded log shared by the HTTP server and setup/state reporting.
#[derive(Debug)]
pub struct ActivityLog {
    entries: Mutex<VecDeque<ActivityEntry>>,
    next_id: AtomicU64,
    agent_writes: AtomicU64,
    last_agent_at_ms: AtomicU64,
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityLog {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(ACTIVITY_CAPACITY)),
            next_id: AtomicU64::new(1),
            agent_writes: AtomicU64::new(0),
            last_agent_at_ms: AtomicU64::new(0),
        }
    }

    /// Record one mutation. `error` is a plain-language message, never a request body.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        source: &str,
        method: &str,
        path: &str,
        summary: impl Into<String>,
        ok: bool,
        error: Option<String>,
    ) {
        let entry = ActivityEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            at_ms: util::now_ms(),
            source: source.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            summary: summary.into(),
            ok,
            error,
        };
        if source.starts_with("agent:") {
            self.agent_writes.fetch_add(1, Ordering::Relaxed);
            self.last_agent_at_ms.store(entry.at_ms, Ordering::Relaxed);
        }
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.len() == ACTIVITY_CAPACITY {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Clear the bounded log (confirmed factory reset). Returns the number of entries removed.
    pub fn clear(&self) -> usize {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let removed = entries.len();
        entries.clear();
        drop(entries);
        self.agent_writes.store(0, Ordering::Relaxed);
        self.last_agent_at_ms.store(0, Ordering::Relaxed);
        removed
    }

    /// Most recent entries first (newest-first, as shown in the UI/API).
    pub fn recent(&self, limit: usize) -> Vec<Value> {
        let limit = limit.clamp(1, ACTIVITY_CAPACITY);
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries
            .iter()
            .rev()
            .take(limit)
            .map(ActivityEntry::to_value)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `true` once this bridge has seen at least one agent-attributed write.
    pub fn has_agent_activity(&self) -> bool {
        self.agent_writes.load(Ordering::Relaxed) > 0
    }

    pub fn last_agent_at_ms(&self) -> u64 {
        self.last_agent_at_ms.load(Ordering::Relaxed)
    }

    /// `GET /api/activity` payload.
    pub fn payload(&self, limit: usize) -> Value {
        json!({
            "entries": self.recent(limit),
            "capacity": ACTIVITY_CAPACITY,
            "agent_connected": self.has_agent_activity(),
            "last_agent_at_ms": (self.last_agent_at_ms() > 0).then(|| self.last_agent_at_ms()),
            "sources": ["user", "cli", "web", "agent:*", "gateway:*"],
        })
    }
}

/// Derive the activity source from request headers.
pub fn source_from_headers(headers: &std::collections::HashMap<String, String>) -> String {
    let get = |name: &str| {
        headers
            .get(name)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };
    if let Some(agent) = get("x-openmesh-agent") {
        let agent = agent.chars().take(48).collect::<String>();
        return format!("agent:{agent}");
    }
    if let Some(client) = get("x-openmesh-client") {
        let client: String = client
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .take(32)
            .collect();
        if !client.is_empty() {
            return client.to_ascii_lowercase();
        }
    }
    "user".to_string()
}

/// `true` for methods that mutate state and must be tracked.
pub fn is_write_method(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn ring_buffer_is_bounded_and_newest_first() {
        let log = ActivityLog::new();
        for index in 0..(ACTIVITY_CAPACITY + 10) {
            log.record(
                "user",
                "POST",
                "/api/announce",
                format!("action {index}"),
                true,
                None,
            );
        }
        assert_eq!(log.len(), ACTIVITY_CAPACITY);
        let recent = log.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(
            recent[0]["summary"],
            format!("action {}", ACTIVITY_CAPACITY + 9)
        );
        assert_eq!(log.recent(1000).len(), ACTIVITY_CAPACITY);
    }

    #[test]
    fn source_attribution_prefers_agent_then_client_then_user() {
        let mut headers = HashMap::new();
        assert_eq!(source_from_headers(&headers), "user");
        headers.insert("x-openmesh-client".into(), "web".into());
        assert_eq!(source_from_headers(&headers), "web");
        headers.insert("x-openmesh-agent".into(), "claude".into());
        assert_eq!(source_from_headers(&headers), "agent:claude");
        headers.insert("x-openmesh-agent".into(), "  ".into());
        assert_eq!(source_from_headers(&headers), "web");
    }

    #[test]
    fn agent_activity_is_tracked_for_setup_state() {
        let log = ActivityLog::new();
        assert!(!log.has_agent_activity());
        log.record("user", "POST", "/api/announce", "announce", true, None);
        assert!(!log.has_agent_activity());
        log.record(
            "agent:test",
            "POST",
            "/api/announce",
            "announce",
            true,
            None,
        );
        assert!(log.has_agent_activity());
        assert!(log.last_agent_at_ms() > 0);
    }
}
