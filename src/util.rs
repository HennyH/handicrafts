//! Small helpers: money, Perth time, random tokens.

use rand::Rng;
use time::{OffsetDateTime, UtcOffset};

/// Perth is UTC+8 all year (no daylight saving).
pub const PERTH: UtcOffset = match UtcOffset::from_hms(8, 0, 0) {
    Ok(o) => o,
    Err(_) => panic!("bad offset"),
};

pub fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn perth(ts: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(ts)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .to_offset(PERTH)
}

/// "31 Oct 2026, 2:32 pm"
pub fn datetime(ts: i64) -> String {
    let t = perth(ts);
    let (h, ampm) = match t.hour() {
        0 => (12, "am"),
        h @ 1..=11 => (h, "am"),
        12 => (12, "pm"),
        h => (h - 12, "pm"),
    };
    format!(
        "{} {} {}, {h}:{:02} {ampm}",
        t.day(),
        month(t),
        t.year(),
        t.minute()
    )
}

/// "31 Oct 2026"
pub fn date(ts: i64) -> String {
    let t = perth(ts);
    format!("{} {} {}", t.day(), month(t), t.year())
}

fn month(t: OffsetDateTime) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][t.month() as usize - 1]
}

/// "RH-202610311432-57", the order number format the old PDF flow used.
pub fn order_number(ts: i64) -> String {
    let t = perth(ts);
    let n: u8 = rand::thread_rng().gen_range(10..100);
    format!(
        "RH-{:04}{:02}{:02}{:02}{:02}-{n}",
        t.year(),
        t.month() as u8,
        t.day(),
        t.hour(),
        t.minute()
    )
}

/// "$1,234.50"
pub fn aud(cents: i64) -> String {
    let neg = cents < 0;
    let c = cents.unsigned_abs();
    let dollars = (c / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{}${grouped}.{:02}", if neg { "-" } else { "" }, c % 100)
}

/// "105.00", for Monokulo's `amount`.
pub fn decimal(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

/// Unguessable token for pay links and CSRF.
pub fn token(len: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

/// The characters of bank references: no 0/O, 1/I/L or U, so nothing can be
/// misread when typed into a banking app.
pub const BANK_REF_ALPHABET: &str = "23456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn bank_ref() -> String {
    let a = BANK_REF_ALPHABET.as_bytes();
    let mut rng = rand::thread_rng();
    (0..5)
        .map(|_| a[rng.gen_range(0..a.len())] as char)
        .collect()
}

/// A reference as someone typed it: upper case, no spaces or dashes.
pub fn normalise_ref(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Percent-encode everything but unreserved characters (RFC 3986).
pub fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Cut to `max` characters, on a character boundary.
pub fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

/// A customer's name as it may be repeated back in an email: short, one line,
/// and with anything link-like removed, so the order form can't be used to
/// send a stranger a message.
pub fn safe_name(s: &str) -> String {
    let one_line: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let words: Vec<&str> = one_line
        .split_whitespace()
        .filter(|w| {
            let l = w.to_ascii_lowercase();
            !(l.contains("://")
                || l.starts_with("www.")
                || l.contains('@')
                || l.contains(".com")
                || l.contains(".net")
                || l.contains(".ru")
                || l.contains(".io"))
        })
        .collect();
    clip(&words.join(" "), 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money() {
        assert_eq!(aud(10500), "$105.00");
        assert_eq!(aud(123456789), "$1,234,567.89");
        assert_eq!(aud(5), "$0.05");
        assert_eq!(decimal(10500), "105.00");
        assert_eq!(decimal(7), "0.07");
    }

    #[test]
    fn perth_time() {
        // 2026-10-31 06:32 UTC is 14:32 in Perth.
        let ts = 1793428320;
        assert_eq!(datetime(ts), "31 Oct 2026, 2:32 pm");
        assert!(order_number(ts).starts_with("RH-202610311432-"));
    }

    #[test]
    fn refs() {
        let r = bank_ref();
        assert_eq!(r.len(), 5);
        assert!(r.chars().all(|c| BANK_REF_ALPHABET.contains(c)));
        assert_eq!(normalise_ref(" k7p-4q "), "K7P4Q");
    }

    #[test]
    fn names_are_defanged() {
        assert_eq!(safe_name("Sam Example"), "Sam Example");
        assert_eq!(
            safe_name("Win cash http://evil.example now"),
            "Win cash now"
        );
        assert_eq!(safe_name("visit www.spam.biz"), "visit");
        assert_eq!(safe_name(&"x".repeat(100)).len(), 60);
        assert_eq!(safe_name("Sam\nExample"), "Sam Example");
    }
}
