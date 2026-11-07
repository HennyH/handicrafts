/* head.js — runs in <head> before first paint: marks JS as available, and
   applies the full-res paper texture once it's known to be cached. A file,
   not an inline script, so a page's Content-Security-Policy can be
   script-src 'self'. */
document.documentElement.classList.add("js");
try {
  if (localStorage.getItem("rh_paper_hires")) {
    document.documentElement.style.setProperty("--paper-bg", 'url("/assets/art/paper_bg_100.webp")');
  }
} catch (e) {}
