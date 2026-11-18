//! The coding gateway's balance, its monthly budget and the days' spend
//! (docs/accounts.md). Two hosts of one account serve them: the gateway, which
//! reports the account's quota as integers in the money its public status
//! names, and the operator's console beside it, whose self-service APIs report
//! the month's budget and each day's spend. Both are read with the key Claude
//! Code already sends the gateway; no client or helper is launched, and no key
//! ever leaves the host it already belongs to.
use super::gateway::*;
use crate::account::{account_id, legacy_unresolved, Host, Reading, Unread};
use crate::l10n;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// The setting that lets the gateway be read; off until the owner agrees.
pub const CONSENT: &str = "readChelperBalance";

/// The gateway every client calls: its public status prices the account's
/// quota, and its key's usage endpoint reports what is left of it.
pub const GATEWAY: &str = "aicoding.gwm.cn";

/// The operator's console beside the gateway: its self-service APIs report the
/// month's budget and each day's spend, against the same account and key.
pub const CONSOLE: &str = "aicoding-admin.gwm.cn";

/// The most days the console reports in one answer.
const DAYS: u32 = 14;

pub(super) fn unreadable() -> String {
    l10n::text(
        "网关返回的数据格式无法识别",
        "The gateway's answer has an unsupported format",
    )
}

pub(super) fn unreachable() -> String {
    l10n::text(
        "网关无法连接，请检查网络后重试",
        "The gateway could not be reached; check the connection",
    )
}

pub(super) fn stopped() -> String {
    l10n::text("账户读取已停止", "Account reading was stopped")
}

/// The provider's reader as the engine takes it: a failure says whether trying
/// again can help. Only the two statements about the connection and about the
/// reading having been stopped pass; a refused key or an unreadable answer is
/// what the gateway said, and asking again would say it again.
pub fn read(host: &dyn Host) -> Result<Reading, Unread> {
    reading(host).map_err(|reason| {
        if reason == stopped() {
            Unread::Transient {
                reason,
                retry_after: None,
            }
        } else if reason == unreachable() {
            Unread::transient(reason)
        } else {
            Unread::Failed(reason)
        }
    })
}

