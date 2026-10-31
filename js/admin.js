/* admin.js — small helpers for /admin. Every page works without it; the
   server checks everything again. */

// Accepted gates Invoiced, Paid, Dispatched and Completed. The first option
// is always the current status.
(() => {
  const box = document.querySelector("#accepted");
  const select = document.querySelector("#status");
  if (!box || !select) return;
  const sync = () => {
    for (const opt of select.options) {
      if (opt.index === 0) continue;
      opt.disabled = opt.dataset.needsAccepted === "true" && !box.checked;
    }
    if (select.selectedOptions[0] && select.selectedOptions[0].disabled) select.selectedIndex = 0;
  };
  box.addEventListener("change", sync);
  sync();
})();

// Copy buttons.
document.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-copy]");
  if (!btn) return;
  const text = btn.getAttribute("data-copy");
  const done = () => {
    const old = btn.textContent;
    btn.textContent = "Copied";
    setTimeout(() => (btn.textContent = old), 1200);
  };
  if (navigator.clipboard) navigator.clipboard.writeText(text).then(done, () => prompt("Copy this:", text));
  else prompt("Copy this:", text);
});

// After a comment is added, empty the box.
document.addEventListener("fx:swapped", () => {
  const ta = document.querySelector(".comment-form textarea");
  if (ta) ta.value = "";
});
