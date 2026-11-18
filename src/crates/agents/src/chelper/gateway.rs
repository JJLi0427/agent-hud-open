//! What the gateway and its console answer, and how their own numbers read:
//! the addresses, the JSON a body is in, the key a client already sends, and
//! the days the console counts. `account` asks for them; nothing here decides
//! what the balance means.
use super::account::{iso, round, unreachable};
use super::account::{CONSOLE, GATEWAY};
use crate::account::{Host, HttpFailure, HttpRequest};
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Utc};
use serde_json::{json, Value};
use std::time::Duration;
/// What the gateway reports that this Mac spent, in its own money: the month
/// so far against the console's budget, and the last days one by one. The
/// local ledger holds none of these calls, so only the provider can say.
pub(super) fn reported(
    unit: &str,
    month: Option<(f64, f64, DateTime<Utc>)>,
    days: &[Value],
) -> Value {
    let mut report = json!({"unit": unit, "days": days});
    if let Some((spent, budget, from)) = month {
        report["month"] =
            json!({"spent": round(spent), "budget": round(budget), "from": iso(from)});
    }
    report
}

/// A number the gateway gives as a number or as a string of digits.
pub(super) fn amount(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
}

/// A whole count, as the gateway gives it.
pub(super) fn count(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().filter(|v| *v >= 0.0).map(|v| v as u64))
        .or_else(|| value.as_str()?.trim().parse().ok())
}

/// A key that can be sent as one header value.
pub(super) fn usable(key: &str) -> bool {
    let key = key.trim();
    !key.is_empty()
        && key.len() <= 8 * 1024
        && !key.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
}

/// Whether an address is a host of this gateway, which is the only place its
/// key may be sent.
pub(super) fn own_url(url: &str) -> bool {
    crate::endpoints::base_url_host(url)
        .as_deref()
        .and_then(crate::chelper::gateway_host)
        .is_some()
}

/// The key a client already sends the gateway: the owner's saved one first,
/// then Claude Code's own settings, then this process's environment. Only a
/// client routed to the gateway names one.
pub(super) fn key(host: &dyn Host) -> Result<Option<String>, String> {
    if let Some(saved) = host.saved_credential("chelper")? {
        if let Some(key) = saved["apiKey"].as_str().filter(|key| usable(key)) {
            return Ok(Some(key.to_owned()));
        }
    }
    let claude = crate::account::variable(host, "CLAUDE_CONFIG_DIR")
        .unwrap_or_else(|| host.home().join(".claude"));
    for name in ["settings.json", "settings.local.json"] {
        let settings = crate::account::read_json(host, &claude.join(name));
        let Some(env) = settings.as_ref().map(|settings| &settings["env"]) else {
            continue;
        };
        if !env["ANTHROPIC_BASE_URL"].as_str().is_some_and(own_url) {
            continue;
        }
        let key = env["ANTHROPIC_AUTH_TOKEN"]
            .as_str()
            .or_else(|| env["ANTHROPIC_API_KEY"].as_str());
        if let Some(key) = key.filter(|key| usable(key)) {
            return Ok(Some(key.to_owned()));
        }
    }
    if !host.env("ANTHROPIC_BASE_URL").is_some_and(own_url) {
        return Ok(None);
    }
    let key = host
        .env("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| host.env("ANTHROPIC_API_KEY"))
        .filter(|key| usable(key));
    Ok(key.map(str::to_owned))
}

pub(super) fn get(
    host: &dyn Host,
    url: &str,
    key: Option<&str>,
    seconds: u64,
) -> Result<Value, String> {
    let bearer = key.map(|key| format!("Bearer {key}"));
    let secrets: &[(&str, &str)] = match bearer.as_deref() {
        Some(bearer) => &[("Authorization", bearer)],
        None => &[],
    };
    host.http_json(&HttpRequest {
        url,
        secrets,
        headers: &[],
        body: None,
        timeout: Duration::from_secs(seconds),
    })
    .map_err(|failure| match failure {
        HttpFailure::Unreachable | HttpFailure::BadHeader => unreachable(),
        other => other.message(),
    })
}

/// The account's own scope on this Mac, as the reader keys it: rotating the
/// gateway key must not make a second account.
pub(super) fn scope(host: &dyn Host) -> String {
    let home = host
        .canonical(host.home())
        .unwrap_or_else(|| host.home().to_path_buf());
    home.to_string_lossy().into_owned()
}

/// The gateway's public status: what one quota unit is worth, and in what
/// money. It names no account, so no key is sent with it.
pub(super) fn status_url() -> String {
    format!("https://{GATEWAY}/api/status")
}
/// The key's own balance, in quota units.
pub(super) fn usage_url() -> String {
    format!("https://{GATEWAY}/api/usage/token/")
}
/// The account as the console reports it, in money: the month's budget, what
/// of it is used, and what is left to call with.
pub(super) fn account_url() -> String {
    format!("https://{CONSOLE}/api/self/token")
}
/// The console's own days: each one's spend in money, its calls and tokens.
pub(super) fn daily_url(days: u32) -> String {
    format!("https://{CONSOLE}/api/self/usage/daily?days={days}")
}

/// Beijing's offset, which the console counts its month and days in.
pub(super) fn beijing() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("eight hours east is a valid offset")
}

/// The first instant of the month `at` falls in, in Beijing's time: the month
/// a budget starts in, whichever month this Mac's own clock shows.
pub(super) fn month_start(at: DateTime<Utc>) -> DateTime<Utc> {
    let offset = beijing();
    let local = at.with_timezone(&offset);
    let Some(first) = local.date_naive().with_day(1) else {
        return at;
    };
    offset
        .from_local_datetime(&first.and_hms_opt(0, 0, 0).unwrap_or_default())
        .single()
        .map(|start| start.with_timezone(&Utc))
        .unwrap_or(at)
}

/// The first instant of the month after `at`'s, in the same time: when the
/// console starts the next budget.
pub(super) fn next_month(at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let offset = beijing();
    let start = month_start(at).with_timezone(&offset).date_naive();
    let first = if start.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)?
    } else {
        chrono::NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)?
    };
    offset
        .from_local_datetime(&first.and_hms_opt(0, 0, 0)?)
        .single()
        .map(|at| at.with_timezone(&Utc))
}

/// The day the console is counting now, in its own time.
#[cfg(test)]
pub(super) fn today(at: DateTime<Utc>) -> String {
    at.with_timezone(&beijing()).format("%Y-%m-%d").to_string()
}

/// The instant the console's day `date` began, in its own time.
pub(super) fn day_start(date: &str, offset: FixedOffset) -> Option<DateTime<Utc>> {
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    offset
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .single()
        .map(|at| at.with_timezone(&Utc))
}
