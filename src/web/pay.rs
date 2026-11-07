//! The customer's pay page, reached only through the secret link in the
//! invoice.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Form, Json,
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::{App, State, html, ok, oops, see_other};
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

    let bank = app.cfg.bank.any().then_some(&app.cfg.bank);
    let monokulo = app.monokulo();
    let method = match (requested, &monokulo, bank) {
        (Some(Method::Bank), _, Some(_)) => Method::Bank,
        (_, Some(_), _) => Method::Monero,
        _ => Method::Bank,
    };
    let view = match order.status {
        Status::New => PayView::NotInvoiced,
        Status::Rejected => PayView::Cancelled,
        s if checkout::paid_status(s) => PayView::Paid,
        _ => {
            let monero = match &monokulo {
                None => None,
                // Bank chosen: Monero is offered but not started.
                Some(_) if method != Method::Monero || order.status != Status::Invoiced => {
                    Some(MoneroState::NotStarted)
                }
                Some(_) => {
                    let state = monero_state(&app, &order, None).await;
                    Some(after_failed_refresh(state, &q))
                }
            };
            PayView::Choose {
                method,
                monero,
                bank,
            }
        }
    };
    ok(checkout::pay_page(&order, &lines, view))
}

/// Where this order's payment code stands. Never makes a code, so a page or a
/// tab left open can ask as often as it likes: only the first visit and the
/// customer pressing Refresh make one.
enum Code {
    /// None yet.
    None,
    Live(MonokuloOrder),
    /// Money is on its way to it, so a new code would ask them to pay twice.
    Seen(MonokuloOrder),
    /// Over, with nothing received.
    RanOut,
    /// Over, with some of the money received.
    PartPaid,
    /// Over, but Monokulo can't say what it received.
    Unknown,
}

async fn assess_code(app: &Arc<App>, order: &Order) -> Code {
    let mk = match app.db.monokulo_order(order.id).await {
        Ok(Some(mk)) if mk.amount_cents == order.total_cents => mk,
        Ok(_) => return Code::None,
        Err(e) => {
            eprintln!("handicrafts: {e}");
            return Code::Unknown;
        }
    };
    let now = util::now();
    let status = mk.status.as_str();
    if matches!(status, "unconfirmed" | "confirming") {
        return Code::Seen(mk);
    }
    if mk.expires_at > now && !matches!(status, "expired" | "cancelled") {
        return Code::Live(mk);
    }
    // Over as far as we know, so what to say depends on what has arrived.
    let Some(client) = app.monokulo() else {
        return Code::Unknown;
    };
    if app.monokulo_checks.blocked(&order.pay_token, now) {
        return Code::Unknown;
    }
    app.monokulo_checks.hit(&order.pay_token, now);
    match client.order_now(&mk.mk_order_id).await {
        Ok(o) => match o.status.as_str() {
            "paid" | "overpaid" | "unconfirmed" | "confirming" => Code::Seen(mk),
            _ if o.amount_received_piconero > 0 => Code::PartPaid,
            "expired" | "cancelled" => Code::RanOut,
            _ => Code::Live(mk),
        },
        Err(e) => {
            eprintln!("handicrafts: {e}");
            Code::Unknown
        }
    }
}

/// What the Monero step shows. The one decision behind both a page load and
/// the panel a tab swaps in, so the two can't disagree. `frame_over` is the
/// Monokulo order an open tab's frame said is over: if we'd show that same
/// frame again, the tab would only ask again, so say we can't tell instead.
async fn monero_state(app: &Arc<App>, order: &Order, frame_over: Option<&str>) -> MoneroState {
    let Some(client) = app.monokulo() else {
        return MoneroState::Unavailable;
    };
    let (mk, seen) = match assess_code(app, order).await {
        Code::None => match make_code(app, order).await {
            Ok(mk) => (mk, false),
            Err(state) => return state,
        },
        Code::Live(mk) => (mk, false),
        Code::Seen(mk) => (mk, true),
        Code::RanOut => return MoneroState::RanOut,
        Code::PartPaid => return MoneroState::PartPaid,
        Code::Unknown => return MoneroState::Unknown,
    };
    if frame_over == Some(mk.mk_order_id.as_str()) {
        return MoneroState::Unknown;
    }
    let layout = client.checkout_layout(&mk.mk_order_id).await;
    MoneroState::Checkout {
        mk,
        cfg: client.cfg().clone(),
        layout,
        seen,
    }
}

/// Refresh couldn't make a code: say why, once, on the page it lands on.
fn after_failed_refresh(state: MoneroState, q: &HashMap<String, String>) -> MoneroState {
    match (&state, q.get("refresh").map(String::as_str)) {
        (MoneroState::RanOut | MoneroState::Unknown, Some("no_price")) => MoneroState::NoPrice,
        (MoneroState::RanOut | MoneroState::Unknown, Some("unavailable")) => {
            MoneroState::Unavailable
        }
        _ => state,
    }
}

/// The Monero step alone, for a tab whose frame said its code is over.
pub async fn panel(
    axum::extract::State(app): State,
    Path(token): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let order = match find(&app, &token).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    if order.status != Status::Invoiced || app.monokulo().is_none() {
        // Nothing to swap in: the tab reloads to whatever the page is now.
        return StatusCode::CONFLICT.into_response();
    }
    let state = monero_state(&app, &order, q.get("over").map(String::as_str)).await;
    let bank = app.cfg.bank.any().then_some(&app.cfg.bank);
    ok(checkout::monero_panel(&order, &state, bank))
}

/// The Refresh button: the only thing that replaces a code. Looks again first,
/// so a stale tab or a second press can't replace a live or part-paid one.
pub async fn new_code(axum::extract::State(app): State, Path(token): Path<String>) -> Response {
    let order = match find(&app, &token).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let back = format!("/pay/{token}?method=monero");
    if order.status != Status::Invoiced || app.monokulo().is_none() {
        return see_other(&back);
    }
    if matches!(
        assess_code(&app, &order).await,
        Code::None | Code::RanOut | Code::Unknown
    ) {
        match make_code(&app, &order).await {
            Ok(_) => {}
            Err(MoneroState::NoPrice) => return see_other(&format!("{back}&refresh=no_price")),
            Err(_) => return see_other(&format!("{back}&refresh=unavailable")),
        }
    }
    see_other(&back)
}

/// Make a new Monokulo order. An error says what to show instead.
async fn make_code(app: &Arc<App>, order: &Order) -> Result<MonokuloOrder, MoneroState> {
    let client = app.monokulo().ok_or(MoneroState::Unavailable)?;
    let now = util::now();
    if app.monokulo_starts.blocked(&order.pay_token, now) {
        return Err(MoneroState::Unavailable);
    }
    app.monokulo_starts.hit(&order.pay_token, now);
    let n = app.db.monokulo_order_count(order.id).await.unwrap_or(0);
    let key = format!("{}-{}", order.number, n + 1);
    match client
        .create_order(order.total_cents, &order.number, &key)
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
    ok(checkout::pay_page(&order, &lines, view))
}
