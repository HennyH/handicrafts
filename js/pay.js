/* pay.js — the pay page: reloads once the order is paid. Monokulo's checkout
   is a plain iframe in the page. The order is marked paid by the signed
   webhook, so this asks the shop rather than trusting the browser. The page
   is complete without it: reloading shows the current state. */

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

  if (!poll) return;
  const started = Date.now();
  const timer = setInterval(async () => {
    if (document.hidden) return;
    if (Date.now() - started > 45 * 60 * 1000 || (await check())) clearInterval(timer);
  }, 3000);
})();
