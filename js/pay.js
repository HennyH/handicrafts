/* pay.js — the pay page: reloads once the order is paid. The Monero card is
   a self-refreshing iframe and works without this; reloading the page shows
   the current state. */

(() => {
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
