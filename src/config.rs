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
  HANDICRAFTS_MONOKULO_URL             e.g. https://www.monokulo.com
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
    pub monokulo: Option<Monokulo>,
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

#[derive(Clone, Debug)]
pub struct Monokulo {
    /// Without a trailing slash.
    pub url: String,
    pub public_key: String,
    pub secret_key: String,
    pub webhook_secret: String,
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

        let mk = [
            "HANDICRAFTS_MONOKULO_URL",
            "HANDICRAFTS_MONOKULO_PUBLIC_KEY",
            "HANDICRAFTS_MONOKULO_SECRET_KEY",
            "HANDICRAFTS_MONOKULO_WEBHOOK_SECRET",
        ]
        .map(&var);
        let monokulo = match mk {
            [None, None, None, None] => None,
            [Some(url), Some(pk), Some(sk), Some(whs)] => {
                let url = url.trim_end_matches('/').to_string();
                if !url.starts_with("https://") && !url.starts_with("http://") {
                    return Err(format!(
                        "HANDICRAFTS_MONOKULO_URL must be a web address, not {url:?}"
                    ));
                }
                if !pk.starts_with("pk_") {
                    return Err("HANDICRAFTS_MONOKULO_PUBLIC_KEY must start with pk_".into());
                }
                if !sk.starts_with("sk_") {
                    return Err("HANDICRAFTS_MONOKULO_SECRET_KEY must start with sk_".into());
                }
                Some(Monokulo {
                    url,
                    public_key: pk,
                    secret_key: sk,
                    webhook_secret: whs,
                })
            }
            _ => {
                return Err("Monokulo needs all four settings: address, public key, secret key and webhook signing secret".into());
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
            monokulo,
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
        assert!(c.monokulo.is_none());
        assert_eq!(c.limits.ip_10min, 3);
    }

    #[test]
    fn monokulo_needs_all_four() {
        let e = cfg(&[("HANDICRAFTS_MONOKULO_URL", "https://www.monokulo.com")]).unwrap_err();
        assert!(e.contains("all four"), "{e}");
        let c = cfg(&[
            ("HANDICRAFTS_MONOKULO_URL", "https://www.monokulo.com/"),
            ("HANDICRAFTS_MONOKULO_PUBLIC_KEY", "pk_x"),
            ("HANDICRAFTS_MONOKULO_SECRET_KEY", "sk_x"),
            ("HANDICRAFTS_MONOKULO_WEBHOOK_SECRET", "whsec"),
        ])
        .unwrap();
        assert_eq!(c.monokulo.unwrap().url, "https://www.monokulo.com");
    }

    #[test]
    fn bad_hash_is_refused() {
        assert!(cfg(&[("HANDICRAFTS_ADMIN_PASSWORD_HASH", "hunter2")]).is_err());
    }
}
