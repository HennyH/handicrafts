/* products.js — loads data/products.json (the editable catalogue) */

async function loadProducts() {
  const res = await fetch("data/products.json", { cache: "no-store" });
  if (!res.ok) throw new Error("Could not load data/products.json");
  return res.json();
}

async function loadConfig() {
  const res = await fetch("data/config.json", { cache: "no-store" });
  if (!res.ok) throw new Error("Could not load data/config.json");
  return res.json();
}

function formatPrice(amount, currency = "AUD") {
  return new Intl.NumberFormat("en-AU", { style: "currency", currency }).format(amount);
}

/* ---------- webp images, with a jpg/png fallback ---------- */
/* Every raster photo has a pre-compressed .webp sibling sitting next to the
   original (generated with `magick <src> -resize '1600x1600>' -quality 82
   <src>.webp` — re-run that if you add new photos). Point <img> tags at the
   webp and remember the original in data-orig; the capture-phase listener
   below swaps back to it if the webp is ever missing, so a photo added since
   the last compression pass just quietly falls back instead of breaking. */
const RASTER_EXT = /\.(jpe?g|png)$/i;

function toWebp(src) {
  return RASTER_EXT.test(src) ? src.replace(RASTER_EXT, ".webp") : src;
}

/* Gallery photos ship at two sizes. The plain .webp is card-sized (600px) and
   is what gets preloaded, so a carousel step is instant; the _full.webp beside
   it is the original, fetched only when someone actually opens the lightbox.
   Falls back to the card image if a photo has no full-size sibling. */
function toFullWebp(src) {
  const webp = toWebp(src);
  return webp.endsWith(".webp") ? webp.replace(/\.webp$/, "_full.webp") : webp;
}

/* Returns the src/data-orig attributes to splice into an <img> tag. */
function webpAttrs(src) {
  const webp = toWebp(src);
  return webp === src ? `src="${escapeAttr(src)}"` : `src="${escapeAttr(webp)}" data-orig="${escapeAttr(src)}"`;
}

/* Sets (or clears) an already-in-the-DOM <img>'s src the same way, for the
   gallery/lightbox next-prev buttons that swap photos after the initial render. */
function setImgSrc(img, src) {
  const webp = toWebp(src);
  img.src = webp;
  if (webp === src) delete img.dataset.orig;
  else img.dataset.orig = src;
}

document.addEventListener(
  "error",
  (e) => {
    const img = e.target;
    if (img.tagName === "IMG" && img.dataset.orig) {
      img.src = img.dataset.orig;
      delete img.dataset.orig;
    }
  },
  true
);

/* A swatch is either a flat colour (`hex`) or a photo/pattern (`image`, a path
   like "assets/swatches/gingham.jpg"). If both are given the image is what you
   see and the hex sits behind it as the fallback while it loads. A swatch may
   also carry its own `materials` string, which replaces the product's default
   Materials line whenever that fabric is the one selected. */
function swatchFillStyle(swatch) {
  if (!swatch) return "";
  const base = swatch.hex ? `background-color:${swatch.hex};` : "";
  if (!swatch.image) return base;
  const sizing = "background-size:cover;background-position:center;";
  const webp = toWebp(swatch.image);
  if (webp === swatch.image) return `${base}background-image:url('${swatch.image}');${sizing}`;
  // Plain url() first, as the fallback for browsers without image-set()
  // support; the image-set() line after it wins wherever it's understood.
  return `${base}background-image:url('${swatch.image}');background-image:image-set(url('${webp}') type('image/webp'), url('${swatch.image}') type('image/jpeg'));${sizing}`;
}

/* Warm every photo a card can show, so stepping a carousel is instant rather
   than starting a download at the moment of the click — which on a phone is
   the difference between "next" and a second or two of nothing.

   Only photo [0] of each product is on the page initially; the rest, and the
   swatch images inside the collapsed picker, are never requested until
   something reveals them. This fetches them in the background once the page
   itself has finished loading, so it competes with nothing the visitor is
   waiting on. Requests go out through <img> rather than fetch() so they land
   in the normal image cache, which is what the gallery reads from later. */
const PRELOADED = [];

