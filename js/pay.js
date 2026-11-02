/* pay.js — the pay page: reloads once the order is paid. The Monero card is
   a self-refreshing iframe and works without this; reloading the page shows
   the current state. */

/* Clicking the Monero card's QR code opens a big copy of it. The card is a
   same-origin iframe that reloads itself, so listen again after each load.
   Without this script the QR is a plain monero: link that opens a wallet. */
(() => {
  const frame = document.querySelector("iframe.mk-frame");
  if (!frame || !window.HTMLDialogElement) return;
  const dlg = document.createElement("dialog");
  dlg.className = "lightbox lightbox--qr";
  dlg.innerHTML =
    '<div class="lightbox__inner">' +
    '<div class="lightbox__qr" data-qr></div>' +
    '<button type="button" class="lightbox__close" aria-label="Close">✕</button>' +
    '<p class="lightbox__caption">Scan with your Monero wallet</p></div>';
  dlg.querySelector(".lightbox__close").addEventListener("click", () => dlg.close());
  dlg.addEventListener("click", (e) => {
    if (e.target === dlg) dlg.close();
  });
  document.body.appendChild(dlg);
  frame.addEventListener("load", () => {
    let doc;
    try {
      doc = frame.contentDocument;
    } catch (e) {
      return;
    }
    if (!doc) return;
    doc.addEventListener("click", (e) => {
      const link = e.target.closest && e.target.closest("a.mkc__qr");
      const svg = link && link.querySelector("svg");
      if (!svg) return;
      e.preventDefault();
      dlg.querySelector("[data-qr]").replaceChildren(svg.cloneNode(true));
      if (!dlg.open) dlg.showModal();
    });
  });
})();

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
