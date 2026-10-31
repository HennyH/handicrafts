//! /admin, behind HTTP Basic auth (over the proxy's HTTPS). Browsers resend
//! Basic credentials by themselves, so every form also carries a CSRF token
//! and cross-site POSTs are refused.

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Form, Router,
    extract::{ConnectInfo, Path, Query, Request},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::{App, State, html, is_fixi, ok, oops, see_other};
use crate::{
    db::Order,
    invoice,
    orders::{self, Status},
    util,
    views::admin as view,
};

pub fn router(app: Arc<App>) -> Router<Arc<App>> {
    Router::new()
        .route("/admin", get(list))
        .route("/admin/", get(list))
        .route("/admin/orders/{id}", get(detail))
        .route("/admin/orders/{id}/status", post(set_status))
        .route("/admin/orders/{id}/comments", post(comment))
        .route("/admin/orders/{id}/invoice.eml", get(invoice_eml))
        .route("/admin/settings", get(settings).post(save_settings))
        .route("/admin/outbox/{id}/resend", post(resend))
        .route("/admin/monokulo", get(monokulo_page))
        .route("/admin/monokulo/connect", post(monokulo_connect))
        .route("/admin/monokulo/return", get(monokulo_return))
        .route("/admin/monokulo/disconnect", post(monokulo_disconnect))
        .route("/admin/monokulo/test", post(monokulo_test))
        .layer(middleware::from_fn_with_state(app, auth))
}

fn csrf_token(app: &App) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(&app.csrf_secret).expect("key");
    mac.update(app.cfg.admin_user.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn csrf_ok(app: &App, headers: &HeaderMap, token: &str) -> bool {
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return false;
    }
    subtle::ConstantTimeEq::ct_eq(token.as_bytes(), csrf_token(app).as_bytes()).into()
}

fn csrf_refused() -> Response {
    html(
        StatusCode::FORBIDDEN,
        view::locked(
            "That form had expired (the shop restarted since the page was loaded). Go back, reload the page and try again.",
        ),
    )
}

async fn auth(
    axum::extract::State(app): State,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let Some(hash) = app.cfg.admin_password_hash.clone() else {
        return html(
            StatusCode::SERVICE_UNAVAILABLE,
            view::locked(
                "The admin password isn't set yet. Set it in LuCI under Services › Handicrafts.",
            ),
        );
    };
    let ip = app.client_ip(peer, req.headers());
    let now = util::now();
    if app.admin_failures.blocked(&ip, now) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "900")],
            "Too many wrong passwords. Try again in 15 minutes.",
        )
            .into_response();
    }
    let creds = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| {
            base64::engine::general_purpose::STANDARD
                .decode(b.trim())
                .ok()
        })
        .and_then(|b| String::from_utf8(b).ok());
    let challenge = || {
        (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"Shop admin\", charset=\"UTF-8\""),
            )],
            "Sign in to see the shop's orders.",
        )
            .into_response()
    };
    let Some(creds) = creds else {
        return challenge();
    };
    let Some((user, pass)) = creds.split_once(':') else {
        return challenge();
    };

    let fingerprint: [u8; 32] = Sha256::digest(format!("{user}\0{pass}\0{hash}")).into();
    let cached = *app.admin_ok.lock().unwrap_or_else(|e| e.into_inner()) == Some(fingerprint);
    let good = cached || {
        let user_ok: bool =
            subtle::ConstantTimeEq::ct_eq(user.as_bytes(), app.cfg.admin_user.as_bytes()).into();
        let pass = pass.to_string();
        let pass_ok = tokio::task::spawn_blocking(move || {
            PasswordHash::new(&hash).is_ok_and(|h| {
                Argon2::default()
                    .verify_password(pass.as_bytes(), &h)
                    .is_ok()
            })
        })
        .await
        .unwrap_or(false);
        user_ok && pass_ok
    };
    if !good {
        app.admin_failures.hit(&ip, now);
        eprintln!("handicrafts: wrong admin password from {ip}");
        return challenge();
    }
    if !cached {
        *app.admin_ok.lock().unwrap_or_else(|e| e.into_inner()) = Some(fingerprint);
        app.admin_failures.clear(&ip);
    }
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    res
}

async fn list(
    axum::extract::State(app): State,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let filter = q.get("status").and_then(|s| Status::parse(s));
    let search = q
        .get("q")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let (orders, counts, alerts, failed) = match tokio::try_join!(
        app.db.list_orders(filter, search.clone()),
        app.db.status_counts(),
        app.db.alerts(),
        app.db.failed_mail(),
    ) {
        Ok(v) => v,
        Err(e) => return oops(e),
    };
    ok(view::list(
        &orders,
        &counts,
        filter,
        search.as_deref(),
        &alerts,
        &failed,
        &csrf_token(&app),
    ))
}

