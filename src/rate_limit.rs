//! Persistent, conservative request accounting for the public Open-Meteo endpoints.
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Deferred {
    pub until: i64,
    pub reason: String,
}
impl std::fmt::Display for Deferred {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let time = chrono::DateTime::from_timestamp(self.until, 0)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%m-%d %H:%M %Z")
                    .to_string()
            })
            .unwrap_or_default();
        write!(f, "{}; retry after {time}", self.reason)
    }
}
impl std::error::Error for Deferred {}

#[derive(Default, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default)]
    pub blocked_until: i64,
    #[serde(default)]
    requests: Vec<(i64, f64)>,
    #[serde(default)]
    refund_day: i64,
    #[serde(default)]
    refunded_requests: u64,
    #[serde(default)]
    refunded_credits: f64,
}
impl Budget {
    #[cfg(test)]
    pub fn reserve(&mut self, now: i64, weight: f64) -> Result<(), Deferred> {
        self.reserve_with_priority(now, weight, false)
    }
    pub fn reserve_with_priority(
        &mut self,
        now: i64,
        weight: f64,
        current: bool,
    ) -> Result<(), Deferred> {
        self.requests
            .retain(|(t, _)| *t >= now.div_euclid(86400) * 86400);
        let mut until = self.blocked_until;
        let mut reason = "Provider quota cooldown";
        // Provider counters reset on UTC boundaries, not rolling windows.
        // Background history leaves 50/min, 250/hour and 500/day for live weather.
        for (window, limit, label) in [
            (60, 550.0, "minute"),
            (3600, 4750.0, "hour"),
            (86400, 9500.0, "day"),
        ] {
            let limit = if current {
                match window {
                    60 => 590.0,
                    3600 => 4950.0,
                    _ => 9950.0,
                }
            } else {
                limit
            };
            let sum: f64 = self
                .requests
                .iter()
                .filter(|(t, _)| *t >= now.div_euclid(window) * window)
                .map(|(_, w)| *w)
                .sum();
            if sum > 0.0 && sum + weight > limit {
                let reset = next_reset(now, window);
                if reset > until {
                    until = reset;
                    reason = label;
                }
            }
        }
        if until > now {
            return Err(Deferred {
                until,
                reason: format!("API {reason} budget; queued until the next quota opening"),
            });
        }
        self.requests.push((now, weight));
        Ok(())
    }
    pub fn rejected(&mut self, now: i64, weight: f64) {
        self.settle(now, weight, 0.0);
    }
    pub fn settle(&mut self, now: i64, weight: f64, charged: f64) {
        if let Some(i) = self
            .requests
            .iter()
            .rposition(|&(t, w)| t == now && w == weight)
        {
            let charged = charged.clamp(0.0, weight);
            if charged == weight {
                return;
            }
            if charged == 0.0 {
                self.requests.remove(i);
            } else {
                self.requests[i].1 = charged;
            }
            let day = now.div_euclid(86400);
            if self.refund_day != day {
                self.refund_day = day;
                self.refunded_requests = 0;
                self.refunded_credits = 0.0;
            }
            self.refunded_requests += 1;
            self.refunded_credits += weight - charged;
        }
    }
}

/// Only failures that establish that no HTTP request reached the provider.
/// Read/write failures and global timeouts may follow server-side computation.
pub(crate) fn unsent(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::HostNotFound
        | ureq::Error::BadUri(_)
        | ureq::Error::InvalidProxyUrl
        | ureq::Error::ConnectionFailed
        | ureq::Error::Timeout(ureq::Timeout::Resolve | ureq::Timeout::Connect) => true,
        ureq::Error::Io(error) => matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::NetworkUnreachable
                | std::io::ErrorKind::HostUnreachable
        ),
        _ => false,
    }
}

pub fn weight(params: &[(&str, String)]) -> f64 {
    let value = |name| {
        params
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, v)| v.as_str())
    };
    let days = value("start_date")
        .zip(value("end_date"))
        .and_then(|(a, b)| {
            Some(
                (chrono::NaiveDate::parse_from_str(b, "%Y-%m-%d").ok()?
                    - chrono::NaiveDate::parse_from_str(a, "%Y-%m-%d").ok()?)
                .num_days()
                    + 1,
            )
        })
        .unwrap_or_else(|| {
            value("forecast_days")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(7)
                + value("past_days")
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0)
        })
        .max(1) as f64;
    let variables: usize = ["hourly", "daily", "current"]
        .iter()
        .filter_map(|key| value(key))
        .map(|v| v.split(',').count())
        .sum();
    // Upstream floors the combined variable/time weight, not each factor.
    // Capacity reserves are applied separately by Budget.
    let models = value("models").map_or(1, |v| v.split(',').count());
    let locations = value("latitude").map_or(1, |v| v.split(',').count());
    ((variables as f64 / 10.0) * (days / 14.0).max(1.0) * models as f64).max(1.0) * locations as f64
}

fn next_reset(now: i64, window: i64) -> i64 {
    // The upstream maintenance callback runs once per minute. Five seconds covers
    // normal boundary jitter; an early rejection supplies the next cooldown.
    (now.div_euclid(window) + 1) * window + 5
}

