use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct GovernorConfig {
    pub ceiling_percent: f64,
    pub max_yield: Duration,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            ceiling_percent: 80.0,
            max_yield: Duration::from_millis(250),
        }
    }
}

/// Memory-bandwidth arbiter.
///
/// Lock-free implementation utilizing atomic bit patterns for `f64`.
#[derive(Debug)]
pub struct BandwidthGovernor {
    ceiling_percent: f64,
    max_yield: Duration,
    ema_bits: AtomicU64,
}

const EMA_ALPHA: f64 = 0.35;

impl BandwidthGovernor {
    pub fn new(config: GovernorConfig) -> Self {
        let ceiling = config.ceiling_percent.clamp(10.0, 100.0);
        Self {
            ceiling_percent: ceiling,
            max_yield: config.max_yield,
            ema_bits: AtomicU64::new(0.0f64.to_bits()),
        }
    }

    pub fn update(&self, memory_bus_percent: f32) {
        let input = (memory_bus_percent as f64).clamp(0.0, 100.0);

        // Lock-free Compare-And-Swap (CAS) loop for EMA calculation
        let mut current_bits = self.ema_bits.load(Ordering::Relaxed);
        loop {
            let current_ema = f64::from_bits(current_bits);
            let next_ema = (current_ema * (1.0 - EMA_ALPHA)) + (input * EMA_ALPHA);

            match self.ema_bits.compare_exchange_weak(
                current_bits,
                next_ema.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual_bits) => current_bits = actual_bits,
            }
        }
    }

    /// Smoothed memory-controller utilization in percent (0..=100).
    pub fn bus_utilization(&self) -> f64 {
        f64::from_bits(self.ema_bits.load(Ordering::Relaxed))
    }

    /// Normalized overload above the ceiling: 0 at/below ceiling, 1 at 100%.
    pub fn pressure(&self) -> f64 {
        let util = self.bus_utilization();
        if util <= self.ceiling_percent {
            return 0.0;
        }

        let headroom_span = 100.0 - self.ceiling_percent;
        if headroom_span <= f64::EPSILON {
            return 1.0;
        }

        let p = (util - self.ceiling_percent) / headroom_span;
        if p.is_nan() { 0.0 } else { p.clamp(0.0, 1.0) }
    }

    /// How long a standard-lane job should pause before its next compute step.
    pub fn standard_yield(&self) -> Duration {
        let p = self.pressure();
        if p <= f64::EPSILON || p.is_nan() {
            return Duration::ZERO;
        }
        self.max_yield.mul_f64(p)
    }

    pub fn snapshot(&self) -> (f64, f64) {
        (self.bus_utilization(), self.pressure())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceiling_at_hundred_does_not_panic() {
        let g = BandwidthGovernor::new(GovernorConfig {
            ceiling_percent: 100.0,
            max_yield: Duration::from_millis(250),
        });
        for _ in 0..100 {
            g.update(100.0);
        }
        // Must not panic on NaN
        let _ = g.standard_yield();
    }

    #[test]
    fn below_ceiling_never_yields() {
        let g = BandwidthGovernor::new(GovernorConfig::default());
        for _ in 0..20 {
            g.update(50.0);
        }
        assert_eq!(g.standard_yield(), Duration::ZERO);
        assert_eq!(g.pressure(), 0.0);
    }

    #[test]
    fn yield_scales_linearly_with_overload() {
        let g = BandwidthGovernor::new(GovernorConfig::default());
        for _ in 0..60 {
            g.update(100.0);
        }
        assert!((g.pressure() - 1.0).abs() < 1e-6);
        assert!(g.standard_yield() >= Duration::from_millis(249));

        let half = BandwidthGovernor::new(GovernorConfig {
            ceiling_percent: 50.0,
            max_yield: Duration::from_millis(200),
        });
        for _ in 0..80 {
            half.update(75.0);
        }
        assert!((half.pressure() - 0.5).abs() < 1e-3);
        let y = half.standard_yield();
        assert!(
            y >= Duration::from_millis(99) && y <= Duration::from_millis(101),
            "yield {y:?}"
        );
    }

    #[test]
    fn ema_smooths_spikes() {
        let g = BandwidthGovernor::new(GovernorConfig::default());
        for _ in 0..20 {
            g.update(20.0);
        }
        g.update(100.0);
        let v = g.bus_utilization();
        assert!(v > 20.0 && v < 55.0, "single spike must be damped, got {v}");
    }
}
