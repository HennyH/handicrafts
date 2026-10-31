//! SQLite, through one connection behind a mutex. A handful of orders a week
//! never needs more; every call runs on tokio's blocking pool.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{catalog::PricedLine, orders::Status, util};

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

pub type Result<T> = std::result::Result<T, String>;

fn e(err: rusqlite::Error) -> String {
    format!("database: {err}")
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS orders (
  id INTEGER PRIMARY KEY,
  number TEXT NOT NULL UNIQUE,
  bank_ref TEXT NOT NULL UNIQUE,
  pay_token TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL CHECK (status IN ('new','invoiced','paid','dispatched','completed','rejected')),
  accepted INTEGER NOT NULL DEFAULT 0,
  customer_name TEXT NOT NULL,
  customer_email TEXT NOT NULL,
  customer_phone TEXT NOT NULL DEFAULT '',
  customer_address TEXT NOT NULL DEFAULT '',
  customer_notes TEXT NOT NULL DEFAULT '',
  total_cents INTEGER NOT NULL,
  currency TEXT NOT NULL DEFAULT 'AUD',
  client_ip TEXT NOT NULL DEFAULT '',
  refund_address TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS orders_ip ON orders (client_ip, created_at);
CREATE INDEX IF NOT EXISTS orders_created ON orders (created_at);

CREATE TABLE IF NOT EXISTS order_items (
  id INTEGER PRIMARY KEY,
  order_id INTEGER NOT NULL REFERENCES orders (id),
  product_id TEXT NOT NULL,
  sku TEXT NOT NULL,
  name TEXT NOT NULL,
  color TEXT,
  materials TEXT,
  image TEXT,
  unit_cents INTEGER NOT NULL,
  qty INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS order_items_order ON order_items (order_id);

-- Comments, status changes, payment claims and payments: the order's timeline.
CREATE TABLE IF NOT EXISTS order_events (
  id INTEGER PRIMARY KEY,
  order_id INTEGER NOT NULL REFERENCES orders (id),
  kind TEXT NOT NULL,
  actor TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  from_status TEXT,
  to_status TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS order_events_order ON order_events (order_id, created_at);

-- Orders made on Monokulo for an order's Monero payment (a new one when the
-- price hold runs out).
CREATE TABLE IF NOT EXISTS monokulo_orders (
  id INTEGER PRIMARY KEY,
  order_id INTEGER NOT NULL REFERENCES orders (id),
  mk_order_id TEXT NOT NULL UNIQUE,
  amount_cents INTEGER NOT NULL,
  xmr_piconero INTEGER NOT NULL DEFAULT 0,
  expires_at INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'pending',
  created_at INTEGER NOT NULL
);

-- Webhook event ids already handled; Monokulo resends byte for byte.
CREATE TABLE IF NOT EXISTS webhook_events (
  event_id TEXT PRIMARY KEY,
  received_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS outbox (
  id INTEGER PRIMARY KEY,
  order_id INTEGER,
  kind TEXT NOT NULL,
  to_addr TEXT NOT NULL,
  to_name TEXT NOT NULL DEFAULT '',
  subject TEXT NOT NULL,
  text TEXT NOT NULL,
  html TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  next_attempt_at INTEGER NOT NULL,
  sent_at INTEGER,
  failed_at INTEGER,
  last_error TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS outbox_due ON outbox (sent_at, failed_at, next_attempt_at);

CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);

-- Problems the admin should see on /admin until they clear, e.g. Monokulo
-- not pricing in AUD.
CREATE TABLE IF NOT EXISTS alerts (
  key TEXT PRIMARY KEY,
  message TEXT NOT NULL,
  since INTEGER NOT NULL,
  emailed_at INTEGER NOT NULL DEFAULT 0
);
"#;

#[derive(Debug, Clone)]
#[allow(dead_code)] // read by the tests and kept for completeness
pub struct Order {
    pub id: i64,
    pub number: String,
    pub bank_ref: String,
    pub pay_token: String,
    pub status: Status,
    pub accepted: bool,
    pub customer_name: String,
    pub customer_email: String,
    pub customer_phone: String,
    pub customer_address: String,
    pub customer_notes: String,
    pub total_cents: i64,
    pub currency: String,
    pub refund_address: String,
    pub created_at: i64,
    pub updated_at: i64,
}

const ORDER_COLS: &str = "id, number, bank_ref, pay_token, status, accepted, customer_name, customer_email, \
    customer_phone, customer_address, customer_notes, total_cents, currency, refund_address, created_at, updated_at";

fn order_row(r: &Row) -> rusqlite::Result<Order> {
    let status: String = r.get(4)?;
    Ok(Order {
        id: r.get(0)?,
        number: r.get(1)?,
        bank_ref: r.get(2)?,
        pay_token: r.get(3)?,
        status: Status::parse(&status).unwrap_or(Status::New),
        accepted: r.get(5)?,
        customer_name: r.get(6)?,
        customer_email: r.get(7)?,
        customer_phone: r.get(8)?,
        customer_address: r.get(9)?,
        customer_notes: r.get(10)?,
        total_cents: r.get(11)?,
        currency: r.get(12)?,
        refund_address: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Event {
    pub kind: String,
    pub actor: String,
    pub body: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewOrder {
    pub customer_name: String,
    pub customer_email: String,
    pub customer_phone: String,
    pub customer_address: String,
    pub customer_notes: String,
    pub client_ip: String,
    pub lines: Vec<PricedLine>,
}

/// An email to queue in the same transaction as what caused it.
#[derive(Debug, Clone)]
pub struct Mail {
    pub kind: &'static str,
    pub to_addr: String,
    pub to_name: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OutboxRow {
    pub id: i64,
    pub order_id: Option<i64>,
    pub kind: String,
    pub to_addr: String,
    pub to_name: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct MonokuloOrder {
    pub mk_order_id: String,
    pub amount_cents: i64,
    pub xmr_piconero: i64,
    pub expires_at: i64,
    pub status: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Alert {
    pub key: String,
    pub message: String,
    pub since: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub ip_10min: u32,
    pub ip_day: u32,
    pub email_day: u32,
    pub shop_day: u32,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| format!("can't make {}: {err}", dir.display()))?;
        }
        let conn = Connection::open(path)
            .map_err(|err| format!("can't open {}: {err}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn memory() -> Db {
        Self::init(Connection::open_in_memory().expect("in-memory database")).expect("schema")
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
        )
        .map_err(e)?;
        conn.execute_batch(SCHEMA).map_err(e)?;
        Ok(Db(Arc::new(Mutex::new(conn))))
    }

    /// Run `f` with the connection on the blocking pool.
    pub async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut conn)
        })
        .await
        .map_err(|err| format!("database task: {err}"))?
    }

    // ---------------------------------------------------------------- orders

    /// Save a new order with its lines, its first timeline entry and the
    /// emails it causes, all or nothing. `mails` gets the saved order so it
    /// can fill in the number and links.
    pub async fn create_order(
        &self,
        new: NewOrder,
        mails: impl FnOnce(&Order, &[PricedLine]) -> Vec<Mail> + Send + 'static,
    ) -> Result<Order> {
        self.call(move |conn| {
            let tx = conn.transaction().map_err(e)?;
            let now = util::now();
            let total: i64 = new.lines.iter().map(PricedLine::total_cents).sum();
            let mut tries = 0;
            let id = loop {
                tries += 1;
                let res = tx.execute(
                    "INSERT INTO orders (number, bank_ref, pay_token, status, customer_name, customer_email, \
                     customer_phone, customer_address, customer_notes, total_cents, client_ip, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, 'new', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                    params![
                        util::order_number(now),
                        util::bank_ref(),
                        util::token(24),
                        new.customer_name,
                        new.customer_email,
                        new.customer_phone,
                        new.customer_address,
                        new.customer_notes,
                        total,
                        new.client_ip,
                        now
                    ],
                );
                match res {
                    Ok(_) => break tx.last_insert_rowid(),
                    // A clash on the number, reference or token: draw again.
                    Err(rusqlite::Error::SqliteFailure(f, _))
                        if f.code == rusqlite::ErrorCode::ConstraintViolation && tries < 20 => {}
                    Err(err) => return Err(e(err)),
                }
            };
            for l in &new.lines {
                tx.execute(
                    "INSERT INTO order_items (order_id, product_id, sku, name, color, materials, image, unit_cents, qty) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![id, l.product_id, l.sku, l.name, l.color, l.materials, l.image, l.unit_cents, l.qty],
                )
                .map_err(e)?;
            }
            event_tx(&tx, id, "system", "website", &format!("Order placed. Total {}.", util::aud(total)), None)?;
            let order = get_order(&tx, id)?.ok_or("order vanished")?;
            for m in mails(&order, &new.lines) {
                queue_tx(&tx, Some(id), &m)?;
            }
            tx.commit().map_err(e)?;
            Ok(order)
        })
        .await
    }

    pub async fn order(&self, id: i64) -> Result<Option<Order>> {
        self.call(move |c| get_order(c, id)).await
    }

    pub async fn order_by_token(&self, token: String) -> Result<Option<Order>> {
        self.call(move |c| {
            c.query_row(
                &format!("SELECT {ORDER_COLS} FROM orders WHERE pay_token = ?1"),
                [token],
                order_row,
            )
            .optional()
            .map_err(e)
        })
        .await
    }

    #[cfg(test)]
    pub async fn order_by_number(&self, number: String) -> Result<Option<Order>> {
        self.call(move |c| {
            c.query_row(
                &format!("SELECT {ORDER_COLS} FROM orders WHERE number = ?1"),
                [number],
                order_row,
            )
            .optional()
            .map_err(e)
        })
        .await
    }

    pub async fn items(&self, order_id: i64) -> Result<Vec<PricedLine>> {
        self.call(move |c| items(c, order_id)).await
    }

    pub async fn events(&self, order_id: i64) -> Result<Vec<Event>> {
        self.call(move |c| {
            let mut st = c
                .prepare(
                    "SELECT kind, actor, body, from_status, to_status, created_at FROM order_events \
                     WHERE order_id = ?1 ORDER BY created_at DESC, id DESC",
                )
                .map_err(e)?;
            st.query_map([order_id], |r| {
                Ok(Event {
                    kind: r.get(0)?,
                    actor: r.get(1)?,
                    body: r.get(2)?,
                    from_status: r.get(3)?,
                    to_status: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })
            .map_err(e)?
            .collect::<rusqlite::Result<_>>()
            .map_err(e)
        })
        .await
    }

    /// Newest first. `status` filters; `q` matches the number, bank
    /// reference, name or email.
    pub async fn list_orders(
        &self,
        status: Option<Status>,
        q: Option<String>,
    ) -> Result<Vec<Order>> {
        self.call(move |c| {
            let like = q.map(|q| {
                let q = q.trim().to_string();
                (format!("%{q}%"), util::normalise_ref(&q))
            });
            let mut st = c
                .prepare(&format!(
                    "SELECT {ORDER_COLS} FROM orders \
                     WHERE (?1 IS NULL OR status = ?1) \
                       AND (?2 IS NULL OR number LIKE ?2 OR customer_name LIKE ?2 OR customer_email LIKE ?2 OR bank_ref = ?3) \
                     ORDER BY created_at DESC, id DESC LIMIT 500"
                ))
                .map_err(e)?;
            let (l, r) = match &like {
                Some((l, r)) => (Some(l.as_str()), Some(r.as_str())),
                None => (None, None),
            };
            st.query_map(params![status.map(Status::as_str), l, r], order_row)
                .map_err(e)?
                .collect::<rusqlite::Result<_>>()
                .map_err(e)
        })
        .await
    }

    pub async fn status_counts(&self) -> Result<Vec<(Status, i64)>> {
        self.call(|c| {
            let mut st = c
                .prepare("SELECT status, COUNT(*) FROM orders GROUP BY status")
                .map_err(e)?;
            let rows: Vec<(String, i64)> = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(e)?
                .collect::<rusqlite::Result<_>>()
                .map_err(e)?;
            Ok(rows
                .into_iter()
                .filter_map(|(s, n)| Status::parse(&s).map(|s| (s, n)))
                .collect())
        })
        .await
    }

    pub async fn add_event(
        &self,
        order_id: i64,
        kind: &'static str,
        actor: &'static str,
        body: String,
    ) -> Result<()> {
        self.call(move |c| event_tx(c, order_id, kind, actor, &body, None))
            .await
    }

    /// An admin's status change, already checked by `orders::admin_change`
    /// against `expected`; refused if someone else changed the order since.
    pub async fn set_status(
        &self,
        order_id: i64,
        expected: Status,
        to: Status,
        accepted: bool,
        actor: &'static str,
    ) -> Result<()> {
        self.call(move |c| {
            let tx = c.transaction().map_err(e)?;
            let current = get_order(&tx, order_id)?.ok_or("no such order")?;
            if current.status != expected {
                return Err(
                    "The order changed while you were looking at it. Check it and try again."
                        .into(),
                );
            }
            change_status_tx(&tx, &current, to, accepted, actor)?;
            tx.commit().map_err(e)
        })
        .await
    }

    pub async fn set_refund_address(&self, order_id: i64, address: String) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "UPDATE orders SET refund_address = ?2, updated_at = ?3 WHERE id = ?1",
                params![order_id, address, util::now()],
            )
            .map_err(e)?;
            Ok(())
        })
        .await
    }

    /// Record a customer's "I've paid" and queue the admin's email, unless
    /// they pressed it for this method in the last 10 minutes. `Ok(false)`
    /// when it was a repeat.
    pub async fn claim_payment(
        &self,
        order_id: i64,
        method: &'static str,
        body: String,
        mail: Option<Mail>,
    ) -> Result<bool> {
        self.call(move |c| {
            let tx = c.transaction().map_err(e)?;
            let recent: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM order_events WHERE order_id = ?1 AND kind = 'claim' AND to_status = ?2 AND created_at > ?3",
                    params![order_id, method, util::now() - 600],
                    |r| r.get(0),
                )
                .map_err(e)?;
            if recent > 0 {
                return Ok(false);
            }
            tx.execute(
                "INSERT INTO order_events (order_id, kind, actor, body, to_status, created_at) VALUES (?1, 'claim', 'customer', ?2, ?3, ?4)",
                params![order_id, body, method, util::now()],
            )
            .map_err(e)?;
            if let Some(m) = mail {
                queue_tx(&tx, Some(order_id), &m)?;
            }
            tx.commit().map_err(e)?;
            Ok(true)
        })
        .await
    }

    /// How many orders this IP, this email address and the whole shop have
    /// placed recently, for the spam limits.
    pub async fn counts(&self, ip: String, email: String) -> Result<Counts> {
        self.call(move |c| {
            let now = util::now();
            let count = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> Result<u32> {
                c.query_row(sql, p, |r| r.get::<_, i64>(0)).map(|n| n as u32).map_err(e)
            };
            Ok(Counts {
                ip_10min: count(
                    "SELECT COUNT(*) FROM orders WHERE client_ip = ?1 AND created_at > ?2",
                    &[&ip, &(now - 600)],
                )?,
                ip_day: count(
                    "SELECT COUNT(*) FROM orders WHERE client_ip = ?1 AND created_at > ?2",
                    &[&ip, &(now - 86400)],
                )?,
                email_day: count(
                    "SELECT COUNT(*) FROM outbox WHERE kind = 'customer_order' AND lower(to_addr) = lower(?1) AND created_at > ?2",
                    &[&email, &(now - 86400)],
                )?,
                shop_day: count("SELECT COUNT(*) FROM orders WHERE created_at > ?1", &[&(now - 86400)])?,
            })
        })
        .await
    }

    // ---------------------------------------------------------------- outbox

    pub async fn queue(&self, order_id: Option<i64>, mail: Mail) -> Result<()> {
        self.call(move |c| queue_tx(c, order_id, &mail)).await
    }

    /// Mail due now, oldest first.
    pub async fn due_mail(&self, limit: i64) -> Result<Vec<OutboxRow>> {
        self.call(move |c| {
            outbox_query(
                c,
                "WHERE sent_at IS NULL AND failed_at IS NULL AND next_attempt_at <= ?1 ORDER BY id LIMIT ?2",
                params![util::now(), limit],
            )
        })
        .await
    }

    pub async fn failed_mail(&self) -> Result<Vec<OutboxRow>> {
        self.call(|c| {
            outbox_query(
                c,
                "WHERE failed_at IS NOT NULL ORDER BY id DESC LIMIT 50",
                [],
            )
        })
        .await
    }

    pub async fn mail_sent(&self, id: i64) -> Result<()> {
        self.call(move |c| {
            c.execute("UPDATE outbox SET sent_at = ?2, attempts = attempts + 1, last_error = NULL WHERE id = ?1", params![id, util::now()])
                .map_err(e)?;
            Ok(())
        })
        .await
    }

    /// A failed attempt: retry with growing gaps (1 min, 2, 4 … up to an
    /// hour), and give up after about a day.
    pub async fn mail_failed(&self, id: i64, attempts: i64, error: String) -> Result<bool> {
        self.call(move |c| {
            let now = util::now();
            let attempts = attempts + 1;
            let give_up = attempts >= 30;
            let wait = (60i64 << (attempts - 1).min(6)).min(3600);
            c.execute(
                "UPDATE outbox SET attempts = ?2, last_error = ?3, next_attempt_at = ?4, failed_at = ?5 WHERE id = ?1",
                params![id, attempts, error, now + wait, give_up.then_some(now)],
            )
            .map_err(e)?;
            Ok(give_up)
        })
        .await
    }

    pub async fn resend_mail(&self, id: i64) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "UPDATE outbox SET failed_at = NULL, attempts = 0, next_attempt_at = ?2 WHERE id = ?1",
                params![id, util::now()],
            )
            .map_err(e)?;
            Ok(())
        })
        .await
    }

    // -------------------------------------------------------------- settings

    pub async fn setting(&self, key: &'static str) -> Result<Option<String>> {
        self.call(move |c| {
            c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(e)
        })
        .await
    }

    pub async fn set_setting(&self, key: &'static str, value: String) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(e)?;
            Ok(())
        })
        .await
    }

    pub async fn delete_setting(&self, key: &'static str) -> Result<()> {
        self.call(move |c| {
            c.execute("DELETE FROM settings WHERE key = ?1", [key])
                .map_err(e)?;
            Ok(())
        })
        .await
    }

    // -------------------------------------------------------------- monokulo

    /// The store connected at /admin/monokulo, if any.
    pub async fn monokulo_connection(&self) -> Result<Option<crate::config::Monokulo>> {
        Ok(self
            .setting("monokulo_connection")
            .await?
            .and_then(|j| serde_json::from_str(&j).ok()))
    }

    pub async fn set_monokulo_connection(&self, c: &crate::config::Monokulo) -> Result<()> {
        let json = serde_json::to_string(c).map_err(|err| err.to_string())?;
        self.set_setting("monokulo_connection", json).await
    }

    /// The newest Monokulo order made for this order.
    pub async fn monokulo_order(&self, order_id: i64) -> Result<Option<MonokuloOrder>> {
        self.call(move |c| {
            c.query_row(
                "SELECT mk_order_id, amount_cents, xmr_piconero, expires_at, status FROM monokulo_orders \
                 WHERE order_id = ?1 ORDER BY id DESC LIMIT 1",
                [order_id],
                |r| {
                    Ok(MonokuloOrder {
                        mk_order_id: r.get(0)?,
                        amount_cents: r.get(1)?,
                        xmr_piconero: r.get(2)?,
                        expires_at: r.get(3)?,
                        status: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(e)
        })
        .await
    }

    pub async fn monokulo_order_count(&self, order_id: i64) -> Result<i64> {
        self.call(move |c| {
            c.query_row(
                "SELECT COUNT(*) FROM monokulo_orders WHERE order_id = ?1",
                [order_id],
                |r| r.get(0),
            )
            .map_err(e)
        })
        .await
    }

    pub async fn add_monokulo_order(&self, order_id: i64, mk: MonokuloOrder) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO monokulo_orders (order_id, mk_order_id, amount_cents, xmr_piconero, expires_at, status, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT (mk_order_id) DO NOTHING",
                params![order_id, mk.mk_order_id, mk.amount_cents, mk.xmr_piconero, mk.expires_at, mk.status, util::now()],
            )
            .map_err(e)?;
            Ok(())
        })
        .await
    }

    /// Handle one verified webhook delivery exactly once: `f` runs inside
    /// the transaction that records the event id, and only the first time.
    /// `Ok(false)` for a repeat.
    pub async fn webhook_once(
        &self,
        event_id: String,
        f: impl FnOnce(&rusqlite::Transaction) -> Result<()> + Send + 'static,
    ) -> Result<bool> {
        self.call(move |c| {
            let tx = c.transaction().map_err(e)?;
            let fresh = tx
                .execute(
                    "INSERT INTO webhook_events (event_id, received_at) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                    params![event_id, util::now()],
                )
                .map_err(e)?
                == 1;
            if fresh {
                f(&tx)?;
            }
            tx.commit().map_err(e)?;
            Ok(fresh)
        })
        .await
    }

    // ---------------------------------------------------------------- alerts

    /// Raise (or keep) an alert. `Ok(true)` when the admin should be emailed
    /// about it: the first time, then at most once an hour.
    pub async fn raise_alert(&self, key: &'static str, message: String) -> Result<bool> {
        self.call(move |c| {
            let now = util::now();
            c.execute(
                "INSERT INTO alerts (key, message, since) VALUES (?1, ?2, ?3) \
                 ON CONFLICT (key) DO UPDATE SET message = excluded.message",
                params![key, message, now],
            )
            .map_err(e)?;
            let n = c
                .execute(
                    "UPDATE alerts SET emailed_at = ?2 WHERE key = ?1 AND emailed_at <= ?3",
                    params![key, now, now - 3600],
                )
                .map_err(e)?;
            Ok(n == 1)
        })
        .await
    }

    pub async fn clear_alert(&self, key: &'static str) -> Result<()> {
        self.call(move |c| {
            c.execute("DELETE FROM alerts WHERE key = ?1", [key])
                .map_err(e)?;
            Ok(())
        })
        .await
    }

    pub async fn alerts(&self) -> Result<Vec<Alert>> {
        self.call(|c| {
            let mut st = c
                .prepare("SELECT key, message, since FROM alerts ORDER BY since")
                .map_err(e)?;
            st.query_map([], |r| {
                Ok(Alert {
                    key: r.get(0)?,
                    message: r.get(1)?,
                    since: r.get(2)?,
                })
            })
            .map_err(e)?
            .collect::<rusqlite::Result<_>>()
            .map_err(e)
        })
        .await
    }
}

