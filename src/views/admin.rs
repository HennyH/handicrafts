//! /admin: orders, their timeline and status, the invoice template.
//! Plain and quick to scan; its own small stylesheet (css/admin.css).

use maud::{DOCTYPE, Markup, html};

use crate::{
    assets,
    catalog::PricedLine,
    db::{Alert, Event, MonokuloOrder, Order, OutboxRow},
    invoice,
    orders::Status,
    util,
};

fn shell(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                meta name="robots" content="noindex";
                title { (title) " · Shop admin" }
                link rel="icon" href="/favicon.ico" sizes="any";
                link rel="stylesheet" href=(assets::url("css/admin.css"));
            }
            body {
                header.top {
                    a.top__home href="/admin" { "Orders" }
                    a href="/admin/settings" { "Invoice template" }
                    a href="/" target="_blank" { "Shop ↗" }
                }
                (body)
                script src=(assets::url("js/fixi.js")) {}
                script src=(assets::url("js/admin.js")) {}
            }
        }
    }
}

pub fn pill(s: Status) -> Markup {
    html! { span class={ "pill pill--" (s.as_str()) } { (s.label()) } }
}

fn banners(alerts: &[Alert], failed: &[OutboxRow], csrf: &str) -> Markup {
    html! {
        @for a in alerts {
            div.banner.banner--bad role="alert" {
                b { "Since " (util::datetime(a.since)) ": " } (a.message)
            }
        }
        @if !failed.is_empty() {
            div.banner.banner--bad {
                b { (failed.len()) " email" @if failed.len() != 1 { "s" } " couldn't be sent." }
                ul.failed {
                    @for m in failed {
                        li {
                            span { (m.subject) " → " (m.to_addr) }
                            small { (m.last_error.as_deref().unwrap_or("")) }
                            form method="post" action={ "/admin/outbox/" (m.id) "/resend" } {
                                input type="hidden" name="csrf" value=(csrf);
                                button.btn.btn--small type="submit" { "Resend" }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn list(
    orders: &[Order],
    counts: &[(Status, i64)],
    filter: Option<Status>,
    q: Option<&str>,
    alerts: &[Alert],
    failed: &[OutboxRow],
    csrf: &str,
) -> Markup {
    let count = |s: Status| {
        counts
            .iter()
            .find(|(c, _)| *c == s)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    };
    let total: i64 = counts.iter().map(|(_, n)| n).sum();
    shell(
        "Orders",
        html! {
            main.page {
                (banners(alerts, failed, csrf))
                div.list-head {
                    h1 { "Orders" }
                    form.search method="get" action="/admin" {
                        @if let Some(f) = filter { input type="hidden" name="status" value=(f.as_str()); }
                        input type="search" name="q" placeholder="Order number, bank ref, name or email" value=(q.unwrap_or("")) aria-label="Search orders";
                        button.btn type="submit" { "Search" }
                    }
                }
                nav.tabs aria-label="Filter by status" {
                    a href="/admin" aria-current=(filter.is_none()) { "All " span.n { (total) } }
                    @for s in Status::ALL {
                        a href={ "/admin?status=" (s.as_str()) } aria-current=(filter == Some(s)) {
                            (s.label()) " " span.n.n--new[s == Status::New && count(s) > 0] { (count(s)) }
                        }
                    }
                }
                @if orders.is_empty() {
                    p.empty { @if q.is_some() || filter.is_some() { "No orders match." } @else { "No orders yet. They'll appear here as soon as someone checks out." } }
                } @else {
                    div.scroll-x {
                        table.orders {
                            thead { tr { th { "Order" } th { "Placed" } th { "Customer" } th.num { "Total" } th { "Status" } th { "Bank ref" } } }
                            tbody {
                                @for o in orders {
                                    tr {
                                        td { a href={ "/admin/orders/" (o.id) } { (o.number) } }
                                        td { (util::datetime(o.created_at)) }
                                        td { (o.customer_name) br; small { (o.customer_email) } }
                                        td.num { (util::aud(o.total_cents)) }
                                        td { (pill(o.status)) }
                                        td.mono { (o.bank_ref) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub struct Detail<'a> {
    pub order: &'a Order,
    pub lines: &'a [PricedLine],
    pub events: &'a [Event],
    pub monokulo: Option<&'a MonokuloOrder>,
    pub pay_link: String,
    pub mailto: Option<String>,
    pub invoice_error: Option<String>,
    pub error: Option<&'a str>,
    pub csrf: &'a str,
}

pub fn detail(d: Detail) -> Markup {
    let o = d.order;
    let options: Vec<Status> = std::iter::once(o.status).chain(o.status.next()).collect();
    shell(
        &o.number,
        html! {
            main.page {
                p.crumbs { a href="/admin" { "Orders" } " / " span.mono { (o.number) } }
                div.detail-head {
                    h1 { (o.number) " " (pill(o.status)) }
                    span.muted { "Placed " (util::datetime(o.created_at)) }
                }
                div.grid {
                    div.col {
                        section.card {
                            h2 { "Status" }
                            form.status-row #status-form method="post" action={ "/admin/orders/" (o.id) "/status" } {
                                input type="hidden" name="csrf" value=(d.csrf);
                                input type="hidden" name="from" value=(o.status.as_str());
                                label.check { input type="checkbox" name="accepted" value="1" #accepted checked[o.accepted]; " Accepted" }
                                select name="status" #status aria-label="Status" {
                                    @for s in &options {
                                        option value=(s.as_str()) selected[*s == o.status] data-needs-accepted=(s.needs_accepted()) {
                                            (s.label()) @if *s == o.status { " (current)" }
                                        }
                                    }
                                }
                                button.btn type="submit" { "Save" }
                            }
                            @if let Some(e) = d.error { p.err role="alert" { (e) } }
                            p.hint { "Untick Accepted and only New and Rejected are on offer. Monero payments move the order to Paid by themselves." }
                        }
                        section.card {
                            h2 { "Customer" }
                            dl.kv {
                                dt { "Name" } dd { (o.customer_name) }
                                dt { "Email" } dd { a href={ "mailto:" (o.customer_email) } { (o.customer_email) } }
                                @if !o.customer_phone.is_empty() { dt { "Phone" } dd { a href={ "tel:" (o.customer_phone) } { (o.customer_phone) } } }
                                dt { "Delivery" } dd.pre { (if o.customer_address.is_empty() { "—" } else { &o.customer_address }) }
                                @if !o.customer_notes.is_empty() { dt { "Notes" } dd.pre { (o.customer_notes) } }
                                dt { "Bank ref" } dd.mono.big { (o.bank_ref) }
                                @if !o.refund_address.is_empty() { dt { "Refund address" } dd.mono.wrap { (o.refund_address) } }
                            }
                        }
                        section.card {
                            h2 { "Items" }
                            table.items {
                                @for l in d.lines {
                                    tr {
                                        td { (l.qty) " × " (l.title()) @if let Some(m) = &l.materials { br; small { (l.sku) " · " (m) } } }
                                        td.num { (util::aud(l.total_cents())) }
                                    }
                                }
                                tr.total { td { "Total" } td.num { (util::aud(o.total_cents)) } }
                            }
                            div.actions {
                                a.btn.btn--accent href={ "/admin/orders/" (o.id) "/invoice.eml" } { "Invoice" }
                                @if let Some(m) = &d.mailto { a.btn href=(m) { "Invoice in mail app (phone)" } }
                                button.btn type="button" data-copy=(d.pay_link) { "Copy pay link" }
                            }
                            @if let Some(e) = &d.invoice_error { p.err { "The invoice template has a problem: " (e) " " a href="/admin/settings" { "Fix it" } } }
                            p.hint { "Invoice downloads an email draft (.eml) that opens in your mail app. Send it, then set the status to Invoiced." }
                        }
                        @if let Some(mk) = d.monokulo {
                            section.card {
                                h2 { "Monero (Monokulo)" }
                                dl.kv {
                                    dt { "Monokulo order" } dd.mono.wrap { (mk.mk_order_id) }
                                    dt { "Amount" } dd { (crate::monokulo::xmr(mk.xmr_piconero)) " XMR for " (util::aud(mk.amount_cents)) }
                                    dt { "Status" } dd { (mk.status) }
                                }
                            }
                        }
                    }
                    div.col {
                        section.card {
                            h2 { "Timeline" }
                            form.comment-form method="post" action={ "/admin/orders/" (o.id) "/comments" }
                                fx-action={ "/admin/orders/" (o.id) "/comments" } fx-method="post" fx-target="#timeline" fx-swap="outerHTML" {
                                input type="hidden" name="csrf" value=(d.csrf);
                                textarea name="body" required maxlength="4000" placeholder="e.g. Called Sam, pickup Saturday morning" aria-label="Comment" {}
                                button.btn type="submit" { "Add comment" }
                            }
                            (timeline(d.events))
                        }
                    }
                }
            }
        },
    )
}

pub fn timeline(events: &[Event]) -> Markup {
    html! {
        ol.timeline #timeline {
            @for e in events {
                li class={ "ev ev--" (e.kind) } {
                    div.ev__meta {
                        span.who { (actor_label(&e.actor)) }
                        time { (util::datetime(e.created_at)) }
                    }
                    div.ev__body { (e.body) }
                }
            }
        }
    }
}

fn actor_label(a: &str) -> &str {
    match a {
        "admin" => "Admin",
        "customer" => "Customer",
        "webhook" => "Monokulo",
        "website" => "Website",
        _ => "System",
    }
}

pub fn settings(
    subject: &str,
    body: &str,
    error: Option<&str>,
    saved: bool,
    preview: Option<&invoice::Draft>,
    csrf: &str,
) -> Markup {
    shell(
        "Invoice template",
        html! {
            main.page {
                h1 { "Invoice template" }
                p.hint { "Used by the Invoice button on each order. Placeholders in " code { "{{ double braces }}" } " are filled in from the order." }
                @if let Some(e) = error { div.banner.banner--bad role="alert" { (e) } }
                @if saved { div.banner.banner--good { "Saved." } }
                div.grid {
                    div.col {
                        form.card.template method="post" action="/admin/settings" {
                            input type="hidden" name="csrf" value=(csrf);
                            label for="subject" { "Subject" }
                            input #subject type="text" name="subject" value=(subject) required maxlength="200";
                            label for="body" { "Body" }
                            textarea #body name="body" rows="18" required maxlength="10000" { (body) }
                            div.actions {
                                button.btn.btn--accent type="submit" name="action" value="save" { "Save" }
                                button.btn type="submit" name="action" value="preview" { "Preview with the newest order" }
                                button.btn type="submit" name="action" value="reset" { "Reset to the default" }
                            }
                        }
                    }
                    div.col {
                        @if let Some(p) = preview {
                            section.card {
                                h2 { "Preview" }
                                p { b { "Subject: " } (p.subject) }
                                pre.preview { (p.body) }
                            }
                        }
                        section.card {
                            h2 { "Placeholders" }
                            dl.kv {
                                @for (k, v) in invoice::PLACEHOLDERS {
                                    dt.mono { "{{ " (k) " }}" } dd { (v) }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub fn locked(reason: &str) -> Markup {
    shell(
        "Admin",
        html! { main.page { h1 { "Admin" } p { (reason) } } },
    )
}
