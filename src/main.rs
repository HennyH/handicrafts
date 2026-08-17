//! Builds the site into `dist/`.
//!
//! Every page shares one `templates/base.html` (head, header, footer), so the
//! nav and footer exist in exactly one place instead of being copy-pasted into
//! four files and slowly drifting apart. `cargo run` produces a complete,
//! servable site — HTML plus the static directories — and CI uploads `dist/`
//! straight to GitHub Pages, so the generated HTML is never committed and can
//! never fall out of sync with the templates.

use minijinja::{context, Environment};
use std::path::Path;
use std::{fs, io};

/// Cache-buster appended to css/style.css. Bump when the stylesheet changes,
/// otherwise returning visitors keep being served the copy they cached.
const CSS_V: u32 = 24;

const PAGES: [&str; 4] = ["index", "shop", "cart", "checkout"];

/// Copied into `dist/` verbatim. CNAME has to travel with the artifact or the
/// custom domain is dropped on deploy; favicon.ico sits at the site root
/// because browsers request /favicon.ico on their own, with or without a
/// <link> tag pointing at it.
const STATIC: [&str; 6] = ["css", "js", "assets", "data", "CNAME", "favicon.ico"];

/// Recursive copy that skips files whose destination is already up to date —
/// assets/ is tens of megabytes and would otherwise be rewritten every build.
fn copy_into(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_into(&entry.path(), &dst.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let (Ok(from), Ok(to)) = (fs::metadata(src), fs::metadata(dst)) {
        if let (Ok(from), Ok(to)) = (from.modified(), to.modified()) {
            if from <= to {
                return Ok(());
            }
        }
    }
    fs::copy(src, dst)?;
    Ok(())
}

fn main() -> io::Result<()> {
    let out = Path::new("dist");
    fs::create_dir_all(out)?;

    let mut env = Environment::new();
    // strips the newline directly after a block tag, so a page that leaves an
    // optional block (like `description`) undefined doesn't leave a blank line
    env.set_trim_blocks(true);
    // without this the final newline of base.html is dropped, leaving files
    // without a trailing newline
    env.set_keep_trailing_newline(true);
    env.set_loader(minijinja::path_loader("templates"));

    for page in PAGES {
        let name = format!("{page}.html");
        let html = env
            .get_template(&name)
            .unwrap_or_else(|e| panic!("template {name}: {e}"))
            .render(context! { css_v => CSS_V })
            .unwrap_or_else(|e| panic!("rendering {name}: {e}"));
        fs::write(out.join(&name), html)?;
    }

    for item in STATIC {
        let path = Path::new(item);
        if path.exists() {
            copy_into(path, &out.join(item))?;
        }
    }

    println!("built {} pages into dist/", PAGES.len());
    Ok(())
}
