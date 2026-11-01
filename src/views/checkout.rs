//! Checkout, the "order received" page and the pay page.

use maud::{Markup, PreEscaped, html};

use super::layout::{Page, page, shop_scripts};
use crate::{assets, catalog::PricedLine, config, db::Order, orders::Status, util};

/// What the customer typed, to put back in the form after an error.
#[derive(Default, Clone, Debug, serde::Deserialize)]
pub struct CheckoutForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub notes: String,
    /// The cart, as JSON, filled from localStorage by js/checkout.js.
    #[serde(default)]
    pub cart: String,
    /// Honeypot: people never see it, simple bots fill it in.
    #[serde(default)]
    pub website: String,
    /// When the form was shown (unix seconds), for the "too fast" check.
    #[serde(default)]
    pub started: String,
}

/// The `<main>` of the checkout page: the summary (filled in by
/// js/checkout.js from the cart) and the form.
pub fn checkout_main(form: &CheckoutForm, error: Option<&str>) -> Markup {
    html! {
        main.wrap #checkout-main {
            h1.section-title style="margin-top:40px;" { "Checkout" }
            p.section-sub { "Send your order to Rachel. Nothing is charged now: once she has checked the details, she'll email you an invoice with a link to pay by Monero, PayID or bank transfer." }
            div.checkout-panel {
                div #checkout-empty .empty-state style="display:none;" {
                    img src="/assets/doodles/heart-stitch.svg" alt="";
                    p { "Your cart's empty, so there's nothing to check out. Head to the " a href="/shop.html" { "shop" } " first." }
                }
                div #checkout-summary {}
                @if let Some(e) = error {
                    p.form-error role="alert" { (e) }
                }
                form #checkout-form method="post" action="/orders"
                    fx-action="/orders" fx-method="post" fx-target="#checkout-main" fx-swap="outerHTML"
                    style="margin-top:24px;" {
                    input type="hidden" name="cart" #cart-json value=(form.cart);
                    input type="hidden" name="started" value=(util::now());
                    div.hp aria-hidden="true" {
                        label for="website" { "Leave this empty" }
                        input type="text" #website name="website" tabindex="-1" autocomplete="off" value="";
                    }
                    div.form-grid {
                        div.field {
                            label for="name" { "Full name" }
                            input type="text" #name name="name" required maxlength="120" autocomplete="name" value=(form.name);
                        }
                        div.field {
                            label for="email" { "Email" }
                            input type="email" #email name="email" required maxlength="254" autocomplete="email" value=(form.email);
                        }
                        div.field {
                            label for="phone" { "Phone (optional)" }
                            input type="tel" #phone name="phone" maxlength="40" autocomplete="tel" value=(form.phone);
                        }
                        div.field.full {
                            label for="address" { "Delivery address, or say \"local pickup\"" }
                            textarea #address name="address" maxlength="600" placeholder="I'm based in Perth WA — local pickup can be arranged, or let me know where to post it." { (form.address) }
                        }
                        div.field.full {
                            label for="notes" { "Notes / customisation requests" }
                            textarea #notes name="notes" maxlength="2000" placeholder="Colour preferences, sizing, anything else handy to know." { (form.notes) }
                        }
                    }
                    button.btn.btn--accent.btn--big.btn--block type="submit" { "Place order" }
                }
            }
        }
    }
}

pub fn checkout_page(form: &CheckoutForm, error: Option<&str>) -> Markup {
    let mut p = Page::new(
        "Checkout — Rachel's Handicrafts",
        checkout_main(form, error),
    );
    p.scripts = html! {
        script src=(assets::url("js/fixi.js")) {}
        script src=(assets::url("js/fixi-forms.js")) {}
        (shop_scripts(true))
        script src=(assets::url("js/checkout.js")) {}
    };
    page(p)
}

fn item_rows(lines: &[PricedLine]) -> Markup {
    html! {
        @for l in lines {
            div.order-item {
                @if let Some(img) = &l.image {
                    img src={ "/" (to_webp(img)) } alt="" loading="lazy";
                } @else {
                    span.order-item__blank {}
                }
                div {
                    b { @if l.qty > 1 { (l.qty) " × " } (l.name) }
                    small {
                        @if let Some(c) = &l.color { (c) }
                        @if l.color.is_some() && l.materials.is_some() { " · " }
                        @if let Some(m) = &l.materials { (m) }
                    }
                }
                span.price { (util::aud(l.total_cents())) }
            }
        }
    }
}