// ------------------------------------------------- helpers usable inside a tx

pub fn get_order(c: &Connection, id: i64) -> Result<Option<Order>> {
    c.query_row(
        &format!("SELECT {ORDER_COLS} FROM orders WHERE id = ?1"),
        [id],
        order_row,
    )
    .optional()
    .map_err(e)
}

pub fn get_order_by_number(c: &Connection, number: &str) -> Result<Option<Order>> {
    c.query_row(
        &format!("SELECT {ORDER_COLS} FROM orders WHERE number = ?1"),
        [number],
        order_row,
    )
    .optional()
    .map_err(e)
}

pub fn items(c: &Connection, order_id: i64) -> Result<Vec<PricedLine>> {
    let mut st = c
        .prepare(
            "SELECT product_id, sku, name, color, materials, image, unit_cents, qty FROM order_items \
             WHERE order_id = ?1 ORDER BY id",
        )
        .map_err(e)?;
    st.query_map([order_id], |r| {
        Ok(PricedLine {
            product_id: r.get(0)?,
            sku: r.get(1)?,
            name: r.get(2)?,
            color: r.get(3)?,
            materials: r.get(4)?,
            image: r.get(5)?,
            unit_cents: r.get(6)?,
            qty: r.get(7)?,
        })
    })
    .map_err(e)?
    .collect::<rusqlite::Result<_>>()
    .map_err(e)
}

