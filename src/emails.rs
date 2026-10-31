//! The emails the shop sends, each as HTML and plain text.
//!
//! Email clients ignore most CSS, so the HTML is a 600px table with every
//! style written on the element: the shop's paper colour, a white card with
//! the 2px ink border, and the order as a table. Nothing depends on images
//! (Tuta and Outlook block remote images by default) and the headline font
//! falls back from Stardom (Apple Mail only) to Libre Franklin to Georgia.

use maud::{DOCTYPE, Markup, html};

use crate::{catalog::PricedLine, db::Mail, db::Order, util};

const PAPER: &str = "#faf6ee";
const INK: &str = "#29343a";
const SOFT: &str = "#6c7478";
const GOLD: &str = "#a67f38";
const CLAY: &str = "#b5654a";
const LINE: &str = "#e3dac6";
const BODY_FONT: &str = "Karla, 'Helvetica Neue', Helvetica, Arial, sans-serif";
const DISPLAY_FONT: &str = "Stardom, 'Libre Franklin', Georgia, 'Times New Roman', serif";

pub const SHOP_NAME: &str = "Rachel's Handicrafts";

/// The whole email around a card of content.
fn page(preheader: &str, card: Markup, footer: Markup) -> String {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light only";
                meta name="supported-color-schemes" content="light";
                // Apple Mail loads web fonts; everything else falls back.
                link rel="stylesheet" href="https://api.fontshare.com/v2/css?f[]=stardom&display=swap";
            }
            body style={ "margin:0;padding:0;background:" (PAPER) ";" } {
                // Shown in the inbox list after the subject, then hidden.
                div style="display:none;max-height:0;overflow:hidden;opacity:0;" { (preheader) }
                table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style={ "background:" (PAPER) ";" } {
                    tr { td align="center" style="padding:28px 12px;" {
                        table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="max-width:560px;" {
                            tr { td style={ "padding:0 4px 14px;font-family:" (DISPLAY_FONT) ";font-style:italic;font-size:22px;color:" (INK) ";" } { (SHOP_NAME) } }
                            tr { td style={ "background:#ffffff;border:2px solid " (INK) ";border-radius:16px;padding:26px 24px;font-family:" (BODY_FONT) ";font-size:16px;line-height:1.55;color:" (INK) ";" } {
                                (card)
                            } }
                            tr { td align="center" style={ "padding:16px 8px 0;font-family:" (BODY_FONT) ";font-size:12px;line-height:1.5;color:" (SOFT) ";" } {
                                (footer)
                            } }
                        }
                    } }
                }
            }
        }
    }
    .into_string()
}

fn h1(text: &str) -> Markup {
    html! { h1 style={ "margin:0 0 10px;font-family:" (DISPLAY_FONT) ";font-style:italic;font-weight:600;font-size:26px;line-height:1.2;color:" (INK) ";" } { (text) } }
}

fn p(content: Markup) -> Markup {
    html! { p style="margin:0 0 12px;" { (content) } }
}

fn button(href: &str, label: &str) -> Markup {
    html! {
        table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:6px 0 14px;" { tr {
            td style={ "background:" (INK) ";border-radius:10px;" } {
                a href=(href) style={ "display:inline-block;padding:11px 20px;font-family:" (BODY_FONT) ";font-weight:700;font-size:15px;color:#ffffff;text-decoration:none;" } { (label) }
            }
        } }
    }
}

/// The order lines and total as a table.
pub fn items_table(lines: &[PricedLine], total_cents: i64) -> Markup {
    html! {
        table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="margin:8px 0 14px;" {
            @for l in lines {
                tr {
                    td style={ "padding:9px 0;border-bottom:1.5px dashed " (LINE) ";" } {
                        strong { (l.qty) " × " (l.name) }
                        @if l.color.is_some() || l.materials.is_some() {
                            br;
                            span style={ "font-size:13px;color:" (SOFT) ";" } {
                                @if let Some(c) = &l.color { (c) }
                                @if l.color.is_some() && l.materials.is_some() { " · " }
                                @if let Some(m) = &l.materials { (m) }
                            }
                        }
                    }
                    td align="right" valign="top" style={ "padding:9px 0 9px 12px;border-bottom:1.5px dashed " (LINE) ";white-space:nowrap;font-weight:600;" } { (util::aud(l.total_cents())) }
                }
            }
            tr {
                td style={ "padding:10px 0 0;border-top:2px solid " (INK) ";font-weight:700;font-size:18px;" } { "Total" }
                td align="right" style={ "padding:10px 0 0;border-top:2px solid " (INK) ";font-weight:700;font-size:18px;white-space:nowrap;" } { (util::aud(total_cents)) " AUD" }
            }
        }
    }
}

