//! Application clock: the computer clock plus an explicit correction measured against
//! internet time. A computer clock that drifts by tens of seconds makes every fresh MT5
//! quote look stale (or from the future) and breaks Binance/Bybit request signatures.
//!
//! The correction is never inferred from broker quotes: a quote cannot tell clock skew from
//! a closed market. It is measured from the `Date` header of several independent HTTPS
//! servers and applied only when the user runs Repair or confirms it.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

static CORRECTION_MS: AtomicI64 = AtomicI64::new(0);

/// Largest correction the app accepts. A larger error means the date is wrong, which must be
/// fixed in Windows itself.
pub const MAX_CORRECTION_MS: i64 = 6 * 3600 * 1000;
/// Below this the Date header resolution (1 s) cannot prove skew.
pub const SIGNIFICANT_SKEW_MS: i64 = 2_000;

/// Hosts queried for their `Date` header. Any HTTPS server answering HEAD works.
pub const TIME_SOURCES: [&str; 4] = [
    "https://www.google.com/",
    "https://www.cloudflare.com/",
    "https://openrouter.ai/",
    "https://www.microsoft.com/",
];

/// Computer clock in Unix milliseconds, without correction.
pub fn system_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Application clock in Unix milliseconds (computer clock plus the active correction).
pub fn now_ms() -> i64 {
    corrected(system_ms(), CORRECTION_MS.load(Ordering::SeqCst))
}

fn corrected(system: i64, correction: i64) -> i64 {
    system.saturating_add(correction)
}

/// Same as [`now_ms`], never negative.
pub fn now_ms_u64() -> u64 {
    now_ms().max(0) as u64
}

pub fn correction_ms() -> i64 {
    CORRECTION_MS.load(Ordering::SeqCst)
}

pub fn set_correction_ms(value: i64) -> Result<(), String> {
    if value.abs() > MAX_CORRECTION_MS {
        return Err(format!(
            "Clock error of {} s is too large to correct automatically: set the correct date and time in Windows",
            value / 1000
        ));
    }
    CORRECTION_MS.store(value, Ordering::SeqCst);
    Ok(())
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ClockSample {
    pub source: String,
    pub offset_ms: i64,
    pub rtt_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ClockCheck {
    /// Internet time minus computer time; positive means the computer is behind.
    pub offset_ms: Option<i64>,
    pub samples: Vec<ClockSample>,
    pub failures: Vec<String>,
    pub correction_ms: i64,
    /// Remaining error of the application clock after the active correction.
    pub residual_ms: Option<i64>,
}

impl ClockCheck {
    pub fn significant(&self) -> bool {
        self.residual_ms.is_some_and(|r| r.abs() >= SIGNIFICANT_SKEW_MS)
    }
}

/// Parses an RFC 7231 IMF-fixdate such as `Tue, 20 Oct 2026 20:55:42 GMT`.
pub fn parse_http_date(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }
    let day: i64 = parts[1].parse().ok()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[2])? as i64
        + 1;
    let year: i64 = parts[3].parse().ok()?;
    let hms: Vec<i64> = parts[4].split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    if hms.len() != 3 || !(1..=31).contains(&day) || hms[0] > 23 || hms[1] > 59 || hms[2] > 60 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400) + hms[0] * 3600 + hms[1] * 60 + hms[2]) * 1000)
}

fn median(values: &mut [i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2
    } else {
        values[mid]
    })
}

/// Measures the computer clock against several HTTPS `Date` headers. Free, no API keys.
pub async fn measure() -> ClockCheck {
    measure_from(&TIME_SOURCES).await
}

