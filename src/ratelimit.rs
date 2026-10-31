//! Spam limits. Order limits are counted from the database (so a restart
//! doesn't reset them, see `Db::counts`); this holds the in-memory ones:
//! wrong admin passwords, and the client's real IP behind the proxy.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
};

use axum::http::HeaderMap;

use crate::{config::Limits, db::Counts};

/// The client's IP. The app listens behind the reverse proxy, so the peer is
/// normally the proxy; only then is `X-Forwarded-For` believed, and its last
/// entry (the one the proxy added) is taken. A spoofed header from anywhere
/// else is ignored.
pub fn client_ip(peer: SocketAddr, headers: &HeaderMap, trusted: &[IpAddr]) -> IpAddr {
    let peer_ip = peer.ip().to_canonical();
    if !trusted.iter().any(|t| t.to_canonical() == peer_ip) {
        return peer_ip;
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical())
        .unwrap_or(peer_ip)
}

/// Which order limit, if any, a new order would break.
#[derive(Debug, PartialEq, Eq)]
pub enum OrderLimit {
    /// Refuse the order.
    Ip {
        retry_after: u64,
    },
    Shop,
    /// Save the order but don't email the customer.
    SkipCustomerEmail,
    None,
}

pub fn check_order(c: &Counts, l: &Limits) -> OrderLimit {
    if c.ip_10min >= l.ip_10min {
        return OrderLimit::Ip { retry_after: 600 };
    }
    if c.ip_day >= l.ip_day {
        return OrderLimit::Ip { retry_after: 3600 };
    }
    if c.shop_day >= l.shop_day {
        return OrderLimit::Shop;
    }
    if c.email_day >= l.email_day {
        return OrderLimit::SkipCustomerEmail;
    }
    OrderLimit::None
}

/// A sliding count of events per key over a window, kept in memory.
pub struct Window {
    secs: i64,
    max: usize,
    hits: Mutex<HashMap<String, Vec<i64>>>,
}

impl Window {
    pub fn new(secs: i64, max: usize) -> Window {
        Window {
            secs,
            max,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Whether `key` is over the limit right now (without counting a hit).
    pub fn blocked(&self, key: &str, now: i64) -> bool {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let v = hits.entry(key.to_string()).or_default();
        v.retain(|t| *t > now - self.secs);
        v.len() >= self.max
    }

    pub fn hit(&self, key: &str, now: i64) {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        hits.retain(|_, v| {
            v.retain(|t| *t > now - self.secs);
            !v.is_empty()
        });
        hits.entry(key.to_string()).or_default().push(now);
    }

    pub fn clear(&self, key: &str) {
        self.hits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(xff: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", xff.parse().unwrap());
        h
    }

    #[test]
    fn forwarded_for_only_from_the_proxy() {
        let trusted: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap()];
        let proxy: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let outsider: SocketAddr = "203.0.113.9:5000".parse().unwrap();
        assert_eq!(
            client_ip(proxy, &headers("1.1.1.1, 198.51.100.7"), &trusted).to_string(),
            "198.51.100.7"
        );
        assert_eq!(
            client_ip(outsider, &headers("1.1.1.1"), &trusted).to_string(),
            "203.0.113.9"
        );
        assert_eq!(
            client_ip(proxy, &HeaderMap::new(), &trusted).to_string(),
            "127.0.0.1"
        );
    }

    #[test]
    fn order_limits() {
        let l = Limits::default();
        let c = |a, b, e, s| Counts {
            ip_10min: a,
            ip_day: b,
            email_day: e,
            shop_day: s,
        };
        assert_eq!(check_order(&c(0, 0, 0, 0), &l), OrderLimit::None);
        assert_eq!(
            check_order(&c(3, 3, 0, 3), &l),
            OrderLimit::Ip { retry_after: 600 }
        );
        assert_eq!(
            check_order(&c(0, 10, 0, 10), &l),
            OrderLimit::Ip { retry_after: 3600 }
        );
        assert_eq!(check_order(&c(0, 0, 0, 40), &l), OrderLimit::Shop);
        assert_eq!(
            check_order(&c(0, 0, 3, 3), &l),
            OrderLimit::SkipCustomerEmail
        );
    }

    #[test]
    fn window() {
        let w = Window::new(900, 5);
        for i in 0..5 {
            assert!(!w.blocked("ip", i));
            w.hit("ip", i);
        }
        assert!(w.blocked("ip", 10));
        assert!(!w.blocked("ip", 1000), "expires");
    }
}
