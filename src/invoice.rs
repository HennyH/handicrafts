//! The Invoice button: an email draft from the template on /admin/settings,
//! downloaded as a .eml with `X-Unsent: 1`, which Outlook, Thunderbird and
//! Apple Mail open as an unsent draft. A mailto: link is offered too, for
//! phones.

use base64::Engine as _;
use minijinja::{Environment, UndefinedBehavior, context};

use crate::{catalog::PricedLine, db::Order, util};

pub const DEFAULT_SUBJECT: &str = "Invoice for order {{ order_number }}";

pub const DEFAULT_BODY: &str = "Hi {{ customer_name }},

Thanks so much for your order! Here's what I'll be making:

{{ items }}
Total: {{ total }} AUD

You can pay by Monero or bank transfer here:
{{ pay_link }}

If you pay by bank transfer, please use the reference {{ bank_ref }}.

Rachel
{{ business_name }}
";

pub const PLACEHOLDERS: [(&str, &str); 12] = [
    ("pay_link", "The customer's pay page. Required."),
    (
        "bank_ref",
        "The short reference for bank transfers, e.g. K7P4Q",
    ),
    ("order_number", "e.g. RH-202610311432-57"),
    ("order_date", "e.g. 31 Oct 2026"),
    ("customer_name", ""),
    ("customer_email", ""),
    ("customer_phone", ""),
    ("customer_address", "Delivery address, or \"local pickup\""),
    ("customer_notes", ""),
    ("items", "One line per item, with prices"),
    ("total", "e.g. $105.00"),
    ("business_name", "Rachel's Handicrafts"),
];

pub struct Draft {
    pub subject: String,
    pub body: String,
}

fn env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    env
}

fn sample_vars() -> minijinja::Value {
    context! {
        pay_link => "https://example/pay/x", bank_ref => "K7P4Q", order_number => "RH-1", order_date => "1 Jan 2026",
        customer_name => "Sam", customer_email => "sam@example.com", customer_phone => "", customer_address => "",
        customer_notes => "", items => "1 × Tote Bag", total => "$60.00", business_name => "Rachel's Handicrafts",
    }
}

/// Check a template before saving it: it must parse, use only known
/// placeholders, and (for the body) include `{{ pay_link }}`.
pub fn validate(subject: &str, body: &str) -> Result<(), String> {
    let env = env();
    if subject.trim().is_empty() {
        return Err("The subject can't be empty.".into());
    }
    if subject.contains('\n') {
        return Err("The subject must be one line.".into());
    }
    env.render_str(subject, sample_vars())
        .map_err(|e| format!("Subject: {}", describe(&e)))?;
    env.render_str(body, sample_vars())
        .map_err(|e| format!("Body: {}", describe(&e)))?;
    if !body.contains("pay_link") {
        return Err("The body needs {{ pay_link }}, or the customer has no way to pay.".into());
    }
    Ok(())
}

fn describe(e: &minijinja::Error) -> String {
    match e.kind() {
        minijinja::ErrorKind::UndefinedError => {
            "it uses a placeholder that doesn't exist. Check the spelling against the list below."
                .into()
        }
        _ => e.to_string(),
    }
}

pub fn items_text(lines: &[PricedLine]) -> String {
    lines
        .iter()
        .map(|l| {
            format!(
                "{} × {}    {}",
                l.qty,
                l.title(),
                util::aud(l.total_cents())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render(
    subject: &str,
    body: &str,
    order: &Order,
    lines: &[PricedLine],
    public_url: &str,
) -> Result<Draft, String> {
    let vars = context! {
        pay_link => format!("{public_url}/pay/{}", order.pay_token),
        bank_ref => order.bank_ref,
        order_number => order.number,
        order_date => util::date(order.created_at),
        customer_name => order.customer_name,
        customer_email => order.customer_email,
        customer_phone => order.customer_phone,
        customer_address => order.customer_address,
        customer_notes => order.customer_notes,
        items => items_text(lines),
        total => util::aud(order.total_cents),
        business_name => "Rachel's Handicrafts",
    };
    let env = env();
    Ok(Draft {
        subject: env
            .render_str(subject, vars.clone())
            .map_err(|e| describe(&e))?
            .trim()
            .to_string(),
        body: env.render_str(body, vars).map_err(|e| describe(&e))?,
    })
}

/// RFC 2047 for headers with non-ASCII text.
fn header_text(s: &str) -> String {
    if s.is_ascii() && !s.contains(['\r', '\n']) {
        s.to_string()
    } else {
        format!(
            "=?utf-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode(s.replace(['\r', '\n'], " "))
        )
    }
}

/// The draft as a .eml file.
pub fn eml(draft: &Draft, to_name: &str, to_addr: &str) -> String {
    let body = draft.body.replace("\r\n", "\n").replace('\n', "\r\n");
    let b64 = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
    let wrapped: Vec<&str> = b64
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let name = to_name.replace(['"', '\\'], "");
    format!(
        "X-Unsent: 1\r\nTo: {} <{}>\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n",
        header_text(&format!("\"{name}\"")),
        to_addr,
        header_text(&draft.subject),
        wrapped.join("\r\n")
    )
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn mailto(draft: &Draft, to_addr: &str) -> String {
    format!(
        "mailto:{}?subject={}&body={}",
        pct(to_addr),
        pct(&draft.subject),
        pct(&draft.body.replace('\n', "\r\n"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::Status;

    fn order() -> Order {
        Order {
            id: 1,
            number: "RH-202610311432-57".into(),
            bank_ref: "K7P4Q".into(),
            pay_token: "tok123".into(),
            status: Status::New,
            accepted: true,
            customer_name: "Zoë Example".into(),
            customer_email: "zoe@example.com".into(),
            customer_phone: String::new(),
            customer_address: "local pickup".into(),
            customer_notes: String::new(),
            total_cents: 10500,
            currency: "AUD".into(),
            refund_address: String::new(),
            created_at: 1793428320,
            updated_at: 0,
        }
    }

    #[test]
    fn default_template_is_valid_and_renders() {
        validate(DEFAULT_SUBJECT, DEFAULT_BODY).unwrap();
        let d = render(
            DEFAULT_SUBJECT,
            DEFAULT_BODY,
            &order(),
            &[],
            "https://shop.example",
        )
        .unwrap();
        assert_eq!(d.subject, "Invoice for order RH-202610311432-57");
        assert!(d.body.contains("https://shop.example/pay/tok123"));
        assert!(d.body.contains("K7P4Q"));
    }

    #[test]
    fn validation() {
        assert!(
            validate("Hi", "no link here")
                .unwrap_err()
                .contains("pay_link")
        );
        assert!(
            validate("Hi", "{{ pay_lnk }}")
                .unwrap_err()
                .contains("placeholder")
        );
        assert!(validate("Hi", "{{ pay_link").is_err());
        assert!(validate("", "{{ pay_link }}").is_err());
    }

    #[test]
    fn eml_is_an_unsent_draft() {
        let d = render(
            DEFAULT_SUBJECT,
            DEFAULT_BODY,
            &order(),
            &[],
            "https://shop.example",
        )
        .unwrap();
        let e = eml(&d, "Zoë Example", "zoe@example.com");
        assert!(e.starts_with("X-Unsent: 1\r\n"));
        assert!(e.contains("To: =?utf-8?B?"), "non-ASCII name encoded");
        assert!(e.contains("<zoe@example.com>"));
        let m = mailto(&d, "zoe@example.com");
        assert!(m.starts_with("mailto:zoe%40example.com?subject=Invoice%20for"));
    }
}
