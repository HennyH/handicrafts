/* pay.js — the pay page: mounts Monokulo's checkout for the Monero payment,
   and reloads once the order is paid. The page is complete without it: the
   Monokulo checkout also opens through a plain link, and reloading shows the
   current state. */

(() => {
  const poll = document.querySelector("[data-poll]");
  const url = poll && poll.getAttribute("data-poll");
  async function check() {
    if (!url) return false;
    try {
      const res = await fetch(url, { cache: "no-store" });
      if (res.ok && (await res.json()).paid) {
        location.reload();
        return true;
      }
    } catch (e) {
      /* try again next time */
    }
    return false;
  }

  // The frame is cross-origin, so its content can't be measured. The shop asks
  // Monokulo which layout the store's default is and sizes the frame to it, the
  // same heights Monokulo's own library uses for each layout.
  const HEIGHTS = { tiny: "152px", compact: "380px", detailed: "880px" };

  const cfg = window.PAY_MONOKULO;
  const mount = document.querySelector("#mk-checkout");
  if (cfg && mount && window.Monokulo) {
    try {
      Monokulo.mount(mount, cfg.orderId, {
        monokulo: cfg.monokulo,
        publicKey: cfg.publicKey,
        theme: "light",
        width: "100%",
        height: HEIGHTS[cfg.layout],
        // A hint only: the order is marked paid by the signed webhook, so
        // ask the shop rather than trusting the browser.
        onPaid: () => setTimeout(check, 1500),
      });
    } catch (e) {
      /* keep the "open it on Monokulo" link that's already there */
    }
  }

  if (!poll) return;
  const started = Date.now();
  const timer = setInterval(async () => {
    if (document.hidden) return;
    if (Date.now() - started > 45 * 60 * 1000 || (await check())) clearInterval(timer);
  }, 10000);
})();
