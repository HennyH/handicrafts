//! The shop's pages, ported from the old minijinja templates.

use maud::{Markup, PreEscaped, html};

use super::layout::{Page, page, shop_scripts};
use crate::{assets, catalog::Recipe};

const MASONRY: &str = "https://cdnjs.cloudflare.com/ajax/libs/masonry/4.2.2/masonry.pkgd.min.js";
const IMAGESLOADED: &str =
    "https://cdnjs.cloudflare.com/ajax/libs/jquery.imagesloaded/5.0.0/imagesloaded.pkgd.min.js";

fn grid_scripts(init: &str) -> Markup {
    html! {
        script src=(MASONRY) {}
        script src=(IMAGESLOADED) {}
        (shop_scripts(true))
        script { (PreEscaped(init)) }
    }
}

pub fn index() -> Markup {
    let content = html! {
        main {
            section.hero {
                div.wrap {
                    div {
                        span.badge.badge--mustard { "Made in Perth, WA" }
                        h1 { "Welcome!" }
                        p.lead {
                            "Items made with care and designed to contribute to a thoughtful lifestyle. Crafted for the person who wants to shop mindfully and build a home that nurtures their family, community and environment."
                        }
                        div.hero__badges {
                            a.btn.btn--sage.btn--big href="/shop.html" { "Browse the shop →" }
                        }
                    }
                }
                img.art-decor.art-hero-chair src="/assets/art/chair.webp" data-orig="/assets/art/chair.jpg" alt="" aria-hidden="true";
            }

            div.divider-wrap {
                img.divider src="/assets/doodles/stitch-divider.svg" alt="";
                img.divider__scissors src="/assets/doodles/scissors.svg" alt="" aria-hidden="true";
            }

            section.wrap.confetti-band #story {
                h2.section-title { "Why it's a bit different" }
                p.section-sub { "Nothing here comes off a factory line. Here's what \"local and sustainable\" actually means, in practice." }
                div.about-block {
                    img src="/assets/art/bernina.webp" data-orig="/assets/art/bernina.jpg" alt="A hand-drawn illustration of a Bernina 930 Record sewing machine";
                    div {
                        p { "I make small batches of home crafted goods, here - in Perth, Western Australia. With my Bernina 930 Record and other crafting tools I make everything from tote bags to knitted beanies." }
                        p { "I keep things small-batch because that's what one person, working by hand, can actually do well. If something you love isn't in stock, just ask — I can usually make more." }
                    }
                }
                div.feature-row {
                    div.feature {
                        img src="/assets/doodles/fabric-offcuts.svg" alt="";
                        h3 { "Deadstock & offcuts first" }
                        p { "I source deadstock and leftover fabric where I can, so good cloth doesn't end up in landfill." }
                    }
                    div.feature {
                        img src="/assets/doodles/hand-stitch-pin.svg" alt="";
                        h3 { "Made locally, by hand" }
                        p { "Using hand picked textiles - every single seam is sewn by me here in Perth." }
                    }
                    div.feature {
                        img src="/assets/doodles/heart-stitch.svg" alt="";
                        h3 { "Made to last" }
                        p { "I'd rather make something you keep for years than something disposable. Simple, sturdy construction, always." }
                    }
                }
            }

            section.wrap {
                h2.section-title { "Some of my favourite craftings" }
                p.section-sub { "The full catalogue lives on the shop page." }
                div.grid.grid--masonry #featured-grid {}
                div style="text-align:center; margin-top:-20px;" {
                    a.btn.btn--sage href="/shop.html" { "See the whole shop →" }
                }
            }

            section.wrap style="text-align:center; margin: 50px auto 70px;" {
                p.section-sub style="margin-bottom:10px;" { "Questions about a piece, custom requests - feel free to get in contact." }
                a.btn.btn--accent.btn--big.btn--with-icon href="mailto:hello@rachelshandicrafts.com" {
                    img.btn__icon src="/assets/doodles/envelope-seal.svg" alt="" aria-hidden="true";
                    "hello@rachelshandicrafts.com"
                }
            }
        }
    };
    page(Page {
        title: "Rachel's Handicrafts",
        description: Some(
            "A small shop of items made with care and designed to contribute to a thoughtful lifestyle. Crafted for the person who wants to shop mindfully and build a home that nurtures their family, community and environment.",
        ),
        head: html! {
            link rel="preload" href="/data/products.json" as="fetch" crossorigin="anonymous";
            link rel="preload" href="/assets/art/chair.webp" as="image" fetchpriority="high";
        },
        content,
        scripts: grid_scripts(r##"initProductGrid("#featured-grid", { limit: 3 });"##),
    })
}

