//! The customer's pay page, reached only through the secret link in the
//! invoice.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Form, Json,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::{App, State, html, is_fixi, ok, oops, see_other};
use crate::{
    db::{MonokuloOrder, Order},
    emails,
    monokulo::CreateError,
    orders::Status,
    util,
    views::{
        checkout::{self, Method, MoneroState, PayView},
        shop,
    },
};

async fn find(app: &App, token: &str) -> Result<Order, Response> {
    if token.len() != 24 || !token.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(not_found());
    }
    match app.db.order_by_token(token.to_string()).await {
        Ok(Some(o)) => Ok(o),
        Ok(None) => Err(not_found()),
        Err(e) => Err(oops(e)),
    }
}

fn not_found() -> Response {
    html(
        StatusCode::NOT_FOUND,
        shop::problem(
            "This pay link doesn't work",
            "Please check you copied the whole link from Rachel's email.",
        ),
    )
}

pub async fn page(
    axum::extract::State(app): State,
    Path(token): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let order = match find(&app, &token).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let lines = match app.db.items(order.id).await {
        Ok(l) => l,
        Err(e) => return oops(e),
    };
    let requested = Method::parse(q.get("method").map(String::as_str));

    let mut mk_row: Option<MonokuloOrder> = None;
    let mut failure: Option<MoneroState<'static>> = None;
    let bank = app.cfg.bank.any().then_some(&app.cfg.bank);
    let method = match (requested, &app.monokulo, bank) {
        (Some(Method::Bank), _, Some(_)) => Method::Bank,
        (_, Some(_), _) => Method::Monero,
        _ => Method::Bank,
    };
    if order.status == Status::Invoiced && method == Method::Monero {
        match ensure_monokulo_order(&app, &order).await {
            Ok(mk) => mk_row = Some(mk),
            Err(state) => failure = Some(state),
        }
    }
    let view = match order.status {
        Status::New => PayView::NotInvoiced,
        Status::Rejected => PayView::Cancelled,
        s if checkout::paid_status(s) => PayView::Paid,
        _ => {
            let monero = app.monokulo.as_ref().map(|c| match (&mk_row, failure) {
                (Some(mk), _) => MoneroState::Checkout { mk, cfg: c.cfg() },
                (None, Some(f)) => f,
                // Bank chosen: Monero is offered but not started.
                (None, None) => MoneroState::NotStarted,
            });
            PayView::Choose {
                method,
                monero,
                bank,
            }
        }
    };
    ok(checkout::pay_page(
        &order,
        &lines,
        view,
        q.contains_key("saved"),
    ))
}

/// The Monokulo order to show for this order: the current one if its price
/// still holds, otherwise a fresh one. An error says what to show instead.
async fn ensure_monokulo_order(
    app: &Arc<App>,
    order: &Order,
) -> Result<MonokuloOrder, MoneroState<'static>> {
    let client = app.monokulo.as_ref().ok_or(MoneroState::Unavailable)?;
    let now = util::now();
    if let Ok(Some(mk)) = app.db.monokulo_order(order.id).await {
        let live =
            mk.expires_at > now + 60 && !matches!(mk.status.as_str(), "expired" | "cancelled");
        if live && mk.amount_cents == order.total_cents {
            return Ok(mk);
        }
    }
    if app.monokulo_starts.blocked(&order.pay_token, now) {
        return Err(MoneroState::Unavailable);
    }
    app.monokulo_starts.hit(&order.pay_token, now);
    let n = app.db.monokulo_order_count(order.id).await.unwrap_or(0);
    let key = format!("{}-{}", order.number, n + 1);
    let description = format!("Order {} from Rachel's Handicrafts", order.number);
    match client
        .create_order(order.total_cents, &order.number, &description, &key)
        .await
    {
        Ok(mk) => {
            let row = MonokuloOrder {
                mk_order_id: mk.order_id,
                amount_cents: order.total_cents,
                xmr_piconero: mk.xmr_amount_piconero,
                expires_at: mk.expires_at,
                status: if mk.status.is_empty() {
                    "pending".into()
                } else {
                    mk.status
                },
            };
            if let Err(e) = app.db.add_monokulo_order(order.id, row.clone()).await {
                eprintln!("handicrafts: {e}");
            }
            let _ = app.db.clear_alert("monokulo_price").await;
            let _ = app.db.clear_alert("monokulo_down").await;
            Ok(row)
        }
        Err(err) => {
            let pricing = matches!(err, CreateError::Pricing(_));
            let (key, msg) = if pricing {
                (
                    "monokulo_price",
                    format!(
                        "Monokulo can't price orders in AUD, so customers can't pay by Monero. Monokulo said: {}",
                        err.detail()
                    ),
                )
            } else {
                (
                    "monokulo_down",
                    format!(
                        "Monokulo couldn't make a Monero payment. Monokulo said: {}",
                        err.detail()
                    ),
                )
            };
            eprintln!("handicrafts: {msg}");
            if let Ok(true) = app.db.raise_alert(key, msg.clone()).await {
                let _ = app
                    .db
                    .add_event(order.id, "warning", "system", msg.clone())
                    .await;
                if let Some(a) = app.admin_email() {
                    let m = emails::admin_notice(
                        "admin_alert",
                        a,
                        if pricing {
                            "Monero can't be priced in AUD".into()
                        } else {
                            "Monero payments aren't working".into()
                        },
                        if pricing {
                            "Monero can't be priced in AUD"
                        } else {
                            "Monero payments aren't working"
                        },
                        &format!(
                            "{msg}\n\nCustomers are being offered PayID / bank transfer instead. This shows on /admin until a Monero payment can be made again."
                        ),
                        Some(format!("{}/admin", app.cfg.public_url)),
                        true,
                    );
                    app.queue(Some(order.id), m).await;
                }
            }
            Err(if pricing {
                MoneroState::NoPrice
            } else {
                MoneroState::Unavailable
            })
        }
    }
}

