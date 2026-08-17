/* cart-page.js — renders cart.html */
async function initCartPage() {
  const wrap = document.querySelector("#cart-contents");
  const summary = document.querySelector("#cart-summary");
  const emptyState = document.querySelector("#cart-empty");
  const checkoutBtn = document.querySelector("#go-to-checkout");
  if (!wrap) return;

  let products;
  try {
    products = await loadProducts();
  } catch (err) {
    wrap.innerHTML = `<p class="error-note">Couldn't load the product list (${err.message}). If you're opening this file directly from disk, run a tiny local server first — see the README.</p>`;
    return;
  }

  pruneCart(products);

  function render() {
    const details = getCartDetails(products);

    if (details.length === 0) {
      wrap.innerHTML = "";
      summary.innerHTML = "";
      emptyState.style.display = "block";
      checkoutBtn.style.display = "none";
      return;
    }

    emptyState.style.display = "none";
    checkoutBtn.style.display = "block";

    wrap.innerHTML = details
      .map(({ key, product, color, qty, lineTotal }) => {
        const swatch = findSwatch(product, color);
        const colorBit = color
          ? ` · ${swatch ? `<span class="cart-row__swatch-dot" style="${swatchFillStyle(swatch)}"></span>` : ""}Colour: ${color}`
          : "";
        // Only worth repeating the composition here when this fabric overrides
        // the product default — otherwise it's the same line on every row.
        const materialsBit = swatch && swatch.materials ? `<span class="cart-row__materials">${swatch.materials}</span>` : "";
        return `
      <div class="cart-row" data-id="${key}">
        <img class="cart-row__img" ${webpAttrs(productImages(product)[0])} alt="${product.name}">
        <div class="cart-row__info">
          <h3>${product.name}</h3>
          <p class="cart-row__price">${formatPrice(product.price, product.currency)} each${colorBit}</p>
          ${materialsBit}
        </div>
        <div class="cart-row__qty">
          <button class="btn btn--tiny" data-qty-down aria-label="Decrease quantity">–</button>
          <input type="number" min="1" value="${qty}" data-qty-input inputmode="numeric">
          <button class="btn btn--tiny" data-qty-up aria-label="Increase quantity">+</button>
        </div>
        <div class="cart-row__total">${formatPrice(lineTotal, product.currency)}</div>
        <button class="cart-row__remove" data-remove aria-label="Remove ${product.name}">✕</button>
      </div>`;
      })
      .join("");

    const subtotal = cartSubtotal(details);
    summary.innerHTML = `
      <div class="summary-line"><span>Subtotal</span><span>${formatPrice(subtotal, "AUD")}</span></div>
      <div class="summary-line summary-line--muted"><span>Delivery / pickup</span><span>arranged after order</span></div>
      <div class="summary-line summary-line--total"><span>Total</span><span>${formatPrice(subtotal, "AUD")}</span></div>
    `;
  }

  wrap.addEventListener("click", (e) => {
    const row = e.target.closest(".cart-row");
    if (!row) return;
    const key = row.getAttribute("data-id");
    const input = row.querySelector("[data-qty-input]");

    if (e.target.closest("[data-remove]")) {
      removeFromCart(key);
      render();
    } else if (e.target.closest("[data-qty-up]")) {
      setQty(key, +input.value + 1);
      render();
    } else if (e.target.closest("[data-qty-down]")) {
      setQty(key, +input.value - 1);
      render();
    }
  });

  wrap.addEventListener("change", (e) => {
    const input = e.target.closest("[data-qty-input]");
    if (!input) return;
    const row = input.closest(".cart-row");
    const key = row.getAttribute("data-id");
    const next = Math.max(1, Math.floor(+input.value) || 1);
    setQty(key, next);
    render();
  });

  render();
}

document.addEventListener("DOMContentLoaded", initCartPage);
