/* checkout.js — the checkout page: shows the cart, sends it with the order.
   The form posts to /orders; fixi (js/fixi.js) swaps in the server's answer
   (the "order received" page, or the form again with an error) without a
   page load. The server prices everything itself: the cart only says which
   products, which fabric and how many. */

let CHECKOUT_PRODUCTS = null;

/* The cart as the server wants it: [{id, color, qty}]. Cart keys are a plain
   product id, or "id::Colour Name" when a fabric was picked. */
function cartForServer() {
  return Object.entries(getCart()).map(([key, qty]) => {
    const [id, color] = key.split("::");
    return { id, color: color || null, qty };
  });
}

async function initCheckout() {
  // Placed: the server answered with the "order received" page.
  if (document.querySelector("[data-order-placed]")) {
    clearCart();
    return;
  }
  const form = document.querySelector("#checkout-form");
  const summaryEl = document.querySelector("#checkout-summary");
  const emptyState = document.querySelector("#checkout-empty");
  if (!form || !summaryEl) return;

  try {
    CHECKOUT_PRODUCTS = CHECKOUT_PRODUCTS || (await loadProducts());
  } catch (err) {
    summaryEl.innerHTML = `<p class="error-note">Couldn't load the product list (${err.message}). Please reload the page.</p>`;
    return;
  }
  pruneCart(CHECKOUT_PRODUCTS);
  const details = getCartDetails(CHECKOUT_PRODUCTS);
  if (details.length === 0) {
    form.style.display = "none";
    summaryEl.innerHTML = "";
    emptyState.style.display = "block";
    return;
  }
  summaryEl.innerHTML = `
    <ul class="checkout-items">
      ${details
        .map(
          ({ product, color, qty, lineTotal }) =>
            `<li><span>${qty} × ${product.name}${color ? ` — ${color}` : ""}</span><span>${formatPrice(lineTotal, product.currency)}</span></li>`
        )
        .join("")}
    </ul>
    <div class="summary-line summary-line--total"><span>Total</span><span>${formatPrice(cartSubtotal(details), "AUD")}</span></div>
  `;
  form.querySelector("#cart-json").value = JSON.stringify(cartForServer());
  // Refresh just before sending, in case the cart changed in another tab.
  form.addEventListener("submit", () => {
    form.querySelector("#cart-json").value = JSON.stringify(cartForServer());
  });
}

document.addEventListener("fx:config", (e) => {
  const btn = e.target.querySelector && e.target.querySelector("button[type=submit]");
  if (btn) {
    btn.setAttribute("aria-busy", "true");
    btn.disabled = true;
    btn.dataset.label = btn.textContent;
    btn.textContent = "Placing your order…";
  }
});

document.addEventListener("fx:error", (e) => {
  const btn = e.target.querySelector && e.target.querySelector("button[type=submit]");
  if (btn) {
    btn.removeAttribute("aria-busy");
    btn.disabled = false;
    btn.textContent = btn.dataset.label || "Place order";
  }
  const panel = document.querySelector(".checkout-panel");
  if (panel && !panel.querySelector(".form-error")) {
    const p = document.createElement("p");
    p.className = "form-error";
    p.setAttribute("role", "alert");
    p.textContent = "Couldn't reach the shop. Check your connection and press Place order again.";
    panel.insertBefore(p, panel.querySelector("form"));
  }
});

// After a swap the page is new: clear the cart if the order went through,
// or set the returned form up again.
document.addEventListener("fx:swapped", () => {
  window.scrollTo({ top: 0, behavior: "smooth" });
  initCheckout();
});

document.addEventListener("DOMContentLoaded", initCheckout);
