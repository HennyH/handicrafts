/* pay.js — the pay page: follows Monokulo's checkout frame and reloads once the
   order is paid. The frame posts its status and height. Those only change what
   the customer sees: the order is marked paid by the signed webhook, so this
   asks the shop rather than trusting the browser. The page is complete without
   it: reloading shows the current state. */

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

  const frame = document.querySelector("#mk-checkout iframe");
  const seen = document.querySelector("#mk-seen");
  const expired = document.querySelector("#mk-expired");
  const refresh = document.querySelector("#mk-refresh");

  function follow(status) {
    lastHeard = Date.now();
    seen.hidden = !["unconfirmed", "confirming"].includes(status);
    if (status === "paid" || status === "overpaid") {
      check();
    } else if (status === "expired" || status === "cancelled") {
      // Take the frame away, so a code that has run out can't be scanned.
      frame.remove();
      document.querySelector(".embed-tag").remove();
      expired.hidden = false;
      refresh.focus();
    }
  }

  if (frame) {
    window.addEventListener("message", (event) => {
      const data = event.data;
      // Any page can post a message: only this frame's count.
      if (!data || !event.source || event.source !== frame.contentWindow) return;
      if (data.type === "monokulo:status") {
        follow(data.status);
      } else if (data.type === "monokulo:resize" && Number.isFinite(data.height) && data.height >= 1) {
        frame.style.height = Math.min(Math.ceil(data.height), 5000) + "px";
      }
    });
    refresh.addEventListener("click", () => {
      refresh.classList.add("is-loading");
      refresh.querySelector("span").textContent = "Getting a new code…";
    });
  }

  if (!poll) return;
  const timer = setInterval(async () => {
    if (document.hidden || Date.now() - lastHeard > POLL_FOR) return;
    if (await check()) clearInterval(timer);
  }, 3000);
})();
