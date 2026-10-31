//! Sending email: over SMTP (lettre, rustls with the bundled Mozilla roots)
//! or from a Tuta account, through Tern's `tern-tuta` crate (Tuta has no
//! SMTP). The SMTP side is adapted from Kringle's `src/mail.rs`.
//!
//! Every email has an HTML body and a plain-text one. SMTP sends both as
//! multipart/alternative; Tuta sends the HTML body only.

use std::{future::Future, sync::Arc, time::Duration};

use lettre::{
    AsyncSmtpTransport, AsyncTransport as _, Message, Tokio1Executor,
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
};

/// One email to one person.
#[derive(Clone, Debug, PartialEq)]
pub struct Letter {
    pub to_name: String,
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

pub enum Transport {
    Smtp {
        relay: AsyncSmtpTransport<Tokio1Executor>,
        from: Mailbox,
        describe: String,
    },
    Tuta(Tuta),
    /// Writes each mail to the log instead of sending it, and keeps the last
    /// few in memory. For development and the tests.
    Log(std::sync::Mutex<Vec<Letter>>),
    /// Email turned off: everything stays in the outbox, unsent.
    Off,
}

pub const ENV_HELP: &str = "\
Email (HANDICRAFTS_MAIL turns it on):
  HANDICRAFTS_MAIL            off (default), smtp, tuta, or log (print mails to the log)
  HANDICRAFTS_MAIL_FROM       the address to send from. SMTP: default HANDICRAFTS_SMTP_USER.
                              Tuta: the account's main address (default) or one of its aliases.
  HANDICRAFTS_MAIL_FROM_NAME  sender name for SMTP (default Rachel's Handicrafts; Tuta uses
                              the name set in Tuta)
  SMTP:
  HANDICRAFTS_SMTP_HOST       server, e.g. smtp.example.com
  HANDICRAFTS_SMTP_PORT       default 465 for tls, 587 for starttls, 25 for none
  HANDICRAFTS_SMTP_SECURITY   tls (default), starttls, or none
  HANDICRAFTS_SMTP_USER       login, if the server needs one
  HANDICRAFTS_SMTP_PASSWORD
  Tuta (signs in to a Tuta account and sends from it):
  HANDICRAFTS_TUTA_USER       the account's address
  HANDICRAFTS_TUTA_PASSWORD   its password (accounts with two-factor sign-in can't be used:
                              use a separate sending account without it)
";

impl Transport {
    pub fn from_env() -> Result<Transport, String> {
        Self::from_vars(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Transport, String> {
        let need = |k: &str| var(k).ok_or_else(|| format!("{k} is not set"));
        let from_name =
            var("HANDICRAFTS_MAIL_FROM_NAME").unwrap_or_else(|| "Rachel's Handicrafts".into());
        match var("HANDICRAFTS_MAIL").as_deref().map(str::trim) {
            None | Some("off") => Ok(Transport::Off),
            Some("log") => Ok(Transport::Log(Default::default())),
            Some("smtp") => {
                let host = need("HANDICRAFTS_SMTP_HOST")?;
                let security = var("HANDICRAFTS_SMTP_SECURITY").unwrap_or_else(|| "tls".into());
                type Smtp = AsyncSmtpTransport<Tokio1Executor>;
                let (builder, default_port) = match security.as_str() {
                    "tls" => (Smtp::relay(&host), 465),
                    "starttls" => (Smtp::starttls_relay(&host), 587),
                    "none" => (Ok(Smtp::builder_dangerous(&host)), 25),
                    other => {
                        return Err(format!(
                            "HANDICRAFTS_SMTP_SECURITY must be tls, starttls or none, not {other:?}"
                        ));
                    }
                };
                let mut b = builder.map_err(|e| format!("SMTP server {host:?}: {e}"))?;
                let port = match var("HANDICRAFTS_SMTP_PORT") {
                    Some(p) => p.trim().parse::<u16>().map_err(|_| {
                        format!("HANDICRAFTS_SMTP_PORT must be a port number, not {p:?}")
                    })?,
                    None => default_port,
                };
                b = b.port(port).timeout(Some(Duration::from_secs(30)));
                let user = var("HANDICRAFTS_SMTP_USER");
                if let Some(user) = &user {
                    let password = need("HANDICRAFTS_SMTP_PASSWORD")?;
                    b = b.credentials(Credentials::new(user.clone(), password));
                }
                let address = var("HANDICRAFTS_MAIL_FROM")
                    .or(user.clone())
                    .ok_or("HANDICRAFTS_MAIL_FROM is not set (and there's no HANDICRAFTS_SMTP_USER to use instead)")?;
                let from = Mailbox::new(
                    Some(from_name),
                    address
                        .trim()
                        .parse()
                        .map_err(|e| format!("HANDICRAFTS_MAIL_FROM {address:?}: {e}"))?,
                );
                Ok(Transport::Smtp {
                    relay: b.build(),
                    describe: format!("SMTP via {host}:{port} ({security}) as {}", from.email),
                    from,
                })
            }
            Some("tuta") => Ok(Transport::Tuta(Tuta {
                user: need("HANDICRAFTS_TUTA_USER")?.trim().to_string(),
                password: need("HANDICRAFTS_TUTA_PASSWORD")?,
                from: var("HANDICRAFTS_MAIL_FROM")
                    .map(|f| {
                        clean_address(&f).ok_or(format!(
                            "HANDICRAFTS_MAIL_FROM {f:?} isn't an email address"
                        ))
                    })
                    .transpose()?,
                session: Default::default(),
            })),
            Some(other) => Err(format!(
                "HANDICRAFTS_MAIL must be off, smtp, tuta or log, not {other:?}"
            )),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Transport::Smtp { describe, .. } => describe.clone(),
            Transport::Tuta(t) => match &t.from {
                Some(f) => format!("Tuta as {}, from {f}", t.user),
                None => format!("Tuta as {}", t.user),
            },
            Transport::Log(_) => "printed to the log, not sent (HANDICRAFTS_MAIL=log)".into(),
            Transport::Off => "off: emails wait in the outbox (HANDICRAFTS_MAIL=off)".into(),
        }
    }

    pub fn is_off(&self) -> bool {
        matches!(self, Transport::Off)
    }

    /// Sign in to the SMTP server or Tuta without sending anything.
    pub async fn check(&self) -> Result<String, String> {
        match self {
            Transport::Smtp {
                relay, describe, ..
            } => match relay.test_connection().await {
                Ok(true) => Ok(format!("Connected and signed in: {describe}.")),
                Ok(false) => Err(format!("Couldn't connect: {describe}.")),
                Err(e) => Err(format!("SMTP: {}", smtp_reason(&e))),
            },
            Transport::Tuta(t) => t.check().await,
            Transport::Log(_) => Ok("Emails are written to the system log, not sent.".into()),
            Transport::Off => Err("Email is turned off.".into()),
        }
    }

    /// Before stopping: sign out of Tuta.
    pub async fn close(&self) {
        if let Transport::Tuta(t) = self {
            t.close().await;
        }
    }

    /// Send one letter. The error is a short reason for the log and /admin.
    pub async fn send(&self, letter: &Letter) -> Result<(), String> {
        match self {
            Transport::Smtp { relay, from, .. } => {
                let to = letter
                    .to
                    .parse()
                    .map_err(|_| "that email address isn't valid".to_string())?;
                let to_name = (!letter.to_name.is_empty()).then(|| letter.to_name.clone());
                let message = Message::builder()
                    .from(from.clone())
                    .to(Mailbox::new(to_name, to))
                    .subject(&letter.subject)
                    .multipart(MultiPart::alternative_plain_html(
                        letter.text.clone(),
                        letter.html.clone(),
                    ))
                    .map_err(|e| format!("couldn't build the email: {e}"))?;
                relay
                    .send(message)
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("SMTP: {}", smtp_reason(&e)))
            }
            Transport::Tuta(t) => t.send(letter).await,
            Transport::Log(outbox) => {
                eprintln!(
                    "handicrafts: email to {} <{}>\nSubject: {}\n\n{}\n",
                    letter.to_name, letter.to, letter.subject, letter.text
                );
                let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
                if outbox.len() >= 100 {
                    outbox.remove(0);
                }
                outbox.push(letter.clone());
                Ok(())
            }
            Transport::Off => Err("email is turned off".into()),
        }
    }

    /// The letters a `Log` transport has "sent", for tests.
    #[cfg(test)]
    pub fn logged(&self) -> Vec<Letter> {
        match self {
            Transport::Log(l) => l.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            _ => vec![],
        }
    }
}

/// lettre's error with its causes, on one line.
fn smtp_reason(e: &lettre::transport::smtp::Error) -> String {
    let mut out = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        let s = s.to_string();
        if !out.contains(&s) {
            out = format!("{out}: {s}");
        }
        source = source.and_then(|s| s.source());
    }
    out
}

/// A Tuta account, through Tern's `tern-tuta` crate. Signs in on the first
/// mail and keeps the session; if Tuta says the session has expired, it signs
/// in again and tries once more.
///
/// Tuta sends from the account's main address or one of its aliases, with
/// the display name set in Tuta, not here.
pub struct Tuta {
    user: String,
    password: String,
    /// The address to send from, an alias of the account; `None` for its
    /// main address.
    from: Option<String>,
    session: tokio::sync::Mutex<Option<Arc<dyn tern_core::Backend>>>,
}

type Session = Arc<dyn tern_core::Backend>;

impl Tuta {
    async fn sign_in(&self) -> Result<Session, String> {
        let (backend, _saved) = tern_tuta::login(&self.user, &self.password)
            .await
            .map_err(|e| format!("Tuta sign-in as {} failed: {}", self.user, tuta_reason(&e)))?;
        if let Some(from) = &self.from
            && !backend.account().owns(from)
        {
            let _ = backend.sign_out().await;
            return Err(format!(
                "the Tuta account {} can't send from {from}: use its own address or one of its aliases ({})",
                self.user,
                backend.account().addresses().join(", ")
            ));
        }
        Ok(backend)
    }