function preloadProductImages(products) {
  // Don't spend someone's data plan for them: Save-Data is an explicit ask,
  // and on 2g the preload would contend with the photos actually on screen.
  const net = navigator.connection;
  if (net && (net.saveData || /(^|-)2g$/.test(net.effectiveType || ""))) return;

  const urls = new Set();
  products.forEach((p) => {
    const photos = p.images || (p.image ? [p.image] : []);
    photos.slice(1).forEach((src) => urls.add(toWebp(src)));
    (p.swatches || []).forEach((s) => s.image && urls.add(toWebp(s.image)));
  });
  if (urls.size === 0) return;

  // Hold the references. Left to be garbage-collected, the bytes stay in the
  // HTTP cache but the decoded image doesn't, so the first real use spends a
  // revalidation round-trip (a 304) and a decode — small on wifi, exactly the
  // delay you feel on a phone. Keeping them alive makes that use a memory hit.
  const warm = () => urls.forEach((url) => {
    const img = new Image();
    img.decoding = "async";
    img.src = url;
    PRELOADED.push(img);
  });
  const whenIdle = () =>
    "requestIdleCallback" in window ? requestIdleCallback(warm, { timeout: 2000 }) : setTimeout(warm, 300);
  if (document.readyState === "complete") whenIdle();
  else window.addEventListener("load", whenIdle, { once: true });
}

function findSwatch(product, colorName) {
  if (!colorName || !product.swatches) return null;
  return product.swatches.find((s) => s.name === colorName) || null;
}

function swatchMaterials(product, swatch) {
  return (swatch && swatch.materials) || product.materials;
}

/* True when at least one swatch states its own composition — those products get
   a "varies by fabric" flag next to the Materials line so nobody assumes the
   first colour's composition applies to all of them. */
function hasMaterialVariants(product) {
  return Boolean(product.swatches && product.swatches.some((s) => s.materials));
}

function swatchPickerHTML(product) {
  if (!product.swatches || product.swatches.length === 0) return "";
  const groupName = `swatch-${product.id}`;
  const first = product.swatches[0];
  return `
  <details class="swatch-picker" data-swatch-picker>
    <summary class="swatch-picker__summary">
      <span class="swatch-dot" data-swatch-dot style="${swatchFillStyle(first)}"></span>
      Colour: <strong data-swatch-name>${first.name}</strong>
      <span class="swatch-picker__chevron" aria-hidden="true">▾</span>
    </summary>
    <div class="swatch-picker__grid">
      ${product.swatches
        .map(
          (s, i) => `
        <label class="swatch-option" title="${s.materials ? `${s.name} — ${s.materials}` : s.name}">
          <input type="radio" name="${groupName}" value="${s.name}" data-materials="${s.materials ? escapeAttr(s.materials) : ""}" ${
            i === 0 ? "checked" : ""
          }>
          <span class="swatch-option__color" style="${swatchFillStyle(s)}"></span>
          <span class="swatch-option__name">${s.name}</span>
        </label>`
        )
        .join("")}
    </div>
  </details>`;
}

