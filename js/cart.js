/* cart.js — tiny localStorage cart, shared by every page */
const CART_KEY = "rh_cart_v1";

function getCart() {
  try {
    return JSON.parse(localStorage.getItem(CART_KEY)) || {};
  } catch (e) {
    return {};
  }
}

function saveCart(cart) {
  localStorage.setItem(CART_KEY, JSON.stringify(cart));
  updateCartBadge();
}

function addToCart(productId, qty = 1) {
  const cart = getCart();
  cart[productId] = (cart[productId] || 0) + qty;
  saveCart(cart);
}

function setQty(productId, qty) {
  const cart = getCart();
  if (qty <= 0) {
    delete cart[productId];
  } else {
    cart[productId] = qty;
  }
  saveCart(cart);
}

function removeFromCart(productId) {
  const cart = getCart();
  delete cart[productId];
  saveCart(cart);
}

function clearCart() {
  localStorage.removeItem(CART_KEY);
  updateCartBadge();
}

function getCartCount() {
  const cart = getCart();
  return Object.values(cart).reduce((sum, qty) => sum + qty, 0);
}

/* Combine the raw cart (key -> qty) with product data, dropping stale ids
   (e.g. a product that has been removed from products.json since it was added).
   Cart keys are either a plain product id, or "id::Colour Name" for products
   with a fabric swatch chosen — split that back apart here. */
function getCartDetails(products) {
  const cart = getCart();
  const details = [];
  for (const [key, qty] of Object.entries(cart)) {
    const [id, color] = key.split("::");
    const product = products.find((p) => p.id === id);
    // Skip products that have since been removed from the catalogue, and any
    // marked "comingSoon" — those can't be bought yet, so they must not reach
    // the order even if they were added before the flag went on.
    if (!product || product.comingSoon) continue;
    details.push({ key, product, color: color || null, qty, lineTotal: +(product.price * qty).toFixed(2) });
  }
  return details;
}

function cartSubtotal(details) {
  return +details.reduce((sum, d) => sum + d.lineTotal, 0).toFixed(2);
}

/* Drop cart keys that getCartDetails would refuse to show — a delisted product,
   or one that's since been flagged "comingSoon". Without this the header badge
   (which counts the raw cart) could out-count the rows on the cart page. */
function pruneCart(products) {
  const cart = getCart();
  const live = new Set(getCartDetails(products).map((d) => d.key));
  const stale = Object.keys(cart).filter((key) => !live.has(key));
  if (stale.length === 0) return;
  stale.forEach((key) => delete cart[key]);
  saveCart(cart);
}

function updateCartBadge() {
  const count = getCartCount();
  document.querySelectorAll("[data-cart-count]").forEach((el) => {
    el.textContent = count;
    el.style.display = count > 0 ? "inline-flex" : "none";
  });
}

document.addEventListener("DOMContentLoaded", updateCartBadge);