/// Card photos are served as .webp next to the original.
fn to_webp(src: &str) -> String {
    let lower = src.to_ascii_lowercase();
    for ext in [".jpg", ".jpeg", ".png"] {
        if lower.ends_with(ext) {
            return format!("{}.webp", &src[..src.len() - ext.len()]);
        }
    }
    src.to_string()
}

fn summary(order: &Order, lines: &[PricedLine], heading: &str, deliver_label: &str) -> Markup {
    let deliver = if order.customer_address.trim().is_empty() {
        "To be arranged"
    } else {
        order.customer_address.trim()
    };
    html! {
        section.panel.order-summary {
            h2 { (heading) }
            (item_rows(lines))
            div.summary-line.summary-line--total { span { "Total" } span { (util::aud(order.total_cents)) } }
            div.deliver {
                span { (deliver_label) }
                @if deliver_label == "For" { (order.customer_name) " · " }
                (deliver)
            }
        }
    }
}

/// What the customer sees after "Place order".
pub fn received_main(order: &Order, lines: &[PricedLine], emailed: bool) -> Markup {
    let first = util::safe_name(&order.customer_name);
    let first = first.split_whitespace().next().unwrap_or("");
    html! {
        main.wrap #checkout-main data-order-placed=(order.number) {
            div.received {
                div.received__hero {
                    img src="/assets/doodles/scribble-star.svg" alt="";
                    h1.section-title { "Thank you" @if !first.is_empty() { ", " (first) } "!" }
                    p { "Your order is in. Nothing has been charged yet." }
                    div.ordernum {
                        span.eyebrow { "Order" }
                        span.mono { (order.number) }
                        button.copy type="button" data-copy=(order.number) { "Copy" }
                    }
                }
                div.received__grid {
                    section.panel {
                        div.eyebrow { "What happens next" }
                        ol.next {
                            @if emailed {
                                li.is-done {
                                    div.next__icon { (tick()) }
                                    div {
                                        h3 { "A copy is on its way to your inbox" }
                                        p { "Sent to " b { (order.customer_email) } ". It has your order number and everything you picked." }
                                    }
                                }
                            }
                            li {
                                div.next__icon { img src="/assets/doodles/hand-stitch-pin.svg" alt=""; }
                                div {
                                    h3 { "Rachel checks the details" }
                                    p { "Fabric, sizing and pickup or postage. If anything needs a chat, she'll email or call you." }
                                    span.when { "Usually within 2 days" }
                                }
                            }
                            li {
                                div.next__icon { img src="/assets/doodles/envelope-seal.svg" alt=""; }
                                div {
                                    h3 { "You get an invoice with a pay link" }
                                    p { "Pay with Monero, or by PayID or bank transfer. Your piece is made once it's paid." }
                                }
                            }
                        }
                    }
                    (summary(order, lines, "Your order", "Delivery"))
                }
                div.received__actions {
                    a.btn.btn--accent href="/shop.html" { "Keep browsing the shop" }
                    a.btn.btn--ghost href="/recipes.html" { "Try a recipe while you wait" }
                }
                p.fineprint {
                    @if emailed { "No email after 10 minutes? Check your spam folder, or write to " } @else { "Keep your order number handy. Questions? Write to " }
                    b { "hello@rachelshandicrafts.com" } " with your order number."
                }
            }
        }
    }
}

pub fn received_page(order: &Order, lines: &[PricedLine], emailed: bool) -> Markup {
    let mut p = Page::new(
        "Order received — Rachel's Handicrafts",
        received_main(order, lines, emailed),
    );
    p.scripts = html! {
        script src=(assets::url("js/fixi.js")) {}
        script src=(assets::url("js/fixi-forms.js")) {}
        (shop_scripts(true))
        script src=(assets::url("js/checkout.js")) {}
    };
    page(p)
}

