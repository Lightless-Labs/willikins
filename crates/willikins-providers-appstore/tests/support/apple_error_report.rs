//! What a live harness STOP may say about a failed App Store Connect call.
//!
//! Included by `#[path]` into `tests/live_write_cycle.rs` (the live
//! harness, compiled only with the `live-tests` feature) and into
//! `tests/redaction.rs` (always compiled), so the tests below run in every
//! ordinary gate and not only on the rare live run.
//!
//! Added 2026-09-29, after `appstore_live_profile_replace_cycle` stopped at
//! its capability enable with nothing but `Provider: <withheld>`: Apple's
//! answer was withheld by design, and the STOP could not say why. Apple's
//! error body is JSON:API, `{"errors": [{"id", "status", "code", "title",
//! "detail", "source", "meta"}]}`. Only `code` (a dotted upper-case enum
//! such as `ENTITY_ERROR.ATTRIBUTE.INVALID`) and `title` (a generic
//! sentence per code) are reported. `detail` is **never** read: it is the
//! one field that quotes the request back, and on this account that can
//! mean a certificate or profile id. Nothing else in the body is read
//! either.

/// The most `errors[]` entries one summary names.
pub const MAX_ERRORS_REPORTED: usize = 5;

/// The most characters of one `title` a summary keeps.
pub const MAX_TITLE_CHARS: usize = 120;

/// `status N; errors: CODE "title"; CODE "title"`, from Apple's error body:
/// every `errors[].code` and `errors[].title`, never `errors[].detail`. A
/// code that is not the dotted enum shape Apple documents is replaced
/// rather than echoed; a title is escaped onto one line and bounded. A body
/// that is not JSON, or has no `errors` array, degrades to the status.
pub fn apple_error_summary(status: u16, body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let Some(errors) = parsed
        .as_ref()
        .and_then(|value| value.get("errors"))
        .and_then(serde_json::Value::as_array)
        .filter(|errors| !errors.is_empty())
    else {
        return format!("status {status}; no errors[] in the body");
    };
    let named: Vec<String> = errors
        .iter()
        .take(MAX_ERRORS_REPORTED)
        .map(|error| {
            let code = match error.get("code").and_then(serde_json::Value::as_str) {
                None => "<no code>".to_string(),
                Some(code) if is_code_shape(code) => code.to_string(),
                Some(_) => "<unexpected code shape>".to_string(),
            };
            let title = error
                .get("title")
                .and_then(serde_json::Value::as_str)
                .map_or_else(
                    || "<no title>".to_string(),
                    |title| format!("\"{}\"", one_line(title)),
                );
            format!("{code} {title}")
        })
        .collect();
    let summary = format!("status {status}; errors: {}", named.join("; "));
    match errors.len().saturating_sub(MAX_ERRORS_REPORTED) {
        0 => summary,
        more => format!("{summary}; and {more} more"),
    }
}

