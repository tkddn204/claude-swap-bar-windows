#![allow(dead_code)]

use serde::Deserialize;

/// One `cswap list --json` response.
///
/// Every optional field is optional because `cswap` omits keys rather than
/// emitting nulls when a value is unavailable, and because new keys appear
/// across `cswap` versions. Unknown keys are ignored by default, which is
/// what keeps this forward-compatible.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse {
    pub schema_version: u32,
    #[serde(default)]
    pub active_account_number: Option<u32>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub number: u32,
    pub email: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub usage_status: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub usage_age_seconds: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
    #[serde(default)]
    pub spend: Option<Spend>,
    #[serde(default)]
    pub scoped: Vec<Scoped>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub pct: f64,
    #[serde(default)]
    pub countdown: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub used: f64,
    pub limit: f64,
    #[allow(dead_code)]
    pub pct: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scoped {
    pub name: String,
    #[allow(dead_code)]
    pub pct: f64,
}

impl ListResponse {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl Account {
    /// The alias if one is set, otherwise the email address.
    pub fn display_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.email)
    }

    /// `cswap` reports `"ok"` for a usable account. Any other value, including
    /// a missing one, means we should not claim the account is fine.
    pub fn is_healthy(&self) -> bool {
        self.usage_status.as_deref() == Some("ok")
    }

    /// The raw status string when it is not `"ok"`, for display verbatim.
    ///
    /// We deliberately do not map these onto an enum: the full set of values
    /// `cswap` can emit is not documented, and inventing categories would
    /// hide states we have never seen.
    pub fn status_note(&self) -> Option<&str> {
        match self.usage_status.as_deref() {
            Some("ok") | None => None,
            Some(other) => Some(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = include_str!("../tests/fixtures/list_two_accounts.json");
    const MINIMAL: &str = include_str!("../tests/fixtures/list_minimal.json");
    const EMPTY: &str = include_str!("../tests/fixtures/list_empty.json");

    #[test]
    fn parses_two_accounts_with_full_usage() {
        let list = ListResponse::parse(TWO).expect("fixture should parse");
        assert_eq!(list.schema_version, 1);
        assert_eq!(list.active_account_number, Some(2));
        assert_eq!(list.accounts.len(), 2);

        let active = &list.accounts[1];
        assert_eq!(active.number, 2);
        assert!(active.active);
        assert!(!active.disabled);

        let usage = active.usage.as_ref().expect("active account has usage");
        assert_eq!(usage.five_hour.as_ref().unwrap().pct, 25.0);
        assert_eq!(
            usage.five_hour.as_ref().unwrap().countdown.as_deref(),
            Some("4h 14m")
        );
        assert_eq!(usage.seven_day.as_ref().unwrap().pct, 55.0);
        let spend = usage.spend.as_ref().expect("active account has spend");
        assert_eq!(spend.used, 90.62);
        assert_eq!(spend.limit, 200.0);
        assert_eq!(usage.scoped.len(), 1);
        assert_eq!(usage.scoped[0].name, "Fable");
    }

    #[test]
    fn five_hour_window_without_countdown_parses() {
        let list = ListResponse::parse(TWO).unwrap();
        let idle = &list.accounts[0];
        let five = idle.usage.as_ref().unwrap().five_hour.as_ref().unwrap();
        assert_eq!(five.pct, 0.0);
        assert_eq!(five.countdown, None);
    }

    #[test]
    fn account_without_usage_parses() {
        let list = ListResponse::parse(MINIMAL).unwrap();
        let account = &list.accounts[0];
        assert!(account.usage.is_none());
        assert!(account.disabled);
        assert_eq!(account.usage_age_seconds, None);
    }

    #[test]
    fn empty_account_list_parses() {
        let list = ListResponse::parse(EMPTY).unwrap();
        assert!(list.accounts.is_empty());
        assert_eq!(list.active_account_number, None);
    }

    #[test]
    fn display_name_prefers_alias_over_email() {
        let list = ListResponse::parse(MINIMAL).unwrap();
        assert_eq!(list.accounts[0].display_name(), "bot");

        let list = ListResponse::parse(TWO).unwrap();
        assert_eq!(list.accounts[0].display_name(), "work@example.com");
    }

    #[test]
    fn health_follows_usage_status_verbatim() {
        let ok = ListResponse::parse(TWO).unwrap();
        assert!(ok.accounts[0].is_healthy());
        assert_eq!(ok.accounts[0].status_note(), None);

        let limited = ListResponse::parse(MINIMAL).unwrap();
        assert!(!limited.accounts[0].is_healthy());
        assert_eq!(limited.accounts[0].status_note(), Some("rate_limited"));
    }

    #[test]
    fn missing_usage_status_counts_as_unhealthy_without_a_note() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true}]}"#;
        let list = ListResponse::parse(json).unwrap();
        assert!(!list.accounts[0].is_healthy());
        assert_eq!(list.accounts[0].status_note(), None);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"schemaVersion":2,"activeAccountNumber":1,"somethingNew":true,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok","brandNewField":[1,2,3]}]}"#;
        let list = ListResponse::parse(json).expect("forward-compatible parse");
        assert_eq!(list.schema_version, 2);
        assert_eq!(list.accounts.len(), 1);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(ListResponse::parse("{").is_err());
    }
}
