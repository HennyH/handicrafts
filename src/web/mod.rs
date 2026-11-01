//! HTTP: the router, shared state and small helpers.

mod admin;
mod pay;
mod shop;
#[cfg(test)]
mod tests;
mod webhook;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use maud::Markup;

use crate::{
    catalog::Catalog,
    config::Config,
    db::{self, Db},
    mail::Transport,
    monokulo,
    ratelimit::{self, Window},
};

pub struct App {
    pub cfg: Config,
    pub db: Db,
    pub catalog: Catalog,
    pub mail: Arc<Transport>,
    /// The Monokulo store Monero goes through, if any: see `refresh_monokulo`.
    monokulo: std::sync::RwLock<Option<Arc<monokulo::Client>>>,
    /// Wakes the outbox sender when mail is queued.
    pub outbox: Arc<tokio::sync::Notify>,
    /// Random per start; admin forms carry an HMAC of it.
    pub csrf_secret: [u8; 32],
    pub admin_failures: Window,
    /// The last admin credentials that passed, hashed, so argon2 runs once
    /// per login rather than on every request.
    pub admin_ok: std::sync::Mutex<Option<[u8; 32]>>,
    /// Monokulo order creation per pay link.
    pub monokulo_starts: Window,
}

pub type State = axum::extract::State<Arc<App>>;

impl App {
    pub fn new(cfg: Config, db: Db, mail: Arc<Transport>) -> App {
        let monokulo = cfg
            .monokulo_from_keys()
            .map(|m| Arc::new(monokulo::Client::new(m)));
        App {
            cfg,
            db,
            catalog: Catalog::load(),
            mail,
            monokulo: std::sync::RwLock::new(monokulo),
            outbox: Default::default(),
            csrf_secret: rand::random(),
            admin_failures: Window::new(15 * 60, 5),
            admin_ok: Default::default(),
            monokulo_starts: Window::new(3600, 6),
        }
    }

    /// The Monokulo client, if Monero is set up.
    pub fn monokulo(&self) -> Option<Arc<monokulo::Client>> {
        self.monokulo
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Where the Monokulo keys come from now.
    pub async fn monokulo_source(&self) -> MonokuloSource {
        match self.db.monokulo_connection().await {
            Ok(Some(c)) if Some(&c.url) == self.cfg.monokulo_url.as_ref() => {
                MonokuloSource::Connected(c)
            }
            _ if self.cfg.monokulo_from_keys().is_some() => MonokuloSource::Keys,
            _ => MonokuloSource::None,
        }
    }

    /// Pick up the store connected at /admin/monokulo (it wins), or else the
    /// keys pasted into LuCI. A connection to a different Monokulo than the
    /// one now set in LuCI is ignored.
    pub async fn refresh_monokulo(&self) {
        let cfg = match self.monokulo_source().await {
            MonokuloSource::Connected(c) => Some(c),
            MonokuloSource::Keys => self.cfg.monokulo_from_keys(),
            MonokuloSource::None => None,
        };
        let client = cfg.map(|c| Arc::new(monokulo::Client::new(c)));
        *self.monokulo.write().unwrap_or_else(|e| e.into_inner()) = client;
    }

    pub fn client_ip(&self, peer: SocketAddr, headers: &HeaderMap) -> String {
        ratelimit::client_ip(peer, headers, &self.cfg.trusted_proxies).to_string()
    }

    /// Queue a notification for the admin, if there's an admin address.
    pub fn admin_email(&self) -> Option<&str> {
        self.cfg.admin_email.as_deref()
    }

    pub async fn queue(&self, order_id: Option<i64>, mail: db::Mail) {
        if let Err(e) = self.db.queue(order_id, mail).await {
            eprintln!("handicrafts: couldn't queue an email: {e}");
        }
        self.outbox.notify_one();
    }
}

pub enum MonokuloSource {
    /// Connected at /admin/monokulo.
    Connected(crate::config::Monokulo),
    /// Keys pasted into LuCI.
    Keys,
    None,
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route(
            "/orders",
            post(shop::place_order).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/pay/{token}", get(pay::page))
        .route("/pay/{token}/status", get(pay::status))
        .route("/pay/{token}/monero", get(pay::monero_card))
        .route(
            "/pay/{token}/refund",
            post(pay::refund).layer(DefaultBodyLimit::max(4 * 1024)),
        )
        .route(
            "/pay/{token}/claim",
            post(pay::claim).layer(DefaultBodyLimit::max(4 * 1024)),
        )
        .route("/pay/{token}/thanks", get(pay::thanks))
        .route(
            "/webhooks/monokulo",
            post(webhook::monokulo).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .merge(admin::router(app.clone()))
        .fallback(shop::page_or_file)
        .with_state(app)
}

/// Whether fixi made the request (it then wants a fragment, not a page).
pub fn is_fixi(headers: &HeaderMap) -> bool {
    headers.contains_key("fx-request")
}

pub fn html(status: StatusCode, m: Markup) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Html(m.into_string()),
    )
        .into_response()
}

pub fn ok(m: Markup) -> Response {
    html(StatusCode::OK, m)
}

pub fn see_other(to: &str) -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, to.to_string())]).into_response()
}

/// A database or other internal failure, logged, with a polite page.
pub fn oops(e: impl std::fmt::Display) -> Response {
    eprintln!("handicrafts: {e}");
    html(
        StatusCode::INTERNAL_SERVER_ERROR,
        crate::views::shop::problem(
            "Something went wrong",
            "Sorry, the shop couldn't do that just now. Please try again in a minute.",
        ),
    )
}