pub fn shop() -> Markup {
    let content = html! {
        main.wrap {
            div.page-header {
                h1.section-title style="margin-top:40px;" { "The Shop" }
            }
            p.section-sub { "Everything here is cut and sewn by hand in small batches. If something's not listed the way you'd like it, just ask — I can usually make more." }
            div.grid.grid--masonry #product-grid {}
        }
    };
    page(Page {
        title: "Shop — Rachel's Handicrafts",
        description: None,
        head: html! { link rel="preload" href="/data/products.json" as="fetch" crossorigin="anonymous"; },
        content,
        scripts: grid_scripts(r##"initProductGrid("#product-grid");"##),
    })
}

pub fn cart() -> Markup {
    let content = html! {
        main.wrap {
            h1.section-title style="margin-top:40px;" { "Your Cart" }
            div.cart-panel {
                div #cart-empty .empty-state style="display:none;" {
                    img src="/assets/doodles/heart-stitch.svg" alt="";
                    p { "Your cart's empty. Go find something lovely on the " a href="/shop.html" { "shop page" } "." }
                }
                div #cart-contents {}
                div #cart-summary style="margin-top:20px;" {}
                a #go-to-checkout .btn.btn--accent.btn--block href="/checkout.html" style="margin-top:24px; display:none;" { "Proceed to checkout" }
            }
        }
    };
    let mut p = Page::new("Your Cart — Rachel's Handicrafts", content);
    p.scripts = html! {
        (shop_scripts(true))
        script src=(assets::url("js/cart-page.js")) {}
    };
    page(p)
}

pub fn recipes(recipes: &[Recipe]) -> Markup {
    let content = html! {
        main.wrap {
            div.page-header {
                h1.section-title style="margin-top:40px;" { "Recipes" }
            }
            p.section-sub { "A handful of family favourites, written down properly at last." }
            div.recipe-grid {
                @for r in recipes {
                    a class={ "card recipe-card recipe-card--" (r.accent) } href={ "/recipe-" (r.id) ".html" } {
                        h2.recipe-card__title { (r.title) }
                        p.recipe-card__tagline { (r.tagline) }
                        span.recipe-card__link { "View recipe →" }
                    }
                }
            }
        }
    };
    page(Page {
        title: "Recipes — Rachel's Handicrafts",
        description: Some(
            "A handful of family recipes from Rachel's kitchen — scones, biscuits, soup and more.",
        ),
        head: html! {},
        content,
        scripts: shop_scripts(false),
    })
}

/// One recipe. Ingredients, method, note, byline and credit are raw HTML
/// from data/recipes.json, rendered as written (see the README).
pub fn recipe(r: &Recipe) -> Markup {
    let title = format!("{} — Rachel's Handicrafts", r.title);
    let content = html! {
        main.wrap {
            article class={ "recipe recipe--" (r.accent) } {
                header.recipe__header {
                    a.recipe__back href="/recipes.html" { "← All recipes" }
                    h1.recipe__title { (r.title) }
                    p.recipe__tagline { (r.tagline) }
                }
                div.recipe__body {
                    section.recipe__ingredients {
                        h2.recipe__section-title { "Ingredients" }
                        form.ingredient-list onsubmit="return false;" {
                            ul {
                                @for line in &r.ingredients {
                                    li {
                                        label {
                                            input type="checkbox";
                                            span.ingredient-check aria-hidden="true" {}
                                            span.ingredient-text { (PreEscaped(line)) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    section.recipe__method {
                        h2.recipe__section-title { "Method" }
                        ol {
                            @for step in &r.method { li { (PreEscaped(step)) } }
                        }
                    }
                }
                @if let Some(note) = &r.note {
                    p.recipe__note { strong { "Note:" } " " (PreEscaped(note)) }
                }
                footer.recipe__footer {
                    p.recipe__byline { (PreEscaped(&r.byline)) }
                    @if let Some(c) = &r.credit { p.recipe__credit { (PreEscaped(c)) } }
                    @if r.donation { (donation()) }
                }
            }
        }
    };
    page(Page {
        title: &title,
        description: Some(&r.tagline),
        head: html! {},
        content,
        scripts: shop_scripts(false),
    })
}

const DONATION_ADDRESS: &str = "89AqqVpcF4QFXrfezB3s77EUUrqG2oEBY5P2xARr9SRMhRJAuoszNYSEsnrmoCPDjXU3CMF38ZjFy1TemqPYXotv36ridye";

fn donation() -> Markup {
    let uri = format!("monero:{DONATION_ADDRESS}");
    html! {
        section.donation {
            details.donation__toggle {
                summary.donation__summary {
                    img.donation__icon src="/assets/doodles/monero.svg" alt="" aria-hidden="true";
                    "Support with Monero"
                    span.donation__chevron aria-hidden="true" { "▾" }
                }
                div.donation__panel {
                    a href=(uri) {
                        img.donation__qr src="/assets/doodles/monero-qr.svg" alt="QR code for the Monero donation address";
                    }
                    a.donation__address href=(uri) { (DONATION_ADDRESS) }
                }
            }
        }
    }
}

pub fn not_found() -> Markup {
    let content = html! {
        main.wrap {
            div.checkout-panel {
                div.empty-state {
                    img src="/assets/doodles/heart-stitch.svg" alt="";
                    p { "There's nothing at this address. Try the " a href="/shop.html" { "shop" } " or the " a href="/index.html" { "home page" } "." }
                }
            }
        }
    };
    page(Page::new("Not found — Rachel's Handicrafts", content))
}

/// A friendly page for errors a customer can hit.
pub fn problem(title: &str, message: &str) -> Markup {
    let content = html! {
        main.wrap {
            div.checkout-panel {
                div.empty-state {
                    img src="/assets/doodles/heart-stitch.svg" alt="";
                    h2 { (title) }
                    p { (message) }
                    p { "You can also write to " b { "hello@rachelshandicrafts.com" } "." }
                }
            }
        }
    };
    page(Page::new("Rachel's Handicrafts", content))
}
