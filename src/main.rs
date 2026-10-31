//! Rachel's Handicrafts: the shop, its orders and payments, in one binary.
//!
//! `handicrafts` serves the site. Other commands, for the LuCI page and for
//! setting up:
//!   handicrafts hash-password      read a password on stdin, print its argon2 hash
//!   handicrafts check-mail         sign in to the mail server or Tuta, send nothing
//!   handicrafts send-test-mail TO  send a test email now
//!   handicrafts check              check the settings and exit

// Handlers return `Result<_, Response>` to bail out early with a page.
#![allow(clippy::result_large_err, clippy::too_many_arguments)]

mod assets;
mod catalog;
mod config;
mod db;
mod emails;
mod invoice;
mod mail;
mod monokulo;
mod orders;
mod outbox;
mod ratelimit;
mod util;
mod views;
mod web;

use std::{net::SocketAddr, process::ExitCode, sync::Arc};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};

fn usage() -> String {
    format!(
        "handicrafts {}: Rachel's Handicrafts shop\n\n\
         Usage:\n  handicrafts                    serve the shop\n  handicrafts hash-password      read a password on stdin, print its argon2 hash\n  \
         handicrafts check-mail         sign in to the mail server or Tuta, send nothing\n  handicrafts send-test-mail TO  send a test email now\n  \
         handicrafts check              check the settings and exit\n\n{}\n{}",
        env!("CARGO_PKG_VERSION"),
        config::ENV_HELP,
        mail::ENV_HELP
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let result = match args.first().map(String::as_str) {
        None | Some("serve") => rt.block_on(serve()),
        Some("hash-password") => hash_password(),
        Some("check-mail") => rt.block_on(check_mail()),
        Some("send-test-mail") => match args.get(1) {
            Some(to) => rt.block_on(send_test_mail(to)),
            None => Err("send-test-mail needs an address".into()),
        },
        Some("check") => settings().map(|(c, m)| {
            println!(
                "Settings are fine. Listening on {}; email {}.",
                c.listen,
                m.describe()
            )
        }),
        Some("-h" | "--help" | "help") => {
            println!("{}", usage());
            Ok(())
        }
        Some("--version") => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n\n{}", usage())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("handicrafts: {e}");
            ExitCode::FAILURE
        }
    }
}

fn settings() -> Result<(config::Config, mail::Transport), String> {
    Ok((config::Config::from_env()?, mail::Transport::from_env()?))
}

async fn serve() -> Result<(), String> {
    let (cfg, transport) = settings()?;
    let db = db::Db::open(&cfg.db_path())?;
    let mail = Arc::new(transport);
    let listen = cfg.listen;
    let app = Arc::new(web::App::new(cfg, db.clone(), mail.clone()));
    app.refresh_monokulo().await;
    let cfg = &app.cfg;
    let monero = match app.monokulo_source().await {
        web::MonokuloSource::Connected(_) => "on (store connected at /admin/monokulo)",
        web::MonokuloSource::Keys => "on (keys set in LuCI)",
        web::MonokuloSource::None if cfg.monokulo_url.is_some() => {
            "off until the store is connected at /admin/monokulo"
        }
        web::MonokuloSource::None => "off (no Monokulo address set)",
    };
    eprintln!(
        "handicrafts {}: serving {} on {listen}; data in {}; email {}; Monero {monero}; admin {}",
        env!("CARGO_PKG_VERSION"),
        cfg.public_url,
        cfg.data_dir.display(),
        mail.describe(),
        if cfg.admin_password_hash.is_some() {
            "on"
        } else {
            "closed (no password set)"
        },
    );
    if cfg.admin_email.is_none() {
        eprintln!("handicrafts: no admin email set, so order notifications aren't sent");
    }
    tokio::spawn(outbox::run(db, mail.clone(), app.outbox.clone()));

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| format!("can't listen on {listen}: {e}"))?;
    let service = web::router(app).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, service)
        .with_graceful_shutdown(async {
            let ctrl_c = tokio::signal::ctrl_c();
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("signal");
            tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
        })
        .await
        .map_err(|e| format!("server: {e}"))?;
    mail.close().await;
    Ok(())
}

fn hash_password() -> Result<(), String> {
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let pass = line.trim_end_matches(['\r', '\n']);
    if pass.chars().count() < 8 {
        return Err("the password must be at least 8 characters".into());
    }
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let hash = Argon2::default()
        .hash_password(pass.as_bytes(), &salt)
        .map_err(|e| e.to_string())?;
    println!("{hash}");
    Ok(())
}

async fn check_mail() -> Result<(), String> {
    let t = mail::Transport::from_env()?;
    let r = t.check().await;
    t.close().await;
    println!("{}", r?);
    Ok(())
}

async fn send_test_mail(to: &str) -> Result<(), String> {
    let to = mail::clean_address(to).ok_or("that isn't an email address")?;
    let t = mail::Transport::from_env()?;
    let m = emails::test_mail(&to);
    let letter = mail::Letter {
        to_name: String::new(),
        to: m.to_addr,
        subject: m.subject,
        text: m.text,
        html: m.html,
    };
    let r = t.send(&letter).await;
    t.close().await;
    r?;
    println!("Sent a test email to {to} ({}).", t.describe());
    Ok(())
}
