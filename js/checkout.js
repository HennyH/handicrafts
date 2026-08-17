/* checkout.js — order summary, PDF generation (jsPDF) and the "email it" handoff.
   This site is 100% static (built for GitHub Pages), so there is no server that can
   send emails on its own. Instead we:
     1. Generate a proper Order PDF in the browser and download it.
     2. Open the customer's email client (mailto:) pre-addressed to the business,
        with the order details already in the body, ready for the PDF to be attached.
   See README.md for how to point this at a real email address, and for the optional
   server-side add-on if you later want fully automatic emailing. */

let CONFIG = null;
let PRODUCTS = null;

function genOrderNumber(prefix) {
  const now = new Date();
  const stamp = now.toISOString().replace(/[-:T]/g, "").slice(0, 12); // YYYYMMDDHHmm
  const rand = Math.floor(Math.random() * 90 + 10); // 2-digit
  return `${prefix}-${stamp}-${rand}`;
}

function buildOrder(details, customer) {
  const subtotal = cartSubtotal(details);
  return {
    orderNumber: genOrderNumber(CONFIG.orderNumberPrefix || "RH"),
    date: new Date(),
    customer,
    items: details.map(({ product, color, qty, lineTotal }) => ({
      name: color ? `${product.name} — ${color}` : product.name,
      // Composition can differ per fabric, so record the one that was actually
      // picked — that's what Rachel needs to cut from.
      materials: swatchMaterials(product, findSwatch(product, color)),
      sku: product.sku,
      qty,
      price: product.price,
      lineTotal,
    })),
    subtotal,
    total: subtotal,
    currency: "AUD",
  };
}

function generateOrderPdf(order) {
  const { jsPDF } = window.jspdf;
  const doc = new jsPDF({ unit: "pt", format: "a4" });
  const pageWidth = doc.internal.pageSize.getWidth();
  const marginX = 48;
  let y = 56;

  // Header
  doc.setFont("helvetica", "bold");
  doc.setFontSize(22);
  doc.setTextColor(58, 46, 38);
  doc.text(CONFIG.businessName, marginX, y);

  doc.setFont("helvetica", "normal");
  doc.setFontSize(10);
  doc.setTextColor(120, 105, 90);
  y += 18;
  doc.text(`${CONFIG.location} · ${CONFIG.email}`, marginX, y);

  // Order meta, right aligned
  doc.setTextColor(58, 46, 38);
  doc.setFont("helvetica", "bold");
  doc.setFontSize(12);
  doc.text(`Order ${order.orderNumber}`, pageWidth - marginX, 56, { align: "right" });
  doc.setFont("helvetica", "normal");
  doc.setFontSize(10);
  doc.text(order.date.toLocaleString("en-AU"), pageWidth - marginX, 72, { align: "right" });

  // Stitched divider
  y += 22;
  doc.setDrawColor(200, 102, 59);
  doc.setLineWidth(1.2);
  doc.setLineDashPattern([3, 3], 0);
  doc.line(marginX, y, pageWidth - marginX, y);
  doc.setLineDashPattern([], 0);

  // Bill to
  y += 26;
  doc.setFont("helvetica", "bold");
  doc.setFontSize(11);
  doc.text("Order for:", marginX, y);
  doc.setFont("helvetica", "normal");
  doc.setFontSize(10.5);
  y += 16;
  doc.text(order.customer.name, marginX, y);
  y += 14;
  doc.text(`Email: ${order.customer.email}`, marginX, y);
  if (order.customer.phone) {
    y += 14;
    doc.text(`Phone: ${order.customer.phone}`, marginX, y);
  }
  if (order.customer.address) {
    y += 14;
    const addrLines = doc.splitTextToSize(order.customer.address, pageWidth - marginX * 2);
    doc.text(`Address: ${addrLines}`, marginX, y);
    y += addrLines.length * 13;
  }
  if (order.customer.notes) {
    y += 10;
    doc.setFont("helvetica", "bold");
    doc.text("Notes:", marginX, y);
    doc.setFont("helvetica", "normal");
    y += 14;
    const noteLines = doc.splitTextToSize(order.customer.notes, pageWidth - marginX * 2);
    doc.text(noteLines, marginX, y);
    y += noteLines.length * 13;
  }

  // Items table
  y += 24;
  const colItem = marginX;
  const colQty = pageWidth - marginX - 170;
  const colPrice = pageWidth - marginX - 110;
  const colTotal = pageWidth - marginX - 40;

  doc.setFont("helvetica", "bold");
  doc.setFontSize(10);
  doc.text("Item", colItem, y);
  doc.text("Qty", colQty, y, { align: "right" });
  doc.text("Price", colPrice, y, { align: "right" });
  doc.text("Total", colTotal, y, { align: "right" });
  y += 8;
  doc.setDrawColor(58, 46, 38);
  doc.setLineWidth(0.75);
  doc.line(marginX, y, pageWidth - marginX, y);

  doc.setFont("helvetica", "normal");
  order.items.forEach((item) => {
    y += 20;
    if (y > 740) {
      doc.addPage();
      y = 56;
    }
    doc.text(`${item.name}`, colItem, y);
    doc.setFontSize(8.5);
    doc.setTextColor(140, 125, 110);
    // SKU line doubles as the composition line — fabric-specific materials
    // matter for cutting, so keep them on the order itself.
    doc.text(item.materials ? `${item.sku} · ${item.materials}` : item.sku, colItem, y + 11, {
      maxWidth: colQty - colItem - 20,
    });
    doc.setFontSize(10);
    doc.setTextColor(58, 46, 38);
    doc.text(String(item.qty), colQty, y, { align: "right" });
    doc.text(formatPrice(item.price, order.currency), colPrice, y, { align: "right" });
    doc.text(formatPrice(item.lineTotal, order.currency), colTotal, y, { align: "right" });
  });

  y += 16;
  doc.line(marginX, y, pageWidth - marginX, y);
  y += 22;
  doc.setFont("helvetica", "bold");
  doc.setFontSize(13);
  doc.text("Total", colPrice, y, { align: "right" });
  doc.text(formatPrice(order.total, order.currency), colTotal, y, { align: "right" });

  // Footer
  const footerY = 780;
  doc.setFont("helvetica", "italic");
  doc.setFontSize(9.5);
  doc.setTextColor(120, 105, 90);
  doc.text(
    `Thank you for supporting small, local, sustainable craft.`,
    marginX,
    footerY,
    { maxWidth: pageWidth - marginX * 2 }
  );

  return doc;
}