pub async fn measure_from(sources: &[&str]) -> ClockCheck {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(6))
        .redirect(reqwest::redirect::Policy::none())
        .build();
    let mut samples = Vec::new();
    let mut failures = Vec::new();
    let Ok(client) = client else {
        failures.push("HTTP client unavailable".into());
        return finish(samples, failures);
    };
    let requests = sources.iter().map(|source| {
        let client = client.clone();
        async move {
            let started = Instant::now();
            let before = system_ms();
            let response = client.head(*source).send().await;
            let rtt = started.elapsed().as_millis() as i64;
            let local_mid = before + rtt / 2;
            match response {
                Ok(response) => match response
                    .headers()
                    .get(reqwest::header::DATE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_http_date)
                {
                    // The header is truncated to whole seconds: add half a second on average.
                    Some(remote) => Ok(ClockSample {
                        source: source.to_string(),
                        offset_ms: remote + 500 - local_mid,
                        rtt_ms: rtt.max(0) as u64,
                    }),
                    None => Err(format!("{source}: no Date header")),
                },
                Err(_) => Err(format!("{source}: unreachable")),
            }
        }
    });
    for result in futures_join(requests).await {
        match result {
            Ok(sample) => samples.push(sample),
            Err(error) => failures.push(error),
        }
    }
    finish(samples, failures)
}

async fn futures_join<F: std::future::Future>(futures: impl Iterator<Item = F>) -> Vec<F::Output> {
    let handles: Vec<_> = futures.collect();
    let mut out = Vec::with_capacity(handles.len());
    // Sequential awaiting of already-created futures would serialize the requests; spawn-free
    // concurrency is achieved by polling them together.
    let mut pinned: Vec<_> = handles.into_iter().map(Box::pin).collect();
    let mut results: Vec<Option<F::Output>> = (0..pinned.len()).map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (index, future) in pinned.iter_mut().enumerate() {
            if results[index].is_some() {
                continue;
            }
            match future.as_mut().poll(cx) {
                std::task::Poll::Ready(value) => results[index] = Some(value),
                std::task::Poll::Pending => pending = true,
            }
        }
        if pending {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    out.extend(results.into_iter().flatten());
    out
}

fn finish(samples: Vec<ClockSample>, failures: Vec<String>) -> ClockCheck {
    // Two agreeing sources are required; a single server with a wrong clock proves nothing.
    let mut offsets: Vec<i64> = samples.iter().map(|s| s.offset_ms).collect();
    let offset = if offsets.len() >= 2 { median(&mut offsets) } else { None };
    let correction = correction_ms();
    ClockCheck {
        offset_ms: offset,
        samples,
        failures,
        correction_ms: correction,
        residual_ms: offset.map(|o| o - correction),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_imf_fixdate() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(
            parse_http_date("Tue, 20 Oct 2026 20:55:42 GMT"),
            Some(1_792_529_742_000)
        );
        assert_eq!(
            parse_http_date("Sun, 29 Feb 2032 12:00:01 GMT"),
            Some(1_961_668_801_000)
        );
        assert!(parse_http_date("Tue, 20 Oct 2026 20:55:42 UTC").is_none());
        assert!(parse_http_date("garbage").is_none());
    }

    #[test]
    fn median_requires_agreeing_sources_and_bounds_correction() {
        let one = finish(
            vec![ClockSample {
                source: "a".into(),
                offset_ms: 42_000,
                rtt_ms: 10,
            }],
            vec![],
        );
        assert_eq!(one.offset_ms, None);
        let three = finish(
            ["a", "b", "c"]
                .iter()
                .zip([-41_500, -42_000, 9_000_000])
                .map(|(s, o)| ClockSample {
                    source: (*s).into(),
                    offset_ms: o,
                    rtt_ms: 10,
                })
                .collect(),
            vec![],
        );
        assert_eq!(three.offset_ms, Some(-41_500));
        assert!(three.significant());
    }

    #[test]
    fn correction_shifts_application_time() {
        // The global correction is not mutated here: other tests read the app clock in parallel.
        assert_eq!(corrected(1_792_529_742_473, -42_000), 1_792_529_700_473);
        assert_eq!(corrected(i64::MAX, 5), i64::MAX);
        assert!(set_correction_ms(-MAX_CORRECTION_MS - 1).is_err());
        assert_eq!(correction_ms(), 0);
    }
}