fn tick() -> Markup {
    html! {
        svg viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            path d="M4 12.5l5 5L20 6.5" {}
        }
    }
}

// ------------------------------------------------------------------ pay page

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Method {
    Monero,
    Bank,
}

impl Method {
    pub fn parse(s: Option<&str>) -> Option<Method> {
        match s {
            Some("monero") => Some(Method::Monero),
            Some("bank") => Some(Method::Bank),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Monero => "monero",
            Method::Bank => "bank",
        }
    }
}

/// What the Monero part of the pay page shows.
pub enum MoneroState<'a> {
    /// Monokulo's checkout for this Monokulo order.
    Checkout {
        mk: &'a crate::db::MonokuloOrder,
        cfg: &'a config::Monokulo,
    },
    /// Monokulo couldn't price in AUD.
    NoPrice,
    /// Monokulo unreachable or refused.
    Unavailable,
    /// Offered, but the customer chose bank transfer, so nothing was made.
    NotStarted,
}

pub enum PayView<'a> {
    NotInvoiced,
    Cancelled,
    Paid,
    Choose {
        method: Method,
        monero: Option<MoneroState<'a>>,
        bank: Option<&'a config::Bank>,
    },
    /// After "I've paid".
    Claimed {
        method: Method,
    },
}

pub fn pay_page(order: &Order, lines: &[PricedLine], view: PayView, refund_saved: bool) -> Markup {
    let base = format!("/pay/{}", order.pay_token);
    let panel = match &view {
        PayView::NotInvoiced => outcome(
            html! { img src="/assets/doodles/envelope-seal.svg" alt="" style="width:46px"; },
            true,
            "Not invoiced yet",
            "Rachel will email you an invoice once she has checked your order. This link will work then.",
        ),
        PayView::Cancelled => outcome(
            html! { img src="/assets/doodles/heart-stitch.svg" alt="" style="width:46px"; },
            true,
            "This order was cancelled",
            "Please get in touch if you think that's a mistake.",
        ),
        PayView::Paid => html! {
            (outcome(tick(), false, "Paid, thank you!", "Your payment has been received. Rachel will let you know when your order is on its way, or ready to pick up."))
            @if !order.refund_address.is_empty() {
                dl.facts { dt { "Refund address" } dd { (short_address(&order.refund_address)) } }
            }
        },
        PayView::Claimed { method } => {
            let (title, text) = match method {
                Method::Bank => (
                    "Thanks, Rachel's been told",
                    "She'll check her bank and confirm by email, usually within a day. You can close this page.",
                ),
                Method::Monero => (
                    "Thanks, Rachel's been told",
                    "Your payment will be confirmed automatically once it reaches the network, and this page updates by itself. Rachel has been told too, in case anything needs sorting.",
                ),
            };
            html! {
                div data-poll=(format!("{base}/status")) {}
                (outcome(html! { img src="/assets/doodles/envelope-seal.svg" alt="" style="width:46px"; }, true, title, text))
                div style="text-align:center;margin-top:12px" {
                    a.btn.btn--ghost href={ (base) "?method=" (method.as_str()) } { "Back to payment details" }
                }
            }
        }
        PayView::Choose {
            method,
            monero,
            bank,
        } => {
            let both = monero.is_some() && bank.is_some();
            let monero_ok = !matches!(
                monero,
                Some(MoneroState::NoPrice | MoneroState::Unavailable)
            );
            html! {
                @if both {
                    p.step-label { i { "1" } "How would you like to pay?" }
                    div.methods {
                        a.method.is-off[!monero_ok] href={ (base) "?method=monero" } aria-current=(*method == Method::Monero) {
                            span.badge.badge--mustard { @if monero_ok { "Preferred" } @else { "Unavailable" } }
                            span.method__icon { img src="/assets/doodles/monero.svg" alt=""; }
                            b { "Monero" }
                            small { "Private, and confirmed automatically" }
                        }
                        a.method href={ (base) "?method=bank" } aria-current=(*method == Method::Bank) {
                            span.method__radio aria-hidden="true" {}
                            span.method__icon { (bank_icon()) }
                            b { "PayID or bank" }
                            small { "Confirmed by Rachel, usually within a day" }
                        }
                    }
                }
                @match (method, monero, bank) {
                    (Method::Monero, Some(state), _) => {
                        @let step = if both { "2" } else { "1" };
                        @match state {
                            MoneroState::Checkout { mk, cfg } => {
                                p.step-label { i { (step) } "Send the Monero" }
                                form.refund.field method="post" action={ (base) "/refund" }
                                    fx-action={ (base) "/refund" } fx-method="post" fx-target="#refund-status" fx-swap="innerHTML" fx-trigger="change" {
                                    label for="refund" { "Refund address " span.muted { "(optional)" } }
                                    input type="text" #refund name="refund_address" placeholder="4… your own Monero address" autocomplete="off" spellcheck="false" maxlength="110" value=(order.refund_address);
                                    small { "Only used if Rachel ever needs to send money back, say if a fabric runs out. " span #refund-status aria-live="polite" { @if refund_saved { "Saved." } } }
                                    noscript { button.btn.btn--tiny type="submit" { "Save" } }
                                }
                                div.mk-mount data-poll=(format!("{base}/status")) {
                                    iframe.mk-frame src={ (base) "/monero" } title="Monero payment details" loading="eager" {}
                                }
                                p.embed-tag { "Payment details come from Monokulo. This page moves on by itself once the payment is seen. " a href={ (cfg.url) "/checkout/" (mk.mk_order_id) } target="_blank" rel="noopener" { "Open on Monokulo" } "." }
                                form.alt method="post" action={ (base) "/claim" } {
                                    input type="hidden" name="method" value="monero";
                                    span { "Paid from a wallet that can't scan this, or something went wrong?" }
                                    button.btn.btn--tiny.btn--ghost type="submit" { "I've paid" }
                                }
                            }
                            MoneroState::NoPrice | MoneroState::Unavailable | MoneroState::NotStarted => {
                                div.mk-error role="alert" {
                                    @if matches!(state, MoneroState::NoPrice) {
                                        b { "Monero can't be priced in Australian dollars right now" }
                                        p { "The payment service couldn't get an AUD price, so it can't tell you how much Monero to send. Rachel has been told." }
                                    } @else {
                                        b { "Monero payments aren't working right now" }
                                        p { "The payment service couldn't be reached. Rachel has been told." }
                                    }
                                    p { @if bank.is_some() { "You can pay by PayID or bank transfer now, or try Monero again a little later." } @else { "Please try again a little later." } }
                                    div.mk-error__actions {
                                        @if bank.is_some() { a.btn.btn--accent href={ (base) "?method=bank" } { "Pay by PayID or bank" } }
                                        a.btn.btn--ghost href={ (base) "?method=monero" } { "Try Monero again" }
                                    }
                                }
                            }
                        }
                    }
                    (_, _, Some(b)) => {
                        p.step-label { i { @if both { "2" } @else { "1" } } "Transfer from your bank app" }
                        (bank_details(order, b))
                        p.tip { img src="/assets/doodles/scribble-star.svg" alt=""; span { "Put " b.mono { (order.bank_ref) } " in the reference so Rachel can match your payment. It's short so it fits every bank's reference box. Then press the button below." } }
                        p.step-label { i { @if both { "3" } @else { "2" } } "Let Rachel know" }
                        form method="post" action={ (base) "/claim" } {
                            input type="hidden" name="method" value="bank";
                            button.btn.btn--accent.btn--big.btn--block type="submit" { "I've sent the payment" }
                        }
                    }
                    _ => {
                        div.mk-error role="alert" {
                            b { "Online payment isn't set up yet" }
                            p { "Please reply to Rachel's email and she'll sort out payment with you." }
                        }
                    }
                }
            }
        }
    };

    let content = html! {
        main.wrap {
            div.pay {
                div.pay__head {
                    div {
                        div.eyebrow { "Invoice · order " (order.number) }
                        h1 { "Pay for your order" }
                    }
                    div.pay__amount { b { (util::aud(order.total_cents)) } span { "AUD" } }
                }
                div.pay__grid {
                    (summary(order, lines, "Order summary", "For"))
                    section.panel #pay-panel { (panel) }
                }
                p.fineprint { "Questions about this invoice? Reply to Rachel's email or write to " b { "hello@rachelshandicrafts.com" } "." }
            }
        }
    };

    let mut p = Page::new("Pay for your order — Rachel's Handicrafts", content);
    p.head =
        html! { meta name="robots" content="noindex"; meta name="referrer" content="no-referrer"; };
    p.scripts = html! {
        script src=(assets::url("js/fixi.js")) {}
        script src=(assets::url("js/fixi-forms.js")) {}
        script src=(assets::url("js/cart.js")) {}
        script src=(assets::url("js/main.js")) {}
        script src=(assets::url("js/pay.js")) {}
    };
    page(p)
}

fn outcome(mark: Markup, waiting: bool, title: &str, text: &str) -> Markup {
    html! {
        div.outcome {
            div.outcome__mark.outcome__mark--wait[waiting] { (mark) }
            h2 { (title) }
            p { (text) }
        }
    }
}

fn bank_icon() -> Markup {
    html! {
        svg viewBox="0 0 40 40" fill="none" stroke="#29343a" stroke-width="2.6" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true" {
            path d="M5 15 L20 6 L35 15 Z" {}
            path d="M9 18v12M16 18v12M24 18v12M31 18v12" {}
            path d="M5 33h30" {}
        }
    }
}

