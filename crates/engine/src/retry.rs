use reqwest::StatusCode;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClassification {
    TransientNetwork,
    RateLimited,
    AuthOrExpired,
    PermanentClient,
    DiskSpace,
    Unknown,
}

pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 30, // Tolerates up to 6+ minutes of internet outage with backoff
            initial_backoff_ms: 500,
            max_backoff_ms: 15000,
        }
    }
}

impl RetryPolicy {
    pub fn classify_status(status: StatusCode) -> ErrorClassification {
        match status {
            StatusCode::TOO_MANY_REQUESTS => ErrorClassification::RateLimited,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorClassification::AuthOrExpired,
            StatusCode::NOT_FOUND | StatusCode::GONE | StatusCode::RANGE_NOT_SATISFIABLE => {
                ErrorClassification::PermanentClient
            }
            StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
            | StatusCode::REQUEST_TIMEOUT => ErrorClassification::TransientNetwork,
            _ if status.is_server_error() => ErrorClassification::TransientNetwork,
            _ => ErrorClassification::Unknown,
        }
    }

    pub fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
        let val = headers.get(RETRY_AFTER)?.to_str().ok()?;
        // Try parsing seconds
        if let Ok(secs) = val.trim().parse::<u64>() {
            return Some(Duration::from_secs(secs.min(300))); // cap to 5 mins
        }
        None
    }

    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        let base = self.initial_backoff_ms.saturating_mul(1 << attempt.min(5));
        let capped = base.min(self.max_backoff_ms);
        // Add simple pseudo-jitter +/- 20%
        let jitter = (capped / 5) * (attempt as u64 % 3);
        Duration::from_millis(capped + jitter)
    }
}