    async fn send(&self, letter: &Letter) -> Result<(), String> {
        let mut session = self.session.lock().await;
        with_session(
            &mut session,
            || self.sign_in(),
            |backend: Session| async move {
                let draft = tern_core::Draft {
                    to: vec![tern_core::Address::new(
                        letter.to_name.clone(),
                        letter.to.clone(),
                    )],
                    cc: vec![],
                    bcc: vec![],
                    subject: letter.subject.clone(),
                    body_html: letter.html.clone(),
                    attachments: vec![],
                    previous: None,
                    existing: None,
                    kept: vec![],
                    from: self.from.clone(),
                };
                backend.send(draft).await.map_err(|e| {
                    (
                        e == tern_core::Error::SessionExpired,
                        format!("Tuta: {}", tuta_reason(&e)),
                    )
                })
            },
        )
        .await
    }

    /// Sign in afresh, say what the account can send from, and sign out.
    async fn check(&self) -> Result<String, String> {
        let backend = self.sign_in().await?;
        let account = backend.account();
        let _ = backend.sign_out().await;
        let from = self.from.clone().unwrap_or(account.address.clone());
        Ok(format!(
            "Signed in to Tuta as {}; mail is sent from {from}.",
            self.user
        ))
    }

    async fn close(&self) {
        if let Some(backend) = self.session.lock().await.take()
            && let Err(e) = backend.sign_out().await
        {
            eprintln!("handicrafts: signing out of Tuta failed: {e}");
        }
    }
}

/// Tern's errors, with the advice that fits a server.
fn tuta_reason(e: &tern_core::Error) -> String {
    match e {
        tern_core::Error::SecondFactorRequired => {
            "this account uses two-factor sign-in, which a server can't answer. Use a separate sending account without it.".into()
        }
        other => other.to_string(),
    }
}

/// Run `send` with the kept session, signing in first if there is none. Any
/// failure drops the session; if the session itself was refused, sign in
/// again and try once more.
pub async fn with_session<S, SignIn, Send>(
    slot: &mut Option<S>,
    sign_in: impl Fn() -> SignIn,
    send: impl Fn(S) -> Send,
) -> Result<(), String>
where
    S: Clone,
    SignIn: Future<Output = Result<S, String>>,
    Send: Future<Output = Result<(), (bool, String)>>,
{
    for attempt in 0..2 {
        let session = match slot.clone() {
            Some(s) => s,
            None => {
                let s = sign_in().await?;
                *slot = Some(s.clone());
                s
            }
        };
        match send(session).await {
            Ok(()) => return Ok(()),
            Err((refused, why)) => {
                *slot = None;
                if !(refused && attempt == 0) {
                    return Err(why);
                }
            }
        }
    }
    unreachable!("the second attempt always returns")
}

/// A trimmed email address if it looks like one. The mail server has the
/// final say.
pub fn clean_address(s: &str) -> Option<String> {
    let s = s.trim();
    let (local, domain) = s.split_once('@')?;
    let ok = s.len() <= 254
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>,;:\"()[]\\".contains(c));
    ok.then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(
            clean_address(" sam@example.com ").as_deref(),
            Some("sam@example.com")
        );
        assert!(clean_address("sam").is_none());
        assert!(clean_address("sam@example").is_none());
        assert!(clean_address("a b@example.com").is_none());
    }

    #[test]
    fn settings() {
        assert!(matches!(
            Transport::from_vars(|_| None).unwrap(),
            Transport::Off
        ));
        let e = Transport::from_vars(|k| (k == "HANDICRAFTS_MAIL").then(|| "tuta".into()))
            .err()
            .unwrap();
        assert!(e.contains("HANDICRAFTS_TUTA_USER"), "{e}");
        let tuta = |from: &'static str| {
            Transport::from_vars(move |k| match k {
                "HANDICRAFTS_MAIL" => Some("tuta".into()),
                "HANDICRAFTS_TUTA_USER" => Some("shop@tuta.com".into()),
                "HANDICRAFTS_TUTA_PASSWORD" => Some("pw".into()),
                "HANDICRAFTS_MAIL_FROM" => Some(from.into()),
                _ => None,
            })
        };
        assert!(tuta("not an address").is_err());
        match tuta("hello@rachelshandicrafts.com").unwrap() {
            Transport::Tuta(t) => assert_eq!(t.from.as_deref(), Some("hello@rachelshandicrafts.com")),
            _ => panic!("expected Tuta"),
        }
    }
}
