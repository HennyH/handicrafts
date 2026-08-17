/* main.js — bits shared by every page: mobile nav + footer year */
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
  const here = location.pathname.split("/").pop() || "index.html";
  document.querySelectorAll(".nav__menu a").forEach((link) => {
    const href = link.getAttribute("href") || "";
    if (href.includes("#")) return;
    if (href.split("/").pop() !== here) return;
    link.classList.add("is-active");
    link.setAttribute("aria-current", "page");
  });

  document.querySelectorAll("[data-year]").forEach((el) => {
    el.textContent = new Date().getFullYear();
  });
});