async fn load(app: &App, id: i64) -> Result<Order, Response> {
    match app.db.order(id).await {
        Ok(Some(o)) => Ok(o),
        Ok(None) => Err(html(
            StatusCode::NOT_FOUND,
            view::locked("There's no such order."),
        )),
        Err(e) => Err(oops(e)),
    }
}

async fn template(app: &App) -> (String, String) {
    let subject = app
        .db
        .setting("invoice_subject")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| invoice::DEFAULT_SUBJECT.into());
    let body = app
        .db
        .setting("invoice_body")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| invoice::DEFAULT_BODY.into());
    (subject, body)
}

async fn render_detail(app: &App, id: i64, error: Option<&str>) -> Response {
    let order = match load(app, id).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let (lines, events, mk) = match tokio::try_join!(
        app.db.items(id),
        app.db.events(id),
        app.db.monokulo_order(id)
    ) {
        Ok(v) => v,
        Err(e) => return oops(e),
    };
    let (subject, body) = template(app).await;
    let (mailto, invoice_error) =
        match invoice::render(&subject, &body, &order, &lines, &app.cfg.public_url) {
            Ok(d) => (Some(invoice::mailto(&d, &order.customer_email)), None),
            Err(e) => (None, Some(e)),
        };
    let status = if error.is_some() {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    html(
        status,
        view::detail(view::Detail {
            order: &order,
            lines: &lines,
            events: &events,
            monokulo: mk.as_ref(),
            pay_link: format!("{}/pay/{}", app.cfg.public_url, order.pay_token),
            mailto,
            invoice_error,
            error,
            csrf: &csrf_token(app),
        }),
    )
}

async fn detail(axum::extract::State(app): State, Path(id): Path<i64>) -> Response {
    render_detail(&app, id, None).await
}

#[derive(serde::Deserialize)]
struct StatusForm {
    #[serde(default)]
    csrf: String,
    from: String,
    status: String,
    #[serde(default)]
    accepted: Option<String>,
}

async fn set_status(
    axum::extract::State(app): State,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(f): Form<StatusForm>,
) -> Response {
    if !csrf_ok(&app, &headers, &f.csrf) {
        return csrf_refused();
    }
    let order = match load(&app, id).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let (Some(from), Some(to)) = (Status::parse(&f.from), Status::parse(&f.status)) else {
        return render_detail(&app, id, Some("That status isn't one the shop knows.")).await;
    };
    if from != order.status {
        return render_detail(
            &app,
            id,
            Some("The order changed while you were looking at it. Check it and try again."),
        )
        .await;
    }
    let accepted = f.accepted.is_some();
    let accepted = match orders::admin_change(from, order.accepted, to, accepted) {
        Ok(a) => a,
        Err(msg) => return render_detail(&app, id, Some(&msg)).await,
    };
    match app.db.set_status(id, from, to, accepted, "admin").await {
        Ok(()) => see_other(&format!("/admin/orders/{id}")),
        Err(msg) => render_detail(&app, id, Some(&msg)).await,
    }
}

#[derive(serde::Deserialize)]
struct CommentForm {
    #[serde(default)]
    csrf: String,
    body: String,
}

async fn comment(
    axum::extract::State(app): State,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(f): Form<CommentForm>,
) -> Response {
    if !csrf_ok(&app, &headers, &f.csrf) {
        return csrf_refused();
    }
    if let Err(r) = load(&app, id).await {
        return r;
    }
    let body = util::clip(&f.body, 4000);
    if !body.is_empty()
        && let Err(e) = app.db.add_event(id, "comment", "admin", body).await
    {
        return oops(e);
    }
    if is_fixi(&headers) {
        match app.db.events(id).await {
            Ok(events) => ok(view::timeline(&events)),
            Err(e) => oops(e),
        }
    } else {
        see_other(&format!("/admin/orders/{id}"))
    }
}

async fn invoice_eml(axum::extract::State(app): State, Path(id): Path<i64>) -> Response {
    let order = match load(&app, id).await {
        Ok(o) => o,
        Err(r) => return r,
    };
    let lines = match app.db.items(id).await {
        Ok(l) => l,
        Err(e) => return oops(e),
    };
    let (subject, body) = template(&app).await;
    match invoice::render(&subject, &body, &order, &lines, &app.cfg.public_url) {
        Ok(d) => (
            [
                (header::CONTENT_TYPE, "message/rfc822".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"invoice-{}.eml\"", order.number),
                ),
            ],
            invoice::eml(&d, &order.customer_name, &order.customer_email),
        )
            .into_response(),
        Err(e) => {
            render_detail(
                &app,
                id,
                Some(&format!("The invoice template has a problem: {e}")),
            )
            .await
        }
    }
}