/// Apple's documented code shape: dotted upper-case words
/// (`ENTITY_ERROR.ATTRIBUTE.INVALID`), kept loose enough for digits,
/// dashes and lower case, never a space or punctuation that could carry a
/// sentence.
fn is_code_shape(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 100
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// At most [`MAX_TITLE_CHARS`] characters of `text`, with every control
/// character, quote and backslash escaped, so a title stays on one line
/// and inside its own quotes.
fn one_line(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().take(MAX_TITLE_CHARS) {
        if c.is_control() || matches!(c, '"' | '\\') {
            out.extend(c.escape_debug());
        } else {
            out.push(c);
        }
    }
    out
}

/// The part of a `ToolError`'s message a STOP may print: only messages
/// willikins builds itself from nothing the provider wrote -- the two
/// fixed `401`/`403` texts, `provider returned status N` (what
/// `willikins-providers-http` says when the body has no top-level
/// `message`, which Apple's never has), and the fixed parse-position text.
/// Everything else is withheld, since it may quote provider text.
pub fn stop_message(message: &str) -> &str {
    let fixed = [
        willikins_providers_http::UNAUTHENTICATED,
        willikins_providers_http::MISSING_PERMISSION,
    ];
    let status = regex::Regex::new(r"^provider returned status [0-9]{3}$").expect("a valid regex");
    let parse = regex::Regex::new(
        r"^could not parse the response body as the expected shape \(line [0-9]+, column [0-9]+\)$",
    )
    .expect("a valid regex");
    if fixed.contains(&message) || status.is_match(message) || parse.is_match(message) {
        message
    } else {
        WITHHELD
    }
}

/// What [`stop_message`] prints in place of a message it will not repeat.
pub const WITHHELD: &str = "<withheld: may quote provider text>";

#[cfg(test)]
mod apple_error_report_tests {
    use super::*;

    const SENTINEL: &str = "SENTINEL-DETAIL-6f1c";

    #[test]
    fn summary_names_status_code_and_title_and_never_detail() {
        let body = serde_json::json!({
            "errors": [{
                "id": format!("id-{SENTINEL}"),
                "status": "409",
                "code": "ENTITY_ERROR.ATTRIBUTE.INVALID",
                "title": "An attribute value is invalid.",
                "detail": format!("The value {SENTINEL} is not allowed"),
                "source": {"pointer": format!("/data/{SENTINEL}")},
                "meta": {"associatedErrors": SENTINEL},
            }]
        })
        .to_string();
        let summary = apple_error_summary(409, &body);
        assert_eq!(
            summary,
            "status 409; errors: ENTITY_ERROR.ATTRIBUTE.INVALID \"An attribute value is invalid.\""
        );
        assert!(!summary.contains(SENTINEL));
    }

    #[test]
    fn summary_names_every_error_up_to_the_bound() {
        let errors: Vec<_> = (0..MAX_ERRORS_REPORTED + 2)
            .map(|n| serde_json::json!({"code": format!("CODE_{n}"), "title": format!("t{n}")}))
            .collect();
        let body = serde_json::json!({ "errors": errors }).to_string();
        let summary = apple_error_summary(422, &body);
        assert_eq!(
            summary,
            "status 422; errors: CODE_0 \"t0\"; CODE_1 \"t1\"; CODE_2 \"t2\"; CODE_3 \"t3\"; \
             CODE_4 \"t4\"; and 2 more"
        );
    }

    #[test]
    fn summary_of_a_body_that_is_not_json_is_the_status_alone() {
        assert_eq!(
            apple_error_summary(502, &format!("<html>{SENTINEL}</html>")),
            "status 502; no errors[] in the body"
        );
    }

    #[test]
    fn summary_of_json_without_an_errors_array_is_the_status_alone() {
        let body = serde_json::json!({"message": SENTINEL}).to_string();
        assert_eq!(
            apple_error_summary(400, &body),
            "status 400; no errors[] in the body"
        );
    }

    #[test]
    fn summary_replaces_a_code_that_is_not_the_documented_enum_shape() {
        let body = serde_json::json!({
            "errors": [{"code": format!("not a code {SENTINEL}"), "title": "x"}]
        })
        .to_string();
        let summary = apple_error_summary(409, &body);
        assert_eq!(summary, "status 409; errors: <unexpected code shape> \"x\"");
    }

    #[test]
    fn summary_marks_a_missing_code_or_title() {
        let body = serde_json::json!({"errors": [{"detail": SENTINEL}]}).to_string();
        assert_eq!(
            apple_error_summary(409, &body),
            "status 409; errors: <no code> <no title>"
        );
    }

    #[test]
    fn summary_escapes_a_title_onto_one_line_and_bounds_it() {
        let long = "a".repeat(MAX_TITLE_CHARS + 50);
        let body = serde_json::json!({
            "errors": [
                {"code": "A", "title": "line one\nline two"},
                {"code": "B", "title": long},
            ]
        })
        .to_string();
        let summary = apple_error_summary(409, &body);
        assert!(!summary.contains('\n'));
        assert!(summary.contains("A \"line one\\nline two\""));
        assert!(summary.contains(&format!("B \"{}\"", "a".repeat(MAX_TITLE_CHARS))));
        assert!(!summary.contains(&"a".repeat(MAX_TITLE_CHARS + 1)));
    }

    #[test]
    fn stop_message_repeats_only_what_willikins_wrote_itself() {
        for fixed in [
            willikins_providers_http::UNAUTHENTICATED,
            willikins_providers_http::MISSING_PERMISSION,
            "provider returned status 409",
            "provider returned status 500",
            "could not parse the response body as the expected shape (line 1, column 42)",
        ] {
            assert_eq!(stop_message(fixed), fixed);
        }
    }

    #[test]
    fn stop_message_withholds_anything_that_may_quote_the_provider() {
        for quoting in [
            "provider says: the profile 1234 is invalid",
            "provider returned status 409: extra",
            "xprovider returned status 409",
            "provider returned status 4090",
            "provider returned status ",
            "could not parse the response body as the expected shape (line 1, column 42) x",
            "App Store Connect returned a malformed bundle id id: bad",
            "",
        ] {
            assert_eq!(stop_message(quoting), WITHHELD, "{quoting:?}");
        }
    }
}
