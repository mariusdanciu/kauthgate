use pingora_prometheus::prometheus::{HistogramOpts, HistogramVec, IntCounterVec, Opts, register_histogram_vec, register_int_counter_vec};
use std::sync::LazyLock;

pub static REQUEST_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new("rbac_gate_requests_total", "Total requests processed"),
        &["protocol", "status"]
    )
    .unwrap()
});

pub static REQUEST_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new("rbac_gate_request_duration_seconds", "Request duration in seconds"),
        &["protocol", "status"]
    )
    .unwrap()
});
