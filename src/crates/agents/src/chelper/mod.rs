//! Chelper: the GWM coding gateway every client here sends its model calls
//! through. It is a billing provider, not an installed execution client: its
//! sessions are the clients' own, so this descriptor carries no transcript.
//! Its balance, monthly budget and daily spend are read from the gateway's
//! two hosts with the key Claude Code already sends it (see `account`).
pub mod account;
pub mod gateway;

use crate::discovery::Look;
use crate::Agent;
use crate::Discovery;
use crate::{Location, Source};

/// Claude Code's own settings, where the gateway's address and key live.
const CLAUDE_SETTINGS: Location =
    Location::home(&["CLAUDE_CONFIG_DIR"], &[".claude"], "settings.json");
const CLAUDE_LOCAL_SETTINGS: Location =
    Location::home(&["CLAUDE_CONFIG_DIR"], &[".claude"], "settings.local.json");

/// Whether an address is the gateway this provider bills for. Only its own
/// hosts count: a key is never sent anywhere else.
pub fn gateway_host(host: &str) -> Option<&'static str> {
    [account::GATEWAY, account::CONSOLE]
        .into_iter()
        .find(|known| *known == host)
}

/// How a gateway is found configured: Claude Code, or this process, sending
/// its calls to the gateway's own address with a key.
pub fn configured(look: &dyn Look) -> Vec<&'static str> {
    let mut found = Vec::new();
    let routes = |base: Option<&str>, key: Option<&str>| {
        let base = base.and_then(crate::endpoints::base_url_host);
        base.as_deref().and_then(gateway_host).is_some()
            && key.is_some_and(|key| !key.trim().is_empty() && !key.starts_with('!'))
    };
    let environment = [Some("ANTHROPIC_AUTH_TOKEN"), Some("ANTHROPIC_API_KEY")]
        .into_iter()
        .flatten()
        .find_map(|name| look.env(name));
    if routes(look.env("ANTHROPIC_BASE_URL"), environment) {
        found.push("provider-environment");
    }
    for location in [CLAUDE_SETTINGS, CLAUDE_LOCAL_SETTINGS] {
        if !look.exists(&location) {
            continue;
        }
        let settings = look.settings(&location);
        let env = &settings["env"];
        let key = env["ANTHROPIC_AUTH_TOKEN"]
            .as_str()
            .or_else(|| env["ANTHROPIC_API_KEY"].as_str());
        if routes(env["ANTHROPIC_BASE_URL"].as_str(), key)
            && !found.contains(&"claude-provider-route")
        {
            found.push("claude-provider-route");
        }
    }
    found
}

static DISCOVERY: Discovery = Discovery {
    configured: Some(configured),
    ..Discovery::NONE
};

pub static AGENT: Agent = Agent {
    id: "chelper",
    name: "Chelper",
    sessions: false,
    hooks: None,
    observer: None,
    channel: None,
    program: None,
    replies: None,
    turns: None,
    items: None,
    sources: crate::everywhere(&[] as &[Source]),
    discovery: &DISCOVERY,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Look;
    use std::collections::HashMap;

    /// The file system a discovery rule is checked against, as platform's own
    /// lookup answers it.
    struct Files(HashMap<String, String>);
    impl Files {
        fn with(files: &[(&str, &str)]) -> Self {
            Self(
                files
                    .iter()
                    .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
                    .collect(),
            )
        }
    }
    impl Look for Files {
        fn env(&self, _key: &str) -> Option<&str> {
            None
        }
        fn exists(&self, _location: &Location) -> bool {
            true
        }
        fn text(&self, _location: &Location) -> Option<String> {
            None
        }
        fn settings(&self, location: &Location) -> serde_json::Value {
            // One settings file stands in for every location a client's own
            // rule reads, as a machine with the gateway configured has it.
            let found = if location.path == "settings.json" {
                self.0
                    .iter()
                    .find(|(path, _)| path.ends_with("/.claude/settings.json"))
                    .map(|(_, text)| text.as_str())
            } else {
                None
            };
            found
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or(serde_json::Value::Null)
        }
    }

    #[test]
    fn only_the_gateways_own_hosts_are_read() {
        assert_eq!(gateway_host("aicoding.gwm.cn"), Some("aicoding.gwm.cn"));
        assert_eq!(
            gateway_host("aicoding-admin.gwm.cn"),
            Some("aicoding-admin.gwm.cn")
        );
        assert_eq!(gateway_host("api.anthropic.com"), None);
        assert_eq!(gateway_host("relay.example.com"), None);
    }

    #[test]
    fn claude_codes_route_to_the_gateway_is_what_configures_it() {
        let routed = Files::with(&[(
            "/u/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://aicoding.gwm.cn","ANTHROPIC_AUTH_TOKEN":"sk-1"}}"#,
        )]);
        assert_eq!(configured(&routed), ["claude-provider-route"]);
        // Another relay with a key is somebody else's balance.
        let elsewhere = Files::with(&[(
            "/u/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com","ANTHROPIC_AUTH_TOKEN":"sk-1"}}"#,
        )]);
        assert!(configured(&elsewhere).is_empty());
        // The gateway named without a key is not readable.
        let keyless = Files::with(&[(
            "/u/.claude/settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://aicoding.gwm.cn"}}"#,
        )]);
        assert!(configured(&keyless).is_empty());
    }
}
