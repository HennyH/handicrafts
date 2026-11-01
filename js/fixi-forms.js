/* fixi-forms.js — loaded right after fixi.js.

   fixi sends a form's fields as multipart/form-data (a FormData body), but
   the shop's forms are read as application/x-www-form-urlencoded, the same
   as a form posted without JavaScript. Send them that way.

   fixi also wraps every swap in a view transition. None of these swaps need
   animating, and two overlapping ones (changing the refund address twice
   quickly) make the browser throw "Transition was skipped", so turn them off. */
window.fixiCfg = Object.assign({ transition: false }, window.fixiCfg);

document.addEventListener("fx:config", (e) => {
  const cfg = e.detail.cfg;
  if (cfg.body instanceof FormData) cfg.body = new URLSearchParams(cfg.body);
});

/* One press per submit. Once a form is sent, its submit buttons go into a
   loading state (disabled, spinning) so a second click, or Enter pressed
   twice, can't send it again: that would place an order twice, or email
   Rachel twice.

   - A form posted the normal way: disable the buttons just after the browser
     has taken the form's values (disabling at once would drop the pressed
     button's name and value), and refuse a second submit meanwhile. If the
     page is restored from the back/forward cache, put the buttons back.
   - A form fixi sends: disable them for the request, and put them back when
     it ends (if the form is still on the page). */
(() => {
  const buttons = (form) => form.querySelectorAll("button[type=submit], input[type=submit]");
  const busy = (form, on) => {
    form.toggleAttribute("data-busy", on);
    for (const b of buttons(form)) {
      b.disabled = on;
      b.toggleAttribute("aria-busy", on);
      b.classList.toggle("is-loading", on);
    }
  };

  document.addEventListener("submit", (e) => {
    const form = e.target;
    if (!(form instanceof HTMLFormElement)) return;
    if (form.hasAttribute("data-busy")) return e.preventDefault();
    if (e.defaultPrevented) return; // fixi's: handled by fx:config below
    form.setAttribute("data-busy", ""); // at once, so a second submit in the same moment is refused
    setTimeout(() => busy(form, true), 0);
  });

  window.addEventListener("pageshow", (e) => {
    if (e.persisted) document.querySelectorAll("form[data-busy]").forEach((f) => busy(f, false));
  });

  document.addEventListener("fx:config", (e) => {
    const form = e.target.closest && e.target.closest("form");
    if (form && !e.detail.cfg.drop) busy(form, true);
  });
  document.addEventListener("fx:finally", (e) => {
    const form = e.target.closest && e.target.closest("form");
    if (form) setTimeout(() => busy(form, false), 0);
  });
})();
