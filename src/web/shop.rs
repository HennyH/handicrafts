//! Shop pages, static files, and placing an order.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Form,
    extract::ConnectInfo,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

use super::{App, State, html, is_fixi, ok, oops};
use crate::{
    assets,
    catalog::CartLine,
    db::NewOrder,
    emails,
    mail::clean_address,
    ratelimit::{OrderLimit, check_order},
    util,
    views::{
        checkout::{self, CheckoutForm},
        shop,
    },
};

/// Every GET that isn't an order, pay or admin route: a shop page, or an
/// embedded file.
pub async fn page_or_file(axum::extract::State(app): State, method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = uri.path();
    let markup = match path {
        "/" | "/index.html" => Some(shop::index()),
        "/shop.html" => Some(shop::shop()),
        "/cart.html" => Some(shop::cart()),
        "/checkout.html" => Some(checkout::checkout_page(&CheckoutForm::default(), None)),
        "/recipes.html" => Some(shop::recipes(&app.catalog.recipes)),
        p => p
            .strip_prefix("/recipe-")
            .and_then(|r| r.strip_suffix(".html"))
            .and_then(|id| app.catalog.recipe(id))
            .map(shop::recipe),
    };
    if let Some(m) = markup {
        let mut res = ok(m);
        // Pages are cheap to render and change with every deploy.
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        return res;
    }
    let versioned = uri.query().is_some_and(|q| q.starts_with("v="));
    if !path.contains("..")
        && let Some(res) = assets::serve(path, versioned)
    {
        return res;
    }
    html(StatusCode::NOT_FOUND, shop::not_found())
}

/// Re-show the checkout with an error, keeping what was typed.
fn form_error(headers: &HeaderMap, status: StatusCode, form: &CheckoutForm, msg: &str) -> Response {
    if is_fixi(headers) {
        html(status, checkout::checkout_main(form, Some(msg)))
    } else {
        html(status, checkout::checkout_page(form, Some(msg)))
    }
}

pub async fn place_order(
    axum::extract::State(app): State,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<CheckoutForm>,
) -> Response {
    let ip = app.client_ip(peer, &headers);
    let bad = |msg: &str| form_error(&headers, StatusCode::UNPROCESSABLE_ENTITY, &form, msg);

    // Cheap checks first: bots fill in the hidden field and post at once.
    if !form.website.is_empty() {
        eprintln!("handicrafts: order from {ip} refused: honeypot filled");
        return bad("Sorry, that didn't go through. Please try again.");
    }
    let started: i64 = form.started.parse().unwrap_or(0);
    if util::now() - started < 3 {
        return bad("That was quick! Please check your details, then press Place order again.");
    }

    let name = util::clip(&form.name, 120);
    if name.is_empty() {
        return bad("Please give your name.");
    }
    let Some(email) = clean_address(&form.email) else {
        return bad("That email address doesn't look right. Please check it.");
    };
    let cart: Vec<CartLine> = match serde_json::from_str(&form.cart) {
        Ok(c) => c,
        Err(_) => {
            return bad("Your cart couldn't be read. Please go back to your cart and try again.");
        }
    };
    let lines = match app.catalog.price_cart(&cart) {
        Ok(l) => l,
        Err(msg) => return bad(&msg),
    };

    let counts = match app.db.counts(ip.clone(), email.clone()).await {
        Ok(c) => c,
        Err(e) => return oops(e),
    };
    let mut skip_customer_email = false;
    match check_order(&counts, &app.cfg.limits) {
        OrderLimit::Ip { retry_after } => {
            eprintln!("handicrafts: order from {ip} refused: too many orders from this IP");
            let mut res = too_many(&headers, &form);
            res.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(retry_after));
            return res;
        }
        OrderLimit::Shop => {
            eprintln!("handicrafts: order from {ip} refused: the shop's daily limit is reached");
            shop_full_alert(&app).await;
            return form_error(
                &headers,
                StatusCode::SERVICE_UNAVAILABLE,
                &form,
                "The shop isn't taking more orders today. Please try again tomorrow, or email hello@rachelshandicrafts.com.",
            );
        }
        OrderLimit::SkipCustomerEmail => skip_customer_email = true,
        OrderLimit::None => {}
    }

    let admin = app.admin_email().map(str::to_string);
    let public_url = app.cfg.public_url.clone();
    let new = NewOrder {
        customer_name: name,
        customer_email: email,
        customer_phone: util::clip(&form.phone, 40),
        customer_address: util::clip(&form.address, 600),
        customer_notes: util::clip(&form.notes, 2000),
        client_ip: ip,
        lines,
    };
    let order = app
        .db
        .create_order(new, move |order, lines| {
            let mut mails = Vec::new();
            if !skip_customer_email {
                mails.push(emails::customer_order(order, lines));
            }
            if let Some(a) = &admin {
                mails.push(emails::admin_new_order(order, lines, a, &public_url));
            }
            mails
        })
        .await;
    let order = match order {
        Ok(o) => o,
        Err(e) => return oops(e),
    };
    if skip_customer_email {
        let _ = app
            .db
            .add_event(
                order.id,
                "system",
                "system",
                "No copy emailed to the customer: that address already got 3 order emails today."
                    .into(),
            )
            .await;
    }
    app.outbox.notify_one();
    let lines = match app.db.items(order.id).await {
        Ok(l) => l,
        Err(e) => return oops(e),
    };
    let emailed = !skip_customer_email && !app.mail.is_off();
    if is_fixi(&headers) {
        ok(checkout::received_main(&order, &lines, emailed))
    } else {
        ok(checkout::received_page(&order, &lines, emailed))
    }
}

fn too_many(headers: &HeaderMap, form: &CheckoutForm) -> Response {
    form_error(
        headers,
        StatusCode::TOO_MANY_REQUESTS,
        form,
        "There have been too many orders from your connection. Please try again in a while, or email hello@rachelshandicrafts.com.",
    )
}

async fn shop_full_alert(app: &Arc<App>) {
    let msg = format!(
        "The shop reached its limit of {} orders in 24 hours, so new orders are being refused. If these are real orders, raise HANDICRAFTS_LIMIT_SHOP_DAY in LuCI.",
        app.cfg.limits.shop_day
    );
    if let Ok(true) = app.db.raise_alert("shop_limit", msg.clone()).await
        && let Some(a) = app.admin_email()
    {
        let m = emails::admin_notice(
            "admin_alert",
            a,
            "The shop is refusing orders (daily limit)".into(),
            "The shop is refusing new orders",
            &msg,
            Some(format!("{}/admin", app.cfg.public_url)),
            true,
        );
        app.queue(None, m).await;
    }
}
