#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CounterSample {
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CounterDelta {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub bps_in: u64,
    pub bps_out: u64,
}

#[derive(Default)]
pub struct CounterTracker {
    last: Option<CounterSample>,
    pub total_in: u64,
    pub total_out: u64,
}

impl CounterTracker {
    pub fn update(&mut self, sample: CounterSample, seconds: f64) -> CounterDelta {
        let Some(last) = self.last.replace(sample) else {
            return CounterDelta::default();
        };
        let bytes_in = delta(last.bytes_in, sample.bytes_in, seconds);
        let bytes_out = delta(last.bytes_out, sample.bytes_out, seconds);
        self.total_in += bytes_in;
        self.total_out += bytes_out;
        CounterDelta {
            bytes_in,
            bytes_out,
            bps_in: bps(bytes_in, seconds),
            bps_out: bps(bytes_out, seconds),
        }
    }
}

/// Counters that went backwards are treated as a 32-bit wrap when the
/// rebased delta is plausible for the interval, otherwise as a reset.
fn delta(last: u64, now: u64, seconds: f64) -> u64 {
    if now >= last {
        return now - last;
    }
    let wrapped = now.wrapping_add(1u64 << 32).wrapping_sub(last);
    let limit = (100_000_000f64).max(seconds * 1_250_000_000.0);
    if wrapped <= (1u64 << 32) && (wrapped as f64) <= limit {
        wrapped
    } else {
        0
    }
}

fn bps(bytes: u64, seconds: f64) -> u64 {
    if seconds <= 0.0 {
        return 0;
    }
    (bytes as f64 * 8.0 / seconds).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_is_zero() {
        let mut t = CounterTracker::default();
        let d = t.update(
            CounterSample {
                bytes_in: 500,
                bytes_out: 100,
            },
            1.0,
        );
        assert_eq!(d, CounterDelta::default());
    }

    #[test]
    fn delta_and_bps() {
        let mut t = CounterTracker::default();
        t.update(
            CounterSample {
                bytes_in: 1000,
                bytes_out: 0,
            },
            1.0,
        );
        let d = t.update(
            CounterSample {
                bytes_in: 3000,
                bytes_out: 500,
            },
            2.0,
        );
        assert_eq!(d.bytes_in, 2000);
        assert_eq!(d.bps_in, 8000);
        assert_eq!(d.bps_out, 2000);
        assert_eq!(t.total_in, 2000);
    }

    #[test]
    fn wrap_rebases_and_reset_zeroes() {
        let mut t = CounterTracker::default();
        t.update(
            CounterSample {
                bytes_in: u32::MAX as u64 - 10,
                bytes_out: 0,
            },
            1.0,
        );
        let d = t.update(
            CounterSample {
                bytes_in: 5,
                bytes_out: 0,
            },
            1.0,
        );
        assert_eq!(d.bytes_in, 16);
        let mut r = CounterTracker::default();
        r.update(
            CounterSample {
                bytes_in: 5_000_000_000,
                bytes_out: 0,
            },
            1.0,
        );
        let d = r.update(
            CounterSample {
                bytes_in: 100,
                bytes_out: 0,
            },
            1.0,
        );
        assert_eq!(d.bytes_in, 0);
    }
}
