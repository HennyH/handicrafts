//! Sends queued email in the background. Mail is queued in the same
//! transaction as the order or payment that caused it, so a mail outage
//! never loses an order: it waits here and retries.

use std::{sync::Arc, time::Duration};

use crate::{
    db::Db,
    mail::{Letter, Transport},
};

pub async fn run(db: Db, mail: Arc<Transport>, wake: Arc<tokio::sync::Notify>) {
    if mail.is_off() {
        eprintln!("handicrafts: email is off; emails wait in the outbox");
    }
    loop {
        if !mail.is_off() {
            send_due(&db, &mail).await;
        }
        tokio::select! {
            _ = wake.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(30)) => {}
        }
    }
}

/// Send everything that's due. Returns how many were sent.
pub async fn send_due(db: &Db, mail: &Transport) -> usize {
    let due = match db.due_mail(20).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("handicrafts: outbox: {e}");
            return 0;
        }
    };
    let mut sent = 0;
    for m in due {
        let letter = Letter {
            to_name: m.to_name.clone(),
            to: m.to_addr.clone(),
            subject: m.subject.clone(),
            text: m.text.clone(),
            html: m.html.clone(),
        };
        match mail.send(&letter).await {
            Ok(()) => {
                sent += 1;
                if let Err(e) = db.mail_sent(m.id).await {
                    eprintln!("handicrafts: outbox: {e}");
                }
            }
            Err(why) => {
                eprintln!(
                    "handicrafts: email \"{}\" to {} failed: {why}",
                    m.subject, m.to_addr
                );
                match db.mail_failed(m.id, m.attempts, why).await {
                    Ok(true) => eprintln!(
                        "handicrafts: gave up on email {} after {} attempts",
                        m.id,
                        m.attempts + 1
                    ),
                    Ok(false) => {}
                    Err(e) => eprintln!("handicrafts: outbox: {e}"),
                }
            }
        }
    }
    sent
}