fn copy_row(label: &str, value: &str, copy: bool, class: &str) -> Markup {
    html! {
        div class={ "bank__row " (class) } {
            dt { (label) }
            dd { (value) }
            @if copy { button.copy type="button" data-copy=(value) { "Copy" } } @else { span {} }
        }
    }
}

fn bank_details(order: &Order, b: &config::Bank) -> Markup {
    let amount = util::decimal(order.total_cents);
    let has_account = b.bsb.is_some() && b.account_number.is_some();
    html! {
        dl.bank {
            @if let Some(id) = &b.payid { (copy_row("PayID", id, true, "")) }
            @if let Some(n) = &b.account_name { (copy_row("Name", n, false, "")) }
            @if b.payid.is_none() && has_account {
                (copy_row("BSB", b.bsb.as_deref().unwrap_or(""), true, ""))
                (copy_row("Account", b.account_number.as_deref().unwrap_or(""), true, ""))
            }
            (copy_row("Amount", &format!("${amount}"), true, ""))
            div.bank__row.bank__row--ref {
                dt { "Reference" }
                dd { (order.bank_ref) }
                button.copy type="button" data-copy=(order.bank_ref) { "Copy" }
            }
        }
        @if b.payid.is_some() && has_account {
            details.field.bank-more {
                summary { "No PayID? Use BSB and account number" }
                dl.bank {
                    (copy_row("BSB", b.bsb.as_deref().unwrap_or(""), true, ""))
                    (copy_row("Account", b.account_number.as_deref().unwrap_or(""), true, ""))
                }
            }
        }
    }
}

