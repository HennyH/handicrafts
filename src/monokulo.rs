//! Monokulo: Monero payments. See docs.monokulo.com.
//!
//! The server makes each Monokulo order with the store's secret key, so a
//! customer can't change the price; the pay page only embeds Monokulo's
//! checkout for it. Only a signed `order.paid` / `order.overpaid` webhook
//! moves an order to Paid.

use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

use crate::config;

#[derive(Debug, Clone, Deserialize)]
pub struct MkOrder {
    pub order_id: String,
    #[serde(default)]
    pub xmr_amount_piconero: i64,
    #[serde(default)]
    pub expires_at: i64,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CreateError {
    /// Monokulo can't price the order: no AUD rate, unknown currency.
    Pricing(String),
    /// Anything else: unreachable, refused key, engine down.
    Other(String),
}

impl CreateError {
    pub fn detail(&self) -> &str {
        match self {
            CreateError::Pricing(d) | CreateError::Other(d) => d,
        }
    }
}

#[derive(Deserialize)]
struct Problem {
    #[serde(default)]
    detail: String,
    #[serde(default, rename = "type")]
    kind: String,
}

pub struct Client {
    http: reqwest::Client,
    cfg: config::Monokulo,
}

impl Client {
    pub fn new(cfg: config::Monokulo) -> Client {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(concat!("handicrafts/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client");
        Client { http, cfg }
    }

    pub fn cfg(&self) -> &config::Monokulo {
        &self.cfg
    }

    /// Make a Monokulo order for `amount_cents` AUD. The idempotency key
    /// makes a retry return the same order, never a second one.
    pub async fn create_order(
        &self,
        amount_cents: i64,
        merchant_order_id: &str,
        description: &str,
        idempotency_key: &str,
    ) -> Result<MkOrder, CreateError> {
        let body = serde_json::json!({
            "amount": crate::util::decimal(amount_cents),
            "currency": "AUD",
            "merchant_order_id": merchant_order_id,
            "description": crate::util::clip(description, 140),
            "idempotency_key": idempotency_key,
            "show_merchant_order_id": true,
        });
        let res = self
            .http
            .post(format!("{}/api/v1/orders", self.cfg.url))
            .bearer_auth(&self.cfg.secret_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| CreateError::Other(format!("couldn't reach Monokulo: {e}")))?;
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|e| CreateError::Other(format!("Monokulo's answer was cut off: {e}")))?;
        if status.is_success() {
            return serde_json::from_str(&text).map_err(|e| {
                CreateError::Other(format!("Monokulo's answer didn't make sense: {e}"))
            });
        }
        let problem: Problem = serde_json::from_str(&text).unwrap_or(Problem {
            detail: text.chars().take(200).collect(),
            kind: String::new(),
        });
        let detail = if problem.detail.is_empty() {
            format!("Monokulo answered {status}")
        } else {
            problem.detail
        };
        // "invalid-amount" (can't be priced) and "rates-unavailable" (no rate
        // for AUD right now) mean Monero can't be priced in AUD.
        if problem.kind.ends_with("/invalid-amount")
            || problem.kind.ends_with("/rates-unavailable")
            || detail.contains("exchange rate")
        {
            Err(CreateError::Pricing(detail))
        } else {
            Err(CreateError::Other(detail))
        }
    }
}

/// A webhook delivery, as far as the shop needs it.
#[derive(Debug, Clone, Deserialize)]
pub struct Event {
    pub event_id: String,
    pub event: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub merchant_order_id: Option<String>,
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub xmr_amount: Option<String>,
    #[serde(default)]
    pub fx_rate: Option<String>,
    #[serde(default)]
    pub fx_source: Option<String>,
    #[serde(default)]
    pub txid: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum VerifyError {
    MissingSignature,
    BadSignature,
    Stale,
    BadBody(String),
}

/// Check `X-Monokulo-Signature: t=<unix>,v1=<hex>`: HMAC-SHA256 of
/// `"<t>.<body>"` with the webhook's signing secret, over the exact bytes
/// received, compared in constant time, and no more than five minutes from
/// our clock.
pub fn verify(
    secret: &str,
    header: Option<&str>,
    body: &[u8],
    now: i64,
) -> Result<Event, VerifyError> {
    let header = header.ok_or(VerifyError::MissingSignature)?;
    let mut t = None;
    let mut sigs = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => t = v.parse::<i64>().ok(),
            Some(("v1", v)) => sigs.push(v.trim().to_string()),
            _ => {}
        }
    }
    let t = t.ok_or(VerifyError::MissingSignature)?;
    if sigs.is_empty() {
        return Err(VerifyError::MissingSignature);
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(t.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let expected = mac.finalize().into_bytes();
    let ok = sigs.iter().any(|s| match hex_decode(s) {
        Some(bytes) => subtle::ConstantTimeEq::ct_eq(bytes.as_slice(), expected.as_slice()).into(),
        None => false,
    });
    if !ok {
        return Err(VerifyError::BadSignature);
    }
    if (now - t).abs() > 300 {
        return Err(VerifyError::Stale);
    }
    serde_json::from_slice(body).map_err(|e| VerifyError::BadBody(e.to_string()))
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Sign like Monokulo does, for tests.
#[cfg(test)]
pub fn sign(secret: &str, t: i64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(format!("{t}.").as_bytes());
    mac.update(body);
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("t={t},v1={hex}")
}

/// "0.121916" from piconero (12 decimals), trimmed to 6 places.
pub fn xmr(piconero: i64) -> String {
    let whole = piconero / 1_000_000_000_000;
    let frac = (piconero % 1_000_000_000_000) / 1_000_000;
    format!("{whole}.{frac:06}")
}

/// Parse a decimal amount like "105.00" into cents.
pub fn cents(amount: &str) -> Option<i64> {
    let (whole, frac) = amount.trim().split_once('.').unwrap_or((amount.trim(), ""));
    let whole: i64 = whole.parse().ok()?;
    let frac = format!("{frac:0<2}");
    if frac.len() > 2 && frac[2..].chars().any(|c| c != '0') {
        return None;
    }
    let f: i64 = frac.get(..2)?.parse().ok()?;
    Some(whole * 100 + f)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = br#"{"api_version":2,"event_id":"evt_1","event":"order.paid","order_id":"order_1","merchant_order_id":"RH-1","amount":"105.00","currency":"AUD","xmr_amount":"0.121916000000"}"#;

    #[test]
    fn verifies_signed_bodies() {
        let h = sign("whsec", 1000, BODY);
        let ev = verify("whsec", Some(&h), BODY, 1100).unwrap();
        assert_eq!(ev.event, "order.paid");
        assert_eq!(ev.merchant_order_id.as_deref(), Some("RH-1"));
    }

    #[test]
    fn refuses_forgeries() {
        let h = sign("whsec", 1000, BODY);
        assert_eq!(
            verify("other", Some(&h), BODY, 1000).unwrap_err(),
            VerifyError::BadSignature
        );
        let mut tampered = BODY.to_vec();
        tampered[30] ^= 1;
        assert_eq!(
            verify("whsec", Some(&h), &tampered, 1000).unwrap_err(),
            VerifyError::BadSignature
        );
        assert_eq!(
            verify("whsec", Some(&h), BODY, 1000 + 301).unwrap_err(),
            VerifyError::Stale
        );
        assert_eq!(
            verify("whsec", None, BODY, 1000).unwrap_err(),
            VerifyError::MissingSignature
        );
        assert_eq!(
            verify("whsec", Some("t=1000,v1=zz"), BODY, 1000).unwrap_err(),
            VerifyError::BadSignature
        );
    }

    #[test]
    fn amounts() {
        assert_eq!(xmr(121_916_000_000), "0.121916");
        assert_eq!(xmr(1_500_000_000_000), "1.500000");
        assert_eq!(cents("105.00"), Some(10500));
        assert_eq!(cents("105"), Some(10500));
        assert_eq!(cents("105.5"), Some(10550));
        assert_eq!(cents("105.500000"), Some(10550));
        assert_eq!(cents("105.501"), None);
        assert_eq!(cents("x"), None);
    }
}