pub fn items_text(lines: &[PricedLine], total_cents: i64) -> String {
    let mut s = String::new();
    for l in lines {
        s.push_str(&format!(
            "  {} × {}  {}\n",
            l.qty,
            l.title(),
            util::aud(l.total_cents())
        ));
        if let Some(m) = &l.materials {
            s.push_str(&format!("      ({m})\n"));
        }
    }
    s.push_str(&format!("  Total: {} AUD\n", util::aud(total_cents)));
    s
}

fn footer_customer(order: &Order) -> Markup {
    html! {
        "Order " (order.number) " · Perth, Western Australia" br;
        "You're getting this because this address was used to order at rachelshandicrafts.com. "
        "Didn't order? Ignore this email and nothing will happen."
    }
}

fn footer_admin() -> Markup {
    html! { "Sent by the shop on the router. Settings: Services › Handicrafts in LuCI." }
}

/// The copy of the order the customer gets straight after placing it.
pub fn customer_order(order: &Order, lines: &[PricedLine]) -> Mail {
    let name = util::safe_name(&order.customer_name);
    let first = name
        .split_whitespace()
        .next()
        .unwrap_or("there")
        .to_string();
    let subject = format!("We've got your order {}", order.number);
    let card = html! {
        (h1(&format!("Thanks for your order, {first}!")))
        (p(html! { "Here's a copy for your records. Nothing has been charged." }))
        (items_table(lines, order.total_cents))
        (p(html! { strong { "What's next: " } "I'll check the details, usually within 2 days, then email you an invoice with a link to pay by Monero, PayID or bank transfer." }))
        (p(html! { "If something's not right, just reply to this email." }))
        p style="margin:0;" { "Rachel" }
    };
    let text = format!(
        "Thanks for your order, {first}!\n\nHere's a copy for your records. Nothing has been charged.\n\n\
         Order {}\n{}\n\
         What's next: I'll check the details, usually within 2 days, then email you an invoice with a link \
         to pay by Monero, PayID or bank transfer.\n\nIf something's not right, just reply to this email.\n\nRachel\n\n\
         --\nYou're getting this because this address was used to order at rachelshandicrafts.com. \
         Didn't order? Ignore this email and nothing will happen.\n",
        order.number,
        items_text(lines, order.total_cents)
    );
    Mail {
        kind: "customer_order",
        to_addr: order.customer_email.clone(),
        to_name: name,
        subject,
        text,
        html: page(
            "A copy of your order. Nothing has been charged.",
            card,
            footer_customer(order),
        ),
    }
}

fn admin_mail(
    kind: &'static str,
    to: &str,
    subject: String,
    preheader: &str,
    card: Markup,
    text: String,
) -> Mail {
    Mail {
        kind,
        to_addr: to.to_string(),
        to_name: String::new(),
        subject,
        text,
        html: page(preheader, card, footer_admin()),
    }
}

fn customer_block(order: &Order) -> (Markup, String) {
    let rows: Vec<(&str, &str)> = [
        ("Name", order.customer_name.as_str()),
        ("Email", order.customer_email.as_str()),
        ("Phone", order.customer_phone.as_str()),
        ("Delivery", order.customer_address.as_str()),
        ("Notes", order.customer_notes.as_str()),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect();
    let markup = html! {
        table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:0 0 12px;font-size:14px;" {
            @for (k, v) in &rows {
                tr {
                    td valign="top" style={ "padding:2px 14px 2px 0;color:" (SOFT) ";white-space:nowrap;" } { (k) }
                    td style="padding:2px 0;white-space:pre-wrap;" { (v) }
                }
            }
        }
    };
    let text = rows.iter().map(|(k, v)| format!("{k}: {v}\n")).collect();
    (markup, text)
}

/// "New order" for the admin.
pub fn admin_new_order(order: &Order, lines: &[PricedLine], admin: &str, public_url: &str) -> Mail {
    let link = format!("{public_url}/admin/orders/{}", order.id);
    let (who, who_text) = customer_block(order);
    let card = html! {
        (h1(&format!("New order {}", order.number)))
        (who)
        (items_table(lines, order.total_cents))
        (button(&link, "Open the order"))
    };
    let text = format!(
        "New order {}\n\n{who_text}\n{}\nOpen it: {link}\n",
        order.number,
        items_text(lines, order.total_cents)
    );
    admin_mail(
        "admin_order",
        admin,
        format!(
            "New order {} from {} ({})",
            order.number,
            util::clip(&order.customer_name, 40),
            util::aud(order.total_cents)
        ),
        "A new order is waiting in /admin.",
        card,
        text,
    )
}

