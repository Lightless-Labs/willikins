//! Parsing a `Retry-After` response header: either a whole number of
//! seconds, or an HTTP-date (RFC 9110 section 10.2.3's `IMF-fixdate` form,
//! e.g. `Wed, 21 Oct 2015 07:28:00 GMT`, which is the form every provider
//! this workspace talks to actually sends). The two legacy HTTP-date
//! forms RFC 9110 also grandfathers in (RFC 850 dates and `asctime`) are
//! not accepted: no provider in this milestone's scope sends them, and
//! accepting a form nothing exercises is a latent bug waiting for a test.

use std::time::Duration;

/// Parse a `Retry-After` header value into a delay from *now*.
///
/// Returns `None` when `value` is neither a plain integer nor a
/// recognised `IMF-fixdate`. A date in the past yields
/// [`Duration::ZERO`] rather than `None`: the server asked for an
/// immediate retry, which is not a parse failure.
#[must_use]
pub fn parse(value: &str, now: std::time::SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let target = parse_imf_fixdate(value)?;
    Some(
        target
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .checked_sub(
                now.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO),
            )
            .unwrap_or(Duration::ZERO),
    )
}

/// Parse `"Wed, 21 Oct 2015 07:28:00 GMT"` into a [`std::time::SystemTime`].
fn parse_imf_fixdate(value: &str) -> Option<std::time::SystemTime> {
    // "<weekday>, <day> <month> <year> <hour>:<minute>:<second> GMT"
    let rest = value.split_once(", ")?.1;
    let mut parts = rest.split(' ');
    let day: u32 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    // `IMF-fixdate`'s grammar is exactly four digits, and holding to it is
    // what keeps `days_from_civil`'s arithmetic (and the seconds
    // multiplication below) inside `i64`: a header of `Sun, 01 Jan
    // 9223372036854775807 00:00:00 GMT` would otherwise overflow and
    // panic in a debug build, on nothing but a provider's say-so.
    let year_text = parts.next()?;
    if year_text.len() != 4 || !year_text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i64 = year_text.parse().ok()?;
    let time = parts.next()?;
    let gmt = parts.next()?;
    if gmt != "GMT" || parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() {
        return None;
    }
    if !(1..=31).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let epoch_seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    let epoch_seconds = u64::try_from(epoch_seconds).ok()?;
    Some(std::time::UNIX_EPOCH + Duration::from_secs(epoch_seconds))
}

fn month_number(name: &str) -> Option<u32> {
    Some(match name {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

/// Days since the Unix epoch (1970-01-01) for a proleptic Gregorian
/// civil date, Howard Hinnant's `days_from_civil` algorithm
/// (<https://howardhinnant.github.io/date_algorithms.html>), which is
/// exact for every year this function's `i64` can represent and needs no
/// external date library.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (i64::from(m) + 9) % 12; // [0, 11], Mar-based
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPOCH: std::time::SystemTime = std::time::UNIX_EPOCH;

    #[test]
    fn parses_plain_seconds() {
        assert_eq!(parse("1", EPOCH), Some(Duration::from_secs(1)));
        assert_eq!(parse(" 120 ", EPOCH), Some(Duration::from_secs(120)));
    }

    #[test]
    fn parses_imf_fixdate_relative_to_now() {
        // Exactly the Unix epoch instant, restated as an IMF-fixdate.
        let delay = parse("Thu, 01 Jan 1970 00:00:10 GMT", EPOCH).expect("parses");
        assert_eq!(delay, Duration::from_secs(10));
    }

    #[test]
    fn imf_fixdate_in_the_past_is_zero_not_none() {
        let now = EPOCH + Duration::from_secs(100);
        let delay = parse("Thu, 01 Jan 1970 00:00:10 GMT", now).expect("parses");
        assert_eq!(delay, Duration::ZERO);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse("not a date", EPOCH), None);
        assert_eq!(parse("", EPOCH), None);
    }

    #[test]
    fn rejects_a_year_outside_the_imf_fixdate_grammar() {
        // `IMF-fixdate` is four digits. Anything else - an absurd year a
        // provider could send to overflow the arithmetic in
        // `days_from_civil`, or a two-digit year - is refused, so the
        // caller falls back to its own backoff instead of panicking.
        for value in [
            "Sun, 01 Jan 9223372036854775807 00:00:00 GMT",
            "Thu, 01 Jan 292277026596 00:00:00 GMT",
            "Thu, 01 Jan 70 00:00:10 GMT",
            "Thu, 01 Jan 197 00:00:10 GMT",
            "Thu, 01 Jan 19700 00:00:10 GMT",
            "Thu, 01 Jan -970 00:00:10 GMT",
        ] {
            assert_eq!(parse(value, EPOCH), None, "value {value:?}");
        }
    }

    #[test]
    fn rejects_rfc850_form() {
        // RFC 850 form is deliberately not supported; see this module's docs.
        assert_eq!(parse("Thursday, 01-Jan-70 00:00:10 GMT", EPOCH), None);
    }

    #[test]
    fn days_from_civil_matches_known_epoch_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
    }
}
