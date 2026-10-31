//! Settings, read from the environment. On the router the init script fills
//! the environment from `/etc/config/handicrafts` (Services › Handicrafts in
//! LuCI); for development, export the variables yourself (see `usage`).

use std::{net::SocketAddr, path::PathBuf};

pub const ENV_HELP: &str = "\
Settings (environment variables):
  HANDICRAFTS_LISTEN              address to serve on        (default 127.0.0.1:8082)
  HANDICRAFTS_PUBLIC_URL          the shop's address, for pay links (default http://localhost:8082)
  HANDICRAFTS_DATA_DIR            where the database lives   (default ./data-dev)
  HANDICRAFTS_TRUSTED_PROXIES     comma-separated proxy IPs whose X-Forwarded-For is believed
                                  (default 127.0.0.1,::1)
  Admin (/admin, HTTP Basic auth):
  HANDICRAFTS_ADMIN_USER          (default admin)
  HANDICRAFTS_ADMIN_PASSWORD_HASH argon2 hash from `handicrafts hash-password`; /admin is
                                  closed until it is set
  HANDICRAFTS_ADMIN_EMAIL         where order and payment notifications go
  Bank transfer details shown on the pay page:
  HANDICRAFTS_PAYID, HANDICRAFTS_BANK_ACCOUNT_NAME, HANDICRAFTS_BSB, HANDICRAFTS_ACCOUNT_NUMBER
  Monokulo (Monero):
  HANDICRAFTS_MONOKULO_URL             e.g. https://www.monokulo.com; then connect the store
                                       at /admin/monokulo, which fetches the keys itself
  Or keys pasted by hand (all three, used until the store is connected at /admin/monokulo):
  HANDICRAFTS_MONOKULO_PUBLIC_KEY      pk_…
  HANDICRAFTS_MONOKULO_SECRET_KEY      sk_…
  HANDICRAFTS_MONOKULO_WEBHOOK_SECRET  the webhook's signing secret
  Spam limits (orders):
  HANDICRAFTS_LIMIT_IP_10MIN (3), HANDICRAFTS_LIMIT_IP_DAY (10),
  HANDICRAFTS_LIMIT_EMAIL_DAY (3), HANDICRAFTS_LIMIT_SHOP_DAY (40)
";

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_url: String,
    pub data_dir: PathBuf,
    pub trusted_proxies: Vec<std::net::IpAddr>,
    pub admin_user: String,
    pub admin_password_hash: Option<String>,
    pub admin_email: Option<String>,
    pub bank: Bank,
    /// Monokulo's address, without a trailing slash.
    pub monokulo_url: Option<String>,
    /// Keys pasted by hand; a connection made at /admin/monokulo wins.
    pub monokulo_keys: Option<MonokuloKeys>,
    pub limits: Limits,
}

#[derive(Clone, Debug, Default)]
pub struct Bank {
    pub payid: Option<String>,
    pub account_name: Option<String>,
    pub bsb: Option<String>,
    pub account_number: Option<String>,
}

impl Bank {
    pub fn any(&self) -> bool {
        self.payid.is_some() || (self.bsb.is_some() && self.account_number.is_some())
    }
}

/// What the shop needs to take Monero through a Monokulo store: from the
/// connect flow at /admin/monokulo, or from keys pasted into LuCI.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Monokulo {
    /// Monokulo's address, without a trailing slash.
    pub url: String,
    /// The HTTP API's base, e.g. https://www.monokulo.com/api/v1.
    pub api: String,
    pub public_key: String,
    pub secret_key: String,
    pub webhook_secret: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonokuloKeys {
    pub public_key: String,
    pub secret_key: String,
    pub webhook_secret: String,
}