/// `{"status":"invoiced"}` for js/pay.js, which reloads the page once the
/// order is paid.
pub async fn status(axum::extract::State(app): State, Path(token): Path<String>) -> Response {
    match find(&app, &token).await {
        Ok(o) => (
            [(axum::http::header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "status": o.status.as_str(), "paid": checkout::paid_status(o.status) })),
        )
            .into_response(),
        Err(r) => r,
    }
}

#[derive(serde::Deserialize)]
pub struct RefundForm {
    #[serde(default)]
    refund_address: String,
}

pub async fn refund(
    axum::extract::State(app): State,
    Path(token): Path<String>,
    headers: HeaderMap,
    Form(f): Form<RefundForm>,
) -> Response {
    let order = match find(&app, &token).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let a = f.refund_address.trim().to_string();
    let reply = |status: StatusCode, msg: &str| -> Response {
        if is_fixi(&headers) {
            (status, msg.to_string()).into_response()
        } else {
            see_other(&format!("/pay/{token}?method=monero&saved=1"))
        }
    };
    if !a.is_empty() && !checkout::looks_like_monero_address(&a) {
        return reply(
            StatusCode::UNPROCESSABLE_ENTITY,
            "That doesn't look like a Monero address. It should start with 4 or 8 and be 95 characters.",
        );
    }
    if a == order.refund_address {
        return reply(StatusCode::OK, if a.is_empty() { "" } else { "Saved." });
    }
    if let Err(e) = app.db.set_refund_address(order.id, a.clone()).await {
        return oops(e);
    }
    let body = if a.is_empty() {
        "Customer removed their refund address.".to_string()
    } else {
        format!("Customer gave a refund address: {a}")
    };
    let _ = app.db.add_event(order.id, "system", "customer", body).await;
    reply(
        StatusCode::OK,
        if a.is_empty() { "Removed." } else { "Saved." },
    )
}

#[derive(serde::Deserialize)]
pub struct ClaimForm {
    method: String,
}

pub async fn claim(
    axum::extract::State(app): State,
    Path(token): Path<String>,
    Form(f): Form<ClaimForm>,
) -> Response {
    let order = match find(&app, &token).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let Some(method) = Method::parse(Some(&f.method)) else {
        return see_other(&format!("/pay/{token}"));
    };
    if order.status != Status::Invoiced {
        return see_other(&format!("/pay/{token}"));
    }
    let how = if method == Method::Monero {
        "Monero"
    } else {
        "PayID / bank transfer"
    };
    let mail = app
        .admin_email()
        .map(|a| emails::admin_claim(&order, method.as_str(), a, &app.cfg.public_url));
    let body = format!("Customer says they've paid by {how}. Status unchanged.");
    match app
        .db
        .claim_payment(order.id, method.as_str(), body, mail)
        .await
    {
        Ok(_) => app.outbox.notify_one(),
        Err(e) => return oops(e),
    }
    see_other(&format!("/pay/{token}/thanks?method={}", method.as_str()))
}

pub async fn thanks(
    axum::extract::State(app): State,
    Path(token): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let order = match find(&app, &token).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let lines = match app.db.items(order.id).await {
        Ok(l) => l,
        Err(e) => return oops(e),
    };
    let method = Method::parse(q.get("method").map(String::as_str)).unwrap_or(Method::Bank);
    let view = if checkout::paid_status(order.status) {
        PayView::Paid
    } else if order.status == Status::Invoiced {
        PayView::Claimed { method }
    } else {
        return see_other(&format!("/pay/{token}"));
    };
    ok(checkout::pay_page(&order, &lines, view, false))
}