/// The customer pressed "I've paid".
pub fn admin_claim(order: &Order, method: &str, admin: &str, public_url: &str) -> Mail {
    let link = format!("{public_url}/admin/orders/{}", order.id);
    let how = if method == "monero" {
        "Monero"
    } else {
        "PayID / bank transfer"
    };
    let advice = if method == "monero" {
        "Monero payments are confirmed automatically by Monokulo; this is only a heads-up."
    } else {
        "Check the bank for the reference below, then set the order to Paid in /admin."
    };
    let card = html! {
        (h1(&format!("{} says they've paid", util::safe_name(&order.customer_name))))
        (p(html! { "Order " strong { (order.number) } " · " (util::aud(order.total_cents)) " · by " (how) }))
        @if method != "monero" {
            (p(html! { "Bank reference: " strong style={ "font-size:20px;letter-spacing:2px;color:" (GOLD) ";" } { (order.bank_ref) } }))
        }
        (p(html! { (advice) }))
        (button(&link, "Open the order"))
    };
    let text = format!(
        "{} says they've paid order {} ({}) by {how}.\nBank reference: {}\n\n{advice}\n\nOpen it: {link}\n",
        util::safe_name(&order.customer_name),
        order.number,
        util::aud(order.total_cents),
        order.bank_ref
    );
    admin_mail(
        "admin_claim",
        admin,
        format!("Customer says {} is paid ({how})", order.number),
        "Check before marking Paid.",
        card,
        text,
    )
}

/// Something from Monokulo (paid, part paid, double spend) or a problem the
/// admin should know about.
pub fn admin_notice(
    kind: &'static str,
    admin: &str,
    subject: String,
    headline: &str,
    body: &str,
    link: Option<String>,
    warning: bool,
) -> Mail {
    let card = html! {
        (h1(headline))
        @if warning {
            p style={ "margin:0 0 12px;padding:10px 12px;border:2px solid " (CLAY) ";border-radius:10px;color:" (CLAY) ";font-weight:700;" } { "Needs a look" }
        }
        @for para in body.split("\n\n") { (p(html! { (para) })) }
        @if let Some(l) = &link { (button(l, "Open in /admin")) }
    };
    let text = format!(
        "{headline}\n\n{body}\n{}",
        link.map(|l| format!("\nOpen it: {l}\n"))
            .unwrap_or_default()
    );
    admin_mail(kind, admin, subject, headline, card, text)
}

/// For LuCI's "Send test email".
pub fn test_mail(to: &str) -> Mail {
    let card = html! {
        (h1("Test email"))
        (p(html! { "If you can read this, the shop can send email. New orders and payments will be sent to this address." }))
    };
    admin_mail(
        "test",
        to,
        "Test email from the shop".into(),
        "The shop can send email.",
        card,
        "Test email\n\nIf you can read this, the shop can send email.\n".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::Status;

    fn order(name: &str) -> Order {
        Order {
            id: 57,
            number: "RH-202610311432-57".into(),
            bank_ref: "K7P4Q".into(),
            pay_token: "tok".into(),
            status: Status::New,
            accepted: false,
            customer_name: name.into(),
            customer_email: "sam@example.com".into(),
            customer_phone: String::new(),
            customer_address: "local pickup".into(),
            customer_notes: String::new(),
            total_cents: 10500,
            currency: "AUD".into(),
            refund_address: String::new(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn lines() -> Vec<PricedLine> {
        vec![PricedLine {
            product_id: "tote-bag".into(),
            sku: "RH-TOTE-01".into(),
            name: "Tote Bag".into(),
            color: Some("Navy".into()),
            materials: Some("100% GOTS cotton".into()),
            image: None,
            unit_cents: 6000,
            qty: 1,
        }]
    }

    #[test]
    fn customer_email_reads_well_and_is_defanged() {
        let m = customer_order(&order("Sam Example"), &lines());
        assert_eq!(m.to_addr, "sam@example.com");
        assert!(m.subject.contains("RH-202610311432-57"));
        assert!(m.html.contains("Thanks for your order, Sam!"));
        assert!(m.html.contains("$60.00"));
        assert!(m.text.contains("Navy"));
        assert!(!m.html.contains("<img"), "no images needed");

        let spam = customer_order(&order("Click https://evil.example <b>now</b>"), &lines());
        assert!(!spam.html.contains("evil.example"));
        assert!(!spam.html.contains("<b>now"), "escaped");
    }

    #[test]
    fn admin_emails_link_to_the_order() {
        let m = admin_new_order(
            &order("Sam"),
            &lines(),
            "rachel@example.com",
            "https://shop.example",
        );
        assert!(m.html.contains("https://shop.example/admin/orders/57"));
        let c = admin_claim(
            &order("Sam"),
            "bank",
            "rachel@example.com",
            "https://shop.example",
        );
        assert!(c.text.contains("K7P4Q"));
    }
}
