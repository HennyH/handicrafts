/* pay.js — the pay page: follows Monokulo's checkout frame and reloads once the
   order is paid. The frame posts its status and height. Those only change what
   the customer sees: the order is marked paid by the signed webhook, so this
   asks the shop rather than trusting the browser. Once the frame says its code
   is over, the shop decides what to show, the same way it does for a page load,
   and this swaps that in. The page is complete without it: reloading shows the
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

  // Polling stops this long after the last sign of life from the checkout.
  const POLL_FOR = 45 * 60 * 1000;
  let lastHeard = Date.now();

  // A panel still to fetch, kept if the shop couldn't be reached.
  let wanted = null;
  let swapping = false;
  async function swapPanel() {
    if (swapping || !wanted) return;
    swapping = true;
    try {
      const res = await fetch(wanted, { cache: "no-store" });
      if (!res.ok) {
        location.reload();
        return;
      }
      const html = await res.text();
      wanted = null;
      document.querySelector("#mk-panel").outerHTML = html;
      const refresh = document.querySelector("#mk-refresh");
      if (refresh) refresh.focus();
    } catch (e) {
      /* the frame says what happened meanwhile: try again next time */
    }
    swapping = false;
  }

  function follow(status) {
    lastHeard = Date.now();
    const seen = document.querySelector("#mk-seen");
    if (seen) seen.hidden = !["unconfirmed", "confirming"].includes(status);
    if (status === "paid" || status === "overpaid") {
      check();
    } else if (status === "expired" || status === "cancelled") {
      wanted = document.querySelector("#mk-checkout").getAttribute("data-panel");
      swapPanel();
    }
  }

  // Swapped panels bring new frames, so look for the current one each time.
  window.addEventListener("message", (event) => {
    const frame = document.querySelector("#mk-checkout iframe");
    const data = event.data;
    // Any page can post a message: only this frame's count.
    if (!frame || !data || !event.source || event.source !== frame.contentWindow) return;
    if (data.type === "monokulo:status") {
      follow(data.status);
    } else if (data.type === "monokulo:resize" && Number.isFinite(data.height) && data.height >= 1) {
      frame.style.height = Math.min(Math.ceil(data.height), 5000) + "px";
    }
  });

  document.addEventListener("submit", (event) => {
    const label = event.target.querySelector("#mk-refresh span");
    if (label) label.textContent = "Getting a new code…";
  });

  if (!poll) return;
  const timer = setInterval(async () => {
    if (document.hidden || Date.now() - lastHeard > POLL_FOR) return;
    swapPanel();
    if (await check()) clearInterval(timer);
  }, 3000);
})();
