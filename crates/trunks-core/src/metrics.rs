//! Prometheus metrics (`prometheus-client`), exposed at `/metrics` when enabled.

use std::sync::{Arc, Mutex};

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{Histogram, exponential_buckets};
use prometheus_client::registry::Registry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct CallLabels {
    pub mcp_server_name: String,
    pub tool_name: String,
    pub outcome: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct BuildLabels {
    pub version: String,
}

#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Mutex<Registry>>,
    tool_calls: Family<CallLabels, Counter>,
    tool_latency: Family<CallLabels, Histogram>,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Metrics")
    }
}

impl Metrics {
    #[must_use]
    pub fn new(version: &str) -> Self {
        let mut registry = Registry::with_prefix("trunks");
        let tool_calls = Family::<CallLabels, Counter>::default();
        registry.register(
            "tool_calls",
            "Tool and prompt calls proxied to upstream servers",
            tool_calls.clone(),
        );
        let tool_latency = Family::<CallLabels, Histogram>::new_with_constructor(|| {
            // 0.005 … 30 s
            Histogram::new(exponential_buckets(0.005, 2.0, 13))
        });
        registry.register(
            "tool_call_latency_seconds",
            "Latency of proxied tool and prompt calls",
            tool_latency.clone(),
        );
        let build = Family::<BuildLabels, Gauge>::default();
        build.get_or_create(&BuildLabels { version: version.to_owned() }).set(1);
        registry.register("build_info", "Build information", build);
        Self { registry: Arc::new(Mutex::new(registry)), tool_calls, tool_latency }
    }

    pub fn record_call(&self, server: &str, tool: &str, success: bool, seconds: f64) {
        let labels = CallLabels {
            mcp_server_name: clip(server),
            tool_name: clip(tool),
            outcome: if success { "success".into() } else { "error".into() },
        };
        self.tool_calls.get_or_create(&labels).inc();
        self.tool_latency.get_or_create(&labels).observe(seconds);
    }

    /// Prometheus text exposition.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Ok(reg) = self.registry.lock() {
            let _ = encode(&mut out, &reg);
        }
        out
    }
}

/// Label values are capped at 64 chars; empty becomes `unknown`.
fn clip(s: &str) -> String {
    if s.is_empty() {
        return "unknown".into();
    }
    s.chars().take(64).collect()
}