function escapeAttr(str) {
  return String(str).replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/* A product can carry either a single "image" or an "images" array. Everything
   downstream (cart rows, order PDF) wants one representative photo, so the
   first entry doubles as the primary. */
function productImages(product) {
  if (Array.isArray(product.images) && product.images.length > 0) return product.images;
  return product.image ? [product.image] : [];
}

/* The card's photo. With more than one photo it becomes a little gallery:
   prev/next step through the photos in place, expand opens the lightbox. */
function productGalleryHTML(product, eager) {
  const images = productImages(product);
  const multiple = images.length > 1;
  const alt = escapeAttr(product.name);
  // First 3 cards land above the fold on both the featured row and the shop
  // grid (masonry keeps source order across columns), so they skip
  // loading="lazy" and get bumped to the front of the request queue.
  const loadingAttrs = eager ? `loading="eager" fetchpriority="high"` : `loading="lazy"`;
  return `
    <figure class="product-gallery${multiple ? " product-gallery--multi" : ""}" data-gallery data-index="0">
      <img class="product-card__img" data-gallery-img ${webpAttrs(images[0])} alt="${alt}" ${loadingAttrs}
           data-images="${escapeAttr(JSON.stringify(images))}">
      ${
        multiple
          ? `
      <button type="button" class="gallery-btn gallery-btn--prev" data-gallery-prev aria-label="Previous photo of ${alt}">‹</button>
      <button type="button" class="gallery-btn gallery-btn--next" data-gallery-next aria-label="Next photo of ${alt}">›</button>
      <button type="button" class="gallery-btn gallery-btn--expand" data-gallery-expand aria-label="View ${alt} larger">⤢</button>
      <figcaption class="gallery-count"><span data-gallery-count>1</span> / ${images.length}</figcaption>`
          : ""
      }
    </figure>`;
}

function productCardHTML(product, index) {
  // Cycle through 5 slightly irregular tilt angles (see css/style.css) instead
  // of a strict left/right alternation, so a grid of cards feels hand-placed.
  const tilt = `tilt-${(index % 5) + 1}`;
  const eager = index < 3;
  const isCustom = product.price === null || product.price === undefined;
  // "comingSoon": true in products.json parks a product on the shop — it still
  // shows with its price and swatches, but can't be added to the cart yet.
  const footerRight = product.comingSoon
    ? `<button class="btn btn--accent" disabled aria-disabled="true">Coming soon!</button>`
    : isCustom
      ? `<a class="btn btn--accent btn--with-icon" href="mailto:hello@rachelshandicrafts.com?subject=${encodeURIComponent(
          "Custom Tailoring enquiry — " + product.name
        )}"><img class="btn__icon" src="assets/doodles/envelope-seal.svg" alt="" aria-hidden="true"> Enquire</a>`
      : `<button class="btn btn--accent" data-add-to-cart="${product.id}">Add to cart</button>`;

  return `
  <article class="card product-card ${tilt}" data-id="${product.id}">
    ${productGalleryHTML(product, eager)}
    <h3 class="product-card__name">${product.name}</h3>
    <p class="product-card__desc">${product.description}</p>
    <p class="product-card__materials">
      <strong>Materials:</strong> <span data-materials-text data-default-materials="${escapeAttr(product.materials)}">${swatchMaterials(
        product,
        product.swatches && product.swatches[0]
      )}</span>
      ${
        hasMaterialVariants(product)
          ? `<span class="materials-flag" title="The composition of this piece changes with the fabric you pick — the line above updates as you choose.">✎ varies by fabric</span>`
          : ""
      }
    </p>
    ${swatchPickerHTML(product)}
    <div class="product-card__footer">
      <span class="product-card__price">${isCustom ? "Custom quote" : formatPrice(product.price, product.currency)}</span>
      ${footerRight}
    </div>
  </article>`;
}

/* ---------- gallery + lightbox ---------- */

function galleryImages(gallery) {
  try {
    return JSON.parse(gallery.querySelector("[data-gallery-img]").dataset.images);
  } catch (e) {
    return [];
  }
}

/* Step a card's gallery by `step` photos, wrapping around at either end.
   Photos aren't all the same shape, so swapping one changes the card's height.
   Masonry positions cards absolutely from measured heights, so it has to be
   told to re-measure or the taller card runs straight over its neighbour.
   `onResize` fires once the new photo has decoded and the card has its final
   height — waiting for `load` matters, because measuring an image that hasn't
   arrived yet just re-measures the old height. */
function showGalleryImage(gallery, step, onResize) {
  const images = galleryImages(gallery);
  if (images.length === 0) return;
  const next = (Number(gallery.dataset.index || 0) + step + images.length) % images.length;
  gallery.dataset.index = String(next);
  const img = gallery.querySelector("[data-gallery-img]");
  setImgSrc(img, images[next]);
  const counter = gallery.querySelector("[data-gallery-count]");
  if (counter) counter.textContent = String(next + 1);
  if (!onResize) return;
  if (img.complete && img.naturalWidth) onResize();
  // "error" too: the webp→jpg fallback swaps src again, and either way the
  // card has settled on a height that Masonry needs to know about.
  else ["load", "error"].forEach((evt) => img.addEventListener(evt, onResize, { once: true }));
}

/* Show a photo in the lightbox at the card-sized image first — it's already in
   memory from the preload, so something appears immediately rather than after
   a download — then fetch the full-resolution copy and fade it in on top.
   The two <img>s are stacked so the swap is a cross-fade rather than a blink
   through empty space, and the low-res one stays underneath as the backdrop. */
function showLightboxPhoto(dialog, src) {
  const small = dialog.querySelector("[data-lightbox-img]");
  const full = dialog.querySelector("[data-lightbox-full]");
  setImgSrc(small, src);
  // Reset to hidden *without* animating: letting the class removal transition
  // out means a cached full-res can arrive mid-fade and snap in at whatever
  // opacity it finds, so the fade plays only by luck. Killing the transition
  // for one frame makes the hide instant and every fade-in start from 0.
  full.style.transition = "none";
  full.classList.remove("is-loaded");
  full.removeAttribute("src");
  void full.offsetWidth; // flush the change before re-enabling the transition
  full.style.transition = "";
  const hi = toFullWebp(src);
  const loader = new Image();
  loader.onload = () => {
    // Ignore a load that finished after the viewer already stepped onward.
    if (small.getAttribute("src") !== toWebp(src)) return;
    full.src = hi;
    full.classList.add("is-loaded");
  };
  // No _full sibling (or it 404s): the card-sized image simply stays.
  loader.src = hi;
}

/* One <dialog> shared by every card — built on first use, then reused. Native
   showModal() gives us the backdrop, Esc-to-close and focus trapping for free. */
function getLightbox() {
  let dialog = document.querySelector("#image-lightbox");
  if (dialog) return dialog;

  dialog = document.createElement("dialog");
  dialog.id = "image-lightbox";
  dialog.className = "lightbox";
  dialog.innerHTML = `
    <div class="lightbox__inner" data-lightbox-inner data-index="0">
      <div class="lightbox__frame">
        <img class="lightbox__img" data-lightbox-img src="" alt="">
        <img class="lightbox__img lightbox__img--full" data-lightbox-full src="" alt="" aria-hidden="true">
      </div>
      <button type="button" class="gallery-btn gallery-btn--prev" data-lightbox-prev aria-label="Previous photo">‹</button>
      <button type="button" class="gallery-btn gallery-btn--next" data-lightbox-next aria-label="Next photo">›</button>
      <button type="button" class="lightbox__close" data-lightbox-close aria-label="Close">✕</button>
      <p class="lightbox__caption"><span data-lightbox-caption></span> <span class="lightbox__count" data-lightbox-count></span></p>
    </div>`;
  document.body.appendChild(dialog);

  const inner = dialog.querySelector("[data-lightbox-inner]");
  const step = (by) => {
    const images = JSON.parse(inner.dataset.images || "[]");
    if (images.length === 0) return;
    const next = (Number(inner.dataset.index || 0) + by + images.length) % images.length;
    inner.dataset.index = String(next);
    showLightboxPhoto(dialog, images[next]);
    dialog.querySelector("[data-lightbox-count]").textContent = images.length > 1 ? `${next + 1} / ${images.length}` : "";
  };

  dialog.querySelector("[data-lightbox-prev]").addEventListener("click", () => step(-1));
  dialog.querySelector("[data-lightbox-next]").addEventListener("click", () => step(1));
  dialog.querySelector("[data-lightbox-close]").addEventListener("click", () => dialog.close());
  // Clicking the backdrop (i.e. the dialog itself, outside .lightbox__inner) closes.
  dialog.addEventListener("click", (e) => {
    if (e.target === dialog) dialog.close();
  });
  dialog.addEventListener("keydown", (e) => {
    if (e.key === "ArrowLeft") step(-1);
    if (e.key === "ArrowRight") step(1);
  });
  dialog._step = step;
  return dialog;
}

function openLightbox(images, startIndex, caption) {
  const dialog = getLightbox();
  const inner = dialog.querySelector("[data-lightbox-inner]");
  inner.dataset.images = JSON.stringify(images);
  inner.dataset.index = String(startIndex);
  showLightboxPhoto(dialog, images[startIndex]);
  dialog.querySelector("[data-lightbox-img]").alt = caption;
  dialog.querySelector("[data-lightbox-caption]").textContent = caption;
  dialog.querySelector("[data-lightbox-count]").textContent = images.length > 1 ? `${startIndex + 1} / ${images.length}` : "";
  dialog.classList.toggle("lightbox--single", images.length < 2);
  dialog.showModal();
}

/* ---------- masonry ---------- */
/* Layout is handed to Masonry (masonry.pkgd, loaded from cdnjs alongside
   imagesLoaded — same arrangement the checkout page uses for jsPDF). Column
   widths live in CSS on .grid-sizer; Masonry just reads that element's width
   and positions the cards against it.

   transitionDuration is 0 deliberately: with animation on, Masonry positions
   items with `transform: translate()`, which would clobber each card's own
   rotate() tilt. At 0 it uses plain left/top and the tilt survives. */
const MASONRY_GUTTER = 28;

function initMasonry(grid) {
  if (!grid.classList.contains("grid--masonry") || typeof Masonry === "undefined") return null;

  const layout = new Masonry(grid, {
    itemSelector: ".product-card",
    columnWidth: ".grid-sizer",
    percentPosition: true,
    gutter: MASONRY_GUTTER,
    transitionDuration: 0,
    // Without this, every re-layout re-runs "put the next card in whichever
    // column is currently shortest". Changing one card's height changes which
    // column that is, so unrelated cards jump between columns and the grid
    // appears to shuffle. horizontalOrder keeps cards in their left-to-right
    // reading order, so a resize pushes neighbours instead of reordering them.
    horizontalOrder: true,
  });

  // Cards are mostly image, so their height isn't known until the photos land.
  if (typeof imagesLoaded === "function") {
    imagesLoaded(grid).on("progress", () => layout.layout());
  }
  return layout;
}

async function initProductGrid(targetSelector, opts = {}) {
  const target = document.querySelector(targetSelector);
  if (!target) return;
  try {
    const products = await loadProducts();
    const list = opts.limit ? products.slice(0, opts.limit) : products;
    // .grid-sizer is a zero-height element that carries the column width for
    // Masonry to measure; it has to live inside the grid, so it goes in with
    // the cards rather than in the page markup.
    target.innerHTML = '<div class="grid-sizer"></div>' + list.map((p, i) => productCardHTML(p, i)).join("");
    const masonry = initMasonry(target);
    // `products`, not `list`: the home page renders only the first three, but
    // its visitor is one click from the shop where the rest are waiting.
    preloadProductImages(products);

    // Keep each swatch picker's compact summary (dot + name) in sync with
    // whichever colour option is selected, and tidy the picker back up once
    // a choice has been made so the grid stays compact.
    target.querySelectorAll("[data-swatch-picker]").forEach((picker) => {
      picker.addEventListener("change", (e) => {
        const input = e.target.closest('input[type="radio"]');
        if (!input) return;
        picker.querySelector("[data-swatch-dot]").style.cssText = input.nextElementSibling.style.cssText;
        picker.querySelector("[data-swatch-name]").textContent = input.value;
        // Swap in this fabric's own composition when it states one; fall back to
        // the product's default line (rendered as data-default-materials) when
        // it doesn't.
        const card = picker.closest(".product-card");
        const materialsEl = card.querySelector("[data-materials-text]");
        if (materialsEl) {
          materialsEl.textContent = input.dataset.materials || materialsEl.dataset.defaultMaterials;
        }
        picker.removeAttribute("open");
      });
      // Every card has its own `transform` (for the hand-placed tilt), which
      // makes each card its own stacking context — so a plain z-index on the
      // dropdown can't rise above a sibling card next to/below it. Bump the
      // whole card above its siblings only while the picker is open.
      picker.addEventListener("toggle", () => {
        const card = picker.closest(".product-card");
        card.classList.toggle("swatch-open", picker.open);
        if (masonry) masonry.layout();
        if (picker.open) {
          // Wait a frame so the expanded grid has actually laid out before
          // measuring where it needs to scroll to.
          requestAnimationFrame(() => {
            picker.querySelector(".swatch-picker__grid").scrollIntoView({ behavior: "smooth", block: "nearest" });
          });
        }
      });
    });

    // Clicking anywhere outside an open picker closes it (each picker's own
    // "toggle" listener above then clears its card's z-index bump).
    if (!document.body.dataset.swatchOutsideBound) {
      document.body.dataset.swatchOutsideBound = "true";
      document.addEventListener("click", (e) => {
        document.querySelectorAll("[data-swatch-picker][open]").forEach((openPicker) => {
          if (!openPicker.contains(e.target)) openPicker.removeAttribute("open");
        });
      });
    }

    target.addEventListener("click", (e) => {
      const gallery = e.target.closest("[data-gallery]");
      if (gallery) {
        const relayout = () => masonry && masonry.layout();
        if (e.target.closest("[data-gallery-prev]")) return showGalleryImage(gallery, -1, relayout);
        if (e.target.closest("[data-gallery-next]")) return showGalleryImage(gallery, 1, relayout);
        if (e.target.closest("[data-gallery-expand]")) {
          const card = gallery.closest(".product-card");
          const name = card.querySelector(".product-card__name").textContent;
          return openLightbox(galleryImages(gallery), Number(gallery.dataset.index || 0), name);
        }
      }

      const btn = e.target.closest("[data-add-to-cart]");
      if (!btn) return;
      const productId = btn.getAttribute("data-add-to-cart");
      const card = btn.closest(".product-card");
      // Scope the lookup to this exact product's radio group (not just "any
      // checked radio in the card") so there's no chance of picking up a
      // colour selection belonging to a different product.
      const selectedSwatch = card.querySelector(`input[name="swatch-${productId}"]:checked`);
      const cartKey = selectedSwatch ? `${productId}::${selectedSwatch.value}` : productId;
      addToCart(cartKey, 1);
      const original = btn.textContent;
      btn.textContent = "Added ✓";
      btn.disabled = true;
      setTimeout(() => {
        btn.textContent = original;
        btn.disabled = false;
      }, 900);
    });
  } catch (err) {
    target.innerHTML = `<p class="error-note">Couldn't load the product list (${err.message}). If you're opening this file directly from disk, run a tiny local server first — see the README.</p>`;
  }
}