async fn settings(
    axum::extract::State(app): State,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let (subject, body) = template(&app).await;
    ok(view::settings(
        &subject,
        &body,
        None,
        q.contains_key("saved"),
        None,
        &csrf_token(&app),
    ))
}

#[derive(serde::Deserialize)]
struct SettingsForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    action: String,
}

async fn save_settings(
    axum::extract::State(app): State,
    headers: HeaderMap,
    Form(f): Form<SettingsForm>,
) -> Response {
    if !csrf_ok(&app, &headers, &f.csrf) {
        return csrf_refused();
    }
    let csrf = csrf_token(&app);
    let body = f.body.replace("\r\n", "\n");
    let subject = f.subject.trim().to_string();
    if f.action == "reset" {
        return ok(view::settings(
            invoice::DEFAULT_SUBJECT,
            invoice::DEFAULT_BODY,
            None,
            false,
            None,
            &csrf,
        ));
    }
    if let Err(e) = invoice::validate(&subject, &body) {
        return html(
            StatusCode::UNPROCESSABLE_ENTITY,
            view::settings(&subject, &body, Some(&e), false, None, &csrf),
        );
    }
    if f.action == "preview" {
        let newest = app
            .db
            .list_orders(None, None)
            .await
            .ok()
            .and_then(|o| o.into_iter().next());
        let preview = match newest {
            Some(o) => {
                let lines = app.db.items(o.id).await.unwrap_or_default();
                invoice::render(&subject, &body, &o, &lines, &app.cfg.public_url).ok()
            }
            None => None,
        };
        let err = preview
            .is_none()
            .then_some("There are no orders yet to preview with.");
        return ok(view::settings(
            &subject,
            &body,
            err,
            false,
            preview.as_ref(),
            &csrf,
        ));
    }
    if let Err(e) = app.db.set_setting("invoice_subject", subject).await {
        return oops(e);
    }
    if let Err(e) = app.db.set_setting("invoice_body", body).await {
        return oops(e);
    }
    see_other("/admin/settings?saved=1")
}

#[derive(serde::Deserialize)]
struct CsrfOnly {
    #[serde(default)]
    csrf: String,
}

async fn resend(
    axum::extract::State(app): State,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(f): Form<CsrfOnly>,
) -> Response {
    if !csrf_ok(&app, &headers, &f.csrf) {
        return csrf_refused();
    }
    if let Err(e) = app.db.resend_mail(id).await {
        return oops(e);
    }
    app.outbox.notify_one();
    see_other("/admin")
}

// ------------------------------------------------------------ Monokulo connect

/// The shop's name on Monokulo's pages.
const SHOP_NAME: &str = "Rachel's Handicrafts";
/// How long the browser has to come back from Monokulo.
const CONNECT_WINDOW: i64 = 15 * 60;

