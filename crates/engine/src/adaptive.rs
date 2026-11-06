//! Dynamic Adaptive AIMD (Additive Increase / Multiplicative Decrease) Concurrency Controller
//! Implements REQ-11 & WBS-M2-01.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimdState {
    /// Actively probing for higher bandwidth by adding connections
    Probing,
    /// Throughput plateaued or reached maximum concurrency; maintaining steady rate
    Steady,
    /// Backoff / cooldown phase triggered by 429 rate limit or network saturation
    Backoff,
}

#[derive(Debug, Clone)]
pub struct AimdConfig {
    /// Minimum allowed connection concurrency (default: 2)
    pub min_concurrency: usize,
    /// Maximum allowed connection concurrency (default: 16)
    pub max_concurrency: usize,
    /// Evaluation cadence for measuring goodput changes (default: 5 seconds)
    pub evaluation_interval: Duration,
    /// Minimum fractional goodput gain required to justify adding a connection (default: 0.10 = 10%)
    pub throughput_gain_threshold: f64,
    /// Default cooldown period after a multiplicative decrease (default: 5 seconds)
    pub backoff_duration: Duration,
}

impl Default for AimdConfig {
    fn default() -> Self {
        Self {
            min_concurrency: 2,
            max_concurrency: 16,
            evaluation_interval: Duration::from_secs(5),
            throughput_gain_threshold: 0.10,
            backoff_duration: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimdDecision {
    /// Maintain current connection concurrency
    Maintain(usize),
    /// Additively increase concurrency (+1)
    Increase(usize),
    /// Multiplicatively decrease concurrency (-50%)
    Decrease(usize),
}

pub struct AimdController {
    config: AimdConfig,
    state: AimdState,
    current_concurrency: usize,
    last_eval: Instant,
    last_bytes: u64,
    prev_goodput: f64,
    errors_in_window: usize,
    rate_limits_in_window: usize,
    cooldown_until: Option<Instant>,
}

impl AimdController {
    pub fn new(config: AimdConfig) -> Self {
        let current_concurrency = config.min_concurrency;
        Self {
            config,
            state: AimdState::Probing,
            current_concurrency,
            last_eval: Instant::now(),
            last_bytes: 0,
            prev_goodput: 0.0,
            errors_in_window: 0,
            rate_limits_in_window: 0,
            cooldown_until: None,
        }
    }

    pub fn with_initial_concurrency(mut config: AimdConfig, initial: usize) -> Self {
        let initial = initial.clamp(config.min_concurrency, config.max_concurrency);
        config.min_concurrency = config.min_concurrency.min(initial);
        Self {
            current_concurrency: initial,
            config,
            state: AimdState::Probing,
            last_eval: Instant::now(),
            last_bytes: 0,
            prev_goodput: 0.0,
            errors_in_window: 0,
            rate_limits_in_window: 0,
            cooldown_until: None,
        }
    }

    pub fn current_concurrency(&self) -> usize {
        self.current_concurrency
    }

    pub fn state(&self) -> AimdState {
        self.state
    }

    pub fn config(&self) -> &AimdConfig {
        &self.config
    }

    /// Record a rate limit event (HTTP 429) or severe server congestion.
    /// Immediately triggers Multiplicative Decrease.
    pub fn record_rate_limit(&mut self, retry_after: Option<Duration>) -> AimdDecision {
        self.rate_limits_in_window += 1;
        let cooldown = retry_after.unwrap_or(self.config.backoff_duration);
        self.cooldown_until = Some(Instant::now() + cooldown);
        self.state = AimdState::Backoff;

        // Multiplicative decrease: cut concurrency in half, bounded by min_concurrency
        let new_concurrency = (self.current_concurrency / 2).max(self.config.min_concurrency);
        self.current_concurrency = new_concurrency;
        // Invalidate previous goodput baseline since server is throttling
        self.prev_goodput = 0.0;
        AimdDecision::Decrease(new_concurrency)
    }

    /// Record a transient network error or socket timeout
    pub fn record_error(&mut self) {
        self.errors_in_window += 1;
    }

    /// Evaluate current throughput against previous window and make an AIMD decision
    pub fn evaluate(&mut self, current_downloaded: u64) -> AimdDecision {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_eval);

        if elapsed < self.config.evaluation_interval {
            return AimdDecision::Maintain(self.current_concurrency);
        }

        // Check if currently in backoff cooldown
        if let Some(cooldown) = self.cooldown_until {
            if now < cooldown {
                // Still in cooldown, maintain decreased rate
                self.last_eval = now;
                self.last_bytes = current_downloaded;
                return AimdDecision::Maintain(self.current_concurrency);
            } else {
                // Cooldown expired, transition back to Probing
                self.cooldown_until = None;
                self.state = AimdState::Probing;
            }
        }

        // Calculate goodput (bytes per second) over the evaluation interval
        let delta_bytes = current_downloaded.saturating_sub(self.last_bytes);
        let current_goodput = delta_bytes as f64 / elapsed.as_secs_f64();

        self.last_eval = now;
        self.last_bytes = current_downloaded;

        // Check for error triggers
        if self.rate_limits_in_window > 0 {
            self.rate_limits_in_window = 0;
            self.errors_in_window = 0;
            return self.record_rate_limit(None);
        }

        if self.errors_in_window >= 3 {
            // Sustained errors without 429 -> cut concurrency
            self.errors_in_window = 0;
            let new_concurrency = (self.current_concurrency / 2).max(self.config.min_concurrency);
            self.current_concurrency = new_concurrency;
            self.state = AimdState::Backoff;
            self.cooldown_until = Some(now + self.config.backoff_duration);
            return AimdDecision::Decrease(new_concurrency);
        }

        self.errors_in_window = 0;

        // AIMD Increase / Steady decision logic:
        if self.current_concurrency >= self.config.max_concurrency {
            self.state = AimdState::Steady;
            self.prev_goodput = current_goodput;
            return AimdDecision::Maintain(self.current_concurrency);
        }

        if self.prev_goodput > 0.0 {
            let relative_gain = (current_goodput - self.prev_goodput) / self.prev_goodput;
            if relative_gain >= self.config.throughput_gain_threshold {
                // Additive increase: goodput improved by at least the threshold (+10%)
                self.current_concurrency =
                    (self.current_concurrency + 1).min(self.config.max_concurrency);
                self.state = AimdState::Probing;
                self.prev_goodput = current_goodput;
                AimdDecision::Increase(self.current_concurrency)
            } else {
                // Diminishing returns / plateau reached: maintain current concurrency
                self.state = AimdState::Steady;
                self.prev_goodput = current_goodput;
                AimdDecision::Maintain(self.current_concurrency)
            }
        } else {
            // First evaluation interval: record baseline goodput and probe
            self.prev_goodput = current_goodput;
            self.current_concurrency =
                (self.current_concurrency + 1).min(self.config.max_concurrency);
            self.state = AimdState::Probing;
            AimdDecision::Increase(self.current_concurrency)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aimd_additive_increase() {
        let config = AimdConfig {
            min_concurrency: 2,
            max_concurrency: 8,
            evaluation_interval: Duration::from_millis(50),
            throughput_gain_threshold: 0.10,
            backoff_duration: Duration::from_millis(100),
        };

        let mut controller = AimdController::new(config);
        assert_eq!(controller.current_concurrency(), 2);

        // Window 1: Establish baseline with 100 KB
        std::thread::sleep(Duration::from_millis(55));
        let dec1 = controller.evaluate(100_000);
        assert_eq!(dec1, AimdDecision::Increase(3));
        assert_eq!(controller.current_concurrency(), 3);

        // Window 2: Goodput increased by 50% (150 KB added -> 250 KB total)
        std::thread::sleep(Duration::from_millis(55));
        let dec2 = controller.evaluate(250_000);
        assert_eq!(dec2, AimdDecision::Increase(4));
        assert_eq!(controller.current_concurrency(), 4);
    }

    #[test]
    fn test_aimd_plateau_detection() {
        let config = AimdConfig {
            min_concurrency: 2,
            max_concurrency: 8,
            evaluation_interval: Duration::from_millis(50),
            throughput_gain_threshold: 0.10,
            backoff_duration: Duration::from_millis(100),
        };

        let mut controller = AimdController::new(config);

        // Window 1: 100 KB
        std::thread::sleep(Duration::from_millis(55));
        controller.evaluate(100_000);

        // Window 2: Only 102 KB added (2% gain < 10% threshold)
        std::thread::sleep(Duration::from_millis(55));
        let dec = controller.evaluate(202_000);
        assert_eq!(dec, AimdDecision::Maintain(3));
        assert_eq!(controller.state(), AimdState::Steady);
    }

    #[test]
    fn test_aimd_multiplicative_decrease_on_429() {
        let config = AimdConfig {
            min_concurrency: 2,
            max_concurrency: 16,
            evaluation_interval: Duration::from_millis(50),
            throughput_gain_threshold: 0.10,
            backoff_duration: Duration::from_millis(200),
        };

        let mut controller = AimdController::with_initial_concurrency(config, 8);
        assert_eq!(controller.current_concurrency(), 8);

        // Record 429
        let dec = controller.record_rate_limit(Some(Duration::from_millis(100)));
        assert_eq!(dec, AimdDecision::Decrease(4));
        assert_eq!(controller.current_concurrency(), 4);
        assert_eq!(controller.state(), AimdState::Backoff);

        // During cooldown, evaluate should maintain
        let dec2 = controller.evaluate(50_000);
        assert_eq!(dec2, AimdDecision::Maintain(4));
    }
}
