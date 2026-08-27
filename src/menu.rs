use crate::model::{Account, ListResponse};

/// Windows caps `NOTIFYICONDATA::szTip` at 128 UTF-16 code units including the
/// terminator. We budget in characters, which is conservative enough.
pub const TOOLTIP_MAX: usize = 127;

/// How old `cswap`'s own usage numbers may be before we call them out rather
/// than showing them as if current.
///
/// This is deliberately not measured against our own fetch clock. Our clock
/// only advances on a successful `cswap list`, and every failure sets an error
/// that outranks this message — so a threshold on our clock could never fire.
/// `cswap` tells us how stale its cached usage is per account
/// (`usageAgeSeconds`), and that is the number spec section 8's
/// "사용량 조회 실패, 계정 목록은 유효" row is about: the account list came back
/// fine, the usage hanging off it did not.
pub const STALE_AFTER_SECS: u64 = 600;

/// Everything the menu needs, with no clock or process dependency, so the
/// whole conversion stays testable.
pub struct View<'a> {
    /// The most recent successful snapshot, if we ever got one.
    pub snapshot: Option<&'a ListResponse>,
    /// Seconds since that snapshot was taken. Drives the `Refresh now`
    /// freshness label only; staleness of the numbers themselves comes from
    /// `usageAgeSeconds` in the snapshot.
    pub age_secs: Option<u64>,
    /// A refresh error that is still current.
    pub error: Option<&'a str>,
    /// A one-shot message such as a failed switch. Outranks `error`.
    pub notice: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// Menu id, always `acct:<number>`.
    pub id: String,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// Disabled line shown above the account rows, when there is something to say.
    pub notice: Option<String>,
    pub accounts: Vec<AccountRow>,
    pub refresh_label: String,
    pub tooltip: String,
}

pub fn build(view: &View) -> MenuModel {
    let accounts = view
        .snapshot
        .map(|s| s.accounts.iter().map(account_row).collect())
        .unwrap_or_default();

    MenuModel {
        notice: notice_line(view),
        accounts,
        refresh_label: refresh_label(view.age_secs),
        tooltip: tooltip(view),
    }
}

/// Marks an account `cswap disable` has held out of automatic rotation.
///
/// Spec section 7 asks for such an account to be greyed out yet still
/// clickable. A native `CheckMenuItem` cannot be both — greying it is exactly
/// what makes it unclickable — so the marker is textual. "자동 제외" says what
/// `cswap disable` actually did; a bare "비활성" would collide with the other
/// meaning of inactive on this menu, which the radio check already carries.
const DISABLED_MARK: &str = "(자동 제외)";

fn account_row(account: &Account) -> AccountRow {
    let mut label = format!(
        "{}  {}   {}",
        account.number,
        account.display_name(),
        row_detail(account)
    );
    if account.disabled {
        label.push_str("  ");
        label.push_str(DISABLED_MARK);
    }
    AccountRow {
        id: format!("acct:{}", account.number),
        label,
        checked: account.active,
        // Disabled accounts are held out of auto-rotation but can still be
        // switched to by hand, so they stay clickable.
        enabled: true,
    }
}

fn row_detail(account: &Account) -> String {
    if let Some(note) = account.status_note() {
        return note.to_string();
    }
    let Some(usage) = account.usage.as_ref() else {
        return "사용량 없음".to_string();
    };
    let mut parts = Vec::new();
    if let Some(w) = usage.five_hour.as_ref() {
        parts.push(format!("5h {}%", round_pct(w.pct)));
    }
    if let Some(w) = usage.seven_day.as_ref() {
        parts.push(format!("7d {}%", round_pct(w.pct)));
    }
    if parts.is_empty() {
        "사용량 없음".to_string()
    } else {
        parts.join("  ")
    }
}

fn notice_line(view: &View) -> Option<String> {
    if let Some(notice) = view.notice {
        return Some(notice.to_string());
    }
    if let Some(error) = view.error {
        return Some(error.to_string());
    }
    let snapshot = view.snapshot?;
    if snapshot.accounts.is_empty() {
        return Some("등록된 계정 없음".to_string());
    }
    stale_usage_age(snapshot).map(|age| format!("사용량 정보 오래됨 ({})", ago(age)))
}

