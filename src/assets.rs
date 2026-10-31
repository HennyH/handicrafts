//! Static files (CSS, JS, images, the product catalogue), compiled into the
//! binary. In debug builds rust-embed reads them from disk, so edits show up
//! on reload.
//!
//! CSS and JS are linked with a content hash (`/css/style.css?v=…`), so a
//! changed file is fetched fresh without bumping a number by hand.

use std::{collections::HashMap, sync::OnceLock};

use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode, header},
};
use rust_embed::RustEmbed;
use sha2::{Digest, Sha256};

#[derive(RustEmbed)]
#[folder = "."]
#[include = "css/*"]
#[include = "js/*"]
#[include = "assets/**/*"]
#[include = "data/products.json"]
#[include = "favicon.ico"]
pub struct Files;

/// `/css/style.css?v=1a2b3c4d`
pub fn url(path: &str) -> String {
    static HASHES: OnceLock<std::sync::Mutex<HashMap<String, String>>> = OnceLock::new();
    let path = path.trim_start_matches('/');
    let cache = HASHES.get_or_init(Default::default);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cfg!(debug_assertions) {
        cache.clear();
    }
    let v = cache
        .entry(path.to_string())
        .or_insert_with(|| match Files::get(path) {
            Some(f) => Sha256::digest(&f.data)
                .iter()
                .take(4)
                .map(|b| format!("{b:02x}"))
                .collect(),
            None => "0".into(),
        })
        .clone();
    format!("/{path}?v={v}")
}

/// Serve an embedded file, or `None` if there's no such file.
pub fn serve(path: &str, versioned: bool) -> Option<Response<Body>> {
    let path = path.trim_start_matches('/');
    let file = Files::get(path)?;
    let mime = file.metadata.mimetype();
    let cache = if versioned {
        "public, max-age=31536000, immutable"
    } else if path.starts_with("data/") {
        "no-cache"
    } else if path.starts_with("assets/") {
        "public, max-age=604800"
    } else {
        "public, max-age=3600"
    };
    let etag = format!(
        "\"{}\"",
        file.metadata
            .sha256_hash()
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime)
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        )
        .header(header::CACHE_CONTROL, cache)
        .header(header::ETAG, etag)
        .body(Body::from(file.data.into_owned()))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_embedded_and_versioned() {
        assert!(Files::get("css/style.css").is_some());
        assert!(Files::get("js/fixi.js").is_some());
        assert!(Files::get("data/products.json").is_some());
        assert!(
            Files::get("data/recipes.json").is_none(),
            "only what pages fetch"
        );
        let u = url("css/style.css");
        assert!(u.starts_with("/css/style.css?v=") && u.len() > 20, "{u}");
        assert!(serve("css/style.css", true).is_some());
        assert!(serve("../Cargo.toml", false).is_none());
    }
}
