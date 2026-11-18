//! The plan beside an account's name (Open's `SourceStatus.planLabel`):
//! Claude's and Codex's tiers in their own names, any other code as words.
//! Provider codes stay out of the UI.

/// The badge for `plan` of `provider` (the app's provider id), or none for an
/// empty plan.
pub fn plan_label(provider: &str, plan: &str) -> Option<String> {
    if plan.is_empty() {
        return None;
    }
    let tier = match (provider, plan.to_lowercase().as_str()) {
        // Anthropic's names since October 2026: Max 5x is Max, Max 20x Ultra.
        ("claude", "max_5x") => Some("Max"),
        ("claude", "max_20x") => Some("Ultra"),
        ("codex", "prolite") => Some("Pro x5"),
        ("codex", "pro") => Some("Pro x20"),
        // The gateway's own group code names no plan a person chose: its
        // parts are the gateway's rate groups ("svip&default"), and reading
        // them as words would put "Svip&Default" beside every account.
        ("chelper", _) => return None,
        _ => None,
    };
    if let Some(tier) = tier {
        return Some(tier.into());
    }
    // Better than Open, which capitalised every code as Swift does: an
    // underscore is a space ("pro_plus" was "Pro_Plus"), and a name already
    // written in both cases keeps its casing ("SuperGrok" was "Supergrok").
    let spaced = plan.replace('_', " ");
    let mixed = spaced.chars().any(char::is_uppercase) && spaced.chars().any(char::is_lowercase);
    Some(if mixed { spaced } else { capitalized(&spaced) })
}

/// Swift's `capitalized`: each word's first letter upper case and the rest
/// lower, words split at spaces and hyphens.
fn capitalized(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start = true;
    for c in text.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start = !c.is_alphanumeric();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::plan_label;

    /// The tiers a person knows, in the vendor's own words.
    #[test]
    fn a_vendors_own_tier_keeps_its_name() {
        assert_eq!(plan_label("claude", "max_5x").as_deref(), Some("Max"));
        assert_eq!(plan_label("claude", "max_20x").as_deref(), Some("Ultra"));
        assert_eq!(plan_label("codex", "pro").as_deref(), Some("Pro x20"));
        assert_eq!(plan_label("codex", "prolite").as_deref(), Some("Pro x5"));
    }

    /// Any other code reads as words, and a name already in both cases keeps
    /// its casing.
    #[test]
    fn another_code_reads_as_words() {
        assert_eq!(
            plan_label("claude", "pro_plus").as_deref(),
            Some("Pro Plus")
        );
        assert_eq!(
            plan_label("grok-bot", "SuperGrok").as_deref(),
            Some("SuperGrok")
        );
        assert_eq!(plan_label("codex", "").as_deref(), None);
    }

    /// The gateway's plan is its rate group, not a plan a person chose: a
    /// badge reading "Svip&Default" would name nothing and take the room the
    /// account's own name needs.
    #[test]
    fn the_gateways_rate_group_is_not_a_badge() {
        assert_eq!(plan_label("chelper", "svip&default").as_deref(), None);
        assert_eq!(plan_label("chelper", "default").as_deref(), None);
    }
}