function buildMailtoLink(order) {
  const subject = `New order ${order.orderNumber} — ${order.customer.name}`;
  const lines = [
    `Hi ${CONFIG.businessName}!`,
    ``,
    `Please find my order attached as a PDF (order-${order.orderNumber}.pdf — it should have just downloaded to my computer, I've attached it to this email).`,
    ``,
    `Order ${order.orderNumber} — ${order.date.toLocaleString("en-AU")}`,
    ...order.items.map(
      (i) =>
        `  • ${i.qty} × ${i.name} — ${formatPrice(i.lineTotal, order.currency)}${i.materials ? `\n      (${i.materials})` : ""}`
    ),
    `Total: ${formatPrice(order.total, order.currency)}`,
    ``,
    order.customer.address ? `Delivery/pickup: ${order.customer.address}` : `I'll arrange local pickup/delivery.`,
    order.customer.notes ? `Notes: ${order.customer.notes}` : ``,
    ``,
    `Thanks!`,
    `${order.customer.name}`,
  ].filter(Boolean);
  const body = lines.join("\n");
  return `mailto:${encodeURIComponent(CONFIG.email)}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
}

async function initCheckoutPage() {
  const summaryEl = document.querySelector("#checkout-summary");
  const form = document.querySelector("#checkout-form");
  const emptyState = document.querySelector("#checkout-empty");
  const confirmEl = document.querySelector("#checkout-confirm");

  try {
    [CONFIG, PRODUCTS] = await Promise.all([loadConfig(), loadProducts()]);
  } catch (err) {
    summaryEl.innerHTML = `<p class="error-note">Couldn't load site data (${err.message}). If you're opening this file directly from disk, run a tiny local server first — see the README.</p>`;
    return;
  }

  pruneCart(PRODUCTS);
  const details = getCartDetails(PRODUCTS);

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

  form.addEventListener("submit", (e) => {
    e.preventDefault();
    const formData = new FormData(form);
    const customer = {
      name: formData.get("name").trim(),
      email: formData.get("email").trim(),
      phone: (formData.get("phone") || "").trim(),
      address: (formData.get("address") || "").trim(),
      notes: (formData.get("notes") || "").trim(),
    };
    if (!customer.name || !customer.email) return;

    const freshDetails = getCartDetails(PRODUCTS);
    const order = buildOrder(freshDetails, customer);
    // Build the PDF up front so it's ready to hand over, but don't save it
    // yet — the customer downloads it themselves from the confirmation screen.
    const doc = generateOrderPdf(order);
    const filename = `order-${order.orderNumber}.pdf`;

    const mailLink = document.querySelector("#mailto-link");
    mailLink.href = buildMailtoLink(order);

    const downloadLink = document.querySelector("#download-pdf-link");
    downloadLink.addEventListener("click", (ev) => {
      ev.preventDefault();
      doc.save(filename);
    });

    document.querySelector("#confirm-order-number").textContent = order.orderNumber;
    document.querySelector("#confirm-filename").textContent = filename;

    form.style.display = "none";
    summaryEl.style.display = "none";
    confirmEl.style.display = "block";

    clearCart();
  });
}

document.addEventListener("DOMContentLoaded", initCheckoutPage);
