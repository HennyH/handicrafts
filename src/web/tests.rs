//! End to end through the router: checkout, admin, pay page, and Monokulo
//! (a fake one on a local port).

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode, header},
    routing::post,
};
use base64::Engine as _;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::{App, router};
use crate::{config::Config, db::Db, mail::Transport, monokulo, outbox, util};

const PASSWORD: &str = "correct horse battery";

struct Fake {
    /// What the fake Monokulo answers to POST /api/v1/orders.
    answer: Mutex<(u16, serde_json::Value)>,
    calls: Mutex<Vec<serde_json::Value>>,
    /// The Authorization header of each order and test-webhook call.
    auth: Mutex<Vec<String>>,
    /// Bodies sent to the plugin connect /finish.
    finishes: Mutex<Vec<serde_json::Value>>,
}

async fn fake_monokulo() -> (String, Arc<Fake>) {
    let fake = Arc::new(Fake {
        answer: Mutex::new((
            201,
            serde_json::json!({ "order_id": "order_abc", "xmr_amount_piconero": 121_916_000_000i64, "expires_at": util::now() + 900, "status": "pending" }),
        )),
        calls: Mutex::new(vec![]),
        auth: Mutex::new(vec![]),
        finishes: Mutex::new(vec![]),
    });
    let f = fake.clone();
    let (f2, f3) = (fake.clone(), fake.clone());
    let bearer = |h: &axum::http::HeaderMap| {
        h.get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let app = Router::new()
        .route(
            "/plugins/connect/handicrafts/finish",
            post(move |body: String| {
                let f = f2.clone();
                async move {
                    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let ok = body["token"] == "conn_good";
                    f.finishes.lock().unwrap().push(body);
                    if !ok {
                        return (
                            StatusCode::UNAUTHORIZED,
                            [(header::CONTENT_TYPE, "application/problem+json")],
                            r#"{"detail":"This connect code is unknown, already used or expired. Start connecting again.","status":401}"#.to_string(),
                        );
                    }
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        serde_json::json!({ "api": "/api/v1", "public_key": "pk_connected", "secret_token": "sk_connected", "webhook_signing_secret": "whsec_connected" }).to_string(),
                    )
                }
            }),
        )
        .route(
            "/api/v1/webhook-tests",
            post(move |headers: axum::http::HeaderMap| {
                let f = f3.clone();
                async move {
                    f.auth.lock().unwrap().push(bearer(&headers));
                    (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], r#"{"delivered":true}"#.to_string())
                }
            }),
        )
        .route(
        "/api/v1/orders",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let f = f.clone();
            async move {
                f.auth.lock().unwrap().push(bearer(&headers));
                f.calls
                    .lock()
                    .unwrap()
                    .push(serde_json::from_str(&body).unwrap());
                let (code, json) = f.answer.lock().unwrap().clone();
                (
                    StatusCode::from_u16(code).unwrap(),
                    [(header::CONTENT_TYPE, "application/json")],
                    json.to_string(),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, fake)
}

struct Shop {
    app: Arc<App>,
    router: Router,
}

async fn shop(monokulo_url: Option<String>) -> Shop {
    shop_with(monokulo_url, true).await
}

/// `keys`: also paste Monokulo keys "into LuCI".
async fn shop_with(monokulo_url: Option<String>, keys: bool) -> Shop {
    let hash = {
        use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
        let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
        Argon2::default()
            .hash_password(PASSWORD.as_bytes(), &salt)
            .unwrap()
            .to_string()
    };
    let mut vars = vec![
        ("HANDICRAFTS_PUBLIC_URL", "https://shop.example".to_string()),
        ("HANDICRAFTS_ADMIN_USER", "rachel".into()),
        ("HANDICRAFTS_ADMIN_PASSWORD_HASH", hash),
        ("HANDICRAFTS_ADMIN_EMAIL", "rachel@example.com".into()),
        ("HANDICRAFTS_PAYID", "hello@example.com".into()),
    ];
    if let Some(u) = monokulo_url {
        vars.push(("HANDICRAFTS_MONOKULO_URL", u));
    }
    if keys && vars.iter().any(|(k, _)| *k == "HANDICRAFTS_MONOKULO_URL") {
        vars.push(("HANDICRAFTS_MONOKULO_PUBLIC_KEY", "pk_test".into()));
        vars.push(("HANDICRAFTS_MONOKULO_SECRET_KEY", "sk_test".into()));
        vars.push(("HANDICRAFTS_MONOKULO_WEBHOOK_SECRET", "whsec".into()));
    }
    let cfg =
        Config::from_vars(|k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())).unwrap();
    let app = Arc::new(App::new(
        cfg,
        Db::memory(),
        Arc::new(Transport::Log(Default::default())),
    ));
    app.refresh_monokulo().await;
    let peer: SocketAddr = "203.0.113.5:4000".parse().unwrap();
    let router = router(app.clone()).layer(MockConnectInfo(peer));
    Shop { app, router }
}

impl Shop {
    async fn send(&self, req: Request<Body>) -> (StatusCode, String, axum::http::HeaderMap) {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned(), headers)
    }

    async fn get(&self, uri: &str) -> (StatusCode, String) {
        let (s, b, _) = self
            .send(Request::get(uri).body(Body::empty()).unwrap())
            .await;
        (s, b)
    }

    async fn post(
        &self,
        uri: &str,
        form: &[(&str, &str)],
        admin: bool,
    ) -> (StatusCode, String, axum::http::HeaderMap) {
        let body: String = form
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let mut req =
            Request::post(uri).header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if admin {
            req = req.header(header::AUTHORIZATION, auth());
        }
        self.send(req.body(Body::from(body)).unwrap()).await
    }

    async fn admin_get(&self, uri: &str) -> (StatusCode, String) {
        let (s, b, _) = self
            .send(
                Request::get(uri)
                    .header(header::AUTHORIZATION, auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        (s, b)
    }

    async fn place_order(&self) -> crate::db::Order {
        let started = (util::now() - 10).to_string();
        let (s, body, _) = self
            .post(
                "/orders",
                &[
                    ("name", "Sam Example"),
                    ("email", "sam@example.com"),
                    ("address", "local pickup"),
                    (
                        "cart",
                        r#"[{"id":"tote-bag","color":"Navy","qty":1},{"id":"banneton","qty":1}]"#,
                    ),
                    ("started", &started),
                    ("website", ""),
                ],
                false,
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{body}");
        let number = body
            .split("data-order-placed=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string();
        self.app.db.order_by_number(number).await.unwrap().unwrap()
    }

    async fn csrf(&self) -> String {
        let (_, page) = self.admin_get("/admin").await;
        page.split("name=\"csrf\" value=\"")
            .nth(1)
            .map(|s| s.split('"').next().unwrap().to_string())
            .unwrap_or_else(|| {
                // The list page only has forms when something failed; use an order page.
                String::new()
            })
    }

    async fn csrf_for(&self, id: i64) -> String {
        let (_, page) = self.admin_get(&format!("/admin/orders/{id}")).await;
        page.split("name=\"csrf\" value=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string()
    }

    async fn invoice(&self, id: i64) {
        let csrf = self.csrf_for(id).await;
        let (s, b, _) = self
            .post(
                &format!("/admin/orders/{id}/status"),
                &[
                    ("csrf", &csrf),
                    ("from", "new"),
                    ("status", "invoiced"),
                    ("accepted", "1"),
                ],
                true,
            )
            .await;
        assert_eq!(s, StatusCode::SEE_OTHER, "{b}");
    }

    async fn webhook(&self, body: &serde_json::Value, secret: &str) -> StatusCode {
        let raw = body.to_string();
        let sig = monokulo::sign(secret, util::now(), raw.as_bytes());
        let req = Request::post("/webhooks/monokulo")
            .header("x-monokulo-signature", sig)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(raw))
            .unwrap();
        self.send(req).await.0
    }
}

fn auth() -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("rachel:{PASSWORD}"))
    )
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn order_is_saved_priced_on_the_server_and_emailed() {
    let s = shop(None).await;
    let o = s.place_order().await;
    assert_eq!(o.total_cents, 10500);
    outbox::send_due(&s.app.db, &s.app.mail).await;
    let sent = s.app.mail.logged();
    let to: Vec<&str> = sent.iter().map(|l| l.to.as_str()).collect();
    assert_eq!(to, ["sam@example.com", "rachel@example.com"]);
}

#[tokio::test]
async fn ip_limit_refuses_the_fourth_order_in_ten_minutes() {
    let s = shop(None).await;
    for _ in 0..3 {
        s.place_order().await;
    }
    let started = (util::now() - 10).to_string();
    let (status, body, headers) = s
        .post(
            "/orders",
            &[
                ("name", "Sam"),
                ("email", "sam@example.com"),
                ("cart", r#"[{"id":"banneton","qty":1}]"#),
                ("started", &started),
            ],
            false,
        )
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(body.contains("too many orders"));
    assert_eq!(headers.get(header::RETRY_AFTER).unwrap(), "600");
}

#[tokio::test]
async fn honeypot_and_speed_checks() {
    let s = shop(None).await;
    let cart = r#"[{"id":"banneton","qty":1}]"#;
    let now = util::now().to_string();
    let (st, _, _) = s
        .post(
            "/orders",
            &[
                ("name", "Sam"),
                ("email", "sam@example.com"),
                ("cart", cart),
                ("started", &now),
            ],
            false,
        )
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "too fast");
    let old = (util::now() - 10).to_string();
    let (st, _, _) = s
        .post(
            "/orders",
            &[
                ("name", "Sam"),
                ("email", "sam@example.com"),
                ("cart", cart),
                ("started", &old),
                ("website", "http://spam"),
            ],
            false,
        )
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "honeypot");
    assert!(s.app.db.list_orders(None, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn admin_needs_the_password_and_csrf() {
    let s = shop(None).await;
    let o = s.place_order().await;
    assert_eq!(s.get("/admin").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(s.admin_get("/admin").await.0, StatusCode::OK);
    let (st, _, _) = s
        .post(
            &format!("/admin/orders/{}/status", o.id),
            &[("csrf", "nope"), ("from", "new"), ("status", "rejected")],
            true,
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let _ = s.csrf().await;
}

#[tokio::test]
async fn monero_flow_from_pay_link_to_paid() {
    let (url, fake) = fake_monokulo().await;
    let s = shop(Some(url)).await;
    let o = s.place_order().await;

    // Not invoiced yet: no Monokulo order is made.
    let (_, page) = s.get(&format!("/pay/{}", o.pay_token)).await;
    assert!(page.contains("Not invoiced yet"));
    assert!(fake.calls.lock().unwrap().is_empty());

    s.invoice(o.id).await;
    let (_, page) = s.get(&format!("/pay/{}", o.pay_token)).await;
    assert!(page.contains("order_abc"), "mounts the Monokulo order");
    assert!(page.contains("/static/monokulo-client.js"));
    let call = fake.calls.lock().unwrap()[0].clone();
    assert_eq!(call["amount"], "105.00");
    assert_eq!(call["currency"], "AUD");
    assert_eq!(call["merchant_order_id"], o.number.as_str());

    // A second visit reuses the order while its price holds.
    s.get(&format!("/pay/{}", o.pay_token)).await;
    assert_eq!(fake.calls.lock().unwrap().len(), 1);

    // The customer leaves a refund address.
    let addr = format!("4{}", "A".repeat(94));
    let (st, _, _) = s
        .post(
            &format!("/pay/{}/refund", o.pay_token),
            &[("refund_address", &addr)],
            false,
        )
        .await;
    assert_eq!(st, StatusCode::SEE_OTHER);

    // A forged webhook is refused; a real one pays the order, once.
    let ev = serde_json::json!({
        "api_version": 2, "event_id": "evt_1", "event": "order.paid", "order_id": "order_abc",
        "merchant_order_id": o.number, "amount": "105.00", "currency": "AUD", "xmr_amount": "0.121916000000"
    });
    assert_eq!(s.webhook(&ev, "wrong").await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        s.app.db.order(o.id).await.unwrap().unwrap().status,
        crate::orders::Status::Invoiced
    );
    assert_eq!(s.webhook(&ev, "whsec").await, StatusCode::OK);
    assert_eq!(
        s.webhook(&ev, "whsec").await,
        StatusCode::OK,
        "a retry is acknowledged"
    );
    let paid = s.app.db.order(o.id).await.unwrap().unwrap();
    assert_eq!(paid.status, crate::orders::Status::Paid);
    let events = s.app.db.events(o.id).await.unwrap();
    let payments: Vec<_> = events.iter().filter(|e| e.kind == "payment").collect();
    assert_eq!(payments.len(), 1, "handled once");
    assert!(
        payments[0].body.contains(&addr),
        "refund address in the comment"
    );

    let (_, page) = s.get(&format!("/pay/{}", o.pay_token)).await;
    assert!(page.contains("Paid, thank you!"));
}

#[tokio::test]
async fn no_aud_price_shows_an_error_and_alerts_the_admin_once() {
    let (url, fake) = fake_monokulo().await;
    *fake.answer.lock().unwrap() = (
        503,
        serde_json::json!({ "type": "https://docs.monokulo.com/problems/rates-unavailable", "detail": "There's no exchange rate for AUD right now", "status": 503 }),
    );
    let s = shop(Some(url)).await;
    let o = s.place_order().await;
    s.invoice(o.id).await;
    let (_, page) = s.get(&format!("/pay/{}", o.pay_token)).await;
    assert!(
        page.contains("can't be priced in Australian dollars"),
        "{page}"
    );
    assert!(page.contains("Pay by PayID or bank"));
    let alerts = s.app.db.alerts().await.unwrap();
    assert_eq!(alerts.len(), 1);
    let (_, admin) = s.admin_get("/admin").await;
    assert!(admin.contains("no exchange rate for AUD"));

    s.get(&format!("/pay/{}", o.pay_token)).await;
    outbox::send_due(&s.app.db, &s.app.mail).await;
    let alerts_sent = s
        .app
        .mail
        .logged()
        .iter()
        .filter(|l| l.subject.contains("priced in AUD"))
        .count();
    assert_eq!(alerts_sent, 1, "emailed once, not on every visit");

    // Bank transfer still works.
    let (_, page) = s.get(&format!("/pay/{}?method=bank", o.pay_token)).await;
    assert!(page.contains(&o.bank_ref));
}

#[tokio::test]
async fn webhook_with_wrong_amount_never_pays() {
    let (url, _) = fake_monokulo().await;
    let s = shop(Some(url)).await;
    let o = s.place_order().await;
    s.invoice(o.id).await;
    let ev = serde_json::json!({
        "event_id": "evt_2", "event": "order.paid", "order_id": "order_x", "merchant_order_id": o.number,
        "amount": "0.01", "currency": "AUD"
    });
    assert_eq!(s.webhook(&ev, "whsec").await, StatusCode::OK);
    assert_eq!(
        s.app.db.order(o.id).await.unwrap().unwrap().status,
        crate::orders::Status::Invoiced
    );
}

#[tokio::test]
async fn invoice_eml_and_template_validation() {
    let s = shop(None).await;
    let o = s.place_order().await;
    let (st, eml) = s
        .admin_get(&format!("/admin/orders/{}/invoice.eml", o.id))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(eml.starts_with("X-Unsent: 1\r\n"));
    let csrf = s.csrf_for(o.id).await;
    let (st, body, _) = s
        .post(
            "/admin/settings",
            &[
                ("csrf", &csrf),
                ("subject", "Invoice"),
                ("body", "no link"),
                ("action", "save"),
            ],
            true,
        )
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("pay_link"));
}

#[tokio::test]
async fn pages_render() {
    let s = shop(None).await;
    for p in [
        "/",
        "/shop.html",
        "/cart.html",
        "/checkout.html",
        "/recipes.html",
        "/recipe-scones.html",
        "/css/style.css",
    ] {
        assert_eq!(s.get(p).await.0, StatusCode::OK, "{p}");
    }
    assert_eq!(s.get("/nope").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        s.get("/pay/aaaaaaaaaaaaaaaaaaaaaaaa").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn connect_flow_fetches_the_keys_and_takes_over() {
    let (url, fake) = fake_monokulo().await;
    let s = shop_with(Some(url.clone()), false).await;
    let o = s.place_order().await;
    s.invoice(o.id).await;

    // Not connected: Monero isn't offered, bank transfer is.
    let (_, page) = s.get(&format!("/pay/{}", o.pay_token)).await;
    assert!(!page.contains("monokulo-client.js") && page.contains(&o.bank_ref));
    let (_, admin) = s.admin_get("/admin/monokulo").await;
    assert!(admin.contains("Not connected") && admin.contains("Connect to Monokulo"));

    // Connect: off to Monokulo with a nonce and the way back.
    let csrf = s.csrf_for(o.id).await;
    let (st, _, h) = s
        .post("/admin/monokulo/connect", &[("csrf", &csrf)], true)
        .await;
    assert_eq!(st, StatusCode::SEE_OTHER);
    let to = h
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(to.starts_with(&format!("{url}/plugins/connect/handicrafts?site_url=https%3A%2F%2Fshop.example&return_url=https%3A%2F%2Fshop.example%2Fadmin%2Fmonokulo%2Freturn&nonce=")), "{to}");
    let nonce = to
        .split("nonce=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();

    // A return with someone else's nonce is refused, and spends the nonce.
    let (st, body) = s
        .admin_get("/admin/monokulo/return?token=conn_good&nonce=forged")
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(fake.finishes.lock().unwrap().is_empty());
    let (st, _) = s
        .admin_get(&format!(
            "/admin/monokulo/return?token=conn_good&nonce={nonce}"
        ))
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "the nonce works once");

    // Again, properly.
    let (_, _, h) = s
        .post("/admin/monokulo/connect", &[("csrf", &csrf)], true)
        .await;
    let to = h
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let nonce = to
        .split("nonce=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let (st, body) = s
        .admin_get(&format!(
            "/admin/monokulo/return?token=conn_good&nonce={nonce}"
        ))
        .await;
    assert_eq!(st, StatusCode::SEE_OTHER, "{body}");
    let finish = fake.finishes.lock().unwrap()[0].clone();
    assert_eq!(finish["token"], "conn_good");
    assert_eq!(
        finish["webhook_url"],
        "https://shop.example/webhooks/monokulo"
    );
    let (_, admin) = s.admin_get("/admin/monokulo").await;
    assert!(
        admin.contains("Connected") && admin.contains("pk_connected"),
        "{admin}"
    );

    // The connected keys are the ones used.
    let (_, page) = s.get(&format!("/pay/{}", o.pay_token)).await;
    assert!(page.contains("pk_connected"));
    assert_eq!(
        fake.auth.lock().unwrap().last().unwrap(),
        "Bearer sk_connected"
    );

    // Its webhooks are verified with the secret the connection gave.
    let ev = serde_json::json!({
        "event_id": "evt_c1", "event": "order.paid", "order_id": "order_abc",
        "merchant_order_id": o.number, "amount": "105.00", "currency": "AUD"
    });
    assert_eq!(
        s.webhook(&ev, "whsec").await,
        StatusCode::UNAUTHORIZED,
        "the old secret no longer works"
    );
    assert_eq!(s.webhook(&ev, "whsec_connected").await, StatusCode::OK);
    assert_eq!(
        s.app.db.order(o.id).await.unwrap().unwrap().status,
        crate::orders::Status::Paid
    );

    // The test webhook button, and a test delivery arriving.
    let (st, _, _) = s
        .post("/admin/monokulo/test", &[("csrf", &csrf)], true)
        .await;
    assert_eq!(st, StatusCode::SEE_OTHER);
    assert_eq!(
        fake.auth.lock().unwrap().last().unwrap(),
        "Bearer sk_connected"
    );
    let test = serde_json::json!({ "api_version": 2, "event_id": "evt_t1", "event": "webhook.test", "created_at": 1 });
    assert_eq!(s.webhook(&test, "whsec_connected").await, StatusCode::OK);
    let (_, admin) = s.admin_get("/admin/monokulo").await;
    assert!(admin.contains("Last test webhook received"), "{admin}");

    // Disconnect: Monero is off again.
    let (st, _, _) = s
        .post("/admin/monokulo/disconnect", &[("csrf", &csrf)], true)
        .await;
    assert_eq!(st, StatusCode::SEE_OTHER);
    assert!(s.app.monokulo().is_none());
}

#[tokio::test]
async fn a_bad_connect_code_says_why() {
    let (url, _) = fake_monokulo().await;
    let s = shop_with(Some(url), false).await;
    let o = s.place_order().await;
    let csrf = s.csrf_for(o.id).await;
    let (_, _, h) = s
        .post("/admin/monokulo/connect", &[("csrf", &csrf)], true)
        .await;
    let to = h
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let nonce = to
        .split("nonce=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let (st, body) = s
        .admin_get(&format!(
            "/admin/monokulo/return?token=conn_used&nonce={nonce}"
        ))
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("already used or expired"), "{body}");
    assert!(s.app.monokulo().is_none());
}