/// How old `cswap` says the active account's usage numbers are, but only once
/// they have crossed `STALE_AFTER_SECS`.
///
/// Reported per account; we speak for the active one, since that is the account
/// whose numbers the tooltip and the notice are describing.
fn stale_usage_age(snapshot: &ListResponse) -> Option<u64> {
    let account = snapshot.accounts.iter().find(|a| a.active)?;
    let secs = account.usage_age_seconds?;
    // A negative or non-finite age is cswap telling us something we cannot use;
    // saying nothing beats inventing a duration from it.
    if !secs.is_finite() || secs < STALE_AFTER_SECS as f64 {
        return None;
    }
    Some(secs as u64)
}

fn refresh_label(age_secs: Option<u64>) -> String {
    match age_secs {
        Some(age) => format!("Refresh now   · {} 갱신", ago(age)),
        None => "Refresh now".to_string(),
    }
}

/// `"방금"`, `"2분 전"`, `"2시간 전"`.
fn ago(secs: u64) -> String {
    if secs < 60 {
        "방금".to_string()
    } else if secs < 3600 {
        format!("{}분 전", secs / 60)
    } else {
        format!("{}시간 전", secs / 3600)
    }
}

/// Percentages arrive as floats but are always shown as whole numbers.
fn round_pct(pct: f64) -> i64 {
    pct.round() as i64
}

fn tooltip(view: &View) -> String {
    if let Some(error) = view.error {
        return truncate_lines(error);
    }
    let Some(snapshot) = view.snapshot else {
        return "Claude Swap Bar".to_string();
    };
    let Some(account) = snapshot.accounts.iter().find(|a| a.active) else {
        return "등록된 계정 없음".to_string();
    };

    let mut lines = Vec::new();
    let head = format!("{} (#{})", account.display_name(), account.number);
    lines.push(match stale_usage_age(snapshot) {
        Some(age) => format!("{head}  ({})", ago(age)),
        None => head,
    });

    if let Some(note) = account.status_note() {
        lines.push(note.to_string());
    }

    if let Some(usage) = account.usage.as_ref() {
        if let Some(w) = usage.five_hour.as_ref() {
            lines.push(window_line("5h", w.pct, w.countdown.as_deref()));
        }
        if let Some(w) = usage.seven_day.as_ref() {
            lines.push(window_line("7d", w.pct, w.countdown.as_deref()));
        }
        if let Some(s) = usage.spend.as_ref() {
            lines.push(format!(
                "$   {:.2} / {}  ({}%)",
                s.used,
                trim_float(s.limit),
                round_pct(s.pct)
            ));
        }
        for scoped in usage.scoped.iter().filter(|s| s.pct > 0.0) {
            lines.push(format!("{}  {}%", scoped.name, round_pct(scoped.pct)));
        }
    }

    truncate_lines(&lines.join("\n"))
}

fn window_line(label: &str, pct: f64, countdown: Option<&str>) -> String {
    let head = format!("{label}  {:>2}%", round_pct(pct));
    match countdown {
        Some(c) => format!("{head}   리셋 {c}"),
        None => head,
    }
}

