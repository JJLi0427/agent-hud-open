//! A balance whose provider reports its own spending (`reported` in the
//! snapshot): the month's budget and the days it counted, as the same
//! `periods` every other balance has. The local ledger holds none of those
//! calls, so a reported balance never consults it.
use super::*;
use crate::l10n::tr;

/// The periods of a balance whose provider reports the spending itself
/// (`reported`: `{unit, month: {spent, budget, from}, days: [{at, spent,
/// calls, tokens}]}`), as the same `periods` the ledger would have filled:
/// each window's cost from the days it covers, its pace, when the balance
/// runs out at that pace, and the days as a series. A reported day is what
/// the provider counted, so a window it does not cover says so rather than
/// showing a cost that is only partly there.
pub(super) fn reported(row: &Value, now: i64) -> Option<Value> {
    let report = &row["reported"];
    let currency = report["unit"].as_str().unwrap_or("");
    if !report.is_object() || currency.is_empty() {
        return None;
    }
    // A day as the provider counted it: when it began, what it cost, and the
    // tokens the calls behind that cost used.
    let mut days: Vec<(i64, f64, u64)> = report["days"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|day| {
            let at = day["at"].as_str().and_then(metering::date_ms)?;
            let spent = day["spent"].as_f64().filter(|spent| spent.is_finite())?;
            let tokens = day["tokens"].as_u64().unwrap_or_default();
            Some((at, spent, tokens))
        })
        .collect();
    days.sort_by_key(|(at, _, _)| *at);
    let oldest = days.first().map(|(at, _, _)| *at);
    let mut periods = serde_json::Map::new();
    for (name, days_back, step) in [
        ("day", 1i64, 3_600_000i64),
        ("week", 7, 6 * 3_600_000),
        ("month", 30, 86_400_000),
    ] {
        let length = days_back * 86_400_000;
        let from = now - length;
        // The provider's own days, and whether they cover the whole window.
        // A day belongs to the window that starts after it: today plus the
        // six before it are the week, not the seven before it as well.
        let held: Vec<(i64, f64, u64)> = days
            .iter()
            .copied()
            .filter(|(at, _, _)| *at > from)
            .collect();
        let whole = name == "day" || oldest.is_some_and(|oldest| oldest <= from);
        let total: f64 = held.iter().map(|(_, spent, _)| spent).sum();
        let tokens: u64 = held.iter().map(|(_, _, tokens)| tokens).sum();
        // A day is the provider's last one: a part of a day is still what it
        // spent so far, not a day's worth of it. A longer window adds up the
        // days it has, and says so when that is not all of them: a cost of
        // what is known beats no cost at all, as long as it is not read as
        // the window's whole.
        let amount = if name == "day" {
            days.last().map(|(_, spent, _)| *spent)
        } else {
            (!held.is_empty()).then_some(total)
        };
        // The tokens behind that money, as the provider counted them: today's
        // for the day, and the window's sum for the longer ones.
        let counted = if name == "day" {
            days.last().map(|(_, _, tokens)| *tokens)
        } else {
            (!held.is_empty()).then_some(tokens)
        };
        // Today's cost is today's, but today is not a day's pace: a morning
        // balance would forecast running out in a century, and one quiet
        // Sunday would forecast a decade. The pace is the mean of the whole
        // days the provider reported this week, which is what "a day" has
        // recently cost; only a provider with no earlier day falls back to
        // today's own spend.
        let whole_days: Vec<f64> = days
            .iter()
            .rev()
            .skip_while(|(at, _, _)| *at > now - 86_400_000)
            .take_while(|(at, _, _)| *at > now - 7 * 86_400_000)
            .map(|(_, spent, _)| *spent)
            .collect();
        let recent_day = (!whole_days.is_empty())
            .then(|| whole_days.iter().sum::<f64>() / whole_days.len() as f64);
        let per_day = if name == "day" {
            recent_day.or(amount)
        } else if held.is_empty() {
            None
        } else {
            // The days it has, not the days the window is long: a fortnight of
            // spending spread over a month would read as half the pace it is.
            Some(total / held.len() as f64)
        };
        let run = per_day
            .zip(row["amount"].as_f64())
            .filter(|(per_day, amount)| *per_day > 0.0 && *amount > 0.0)
            .and_then(|(per_day, amount)| now.checked_add((amount / per_day * 86_400_000.0) as i64))
            .and_then(DateTime::<Utc>::from_timestamp_millis)
            .map(|at| at.to_rfc3339());
        let mut buckets = BTreeMap::<i64, f64>::new();
        for (at, spent, _) in &held {
            *buckets.entry(at.div_euclid(step) * step).or_default() += spent;
        }
        let series: Vec<Value> = buckets
            .into_iter()
            .map(|(at, spent)| {
                json!({
                    "timestamp": DateTime::<Utc>::from_timestamp_millis(at).map(|at| at.to_rfc3339()),
                    "amount": spent,
                    "currency": currency,
                })
            })
            .collect();
        // Why a window is not the whole: the provider reports two weeks, and
        // a reading that brought none of its days yet is not a month without
        // spending — the difference between no data and no cost is the whole
        // point of the notice.
        let notice = if days.is_empty() {
            Some(tr!("The gateway has not reported these days' spending yet"))
        } else if !whole {
            Some(tr!("The gateway reports the last two weeks, so this window holds only the days it reports"))
        } else {
            None
        };
        periods.insert(
            name.into(),
            json!({
                "from": DateTime::<Utc>::from_timestamp_millis(from).map(|at| at.to_rfc3339()),
                "to": DateTime::<Utc>::from_timestamp_millis(now).map(|at| at.to_rfc3339()),
                "hours": length / 3_600_000,
                "cost": amount.map(|amount| json!({"currency": currency, "amount": amount})),
                "burnRate": per_day.map(|per_day| json!({"currency": currency, "perDay": per_day})),
                "runsOutAt": run,
                "replyRate": Value::Null,
                "consumed": counted.map(|tokens| json!({"used": tokens, "capacity": null, "unit": "tokens"})),
                "tokensPerHour": counted.map(|tokens| {
                    // Today so far, at the pace the day has run at; a longer
                    // window spread over its whole length.
                    let hours = if name == "day" {
                        let start = days.last().map(|(at, _, _)| *at).unwrap_or(now);
                        (((now - start) as f64) / 3_600_000.0).max(1.0)
                    } else {
                        (length as f64) / 3_600_000.0
                    };
                    tokens as f64 / hours
                }),
                "series": series,
                "coverage": {"complete": whole && !days.is_empty(), "notice": notice},
            }),
        );
    }
    Some(json!({
        "periods": periods,
        "defaultPeriod": "day",
        "estimate": "reported-by-provider",
        "pricingSnapshot": pricing::checked_on(),
    }))
}