pub fn event_tx(
    c: &Connection,
    order_id: i64,
    kind: &str,
    actor: &str,
    body: &str,
    status: Option<(Status, Status)>,
) -> Result<()> {
    c.execute(
        "INSERT INTO order_events (order_id, kind, actor, body, from_status, to_status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            order_id,
            kind,
            actor,
            body,
            status.map(|(f, _)| f.as_str()),
            status.map(|(_, t)| t.as_str()),
            util::now()
        ],
    )
    .map_err(e)?;
    Ok(())
}

/// Change status (and Accepted) and write it to the timeline.
pub fn change_status_tx(
    c: &Connection,
    order: &Order,
    to: Status,
    accepted: bool,
    actor: &str,
) -> Result<()> {
    c.execute(
        "UPDATE orders SET status = ?2, accepted = ?3, updated_at = ?4 WHERE id = ?1",
        params![order.id, to.as_str(), accepted, util::now()],
    )
    .map_err(e)?;
    if accepted != order.accepted && to == order.status {
        let body = if accepted {
            "Ticked Accepted."
        } else {
            "Unticked Accepted."
        };
        event_tx(c, order.id, "status", actor, body, None)?;
    }
    if to != order.status {
        let body = format!("{} → {}", order.status.label(), to.label());
        event_tx(
            c,
            order.id,
            "status",
            actor,
            &body,
            Some((order.status, to)),
        )?;
    }
    Ok(())
}

