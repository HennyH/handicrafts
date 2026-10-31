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