#[cfg(test)]
mod reported_tests {
    use super::reported;
    use serde_json::json;

    /// A balance the provider reports on, with days counted back from `now`.
    fn balance(now: i64, days: &[(i64, f64)], amount: f64) -> serde_json::Value {
        let days: Vec<serde_json::Value> = days
            .iter()
            .map(|(back, spent)| {
                json!({
                    "date": "2026-10-30",
                    "at": chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now - back * 86_400_000)
                        .unwrap()
                        .to_rfc3339(),
                    "spent": spent,
                    "calls": 1,
                    "tokens": 1,
                })
            })
            .collect();
        json!({
            "vendor": "chelper",
            "amount": amount,
            "currency": "CNY",
            "reported": {"unit": "CNY", "days": days},
        })
    }

    #[test]
    fn a_provider_that_reports_its_spending_answers_before_the_ledger() {
        let now = 1_700_000_000_000;
        let row = balance(now, &[(0, 12.0), (1, 30.0)], 500.0);
        let metrics = reported(&row, now).expect("reported");
        assert_eq!(metrics["estimate"], "reported-by-provider");
        let day = &metrics["periods"]["day"];
        // Today's cost is today's; the pace is yesterday's whole day.
        assert_eq!(day["cost"], json!({"currency": "CNY", "amount": 12.0}));
        assert_eq!(day["burnRate"], json!({"currency": "CNY", "perDay": 30.0}));
        assert_eq!(day["coverage"]["complete"], true);
        // The tokens behind the money are the provider's count, not a rate.
        assert_eq!(
            day["consumed"],
            json!({"used": 1, "capacity": null, "unit": "tokens"})
        );
        assert_eq!(day["replyRate"], serde_json::Value::Null);
        // A balance of 500 at 30 a day runs out in about 16 days.
        let run = day["runsOutAt"].as_str().unwrap();
        let at = chrono::DateTime::parse_from_rfc3339(run)
            .unwrap()
            .timestamp_millis();
        let days = (at - now) as f64 / 86_400_000.0;
        assert!((16.0..17.0).contains(&days), "{days}");
    }

    #[test]
    fn a_week_holds_only_the_days_the_provider_reports() {
        let now = 1_700_000_000_000;
        // Three days of a seven-day window: the window is not whole.
        let row = balance(now, &[(0, 1.0), (1, 1.0), (2, 1.0)], 100.0);
        let metrics = reported(&row, now).expect("reported");
        let week = &metrics["periods"]["week"];
        // What it knows is added up, and the window says it is not the whole.
        assert_eq!(week["cost"], json!({"currency": "CNY", "amount": 3.0}));
        assert_eq!(week["coverage"]["complete"], false);
        assert!(week["coverage"]["notice"].as_str().is_some());
        assert_eq!(week["series"].as_array().unwrap().len(), 3);
        // Three days reported, 3.00 spent: a day has cost 1.00.
        assert_eq!(week["burnRate"], json!({"currency": "CNY", "perDay": 1.0}));
    }

    #[test]
    fn a_whole_week_is_its_days_added_up() {
        let now = 1_700_000_000_000;
        let days: Vec<(i64, f64)> = (0..14).map(|back| (back, 10.0)).collect();
        let row = balance(now, &days, 1000.0);
        let metrics = reported(&row, now).expect("reported");
        assert_eq!(
            metrics["periods"]["week"]["cost"],
            json!({"currency": "CNY", "amount": 70.0})
        );
        assert_eq!(
            metrics["periods"]["month"]["cost"],
            json!({"currency": "CNY", "amount": 140.0})
        );
        assert_eq!(metrics["periods"]["week"]["coverage"]["complete"], true);
    }

    #[test]
    fn the_windows_carry_the_tokens_the_provider_counted() {
        let now = 1_700_000_000_000;
        let row = balance(now, &[(0, 2.0), (1, 3.0)], 100.0);
        let metrics = reported(&row, now).expect("reported");
        let day = &metrics["periods"]["day"];
        assert_eq!(
            day["consumed"],
            json!({"used": 1, "capacity": null, "unit": "tokens"})
        );
        assert!(day["tokensPerHour"].as_f64().is_some_and(|rate| rate > 0.0));
        // A window's tokens are its days' sum.
        assert_eq!(
            metrics["periods"]["week"]["consumed"],
            json!({"used": 2, "capacity": null, "unit": "tokens"})
        );
    }

    #[test]
    fn a_report_with_no_days_yet_says_so_instead_of_costing_nothing() {
        let now = 1_700_000_000_000;
        let row = balance(now, &[], 100.0);
        let metrics = reported(&row, now).expect("reported");
        let day = &metrics["periods"]["day"];
        assert_eq!(day["cost"], serde_json::Value::Null);
        assert_eq!(day["coverage"]["complete"], false);
        assert!(day["coverage"]["notice"].as_str().is_some());
    }

    #[test]
    fn a_balance_without_a_report_is_left_to_the_ledger() {
        let now = 1_700_000_000_000;
        assert!(reported(&json!({"vendor": "deepseek", "amount": 1.0}), now).is_none());
        // A report in no currency says nothing a person could read.
        assert!(reported(
            &json!({"amount": 1.0, "reported": {"days": [{"at": "2026-10-30T00:00:00Z", "spent": 1.0}]}}),
            now
        )
        .is_none());
    }

    #[test]
    fn a_morning_forecast_uses_whole_days_not_the_mornings() {
        let now = 1_700_000_000_000;
        // Today has barely begun; the whole days behind it are the pace.
        let row = balance(now, &[(0, 0.26), (1, 400.0), (2, 380.0)], 10_000.0);
        let metrics = reported(&row, now).expect("reported");
        let day = &metrics["periods"]["day"];
        assert_eq!(day["cost"], json!({"currency": "CNY", "amount": 0.26}));
        assert_eq!(day["burnRate"], json!({"currency": "CNY", "perDay": 390.0}));
        let run = day["runsOutAt"].as_str().unwrap();
        let at = chrono::DateTime::parse_from_rfc3339(run)
            .unwrap()
            .timestamp_millis();
        let days = (at - now) as f64 / 86_400_000.0;
        // 10000 at 390 a day, not 10000 at 0.26 a day.
        assert!((25.0..26.0).contains(&days), "{days}");
    }

    #[test]
    fn a_quiet_weekend_averages_into_the_weeks_pace() {
        let now = 1_700_000_000_000;
        // A Saturday with nothing so far, after three working days.
        let row = balance(
            now,
            &[(0, 0.0), (1, 0.0), (2, 300.0), (3, 300.0), (4, 300.0)],
            10_000.0,
        );
        let metrics = reported(&row, now).expect("reported");
        let burn = &metrics["periods"]["day"]["burnRate"];
        // Today is left out; the quiet Saturday and three working days are
        // not, so the pace is what a day has averaged, not a spike.
        assert_eq!(burn["perDay"], 225.0);
    }

    #[test]
    fn a_provider_with_one_day_paces_at_that_day() {
        let now = 1_700_000_000_000;
        let row = balance(now, &[(0, 12.0)], 500.0);
        let metrics = reported(&row, now).expect("reported");
        assert_eq!(
            metrics["periods"]["day"]["burnRate"],
            json!({"currency": "CNY", "perDay": 12.0})
        );
    }

    #[test]
    fn a_day_with_no_spend_costs_nothing_and_forecasts_nothing() {
        let now = 1_700_000_000_000;
        let row = balance(now, &[(0, 0.0)], 100.0);
        let metrics = reported(&row, now).expect("reported");
        assert_eq!(
            metrics["periods"]["day"]["cost"],
            json!({"currency": "CNY", "amount": 0.0})
        );
        assert_eq!(
            metrics["periods"]["day"]["runsOutAt"],
            serde_json::Value::Null
        );
    }
}