fn reading(host: &dyn Host) -> Result<Reading, String> {
    let mut reading = Reading::empty("chelper");
    if !host.consented(CONSENT) {
        reading.notice = Some(l10n::text(
            "开启网关余额读取后，才会使用 Claude Code 里已有的网关 key",
            "Turn on gateway reading to use the gateway key Claude Code already holds",
        ));
        return Ok(reading);
    }
    let Some(key) = key(host)? else {
        reading.notice = Some(l10n::text(
            "Claude Code 的设置里没有网关地址和 key，无法读取余额",
            "Claude Code's settings name no gateway address and key, so the balance cannot be read",
        ));
        return Ok(reading);
    };
    let key = key.as_str();
    let now = Utc::now();
    // What the money is: the gateway's own arithmetic, a quota unit being
    // worth `usd_exchange_rate / quota_per_unit` of the money it displays.
    let status = get(host, &status_url(), None, 10)?;
    let status = &status["data"];
    let per_unit = amount(&status["quota_per_unit"])
        .filter(|per_unit| *per_unit > 0.0)
        .ok_or_else(unreadable)?;
    let rate = amount(&status["usd_exchange_rate"]).ok_or_else(unreadable)?;
    let currency = status["quota_display_type"]
        .as_str()
        .filter(|currency| matches!(*currency, "CNY" | "USD"))
        .ok_or_else(|| {
            l10n::text(
                "此网关的余额不是以人民币或美元计价的",
                "This gateway's balance is not in a currency Agent HUD reads",
            )
        })?;
    let unit = rate / per_unit;
    if host.cancelled() {
        return Err(stopped());
    }
    let used = get(host, &usage_url(), Some(key), 10)?;
    if used["code"] != true {
        return Err(l10n::text(
            "网关没有认可这个 key",
            "The gateway did not accept this key",
        ));
    }
    let data = &used["data"];
    let available = amount(&data["total_available"]).ok_or_else(unreadable)?;
    // The console answers with the month's budget and the days' spend. A
    // console that cannot be read leaves the balance standing, because what
    // the gateway itself reported is still true; its reason is kept as the
    // notice.
    let console = if host.cancelled() {
        Err(stopped())
    } else {
        get(host, &account_url(), Some(key), 10)
    };
    let console = console.and_then(|body| {
        if body["unlimited_quota"] == true {
            return Err(l10n::text(
                "此网关按不限额度计费，没有月度预算",
                "This gateway bills without a limit, so it has no monthly budget",
            ));
        }
        Ok(body)
    });
    let id = account_id("chelper", &[&scope(host)]);
    let identity = legacy_unresolved("Chelper", &scope(host));
    let started = month_start(now);
    let month = console.as_ref().ok().and_then(|body| {
        let spent = amount(&body["month_used"])?;
        let budget = amount(&body["month_total_quota"])?;
        (budget > 0.0).then_some((spent, budget, started))
    });
    // The days, oldest first as the console gives them.
    // The console adds a day up as it is asked, and on a busy morning that
    // takes it a while: the days are worth waiting for, and worth asking twice
    // for, since a window with no cost reads as a month that spent nothing.
    let days: Vec<Value> = if console.is_ok() && !host.cancelled() {
        get(host, &daily_url(DAYS), Some(key), 20)
            .or_else(|_| {
                if host.cancelled() {
                    Err(unreachable())
                } else {
                    get(host, &daily_url(DAYS), Some(key), 20)
                }
            })
            .ok()
            .and_then(|body| {
                Some(
                    body["rows"]
                        .as_array()?
                        .iter()
                        .filter_map(|row| {
                            // The console counts its days in Beijing's time;
                            // each becomes the instant that day began, so the
                            // core needs no calendar of its own.
                            let at = day_start(row["date"].as_str()?, beijing())?;
                            Some(json!({
                                "date": row["date"].as_str()?,
                                "at": iso(at),
                                "spent": amount(&row["total_cny"])?,
                                "calls": count(&row["calls"]),
                                "tokens": count(&row["tokens"]),
                            }))
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let report = reported(currency, month, &days);
    let billing = json!({
        "id": format!("{id}:{currency}"),
        "vendor": "chelper",
        "accountId": identity,
        "label": l10n::text("网关余额", "Gateway balance"),
        "amount": round(available * unit),
        "currency": currency,
        "available": true,
        "reported": report,
    });
    reading.billing.push(billing);
    // The month's budget, as one window, so the ring, the pace and the
    // forecast are the same arithmetic as every other quota: what the console
    // says is used of the total it budgets, resetting when it starts the next.
    if let Some((spent, budget, _)) = month {
        let percent = (spent / budget * 100.0).clamp(0.0, 100.0);
        let reset = next_month(now);
        let mut window = json!({
            "id": format!("{id}:month"),
            "vendor": "chelper",
            "accountId": identity,
            "account": console.as_ref().ok().and_then(|body| body["name"].as_str()).unwrap_or("Chelper"),
            "name": l10n::text("月度预算", "Monthly budget"),
            "shortLabel": l10n::text("本月", "Monthly"),
            "usedPercent": percent,
            "resetAt": reset.map(iso),
            "status": crate::account::status(percent),
            "allModels": true,
            "consumerSource": "chelper",
            "observedAt": iso(now),
        });
        if let Some(reset) = reset {
            window["windowDurationSeconds"] = json!((reset - started).num_seconds().max(0) as f64);
        }
        // What the month's calls used, as the console counted it: this
        // window's consumption, in tokens, since the console spends it on
        // tokens rather than on the budget's money.
        let tokens: u64 = days
            .iter()
            .filter(|day| {
                day["at"]
                    .as_str()
                    .and_then(crate::transcript::metering::date_ms)
                    .is_some_and(|at| at >= started.timestamp_millis())
            })
            .filter_map(|day| day["tokens"].as_u64())
            .sum();
        if tokens > 0 {
            window["nativeUsage"] = json!({"used": tokens, "capacity": null, "unit": "tokens"});
        }
        reading.quotas.push(window);
    }
    reading.account = Some(json!({
        "id": identity,
        "provider": "chelper",
        "label": data["name"].as_str().unwrap_or("Chelper"),
        "plan": console.as_ref().ok().and_then(|body| body["group"].as_str()),
        "identity": "gateway-key",
        "checkedAt": iso(now),
    }));
    if let Err(notice) = console {
        reading.notice = Some(notice);
    }
    Ok(reading)
}

/// Money to the cent, as every balance here is shown.
pub(super) fn round(amount: f64) -> f64 {
    (amount * 100.0).round() / 100.0
}

pub(super) fn iso(at: DateTime<Utc>) -> String {
    at.to_rfc3339()
}

/// This provider in the engine's list: the gateway bills every client routed
/// to it, so Claude Code is the client whose work spends it here, and its
/// month is money spent on any device rather than only on this Mac.
pub static PROVIDER: crate::account::Provider = crate::account::Provider {
    work_sources: &["claude"],
    local_work: false,
    ..crate::account::Provider::new("chelper", read)
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::fake::FakeHost;
    use crate::account::HttpFailure;
    use chrono::{Datelike, Timelike};
    use std::sync::atomic::Ordering;

    fn consented() -> FakeHost {
        let host = FakeHost::new("/u").file(
            "/u/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://aicoding.gwm.cn","ANTHROPIC_AUTH_TOKEN":"sk-1"}}"#,
        );
        host.consent.store(true, Ordering::Release);
        host
    }

    fn status() -> Value {
        json!({"data":{"quota_per_unit":500000,"usd_exchange_rate":7.3,"quota_display_type":"CNY"}})
    }
    fn used(available: u64) -> Value {
        json!({"code":true,"data":{"name":"GW01-李","total_available":available,"total_used":3000,"total_granted":4000}})
    }
    fn account(month: f64, budget: f64) -> Value {
        json!({"name":"GW01-李","group":"svip","remain_quota":500.0,"used_quota":100.0,
            "month_used":month,"month_total_quota":budget,"unlimited_quota":false})
    }
    fn with_console(month: f64, budget: f64) -> FakeHost {
        consented()
            .answer(&status_url(), Ok(status()))
            .answer(&usage_url(), Ok(used(370_307_160)))
            .answer(&account_url(), Ok(account(month, budget)))
            .answer(&daily_url(DAYS), Ok(json!({"rows":[]})))
    }

    #[test]
    fn nothing_is_read_until_the_owner_agrees() {
        let host = FakeHost::new("/u");
        let reading = read(&host).unwrap();
        assert!(reading.notice.is_some());
        assert!(host.sent.borrow().is_empty());
    }

    #[test]
    fn the_key_is_the_one_claude_code_already_sends_the_gateway() {
        let host = with_console(1000.0, 4000.0);
        let reading = read(&host).unwrap();
        // The public status names no account, so no key goes with it.
        let asked = host.asked.borrow();
        assert_eq!(asked[0], (status_url(), None));
        assert_eq!(asked[1], (usage_url(), Some("Bearer sk-1".into())));
        let spend = 370_307_160.0 * 7.3 / 500_000.0;
        assert_eq!(reading.billing[0]["amount"], json!(round(spend)));
        assert_eq!(reading.billing[0]["currency"], "CNY");
        assert_eq!(reading.billing[0]["vendor"], "chelper");
        assert_eq!(reading.account.unwrap()["label"], "GW01-李");
    }

    #[test]
    fn a_rotated_key_is_the_same_account() {
        let first = consented()
            .file(
                "/u/.claude/settings.json",
                r#"{"env":{"ANTHROPIC_BASE_URL":"https://aicoding.gwm.cn","ANTHROPIC_AUTH_TOKEN":"sk-1"}}"#,
            )
            .answer(&status_url(), Ok(status()))
            .answer(&usage_url(), Ok(used(1)))
            .answer(&account_url(), Ok(account(1.0, 2.0)))
            .answer(&daily_url(DAYS), Ok(json!({"rows":[]})));
        let second = consented()
            .file(
                "/u/.claude/settings.json",
                r#"{"env":{"ANTHROPIC_BASE_URL":"https://aicoding.gwm.cn","ANTHROPIC_AUTH_TOKEN":"sk-2"}}"#,
            )
            .answer(&status_url(), Ok(status()))
            .answer(&usage_url(), Ok(used(1)))
            .answer(&account_url(), Ok(account(1.0, 2.0)))
            .answer(&daily_url(DAYS), Ok(json!({"rows":[]})));
        assert_eq!(
            read(&first).unwrap().billing[0]["id"],
            read(&second).unwrap().billing[0]["id"]
        );
    }

    #[test]
    fn without_a_route_to_the_gateway_nothing_is_sent() {
        let host = FakeHost::new("/u");
        host.consent.store(true, Ordering::Release);
        let reading = read(&host).unwrap();
        assert!(reading.notice.is_some());
        assert!(host.sent.borrow().is_empty());
    }

    #[test]
    fn another_relay_is_not_read_as_the_gateway() {
        let host = FakeHost::new("/u").file(
            "/u/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com","ANTHROPIC_AUTH_TOKEN":"sk-1"}}"#,
        );
        host.consent.store(true, Ordering::Release);
        let reading = read(&host).unwrap();
        assert!(reading.notice.is_some());
        assert!(host.sent.borrow().is_empty());
    }

    #[test]
    fn the_months_budget_is_a_window_that_resets_on_the_first() {
        let reading = read(&with_console(1000.0, 4000.0)).unwrap();
        let window = &reading.quotas[0];
        assert_eq!(window["usedPercent"], 25.0);
        assert_eq!(window["name"], "Monthly budget");
        assert_eq!(window["consumerSource"], "chelper");
        // Instants are written in UTC, as everywhere else in Core: read back
        // in the console's own time, the reset is a first of a month at
        // midnight, and it is the next one.
        let reset = DateTime::parse_from_rfc3339(window["resetAt"].as_str().unwrap()).unwrap();
        let beijing = reset.with_timezone(&beijing());
        assert_eq!(beijing.day(), 1);
        assert_eq!(beijing.hour(), 0);
        assert_eq!(beijing.minute(), 0);
        assert!(reset.to_utc() > Utc::now());
        let duration = window["windowDurationSeconds"].as_f64().unwrap();
        assert!((28.0..=31.0).contains(&(duration / 86_400.0)));
    }

    #[test]
    fn a_console_that_cannot_be_reached_still_leaves_the_balance() {
        let host = consented()
            .answer(&status_url(), Ok(status()))
            .answer(&usage_url(), Ok(used(370_307_160)))
            .answer(&account_url(), Err(HttpFailure::Status(500)));
        let reading = read(&host).unwrap();
        assert_eq!(reading.billing.len(), 1);
        assert!(reading.billing[0]["amount"].as_f64().unwrap() > 0.0);
        assert!(reading.quotas.is_empty());
        // The gateway's own reading stands, and says why the rest is not.
        assert!(reading.notice.is_some());
    }

    #[test]
    fn an_unlimited_account_has_no_budget_window() {
        let mut body = account(1000.0, 4000.0);
        body["unlimited_quota"] = json!(true);
        let reading = read(
            &consented()
                .answer(&status_url(), Ok(status()))
                .answer(&usage_url(), Ok(used(370_307_160)))
                .answer(&account_url(), Ok(body))
                .answer(&daily_url(DAYS), Ok(json!({"rows":[]}))),
        )
        .unwrap();
        assert!(reading.quotas.is_empty());
        assert_eq!(reading.billing.len(), 1);
    }

    #[test]
    fn a_gateway_that_refuses_the_key_is_reported_not_shown_as_spent() {
        let host = consented().answer(&status_url(), Ok(status())).answer(
            &usage_url(),
            Ok(json!({"code":false,"message":"Invalid token"})),
        );
        assert!(read(&host).is_err());
    }

    #[test]
    fn a_balance_in_another_money_is_not_guessed_at() {
        let mut body = status();
        body["data"]["quota_display_type"] = json!("EUR");
        let host = consented()
            .answer(&status_url(), Ok(body))
            .answer(&usage_url(), Ok(used(1)));
        assert!(read(&host).is_err());
    }

    #[test]
    fn the_months_tokens_are_the_windows_consumption() {
        let now = Utc::now();
        let day = |at: DateTime<Utc>, tokens: u64| {
            json!({"date": at.format("%Y-%m-%d").to_string(),
                "total_cny": 1.0, "calls": 2, "tokens": tokens})
        };
        let host = consented()
            .answer(&status_url(), Ok(status()))
            .answer(&usage_url(), Ok(used(370_307_160)))
            .answer(&account_url(), Ok(account(1000.0, 4000.0)))
            .answer(
                &daily_url(DAYS),
                Ok(json!({"rows":[
                    // Last month's day: its tokens are not this window's.
                    day(month_start(now) - chrono::Duration::days(1), 999),
                    day(now, 5_000),
                ]})),
            );
        let reading = read(&host).unwrap();
        assert_eq!(reading.quotas[0]["nativeUsage"]["used"], 5_000);
        assert_eq!(
            reading.quotas[0]["nativeUsage"]["capacity"],
            serde_json::Value::Null
        );
        assert_eq!(reading.quotas[0]["nativeUsage"]["unit"], "tokens");
    }

    #[test]
    fn each_day_is_kept_with_its_calls_and_tokens() {
        let now = today(Utc::now());
        let host = consented()
            .answer(&status_url(), Ok(status()))
            .answer(&usage_url(), Ok(used(370_307_160)))
            .answer(&account_url(), Ok(account(1000.0, 4000.0)))
            .answer(
                &daily_url(DAYS),
                Ok(json!({"rows":[
                    {"date":"2026-10-30","total_cny":832.78,"calls":521,"tokens":40678195},
                    {"date":now,"total_cny":0.64,"calls":48,"tokens":8595862}
                ]})),
            );
        let reading = read(&host).unwrap();
        // The report rides on the balance, which is the row Core turns into
        // the periods; the window above it is the budget those days spend.
        let report = &reading.billing[0]["reported"];
        assert_eq!(report["days"].as_array().unwrap().len(), 2);
        assert_eq!(report["days"][1]["spent"], 0.64);
        assert_eq!(report["days"][1]["calls"], 48);
        assert_eq!(report["days"][1]["tokens"], 8_595_862);
        assert_eq!(report["unit"], "CNY");
        assert_eq!(report["month"]["spent"], 1000.0);
        assert_eq!(report["month"]["budget"], 4000.0);
    }

    #[test]
    fn the_network_is_asked_only_about_the_gateways_own_hosts() {
        read(&with_console(1.0, 2.0)).unwrap();
        let host = with_console(1.0, 2.0);
        read(&host).unwrap();
        for (url, _) in host.asked.borrow().iter() {
            assert!(own_url(url), "{url} is not the gateway's");
        }
    }
}