pub fn retry_deadline(now: i64, header: Option<&str>, reason: &str) -> i64 {
    if let Some(seconds) = header
        .and_then(|h| h.parse::<i64>().ok())
        .filter(|n| *n >= 0)
    {
        return now.saturating_add(seconds.max(1));
    }
    if let Some(date) = header.and_then(|h| chrono::DateTime::parse_from_rfc2822(h).ok()) {
        return date.timestamp().max(now + 1);
    }
    let reason = reason.to_ascii_lowercase();
    if reason.contains("concurrent") {
        return now + 5;
    }
    let window = if reason.contains("day") || reason.contains("daily") {
        86400
    } else if reason.contains("hour") {
        3600
    } else {
        60
    };
    // A quota response just after an hour/day boundary may precede the provider's
    // minutely maintenance callback. Probe after that callback, not a whole period later.
    if window > 60 && now.rem_euclid(window) < 65 {
        now.div_euclid(window) * window + 65
    } else {
        next_reset(now, window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weighted_requests_defer_and_resume() {
        let mut budget = Budget::default();
        budget.reserve(1000, 522.0).unwrap();
        assert_eq!(budget.reserve(1001, 522.0).unwrap_err().until, 1025);
        budget.reserve(1061, 522.0).unwrap();
        for n in 2..9 {
            budget.reserve(1000 + n * 61, 522.0).unwrap();
        }
        assert_eq!(budget.reserve(1549, 522.0).unwrap_err().until, 3605);
    }
    #[test]
    fn retry_header_and_reason() {
        assert_eq!(retry_deadline(100, Some("120"), "hour limit"), 220);
        assert_eq!(
            retry_deadline(100, None, "Hourly API request limit exceeded"),
            3605
        );
        assert_eq!(retry_deadline(100, None, "Daily limit exceeded"), 86405);
        assert_eq!(retry_deadline(3606, None, "Hourly limit exceeded"), 3665);
        assert_eq!(retry_deadline(86406, None, "Daily limit exceeded"), 86465);
        assert_eq!(retry_deadline(100, Some("2"), "Daily limit exceeded"), 102);
    }
    #[test]
    fn accounting_and_cooldowns_survive_reload() {
        let mut budget = Budget::default();
        budget.reserve(1000, 522.0).unwrap();
        budget.blocked_until = 4600;
        let mut restored: Budget =
            serde_json::from_slice(&serde_json::to_vec(&budget).unwrap()).unwrap();
        assert_eq!(restored.reserve(1001, 522.0).unwrap_err().until, 4600);
        restored.reserve(4601, 522.0).unwrap();
    }

    #[test]
    fn small_field_sets_match_provider_combined_weight_floor() {
        let params = [
            ("daily", "a,b,c,d,e,f,g,h".into()),
            ("start_date", "2041-01-01".into()),
            ("end_date", "2041-12-31".into()),
        ];
        assert_eq!(weight(&params), 365.0 / 14.0 * 0.8);
    }
    #[test]
    fn utc_reset_reserved_capacity_and_rejections() {
        let mut budget = Budget::default();
        budget.reserve(86390, 550.0).unwrap();
        assert!(budget.reserve(86391, 1.0).is_err());
        budget.reserve_with_priority(86391, 3.0, true).unwrap();
        budget.rejected(86391, 3.0);
        assert_eq!(budget.requests.len(), 1);
        budget.reserve(86405, 550.0).unwrap();
        assert_eq!(budget.requests.len(), 1);
    }
}

#[cfg(test)]
mod accounting_tests {
    use super::*;
    #[test]
    fn settlement_preserves_error_cost_capacity_reserve_and_audit_on_restart() {
        let now = 100_000;
        let mut budget = Budget::default();
        budget.reserve(now, 549.0).unwrap();
        assert!(budget.reserve(now, 2.0).is_err());
        budget.settle(now, 549.0, 1.0);
        let bytes = serde_json::to_vec(&budget).unwrap();
        let mut budget: Budget = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(budget.requests, vec![(now, 1.0)]);
        assert_eq!(budget.refunded_requests, 1);
        assert_eq!(budget.refunded_credits, 548.0);
        budget.reserve(now, 549.0).unwrap();
        assert!(budget.reserve(now, 1.0).is_err());
        budget.reserve_with_priority(now, 40.0, true).unwrap();
        assert!(budget.reserve_with_priority(now, 1.0, true).is_err());
    }
    #[test]
    fn refunds_do_not_clear_provider_cooldowns_or_unknown_legacy_usage() {
        let mut budget: Budget = serde_json::from_str(
            r#"{"blocked_until":2000,"requests":[[1000,54.75],[1001,54.75]]}"#,
        )
        .unwrap();
        budget.rejected(1001, 54.75);
        assert_eq!(budget.requests, vec![(1000, 54.75)]);
        assert_eq!(budget.reserve(1002, 1.0).unwrap_err().until, 2000);
        budget.rejected(1001, 54.75);
        assert_eq!(budget.refunded_requests, 1);
    }
    #[test]
    fn only_definitely_unsent_transport_errors_are_refunded() {
        assert!(unsent(&ureq::Error::HostNotFound));
        assert!(unsent(&ureq::Error::Timeout(ureq::Timeout::Connect)));
        assert!(unsent(&ureq::Error::Io(
            std::io::ErrorKind::ConnectionRefused.into()
        )));
        for timeout in [
            ureq::Timeout::Global,
            ureq::Timeout::RecvResponse,
            ureq::Timeout::RecvBody,
            ureq::Timeout::SendRequest,
        ] {
            assert!(!unsent(&ureq::Error::Timeout(timeout)));
        }
        assert!(!unsent(&ureq::Error::Io(
            std::io::ErrorKind::ConnectionReset.into()
        )));
    }
    #[test]
    fn combined_cost_floors_per_location_after_model_multiplication() {
        assert_eq!(
            weight(&[("daily", "a,b".into()), ("forecast_days", "14".into())]),
            1.0
        );
        assert_eq!(
            weight(&[
                ("daily", "a,b".into()),
                ("forecast_days", "28".into()),
                ("models", "a,b,c".into()),
                ("latitude", "1,2".into())
            ]),
            2.4000000000000004
        );
    }
}