fn short_address(a: &str) -> String {
    if a.len() > 20 {
        format!("{}…{}", &a[..8], &a[a.len() - 6..])
    } else {
        a.to_string()
    }
}

/// A plausible Monero address: standard (4…) or subaddress (8…), base58.
pub fn looks_like_monero_address(a: &str) -> bool {
    const B58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    (a.starts_with('4') || a.starts_with('8'))
        && (a.len() == 95 || a.len() == 106)
        && a.chars().all(|c| B58.contains(c))
}

pub fn paid_status(s: Status) -> bool {
    matches!(s, Status::Paid | Status::Dispatched | Status::Completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webp_names() {
        assert_eq!(
            to_webp("assets/products/tote/gallery_1.jpg"),
            "assets/products/tote/gallery_1.webp"
        );
        assert_eq!(to_webp("assets/products/x.svg"), "assets/products/x.svg");
    }

    #[test]
    fn monero_addresses() {
        let a = format!("4{}", "A".repeat(94));
        assert!(looks_like_monero_address(&a));
        assert!(!looks_like_monero_address("4abc"));
        assert!(
            !looks_like_monero_address(&format!("4{}", "0".repeat(94))),
            "0 isn't base58"
        );
    }
}

/// What the framed payment card shows.
pub enum CardState<'a> {
    Live(&'a crate::monokulo::MkDetail),
    /// Monokulo couldn't be read just now; the card tries again by itself.
    Retry,
}

/// The Monero payment card the pay page frames: amount, address, QR code and
/// progress, drawn from Monokulo's API. It refreshes itself (meta refresh), so
/// it needs no script.
pub fn monero_card(state: CardState, checkout_url: &str, now: i64) -> Markup {
    let mut refresh = 5;
    let body = match state {
        CardState::Retry => html! {
            div.mkc__state.mkc__state--warn {
                b { "Having trouble reaching the payment service" }
                span { "Trying again in a moment." }
            }
            p.mkc__fine { a href=(checkout_url) target="_blank" rel="noopener" { "Open the payment on Monokulo instead" } }
        },
        CardState::Live(d) => {
            let amount = crate::monokulo::xmr(d.xmr_amount_piconero);
            let left = d.expires_at - now;
            let (class, title, note): (&str, String, String) = match d.status.as_str() {
                "paid" | "overpaid" => ("ok", "Payment received".into(), "Thank you! This page will update in a moment.".into()),
                "unconfirmed" | "confirming" => ("wait", "Payment seen".into(), format!(
                    "Waiting for the network to confirm it{}.",
                    if d.confirmations_required > 0 { format!(" ({} of {})", d.confirmations, d.confirmations_required) } else { String::new() }
                )),
                "partial" => ("warn", "Part of the payment has arrived".into(), format!(
                    "{} XMR received so far. Please send the rest to the same address.",
                    crate::monokulo::xmr(d.amount_received_piconero)
                )),
                "expired" => ("warn", "This price has expired".into(), "Reload this page to get a fresh price. If you've already sent it, press \"I've paid\" below.".into()),
                "cancelled" => ("warn", "This payment was cancelled".into(), "Reload this page to start again.".into()),
                _ => ("wait", "Waiting for your payment".into(), if left > 0 {
                    format!("This price is held for about {} more minute{}.", (left + 59) / 60, if left <= 60 { "" } else { "s" })
                } else {
                    "Checking…".into()
                }),
            };
            if d.is_final {
                refresh = 30;
            }
            let live = !d.is_final && !matches!(d.status.as_str(), "paid" | "overpaid");
            let uri = format!("monero:{}?tx_amount={}", d.address, amount);
            let qr = if live && !d.address.is_empty() {
                qrcode::QrCode::new(uri.as_bytes()).ok().map(|code| {
                    PreEscaped(
                        code.render::<qrcode::render::svg::Color>()
                            .quiet_zone(true)
                            .min_dimensions(88, 88)
                            .dark_color(qrcode::render::svg::Color("#29343a"))
                            .light_color(qrcode::render::svg::Color("#ffffff"))
                            .build()
                            .replacen("<?xml version=\"1.0\" standalone=\"yes\"?>", "", 1),
                    )
                })
            } else {
                None
            };
            html! {
                @if live && !d.address.is_empty() {
                    div.mkc__card {
                        div.mkc__main {
                            p.mkc__label { "Send exactly" }
                            p.mkc__amount { b { (amount) } " XMR" }
                            p.mkc__addr { (d.address) }
                        }
                        @if let Some(svg) = qr {
                            div.mkc__qr { (svg) }
                        }
                        div class={ "mkc__foot mkc__foot--" (class) } {
                            span { b { (title) } " " (note) }
                            a href=(uri) { "Open wallet" }
                        }
                    }
                } @else {
                    div class={ "mkc__state mkc__state--" (class) } {
                        b { (title) }
                        span { (note) }
                    }
                }
            }
        }
    };
    html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                meta name="robots" content="noindex";
                meta name="referrer" content="no-referrer";
                meta http-equiv="refresh" content=(refresh);
                title { "Monero payment" }
                link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Karla:wght@400;500;700&family=IBM+Plex+Mono:wght@500&display=swap";
                link rel="stylesheet" href=(assets::url("css/style.css"));
            }
            body.mkc { (body) }
        }
    }
}
