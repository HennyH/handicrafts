//! `POST /webhooks/monokulo`: Monokulo's signed order events. The only
//! thing that moves an order to Paid by itself.

use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::State;
use crate::{
    db::{self, Mail},
    emails,
    monokulo::{self, VerifyError},
    orders::{self, Status},
    util,
};

pub async fn monokulo(
    axum::extract::State(app): State,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(cfg) = app.monokulo().map(|c| c.cfg().clone()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let sig = headers
        .get("x-monokulo-signature")
        .and_then(|v| v.to_str().ok());
    let event = match monokulo::verify(&cfg.webhook_secret, sig, &body, util::now()) {
        Ok(e) => e,
        Err(VerifyError::BadBody(e)) => {
            eprintln!("handicrafts: Monokulo webhook with a body that didn't parse: {e}");
            return StatusCode::BAD_REQUEST.into_response();
        }
        Err(e) => {
            eprintln!("handicrafts: Monokulo webhook refused: {e:?}");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    if event.event == "webhook.test" {
        // From "Send a test webhook": shown on /admin/monokulo.
        let _ = app
            .db
            .set_setting("monokulo_last_test", util::now().to_string())
            .await;
        return StatusCode::OK.into_response();
    }
    let Some(number) = event.merchant_order_id.clone() else {
        // Not one of ours (a test delivery, or an order made elsewhere).
        return StatusCode::OK.into_response();
    };
    let admin = app.admin_email().map(str::to_string);
    let public_url = app.cfg.public_url.clone();
    let event_id = event.event_id.clone();
    let res = app
        .db
        .webhook_once(event_id, move |tx| {
            let Some(order) = db::get_order_by_number(tx, &number)? else {
                eprintln!("handicrafts: Monokulo webhook for unknown order {number}");
                return Ok(());
            };
            if !event.order_id.is_empty() {
                let status = event.event.trim_start_matches("order.");
                db::set_monokulo_status(tx, &event.order_id, status)?;
            }
            for (kind, actor, body, change, mail) in
                decide(&order, &event, admin.as_deref(), &public_url)
            {
                db::event_tx(tx, order.id, kind, actor, &body, None)?;
                if let Some(to) = change {
                    db::change_status_tx(tx, &order, to, true, "webhook")?;
                }
                if let Some(m) = mail {
                    db::queue_tx(tx, Some(order.id), &m)?;
                }
            }
            Ok(())
        })
        .await;
    match res {
        Ok(_) => {
            app.outbox.notify_one();
            StatusCode::OK.into_response()
        }
        Err(e) => {
            // Monokulo retries anything that isn't a 2xx.
            eprintln!("handicrafts: Monokulo webhook: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

type Step = (
    &'static str,
    &'static str,
    String,
    Option<Status>,
    Option<Mail>,
);

/// What one event means for the order: timeline entries, at most one status
/// change, and emails for the admin.
pub fn decide(
    order: &db::Order,
    ev: &monokulo::Event,
    admin: Option<&str>,
    public_url: &str,
) -> Vec<Step> {
    let link = Some(format!("{public_url}/admin/orders/{}", order.id));
    let xmr = ev
        .xmr_amount
        .as_deref()
        .map(|x| x.trim_end_matches('0').trim_end_matches('.').to_string())
        .unwrap_or_default();
    let refund = if order.refund_address.is_empty() {
        "not given".to_string()
    } else {
        order.refund_address.clone()
    };
    let notice = |subject: String, headline: &str, body: &str, warning: bool| {
        admin.map(|a| {
            emails::admin_notice(
                "admin_payment",
                a,
                subject,
                headline,
                body,
                link.clone(),
                warning,
            )
        })
    };

    match ev.event.as_str() {
        "order.paid" | "order.overpaid" => {
            let paid_cents = ev.amount.as_deref().and_then(monokulo::cents);
            let matches =
                ev.currency.as_deref() == Some("AUD") && paid_cents == Some(order.total_cents);
            let rate = match (&ev.fx_rate, &ev.fx_source) {
                (Some(r), Some(s)) => format!(" at {r} AUD/XMR ({s})"),
                (Some(r), None) => format!(" at {r} AUD/XMR"),
                _ => String::new(),
            };
            let mut body = format!(
                "Monero payment confirmed by Monokulo: {xmr} XMR for {} {}{rate}. Monokulo order {}, event {}. Refund address: {refund}.",
                ev.amount.as_deref().unwrap_or("?"),
                ev.currency.as_deref().unwrap_or("?"),
                ev.order_id,
                ev.event_id
            );
            if ev.event == "order.overpaid" {
                body.push_str(" The customer sent more than asked, so a part refund may be due.");
            }
            if !matches {
                let warn = format!(
                    "{body}\n\nThe paid price doesn't match this order's total of {}, so the order was not marked Paid. Check it in Monokulo.",
                    util::aud(order.total_cents)
                );
                let mail = notice(
                    format!("Check the Monero payment for {}", order.number),
                    "A Monero payment doesn't match its order",
                    &warn,
                    true,
                );
                return vec![("warning", "webhook", warn, None, mail)];
            }
            if orders::webhook_pays(order.status) {
                let mail = notice(
                    format!("Order {} paid (Monero)", order.number),
                    &format!("Order {} is paid", order.number),
                    &body,
                    ev.event == "order.overpaid",
                );
                vec![("payment", "webhook", body, Some(Status::Paid), mail)]
            } else {
                let body = format!(
                    "{body} The order was already {}, so its status was left alone.",
                    order.status.label()
                );
                let mail = notice(
                    format!(
                        "Monero payment for {} ({})",
                        order.number,
                        order.status.label()
                    ),
                    "A Monero payment arrived",
                    &body,
                    true,
                );
                vec![("payment", "webhook", body, None, mail)]
            }
        }
        "order.partial" => {
            let body = format!(
                "Part of the Monero payment has arrived (Monokulo order {}). Not marked Paid.",
                ev.order_id
            );
            let mail = notice(
                format!("Order {} part paid (Monero)", order.number),
                "Part of a Monero payment arrived",
                &body,
                true,
            );
            vec![("payment", "webhook", body, None, mail)]
        }
        "order.unconfirmed" => {
            vec![(
                "payment",
                "webhook",
                format!(
                    "Monero payment seen on the network, waiting for confirmations (Monokulo order {}).",
                    ev.order_id
                ),
                None,
                None,
            )]
        }
        "order.expired" => {
            vec![(
                "system",
                "webhook",
                format!(
                    "Monokulo order {} expired unpaid. The pay link makes a new one at the current rate.",
                    ev.order_id
                ),
                None,
                None,
            )]
        }
        "order.cancelled" => vec![(
            "system",
            "webhook",
            format!("Monokulo order {} was cancelled.", ev.order_id),
            None,
            None,
        )],
        "order.double_spend_detected" => {
            let body = format!(
                "Monokulo saw a double spend on this order's payment (transaction {}). The money may not arrive. Status left alone; decide what to do.",
                ev.txid.as_deref().unwrap_or("?")
            );
            let mail = notice(
                format!("Double spend on {}", order.number),
                "A Monero payment may not arrive",
                &body,
                true,
            );
            vec![("warning", "webhook", body, None, mail)]
        }
        "order.double_spend_reversed" => {
            vec![(
                "payment",
                "webhook",
                format!(
                    "The double spend warning has cleared (transaction {}).",
                    ev.txid.as_deref().unwrap_or("?")
                ),
                None,
                None,
            )]
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(status: Status) -> db::Order {
        db::Order {
            id: 7,
            number: "RH-1".into(),
            bank_ref: "K7P4Q".into(),
            pay_token: "t".into(),
            status,
            accepted: true,
            customer_name: "Sam".into(),
            customer_email: "s@example.com".into(),
            customer_phone: String::new(),
            customer_address: String::new(),
            customer_notes: String::new(),
            total_cents: 10500,
            currency: "AUD".into(),
            refund_address: "4refund".into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn ev(event: &str, amount: &str) -> monokulo::Event {
        serde_json::from_value(serde_json::json!({
            "event_id": "evt_1", "event": event, "order_id": "order_1", "merchant_order_id": "RH-1",
            "amount": amount, "currency": "AUD", "xmr_amount": "0.121916000000", "fx_rate": "861.26", "fx_source": "haveno"
        }))
        .unwrap()
    }

    #[test]
    fn paid_moves_invoiced_to_paid_with_refund_address() {
        let steps = decide(
            &order(Status::Invoiced),
            &ev("order.paid", "105.00"),
            Some("a@example.com"),
            "https://s",
        );
        assert_eq!(steps.len(), 1);
        let (_, _, body, change, mail) = &steps[0];
        assert_eq!(*change, Some(Status::Paid));
        assert!(
            body.contains("0.121916 XMR") && body.contains("4refund"),
            "{body}"
        );
        assert!(mail.is_some());
    }

    #[test]
    fn mismatched_amount_never_pays() {
        let steps = decide(
            &order(Status::Invoiced),
            &ev("order.paid", "1.00"),
            None,
            "https://s",
        );
        assert_eq!(steps[0].0, "warning");
        assert_eq!(steps[0].3, None);
    }

    #[test]
    fn already_paid_or_rejected_is_left_alone() {
        for s in [Status::Paid, Status::Rejected, Status::Dispatched] {
            assert_eq!(
                decide(&order(s), &ev("order.paid", "105.00"), None, "https://s")[0].3,
                None
            );
        }
    }

    #[test]
    fn other_events_never_change_status() {
        for e in [
            "order.partial",
            "order.unconfirmed",
            "order.expired",
            "order.double_spend_detected",
            "order.confirming",
            "weird",
        ] {
            assert!(
                decide(
                    &order(Status::Invoiced),
                    &ev(e, "105.00"),
                    None,
                    "https://s"
                )
                .iter()
                .all(|s| s.3.is_none())
            );
        }
    }
}
