/* pay.js — the pay page: mounts Monokulo's checkout for the Monero payment,
   and reloads once the order is paid. The page is complete without it: the
   Monokulo checkout also opens through a plain link, and reloading shows the
   current state. */

(() => {
  const cfg = window.PAY_MONOKULO;
  const mount = document.querySelector("#mk-checkout");
  if (cfg && mount && window.Monokulo) {
    try {
      Monokulo.mount(mount, cfg.orderId, {
        monokulo: cfg.monokulo,
        publicKey: cfg.publicKey,
        refund: false, // asked for on this page instead; Monokulo's API doesn't return it
        theme: "light",
        width: "100%",
        // A hint only: the order is marked paid by the signed webhook, so
        // ask the shop rather than trusting the browser.
        onPaid: () => setTimeout(check, 1500),
      });
    } catch (e) {
      /* keep the "open it on Monokulo" link that's already there */
    }
  }

  const poll = document.querySelector("[data-poll]");
  if (!poll) return;
  const url = poll.getAttribute("data-poll");
  const started = Date.now();
  async function check() {
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
  const timer = setInterval(async () => {
    if (document.hidden) return;
    if (Date.now() - started > 45 * 60 * 1000 || (await check())) clearInterval(timer);
  }, 10000);
})();