async fn render_monokulo(app: &App, notice: Option<&str>, error: Option<&str>) -> Response {
    let source = app.monokulo_source().await;
    let connected_at = app
        .db
        .setting("monokulo_connected_at")
        .await
        .ok()
        .flatten()
        .and_then(|t| t.parse().ok());
    let last_test = app
        .db
        .setting("monokulo_last_test")
        .await
        .ok()
        .flatten()
        .and_then(|t| t.parse().ok());
    let pk = match &source {
        super::MonokuloSource::Connected(c) => Some(c.public_key.clone()),
        _ => None,
    };
    let status = if error.is_some() {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    html(
        status,
        view::monokulo(view::MonokuloPage {
            url: app.cfg.monokulo_url.as_deref(),
            shop_url: &app.cfg.public_url,
            connected: pk.as_deref().map(|pk| (pk, connected_at)),
            keys: matches!(source, super::MonokuloSource::Keys),
            last_test,
            notice,
            error,
            csrf: &csrf_token(app),
        }),
    )
}

async fn monokulo_page(
    axum::extract::State(app): State,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let notice = match q.get("done").map(String::as_str) {
        Some("connected") => Some(
            "Connected. Monokulo added this shop's webhook; send a test webhook to check it arrives.",
        ),
        Some("disconnected") => Some(
            "Disconnected. The shop no longer uses this store's keys. Remove the webhook in Monokulo's store settings if you won't connect again.",
        ),
        Some("tested") => {
            Some("Monokulo took the test webhook. The time below shows when it arrived.")
        }
        _ => None,
    };
    render_monokulo(&app, notice, None).await
}

#[derive(serde::Deserialize)]
struct CsrfForm {
    #[serde(default)]
    csrf: String,
}

/// Start connecting: remember a one-time nonce and send the browser to
/// Monokulo, which brings it back to /admin/monokulo/return.
async fn monokulo_connect(
    axum::extract::State(app): State,
    headers: HeaderMap,
    Form(f): Form<CsrfForm>,
) -> Response {
    if !csrf_ok(&app, &headers, &f.csrf) {
        return csrf_refused();
    }
    let Some(url) = app.cfg.monokulo_url.clone() else {
        return render_monokulo(&app, None, Some("Set the Monokulo address in LuCI first.")).await;
    };
    let nonce = util::token(32);
    if let Err(e) = app
        .db
        .set_setting("monokulo_nonce", format!("{nonce} {}", util::now()))
        .await
    {
        return oops(e);
    }
    let shop = &app.cfg.public_url;
    see_other(&crate::monokulo::connect_url(
        &url,
        shop,
        &format!("{shop}/admin/monokulo/return"),
        &nonce,
        SHOP_NAME,
    ))
}

/// Back from Monokulo with `token` and our `nonce`: swap the token for the
/// store's keys, registering the webhook.
async fn monokulo_return(
    axum::extract::State(app): State,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(url) = app.cfg.monokulo_url.clone() else {
        return render_monokulo(&app, None, Some("Set the Monokulo address in LuCI first.")).await;
    };
    let stored = app.db.setting("monokulo_nonce").await.ok().flatten();
    // The nonce works once, whatever happens next.
    let _ = app.db.delete_setting("monokulo_nonce").await;
    let (Some(token), Some(nonce)) = (q.get("token"), q.get("nonce")) else {
        return render_monokulo(&app, None, Some("Connecting was cancelled, or Monokulo didn't send the shop back a connect code. Try again.")).await;
    };
    let fresh = stored
        .as_deref()
        .and_then(|s| s.split_once(' '))
        .is_some_and(|(expected, at)| {
            let at: i64 = at.parse().unwrap_or(0);
            let same: bool =
                subtle::ConstantTimeEq::ct_eq(expected.as_bytes(), nonce.as_bytes()).into();
            same && util::now() - at <= CONNECT_WINDOW
        });
    if !fresh {
        return render_monokulo(
            &app,
            None,
            Some("That connect link wasn't started here, or it's more than 15 minutes old. Press Connect to Monokulo again."),
        )
        .await;
    }
    let webhook = format!("{}/webhooks/monokulo", app.cfg.public_url);
    match crate::monokulo::finish_connect(&url, token, &webhook, SHOP_NAME).await {
        Ok(conn) => {
            if let Err(e) = app.db.set_monokulo_connection(&conn).await {
                return oops(e);
            }
            let _ = app
                .db
                .set_setting("monokulo_connected_at", util::now().to_string())
                .await;
            let _ = app.db.clear_alert("monokulo_down").await;
            app.refresh_monokulo().await;
            eprintln!("handicrafts: connected to the Monokulo store at {url}");
            see_other("/admin/monokulo?done=connected")
        }
        Err(e) => {
            render_monokulo(
                &app,
                None,
                Some(&format!("Monokulo didn't connect the store: {e}")),
            )
            .await
        }
    }
}

async fn monokulo_disconnect(
    axum::extract::State(app): State,
    headers: HeaderMap,
    Form(f): Form<CsrfForm>,
) -> Response {
    if !csrf_ok(&app, &headers, &f.csrf) {
        return csrf_refused();
    }
    if let Err(e) = app.db.delete_setting("monokulo_connection").await {
        return oops(e);
    }
    let _ = app.db.delete_setting("monokulo_connected_at").await;
    app.refresh_monokulo().await;
    see_other("/admin/monokulo?done=disconnected")
}

async fn monokulo_test(
    axum::extract::State(app): State,
    headers: HeaderMap,
    Form(f): Form<CsrfForm>,
) -> Response {
    if !csrf_ok(&app, &headers, &f.csrf) {
        return csrf_refused();
    }
    let Some(client) = app.monokulo() else {
        return render_monokulo(&app, None, Some("The store isn't connected yet.")).await;
    };
    match client.send_test_webhook().await {
        Ok(true) => see_other("/admin/monokulo?done=tested"),
        Ok(false) => {
            render_monokulo(
                &app,
                None,
                Some("Monokulo sent the test webhook, but this shop didn't take it. Check that the reverse proxy passes /webhooks/monokulo through, and that the keys are this store's."),
            )
            .await
        }
        Err(e) => render_monokulo(&app, None, Some(&format!("Monokulo couldn't send a test webhook: {e}"))).await,
    }
}