pub fn queue_tx(c: &Connection, order_id: Option<i64>, m: &Mail) -> Result<()> {
    let now = util::now();
    c.execute(
        "INSERT INTO outbox (order_id, kind, to_addr, to_name, subject, text, html, next_attempt_at, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![order_id, m.kind, m.to_addr, m.to_name, m.subject, m.text, m.html, now],
    )
    .map_err(e)?;
    Ok(())
}

pub fn set_monokulo_status(c: &Connection, mk_order_id: &str, status: &str) -> Result<()> {
    c.execute(
        "UPDATE monokulo_orders SET status = ?2 WHERE mk_order_id = ?1",
        params![mk_order_id, status],
    )
    .map_err(e)?;
    Ok(())
}

fn outbox_query(c: &Connection, tail: &str, p: impl rusqlite::Params) -> Result<Vec<OutboxRow>> {
    let mut st = c
        .prepare(&format!(
            "SELECT id, order_id, kind, to_addr, to_name, subject, text, html, attempts, last_error, created_at FROM outbox {tail}"
        ))
        .map_err(e)?;
    st.query_map(p, |r| {
        Ok(OutboxRow {
            id: r.get(0)?,
            order_id: r.get(1)?,
            kind: r.get(2)?,
            to_addr: r.get(3)?,
            to_name: r.get(4)?,
            subject: r.get(5)?,
            text: r.get(6)?,
            html: r.get(7)?,
            attempts: r.get(8)?,
            last_error: r.get(9)?,
            created_at: r.get(10)?,
        })
    })
    .map_err(e)?
    .collect::<rusqlite::Result<_>>()
    .map_err(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CartLine, Catalog};

    fn new_order(ip: &str) -> NewOrder {
        let lines = Catalog::load()
            .price_cart(&[CartLine {
                id: "banneton".into(),
                color: None,
                qty: 2,
            }])
            .unwrap();
        NewOrder {
            customer_name: "Sam Example".into(),
            customer_email: "sam@example.com".into(),
            customer_phone: String::new(),
            customer_address: "local pickup".into(),
            customer_notes: String::new(),
            client_ip: ip.into(),
            lines,
        }
    }

    fn mail(kind: &'static str, to: &str) -> Mail {
        Mail {
            kind,
            to_addr: to.into(),
            to_name: String::new(),
            subject: "s".into(),
            text: "t".into(),
            html: "h".into(),
        }
    }

    #[tokio::test]
    async fn order_round_trip() {
        let db = Db::memory();
        let o = db
            .create_order(new_order("1.2.3.4"), |o, _| {
                vec![mail("customer_order", &o.customer_email)]
            })
            .await
            .unwrap();
        assert_eq!(o.total_cents, 9000);
        assert_eq!(o.status, Status::New);
        assert_eq!(o.bank_ref.len(), 5);
        assert_eq!(db.items(o.id).await.unwrap().len(), 1);
        assert_eq!(db.events(o.id).await.unwrap().len(), 1);
        assert_eq!(db.due_mail(10).await.unwrap().len(), 1);
        let found = db
            .list_orders(None, Some(o.bank_ref.to_lowercase()))
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        let c = db
            .counts("1.2.3.4".into(), "SAM@example.com".into())
            .await
            .unwrap();
        assert_eq!(
            (c.ip_10min, c.ip_day, c.email_day, c.shop_day),
            (1, 1, 1, 1)
        );
    }

    #[tokio::test]
    async fn status_and_claims() {
        let db = Db::memory();
        let o = db.create_order(new_order(""), |_, _| vec![]).await.unwrap();
        db.set_status(o.id, Status::New, Status::Invoiced, true, "admin")
            .await
            .unwrap();
        assert!(
            db.set_status(o.id, Status::New, Status::Rejected, false, "admin")
                .await
                .is_err(),
            "stale"
        );
        let o = db.order(o.id).await.unwrap().unwrap();
        assert_eq!((o.status, o.accepted), (Status::Invoiced, true));
        assert!(
            db.claim_payment(o.id, "bank", "said so".into(), None)
                .await
                .unwrap()
        );
        assert!(
            !db.claim_payment(o.id, "bank", "again".into(), None)
                .await
                .unwrap()
        );
        assert!(
            db.claim_payment(o.id, "monero", "xmr".into(), None)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn outbox_retries_then_gives_up() {
        let db = Db::memory();
        db.queue(None, mail("admin_order", "a@example.com"))
            .await
            .unwrap();
        let m = &db.due_mail(10).await.unwrap()[0];
        assert!(!db.mail_failed(m.id, 0, "down".into()).await.unwrap());
        assert!(
            db.due_mail(10).await.unwrap().is_empty(),
            "waits before retrying"
        );
        assert!(db.mail_failed(m.id, 29, "down".into()).await.unwrap());
        assert_eq!(db.failed_mail().await.unwrap().len(), 1);
        db.resend_mail(m.id).await.unwrap();
        assert_eq!(db.due_mail(10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn webhooks_and_alerts_are_once() {
        let db = Db::memory();
        assert!(db.webhook_once("evt_1".into(), |_| Ok(())).await.unwrap());
        assert!(
            !db.webhook_once("evt_1".into(), |_| panic!("ran twice"))
                .await
                .unwrap()
        );
        assert!(db.raise_alert("monokulo", "no AUD".into()).await.unwrap());
        assert!(
            !db.raise_alert("monokulo", "no AUD".into()).await.unwrap(),
            "once an hour"
        );
        assert_eq!(db.alerts().await.unwrap().len(), 1);
        db.clear_alert("monokulo").await.unwrap();
        assert!(db.alerts().await.unwrap().is_empty());
    }
}
