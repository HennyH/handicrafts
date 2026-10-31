//! The shared shell of every shop page: head, header and footer. Ported from
//! the old `templates/base.html`.

use maud::{DOCTYPE, Markup, PreEscaped, html};

use crate::assets;

pub struct Page<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    /// Extra tags in <head> (preloads).
    pub head: Markup,
    pub content: Markup,
    pub scripts: Markup,
}

impl<'a> Page<'a> {
    pub fn new(title: &'a str, content: Markup) -> Page<'a> {
        Page {
            title,
            description: None,
            head: html! {},
            content,
            scripts: html! {},
        }
    }
}

/// The inline head script from base.html: marks JS as available before first
/// paint, and applies the full-res paper texture once it's known to be cached.
const HEAD_SCRIPT: &str = r#"document.documentElement.classList.add("js");
try {
  if (localStorage.getItem("rh_paper_hires")) {
    document.documentElement.style.setProperty("--paper-bg", 'url("/assets/art/paper_bg_100.webp")');
  }
} catch (e) {}"#;

pub fn page(p: Page) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (p.title) }
                @if let Some(d) = p.description { meta name="description" content=(d); }
                link rel="icon" href="/favicon.ico" sizes="any";
                link rel="apple-touch-icon" href="/assets/icons/apple-touch-icon.png";
                link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
                link rel="preconnect" href="https://cdn.fontshare.com" crossorigin;
                link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Libre+Franklin:ital,wght@0,500;0,600;0,700;0,800;1,600&family=Karla:wght@400;500;700&family=IBM+Plex+Mono:wght@500&display=swap";
                link rel="stylesheet" href="https://api.fontshare.com/v2/css?f[]=stardom&display=swap";
                link rel="stylesheet" href=(assets::url("css/style.css"));
                script { (PreEscaped(HEAD_SCRIPT)) }
                script defer src=(assets::url("js/bg-swap.js")) {}
                (p.head)
            }
            body {
                header.site-header {
                    div.wrap {
                        a.brand href="/index.html" {
                            img src="/assets/art/cropped_logo.webp" width="176" height="219" alt="";
                            span.brand__name { "Rachel's  Handicrafts" }
                        }
                        button.nav__toggle aria-label="Toggle menu" aria-expanded="false" {
                            "☰" span.cart-count data-cart-count aria-hidden="true" { "0" }
                        }
                        ul.nav__menu {
                            li { a href="/index.html" { "Home" } }
                            li { a href="/shop.html" { "Shop" } }
                            li { a href="/recipes.html" { "Recipes" } }
                            li { a.cart-link href="/cart.html" { "Cart " span.cart-count data-cart-count { "0" } } }
                        }
                    }
                }
                (p.content)
                footer.site-footer {
                    div.wrap {
                        div {
                            span.brand__name.script { "Rachel's Handicrafts" }
                            p { "Perth, Western Australia" }
                            p { "hello@rachelshandicrafts.com" }
                        }
                        div {
                            p { "© " span data-year {} " Rachel's Handicrafts" }
                            p { "Made by hand (with some assistance from my Bernina)." }
                        }
                    }
                    img.art-decor.art-footer src="/assets/art/leaf.webp" data-orig="/assets/art/leaf.jpg" alt="" aria-hidden="true";
                }
                (p.scripts)
            }
        }
    }
}

/// The scripts most shop pages load, in the old order.
pub fn shop_scripts(products: bool) -> Markup {
    html! {
        @if products { script src=(assets::url("js/products.js")) {} }
        script src=(assets::url("js/cart.js")) {}
        script src=(assets::url("js/main.js")) {}
    }
}
