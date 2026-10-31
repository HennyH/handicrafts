/* main.js — bits shared by every page: mobile nav + footer year */

// The Monero donation disclosure is a native <details> — it opens and closes
// perfectly well with zero JS, so that stays the fallback. This only adds the
// bit native <details> doesn't do on its own: closing when you click outside
// it, which is what makes it read as a dropdown rather than an expanding
// block of page, matching the fabric swatch picker's dropdown behaviour.
document.addEventListener("click", (e) => {
  document.querySelectorAll(".donation__toggle[open]").forEach((details) => {
    if (!details.contains(e.target)) details.removeAttribute("open");
  });
});

// Copy buttons (order number, bank details on the pay page).
document.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-copy]");
  if (!btn) return;
  const text = btn.getAttribute("data-copy");
  const old = btn.textContent;
  const done = () => {
    btn.textContent = "Copied";
    setTimeout(() => (btn.textContent = old), 1200);
  };
  try {
    navigator.clipboard.writeText(text).then(done, () => window.prompt("Copy this:", text));
  } catch (err) {
    window.prompt("Copy this:", text);
  }
});

document.addEventListener("DOMContentLoaded", () => {
  const toggle = document.querySelector(".nav__toggle");
  const menu = document.querySelector(".nav__menu");
  if (toggle && menu) {
    toggle.addEventListener("click", () => {
      const open = menu.classList.toggle("is-open");
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
    });
  }
  // Mark the nav link for the page you're on. Links that point at an anchor
  // (About -> index.html#story) are skipped: they're a place on a page, not a
  // page, and matching them would bold two items at once on the home page.
  // Each recipe is its own page (recipe-scones.html, ...), so it never matches
  // the "Recipes" link (recipes.html) by filename alone — treated as a match
  // here, the same way a product's own page would belong under "Shop" if one
  // existed.
  const here = location.pathname.split("/").pop() || "index.html";
  const onRecipePage = here.startsWith("recipe-");
  document.querySelectorAll(".nav__menu a").forEach((link) => {
    const href = link.getAttribute("href") || "";
    if (href.includes("#")) return;
    const hrefFile = href.split("/").pop();
    if (hrefFile !== here && !(onRecipePage && hrefFile === "recipes.html")) return;
    link.classList.add("is-active");
    link.setAttribute("aria-current", "page");
  });

  document.querySelectorAll("[data-year]").forEach((el) => {
    el.textContent = new Date().getFullYear();
  });
});