/// `200.0` renders as `200`, `12.5` as `12.5`.
fn trim_float(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Drop whole trailing lines until the text fits `TOOLTIP_MAX`.
fn truncate_lines(text: &str) -> String {
    if text.chars().count() <= TOOLTIP_MAX {
        return text.to_string();
    }
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.len() > 1 {
        lines.pop();
        let candidate = lines.join("\n");
        if candidate.chars().count() <= TOOLTIP_MAX {
            return candidate;
        }
    }
    // A single line that is still too long: cut it at the character boundary.
    text.chars().take(TOOLTIP_MAX).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ListResponse;

    const TWO: &str = include_str!("../tests/fixtures/list_two_accounts.json");
    const MINIMAL: &str = include_str!("../tests/fixtures/list_minimal.json");
    const EMPTY: &str = include_str!("../tests/fixtures/list_empty.json");

    /// Spec section 8's "사용량 조회 실패, 계정 목록은 유효" row: cswap answered
    /// with a perfectly good account list, but the usage numbers hanging off it
    /// are twelve minutes old because cswap could not refresh them.
    const STALE_USAGE: &str = r#"{"schemaVersion":1,"activeAccountNumber":2,
        "accounts":[
          {"number":1,"email":"work@example.com","active":false,
           "usageStatus":"ok","usageAgeSeconds":720.0,
           "usage":{"fiveHour":{"pct":0.0},"sevenDay":{"pct":12.0}}},
          {"number":2,"email":"personal@example.com","active":true,
           "usageStatus":"ok","usageAgeSeconds":720.0,
           "usage":{"fiveHour":{"pct":25.0},"sevenDay":{"pct":55.0}}}]}"#;

    fn view_of(json: &str) -> ListResponse {
        ListResponse::parse(json).unwrap()
    }

    #[test]
    fn account_rows_carry_ids_labels_and_the_active_check() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });

        assert_eq!(model.accounts.len(), 2);
        assert_eq!(model.accounts[0].id, "acct:1");
        assert_eq!(model.accounts[0].label, "1  work@example.com   5h 0%  7d 12%");
        assert!(!model.accounts[0].checked);
        assert!(model.accounts[0].enabled);

        assert_eq!(model.accounts[1].id, "acct:2");
        assert_eq!(
            model.accounts[1].label,
            "2  personal@example.com   5h 25%  7d 55%"
        );
        assert!(model.accounts[1].checked);
    }

    #[test]
    fn an_alias_replaces_the_email_and_a_bad_status_is_appended() {
        let list = view_of(MINIMAL);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(model.accounts[0].label, "3  bot   rate_limited  (자동 제외)");
        assert!(model.accounts[0].checked);
    }

    #[test]
    fn a_disabled_account_is_marked_yet_stays_clickable() {
        // MINIMAL's only account carries `"disabled": true`.
        let list = view_of(MINIMAL);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(
            model.accounts[0].label.ends_with("(자동 제외)"),
            "label was {:?}",
            model.accounts[0].label
        );
        assert!(
            model.accounts[0].enabled,
            "spec 7: a disabled account is excluded from rotation, not from the menu"
        );

        // An account cswap has not disabled carries no marker.
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(!model.accounts[0].label.contains("자동 제외"));
        assert!(!model.accounts[1].label.contains("자동 제외"));
    }

    #[test]
    fn an_account_with_no_usage_shows_a_dash() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok"}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(model.accounts[0].label, "1  a@example.com   사용량 없음");
    }

    #[test]
    fn zero_accounts_produce_no_rows_and_a_notice() {
        let list = view_of(EMPTY);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(model.accounts.is_empty());
        assert_eq!(model.notice.as_deref(), Some("등록된 계정 없음"));
    }

    #[test]
    fn an_error_takes_priority_over_the_snapshot_for_the_notice() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: Some("갱신 실패: boom"),
            notice: None,
        });
        assert_eq!(model.notice.as_deref(), Some("갱신 실패: boom"));
        assert_eq!(model.accounts.len(), 2, "rows survive a failed refresh");
    }

    #[test]
    fn a_transient_notice_outranks_an_error() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: Some("갱신 실패: boom"),
            notice: Some("전환 실패: 잠김"),
        });
        assert_eq!(model.notice.as_deref(), Some("전환 실패: 잠김"));
    }

    #[test]
    fn usage_cswap_reports_as_stale_is_called_out_in_the_notice() {
        let list = view_of(STALE_USAGE);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(3),
            error: None,
            notice: None,
        });
        assert_eq!(
            model.notice.as_deref(),
            Some("사용량 정보 오래됨 (12분 전)")
        );
        assert_eq!(
            model.refresh_label, "Refresh now   · 방금 갱신",
            "our own fetch is seconds old even though cswap's numbers are not"
        );
    }

    #[test]
    fn our_own_fetch_age_does_not_make_current_usage_look_stale() {
        // The active account in TWO has `usageAgeSeconds: 1.6`. cswap's numbers
        // are current; only our clock since the last successful list is old.
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(720),
            error: None,
            notice: None,
        });
        assert_eq!(model.notice, None);
        assert_eq!(model.refresh_label, "Refresh now   · 12분 전 갱신");
        assert!(!model.tooltip.contains("12분 전"));
    }

    #[test]
    fn usage_age_missing_altogether_is_not_reported_as_stale() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok","usage":{"fiveHour":{"pct":3.0}}}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(999_999),
            error: None,
            notice: None,
        });
        assert_eq!(model.notice, None);
    }

    #[test]
    fn the_refresh_label_reports_the_last_successful_update() {
        let list = view_of(TWO);
        let at = |secs| {
            build(&View {
                snapshot: Some(&list),
                age_secs: Some(secs),
                error: None,
                notice: None,
            })
            .refresh_label
        };
        assert_eq!(at(5), "Refresh now   · 방금 갱신");
        assert_eq!(at(120), "Refresh now   · 2분 전 갱신");
        assert_eq!(at(7200), "Refresh now   · 2시간 전 갱신");

        let never = build(&View {
            snapshot: None,
            age_secs: None,
            error: None,
            notice: None,
        });
        assert_eq!(never.refresh_label, "Refresh now");
    }

    #[test]
    fn the_tooltip_describes_the_active_account() {
        let list = view_of(TWO);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(
            model.tooltip,
            "personal@example.com (#2)\n\
             5h  25%   리셋 4h 14m\n\
             7d  55%   리셋 2d 4h\n\
             $   90.62 / 200  (45%)"
        );
    }

    #[test]
    fn the_tooltip_omits_the_spend_line_when_there_is_no_spend() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok","usage":{"fiveHour":{"pct":3.0},
            "sevenDay":{"pct":8.0,"countdown":"5d 1h"}}}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(
            model.tooltip,
            "a@example.com (#1)\n5h   3%\n7d   8%   리셋 5d 1h"
        );
    }

    #[test]
    fn the_tooltip_lists_non_zero_per_model_limits() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,"email":"a@example.com","active":true,
            "usageStatus":"ok","usage":{"fiveHour":{"pct":3.0},
            "scoped":[{"name":"Fable","pct":0.0},{"name":"Opus","pct":42.0}]}}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(model.tooltip.contains("Opus  42%"));
        assert!(!model.tooltip.contains("Fable"), "0% entries are dropped");
    }

    #[test]
    fn the_tooltip_reports_errors_instead_of_stale_numbers() {
        let model = build(&View {
            snapshot: None,
            age_secs: None,
            error: Some("cswap을 찾을 수 없음"),
            notice: None,
        });
        assert_eq!(model.tooltip, "cswap을 찾을 수 없음");
    }

    #[test]
    fn the_tooltip_marks_usage_cswap_reports_as_stale() {
        let list = view_of(STALE_USAGE);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(3),
            error: None,
            notice: None,
        });
        assert!(model.tooltip.starts_with("personal@example.com (#2)"));
        assert!(model.tooltip.contains("(12분 전)"));
    }

    #[test]
    fn the_tooltip_drops_whole_trailing_lines_to_fit_the_cap() {
        let json = r#"{"schemaVersion":1,"activeAccountNumber":1,
            "accounts":[{"number":1,
            "email":"a-very-long-address-for-testing-truncation@example.com",
            "active":true,"usageStatus":"ok","usage":{
            "fiveHour":{"pct":3.0,"countdown":"4h 14m"},
            "sevenDay":{"pct":8.0,"countdown":"5d 1h"},
            "spend":{"used":90.62,"limit":200.0,"pct":45.31},
            "scoped":[{"name":"Opus","pct":42.0},{"name":"Sonnet","pct":31.0}]}}]}"#;
        let list = view_of(json);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert!(
            model.tooltip.chars().count() <= TOOLTIP_MAX,
            "tooltip was {} chars: {:?}",
            model.tooltip.chars().count(),
            model.tooltip
        );
        assert!(model.tooltip.starts_with(
            "a-very-long-address-for-testing-truncation@example.com (#1)"
        ));
        assert!(!model.tooltip.ends_with('\n'));
    }

    #[test]
    fn a_snapshot_with_no_active_account_still_produces_a_tooltip() {
        let list = view_of(EMPTY);
        let model = build(&View {
            snapshot: Some(&list),
            age_secs: Some(0),
            error: None,
            notice: None,
        });
        assert_eq!(model.tooltip, "등록된 계정 없음");
    }
}