impl Config {
    /// The Monokulo store from keys pasted by hand, if there are any.
    pub fn monokulo_from_keys(&self) -> Option<Monokulo> {
        let (url, keys) = (self.monokulo_url.as_ref()?, self.monokulo_keys.as_ref()?);
        Some(Monokulo {
            url: url.clone(),
            api: format!("{url}/api/v1"),
            public_key: keys.public_key.clone(),
            secret_key: keys.secret_key.clone(),
            webhook_secret: keys.webhook_secret.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub ip_10min: u32,
    pub ip_day: u32,
    pub email_day: u32,
    pub shop_day: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            ip_10min: 3,
            ip_day: 10,
            email_day: 3,
            shop_day: 40,
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        Self::from_vars(|k| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let listen = var("HANDICRAFTS_LISTEN").unwrap_or_else(|| "127.0.0.1:8082".into());
        let listen = listen.parse().map_err(|_| {
            format!(
                "HANDICRAFTS_LISTEN must be an address and port like 127.0.0.1:8082, not {listen:?}"
            )
        })?;
        let public_url = var("HANDICRAFTS_PUBLIC_URL")
            .unwrap_or_else(|| "http://localhost:8082".into())
            .trim_end_matches('/')
            .to_string();
        if !(public_url.starts_with("https://") || public_url.starts_with("http://")) {
            return Err(format!(
                "HANDICRAFTS_PUBLIC_URL must start with https://, not {public_url:?}"
            ));
        }
        let trusted_proxies = var("HANDICRAFTS_TRUSTED_PROXIES")
            .unwrap_or_else(|| "127.0.0.1,::1".into())
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse()
                    .map_err(|_| format!("HANDICRAFTS_TRUSTED_PROXIES: {s:?} isn't an IP address"))
            })
            .collect::<Result<_, _>>()?;
        let admin_password_hash = var("HANDICRAFTS_ADMIN_PASSWORD_HASH");
        if let Some(h) = &admin_password_hash {
            argon2::PasswordHash::new(h).map_err(|_| {
                "HANDICRAFTS_ADMIN_PASSWORD_HASH isn't an argon2 hash; set the password again"
                    .to_string()
            })?;
        }
        let admin_email = var("HANDICRAFTS_ADMIN_EMAIL")
            .map(|e| {
                crate::mail::clean_address(&e).ok_or(format!(
                    "HANDICRAFTS_ADMIN_EMAIL {e:?} isn't an email address"
                ))
            })
            .transpose()?;

        let monokulo_url = var("HANDICRAFTS_MONOKULO_URL")
            .map(|url| {
                let url = url.trim_end_matches('/').to_string();
                if url.starts_with("https://") || url.starts_with("http://") {
                    Ok(url)
                } else {
                    Err(format!(
                        "HANDICRAFTS_MONOKULO_URL must be a web address, not {url:?}"
                    ))
                }
            })
            .transpose()?;
        let keys = [
            "HANDICRAFTS_MONOKULO_PUBLIC_KEY",
            "HANDICRAFTS_MONOKULO_SECRET_KEY",
            "HANDICRAFTS_MONOKULO_WEBHOOK_SECRET",
        ]
        .map(&var);
        let monokulo_keys = match keys {
            [None, None, None] => None,
            [Some(pk), Some(sk), Some(whs)] => {
                if !pk.starts_with("pk_") {
                    return Err("HANDICRAFTS_MONOKULO_PUBLIC_KEY must start with pk_".into());
                }
                if !sk.starts_with("sk_") {
                    return Err("HANDICRAFTS_MONOKULO_SECRET_KEY must start with sk_".into());
                }
                if monokulo_url.is_none() {
                    return Err("Monokulo keys are set but HANDICRAFTS_MONOKULO_URL isn't".into());
                }
                Some(MonokuloKeys {
                    public_key: pk,
                    secret_key: sk,
                    webhook_secret: whs,
                })
            }
            _ => {
                return Err("Monokulo keys pasted by hand need all three: public key, secret key and webhook signing secret. Or connect the store at /admin/monokulo instead.".into());
            }
        };

        let num = |k: &str, d: u32| -> Result<u32, String> {
            match var(k) {
                None => Ok(d),
                Some(v) => v
                    .parse()
                    .map_err(|_| format!("{k} must be a whole number, not {v:?}")),
            }
        };
        let limits = Limits {
            ip_10min: num("HANDICRAFTS_LIMIT_IP_10MIN", 3)?,
            ip_day: num("HANDICRAFTS_LIMIT_IP_DAY", 10)?,
            email_day: num("HANDICRAFTS_LIMIT_EMAIL_DAY", 3)?,
            shop_day: num("HANDICRAFTS_LIMIT_SHOP_DAY", 40)?,
        };

        Ok(Config {
            listen,
            public_url,
            data_dir: var("HANDICRAFTS_DATA_DIR")
                .unwrap_or_else(|| "data-dev".into())
                .into(),
            trusted_proxies,
            admin_user: var("HANDICRAFTS_ADMIN_USER").unwrap_or_else(|| "admin".into()),
            admin_password_hash,
            admin_email,
            bank: Bank {
                payid: var("HANDICRAFTS_PAYID"),
                account_name: var("HANDICRAFTS_BANK_ACCOUNT_NAME"),
                bsb: var("HANDICRAFTS_BSB"),
                account_number: var("HANDICRAFTS_ACCOUNT_NUMBER"),
            },
            monokulo_url,
            monokulo_keys,
            limits,
        })
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("handicrafts.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_vars(|k| m.get(k).cloned())
    }

    #[test]
    fn defaults_work() {
        let c = cfg(&[]).unwrap();
        assert_eq!(c.listen.to_string(), "127.0.0.1:8082");
        assert!(c.monokulo_url.is_none() && c.monokulo_keys.is_none());
        assert_eq!(c.limits.ip_10min, 3);
    }

    #[test]
    fn monokulo_address_alone_is_fine_keys_need_all_three() {
        let c = cfg(&[("HANDICRAFTS_MONOKULO_URL", "https://www.monokulo.com/")]).unwrap();
        assert_eq!(c.monokulo_url.as_deref(), Some("https://www.monokulo.com"));
        assert!(c.monokulo_from_keys().is_none());
        let e = cfg(&[
            ("HANDICRAFTS_MONOKULO_URL", "https://www.monokulo.com"),
            ("HANDICRAFTS_MONOKULO_SECRET_KEY", "sk_x"),
        ])
        .unwrap_err();
        assert!(e.contains("all three"), "{e}");
        let e = cfg(&[
            ("HANDICRAFTS_MONOKULO_PUBLIC_KEY", "pk_x"),
            ("HANDICRAFTS_MONOKULO_SECRET_KEY", "sk_x"),
            ("HANDICRAFTS_MONOKULO_WEBHOOK_SECRET", "whsec"),
        ])
        .unwrap_err();
        assert!(e.contains("HANDICRAFTS_MONOKULO_URL"), "{e}");
        let c = cfg(&[
            ("HANDICRAFTS_MONOKULO_URL", "https://www.monokulo.com/"),
            ("HANDICRAFTS_MONOKULO_PUBLIC_KEY", "pk_x"),
            ("HANDICRAFTS_MONOKULO_SECRET_KEY", "sk_x"),
            ("HANDICRAFTS_MONOKULO_WEBHOOK_SECRET", "whsec"),
        ])
        .unwrap();
        let m = c.monokulo_from_keys().unwrap();
        assert_eq!(m.api, "https://www.monokulo.com/api/v1");
    }

    #[test]
    fn bad_hash_is_refused() {
        assert!(cfg(&[("HANDICRAFTS_ADMIN_PASSWORD_HASH", "hunter2")]).is_err());
    }
}
