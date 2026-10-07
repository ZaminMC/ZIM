//! `metrics.range` — the ring-backed history read (protocol spec §6,
//! ADR-0006). The live `metrics` stream delivers each sample once;
//! the daemon keeps a bounded per-server ring of recent samples, and
//! this method returns it so a freshly (re)connected client can draw a
//! chart without waiting for history to accumulate.
//!
//! The ring is the *entire* stored history, bounded by design (ADR-0006:
//! preallocated rings bounded by servers). There is no paging and no
//! `olderAvailable`: what the response holds is everything the daemon
//! knows. A future file-backed history would be an additive extension.

use serde::{Deserialize, Serialize};

use crate::streams::MetricsSample;

/// How many samples a single `metrics.range` call may return. Bounds the
/// response frame; the default ring holds 600 (ten minutes at 1 Hz).
pub const METRICS_RANGE_MAX_SAMPLES: u32 = 600;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsRangeParams {
    pub server_id: String,
    /// Samples to return. Defaults to 120; capped at
    /// [`METRICS_RANGE_MAX_SAMPLES`]. Chronological, oldest first — the
    /// newest samples are kept when trimming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_samples: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsRangeResult {
    /// Chronological (oldest first), at most `maxSamples` samples.
    /// Fields mirror the live stream's sample shape exactly; absent
    /// fields were not measured at that tick.
    pub samples: Vec<MetricsSample>,
}
