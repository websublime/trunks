//! Upstream session management.
//!
//! - `stateless` (default): a fresh connection per call, closed on release.
//! - `stateful`: one connection per server, reused; locked **per server** (a slow upstream never
//!   blocks the others); invalidated on transport errors; swept when idle; closed on shutdown.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use trunks_types::{ServerName, SessionMode};

use crate::domain::ServerRecord;
use crate::upstream::{ConnectOptions, UpstreamConn, is_connection_error};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub connect: ConnectOptions,
    /// `None` = stateful connections never expire.
    pub idle_timeout: Option<Duration>,
    pub sweep_interval: Duration,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            connect: ConnectOptions::default(),
            idle_timeout: None,
            sweep_interval: Duration::from_mins(1),
        }
    }
}

struct Slot {
    conn: Mutex<Option<Arc<UpstreamConn>>>,
    last_used: AtomicU64, // seconds since `epoch`
}

/// What the gateway last saw from an upstream (for diagnostics; no active probing).
#[derive(Debug, Clone, Default)]
pub struct UpstreamHealth {
    pub last_error: Option<String>,
    pub last_error_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_ok_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// A borrowed connection. Dropping it without `release` is safe (stateless: closed by the
/// drop of the last `Arc`; stateful: stays cached).
pub struct Lease {
    pub conn: Arc<UpstreamConn>,
    server: ServerName,
    stateful: bool,
}

impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lease")
            .field("server", &self.server)
            .field("stateful", &self.stateful)
            .finish_non_exhaustive()
    }
}

pub struct SessionManager {
    slots: DashMap<ServerName, Arc<Slot>>,
    health: DashMap<ServerName, UpstreamHealth>,
    opts: SessionOptions,
    epoch: Instant,
    cancel: CancellationToken,
    active: AtomicU64,
}

impl std::fmt::Debug for SessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionManager")
            .field("stateful_slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl SessionManager {
    #[must_use]
    pub fn new(opts: SessionOptions) -> Arc<Self> {
        let me = Arc::new(Self {
            slots: DashMap::new(),
            health: DashMap::new(),
            opts,
            epoch: Instant::now(),
            cancel: CancellationToken::new(),
            active: AtomicU64::new(0),
        });
        if me.opts.idle_timeout.is_some() {
            let weak = Arc::downgrade(&me);
            let interval = me.opts.sweep_interval;
            let cancel = me.cancel.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(interval);
                tick.tick().await;
                loop {
                    tokio::select! {
                        () = cancel.cancelled() => break,
                        _ = tick.tick() => {
                            let Some(me) = weak.upgrade() else { break };
                            me.sweep_idle().await;
                        }
                    }
                }
            });
        }
        me
    }

    fn now(&self) -> u64 {
        self.epoch.elapsed().as_secs()
    }

    /// Number of live stateful connections (for metrics/diagnostics).
    #[must_use]
    pub fn stateful_active(&self) -> u64 {
        self.active.load(Ordering::Relaxed)
    }

    /// Last known state of each upstream the gateway talked to.
    #[must_use]
    pub fn health(&self) -> Vec<(ServerName, UpstreamHealth)> {
        self.health.iter().map(|e| (e.key().clone(), e.value().clone())).collect()
    }

    fn note_ok(&self, server: &ServerName) {
        let mut h = self.health.entry(server.clone()).or_default();
        h.last_ok_at = Some(chrono::Utc::now());
        h.last_error = None;
    }

    fn note_error(&self, server: &ServerName, error: &str) {
        let mut h = self.health.entry(server.clone()).or_default();
        h.last_error = Some(error.to_owned());
        h.last_error_at = Some(chrono::Utc::now());
    }

    /// Obtains a connection for `server` according to its session mode.
    pub async fn acquire(&self, server: &ServerRecord) -> Result<Lease> {
        let lease = self.acquire_inner(server).await;
        match &lease {
            Ok(_) => self.note_ok(&server.name),
            Err(e) => self.note_error(&server.name, &e.to_string()),
        }
        lease
    }

    async fn acquire_inner(&self, server: &ServerRecord) -> Result<Lease> {
        if self.cancel.is_cancelled() {
            return Err(Error::Internal { reason: "gateway is shutting down".into() });
        }
        match server.session_mode {
            SessionMode::Stateless => {
                let conn =
                    UpstreamConn::connect(&server.name, &server.config, &self.opts.connect).await?;
                Ok(Lease { conn: Arc::new(conn), server: server.name.clone(), stateful: false })
            }
            SessionMode::Stateful => {
                let slot = self
                    .slots
                    .entry(server.name.clone())
                    .or_insert_with(|| {
                        Arc::new(Slot { conn: Mutex::new(None), last_used: AtomicU64::new(0) })
                    })
                    .clone();
                let mut guard = slot.conn.lock().await;
                let conn = if let Some(c) = guard.as_ref() {
                    c.clone()
                } else {
                    let c = Arc::new(
                        UpstreamConn::connect(&server.name, &server.config, &self.opts.connect)
                            .await?,
                    );
                    *guard = Some(c.clone());
                    self.active.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(server = %server.name, "stateful upstream session opened");
                    c
                };
                slot.last_used.store(self.now(), Ordering::Relaxed);
                Ok(Lease { conn, server: server.name.clone(), stateful: true })
            }
        }
    }

    /// Returns the connection. `error` is the rmcp error of the call, if any: a transport error
    /// invalidates a stateful session so the next call reconnects.
    pub async fn release(&self, lease: Lease, error: Option<&rmcp::service::ServiceError>) {
        let broken = error.is_some_and(is_connection_error);
        if broken && let Some(e) = error {
            self.note_error(&lease.server, &e.to_string());
        }
        if lease.stateful {
            if broken {
                self.invalidate(&lease.server).await;
            } else if let Some(slot) = self.slots.get(&lease.server) {
                slot.last_used.store(self.now(), Ordering::Relaxed);
            }
            drop(lease);
        } else {
            let Lease { conn, .. } = lease;
            if let Ok(conn) = Arc::try_unwrap(conn) {
                conn.close().await;
            }
        }
    }

    /// Drops the stateful session of `server` (deregistration, transport error).
    pub async fn invalidate(&self, server: &ServerName) {
        if let Some((_, slot)) = self.slots.remove(server) {
            let conn = slot.conn.lock().await.take();
            if let Some(conn) = conn {
                self.active.fetch_sub(1, Ordering::Relaxed);
                tracing::info!(server = %server, "stateful upstream session closed");
                if let Ok(conn) = Arc::try_unwrap(conn) {
                    conn.close().await;
                }
            }
        }
    }

    async fn sweep_idle(&self) {
        let Some(limit) = self.opts.idle_timeout else { return };
        let now = self.now();
        let idle: Vec<ServerName> = self
            .slots
            .iter()
            .filter(|e| now.saturating_sub(e.last_used.load(Ordering::Relaxed)) > limit.as_secs())
            .map(|e| e.key().clone())
            .collect();
        for name in idle {
            tracing::info!(server = %name, "closing idle stateful session");
            self.invalidate(&name).await;
        }
    }

    /// Closes every stateful session and stops the sweeper.
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let names: Vec<ServerName> = self.slots.iter().map(|e| e.key().clone()).collect();
        for name in names {
            self.invalidate(&name).await;
        }
    }
}
