//! The product catalogue and the recipes, from `data/products.json` and
//! `data/recipes.json`, compiled into the binary. Editing either file and
//! rebuilding is still how products and recipes change.
//!
//! The server prices every order from here: the browser's cart only says
//! which products, which fabric and how many.

use serde::Deserialize;

pub const PRODUCTS_JSON: &str = include_str!("../data/products.json");
pub const RECIPES_JSON: &str = include_str!("../data/recipes.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: String,
    pub sku: String,
    pub name: String,
    /// AUD; `None` is a "custom quote" item that can't be ordered online.
    pub price: Option<f64>,
    #[serde(default)]
    pub materials: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub images: Vec<String>,
    #[serde(default)]
    pub swatches: Vec<Swatch>,
    #[serde(default)]
    pub coming_soon: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Swatch {
    pub name: String,
    #[serde(default)]
    pub materials: Option<String>,
}

impl Product {
    pub fn first_image(&self) -> Option<&str> {
        self.images
            .first()
            .map(String::as_str)
            .or(self.image.as_deref())
    }

    pub fn price_cents(&self) -> Option<i64> {
        self.price.map(|p| (p * 100.0).round() as i64)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Recipe {
    pub id: String,
    pub title: String,
    pub tagline: String,
    pub accent: String,
    pub byline: String,
    #[serde(default)]
    pub donation: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub credit: Option<String>,
    pub ingredients: Vec<String>,
    pub method: Vec<String>,
}

pub struct Catalog {
    pub products: Vec<Product>,
    pub recipes: Vec<Recipe>,
}

impl Catalog {
    pub fn load() -> Catalog {
        Catalog {
            products: serde_json::from_str(PRODUCTS_JSON).expect("data/products.json: invalid"),
            recipes: serde_json::from_str(RECIPES_JSON).expect("data/recipes.json: invalid"),
        }
    }

    pub fn product(&self, id: &str) -> Option<&Product> {
        self.products.iter().find(|p| p.id == id)
    }

    pub fn recipe(&self, id: &str) -> Option<&Recipe> {
        self.recipes.iter().find(|r| r.id == id)
    }

    /// Turn the browser's cart into priced order lines, refusing anything
    /// that can't be bought: unknown or "coming soon" products, custom-quote
    /// items, made-up fabric names, silly quantities.
    pub fn price_cart(&self, cart: &[CartLine]) -> Result<Vec<PricedLine>, String> {
        if cart.is_empty() {
            return Err("Your cart is empty.".into());
        }
        if cart.len() > 30 {
            return Err(
                "That's more lines than one order can take. Please get in touch instead.".into(),
            );
        }
        let mut lines: Vec<PricedLine> = Vec::new();
        for line in cart {
            let p = self
                .product(&line.id)
                .filter(|p| !p.coming_soon)
                .ok_or_else(|| {
                    "Something in your cart isn't available any more. Please check your cart."
                        .to_string()
                })?;
            let unit = p.price_cents().ok_or_else(|| {
                format!(
                    "{} is made to quote, so it can't be ordered online. Please get in touch.",
                    p.name
                )
            })?;
            if !(1..=20).contains(&line.qty) {
                return Err(format!("Please choose between 1 and 20 of {}.", p.name));
            }
            let swatch = match line.color.as_deref().filter(|c| !c.is_empty()) {
                None if p.swatches.is_empty() => None,
                None => return Err(format!("Please pick a fabric for {}.", p.name)),
                Some(c) => Some(
                    p.swatches
                        .iter()
                        .find(|s| s.name == c)
                        .ok_or_else(|| format!("{} doesn't come in {c}.", p.name))?,
                ),
            };
            let materials = swatch
                .and_then(|s| s.materials.clone())
                .or_else(|| p.materials.clone());
            let color = swatch.map(|s| s.name.clone());
            if let Some(existing) = lines
                .iter_mut()
                .find(|l| l.product_id == p.id && l.color == color)
            {
                existing.qty += line.qty;
                continue;
            }
            lines.push(PricedLine {
                product_id: p.id.clone(),
                sku: p.sku.clone(),
                name: p.name.clone(),
                color,
                materials,
                image: p.first_image().map(str::to_string),
                unit_cents: unit,
                qty: line.qty,
            });
        }
        Ok(lines)
    }
}

/// One line of the browser's cart, as posted with the order.
#[derive(Debug, Clone, Deserialize)]
pub struct CartLine {
    pub id: String,
    #[serde(default)]
    pub color: Option<String>,
    pub qty: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PricedLine {
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub color: Option<String>,
    pub materials: Option<String>,
    pub image: Option<String>,
    pub unit_cents: i64,
    pub qty: i64,
}

impl PricedLine {
    pub fn total_cents(&self) -> i64 {
        self.unit_cents * self.qty
    }

    /// "Tote Bag — Navy", as the old order PDF named it.
    pub fn title(&self) -> String {
        match &self.color {
            Some(c) => format!("{} — {c}", self.name),
            None => self.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, color: Option<&str>, qty: i64) -> CartLine {
        CartLine {
            id: id.into(),
            color: color.map(Into::into),
            qty,
        }
    }

    #[test]
    fn catalogue_parses() {
        let c = Catalog::load();
        assert!(c.product("tote-bag").is_some());
        assert!(!c.recipes.is_empty());
    }

    #[test]
    fn prices_from_the_catalogue_and_merges_duplicates() {
        let c = Catalog::load();
        let lines = c
            .price_cart(&[
                line("tote-bag", Some("Navy"), 1),
                line("banneton", None, 1),
                line("tote-bag", Some("Navy"), 1),
            ])
            .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].qty, 2);
        assert_eq!(lines[0].materials.as_deref(), Some("100% GOTS cotton"));
        let total: i64 = lines.iter().map(PricedLine::total_cents).sum();
        assert_eq!(total, 2 * 6000 + 4500);
    }

    #[test]
    fn refuses_what_cant_be_bought() {
        let c = Catalog::load();
        assert!(c.price_cart(&[]).is_err());
        assert!(c.price_cart(&[line("nope", None, 1)]).is_err());
        assert!(
            c.price_cart(&[line("handkerchiefs", None, 1)]).is_err(),
            "coming soon"
        );
        assert!(
            c.price_cart(&[line("custom-tailoring", None, 1)]).is_err(),
            "custom quote"
        );
        assert!(
            c.price_cart(&[line("tote-bag", Some("Tartan"), 1)])
                .is_err(),
            "made-up fabric"
        );
        assert!(
            c.price_cart(&[line("tote-bag", None, 1)]).is_err(),
            "fabric missing"
        );
        assert!(c.price_cart(&[line("banneton", None, 0)]).is_err());
        assert!(c.price_cart(&[line("banneton", None, 21)]).is_err());
    }
}
