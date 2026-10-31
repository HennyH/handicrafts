// The whole flow through a real browser (Chromium, via playwright-core):
// shop -> cart -> checkout -> admin -> invoice -> Monokulo connect -> pay
// page (Monero, refund address, webhook -> Paid) and bank transfer, on a
// phone too. Run it with scripts/test-browser.sh, which builds the shop and
// starts it with HANDICRAFTS_MONOKULO_URL pointing at the fake Monokulo this
// script serves on 127.0.0.1:9099.
import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs";
import { chromium } from "playwright-core";

const SHOP = "http://localhost:8082";
const SHOTS = process.argv[2] || ".";
const ADMIN = { username: "rachel", password: "correct horse battery" };
const WHSEC = "whsec_e2e";
const log = (...a) => console.log("•", ...a);
let failures = 0;
const check = (ok, what) => {
  console.log(ok ? "ok -" : "NOT OK -", what);
  if (!ok) failures++;
};

// ------------------------------------------------------------ fake Monokulo
const fake = { orders: [], finishes: [] };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://127.0.0.1:9099");
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    if (url.pathname === "/plugins/connect/handicrafts") {
      // Monokulo would sign the merchant in and ask; send them straight back.
      const back = new URL(url.searchParams.get("return_url"));
      back.searchParams.set("token", "conn_e2e");
      back.searchParams.set("nonce", url.searchParams.get("nonce"));
      res.writeHead(302, { location: back.toString() });
      return res.end();
    }
    if (url.pathname === "/plugins/connect/handicrafts/finish") {
      fake.finishes.push(JSON.parse(body));
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ api: "/api/v1", public_key: "pk_e2e", secret_token: "sk_e2e", webhook_signing_secret: WHSEC }));
    }
    if (url.pathname === "/api/v1/orders") {
      const o = JSON.parse(body);
      fake.orders.push({ auth: req.headers.authorization, ...o });
      res.writeHead(201, { "content-type": "application/json" });
      return res.end(JSON.stringify({ order_id: "order_e2e_" + fake.orders.length, xmr_amount_piconero: 121916000000, expires_at: Math.floor(Date.now() / 1000) + 900, status: "pending" }));
    }
    if (url.pathname === "/static/monokulo-client.js") {
      res.writeHead(200, { "content-type": "text/javascript" });
      return res.end(`window.Monokulo = { mount(target, id, opts) {
        const el = typeof target === "string" ? document.querySelector(target) : target;
        el.innerHTML = '<div id="fake-mk" style="padding:24px;border:2px solid #f26822;border-radius:12px">Fake Monokulo checkout for ' + id + ' (key ' + opts.publicKey + ', refund form ' + (opts.refund === false ? 'off' : 'on') + ')</div>';
        window.__mkOpts = opts; return { destroy() {} };
      } };`);
    }
    if (url.pathname === "/api/v1/webhook-tests") {
      res.writeHead(200, { "content-type": "application/json" });
      return res.end(JSON.stringify({ delivered: true }));
    }
    res.writeHead(404);
    res.end();
  });
});
await new Promise((r) => server.listen(9099, "127.0.0.1", r));

async function webhook(event) {
  const raw = JSON.stringify(event);
  const t = Math.floor(Date.now() / 1000);
  const sig = crypto.createHmac("sha256", WHSEC).update(`${t}.${raw}`).digest("hex");
  const r = await fetch(SHOP + "/webhooks/monokulo", {
    method: "POST",
    headers: { "content-type": "application/json", "x-monokulo-signature": `t=${t},v1=${sig}` },
    body: raw,
  });
  return r.status;
}

// ------------------------------------------------------------------ browser
const browser = await chromium.launch({ executablePath: process.env.CHROME || "/usr/bin/chromium", args: ["--no-sandbox"] });
const consoleErrors = [];
let expect422 = false; // the bad refund address is refused on purpose
const watch = (page, who) => {
  page.on("pageerror", (e) => consoleErrors.push(`${who}: ${e.message}`));
  page.on("console", (m) => m.type() === "error" && !/favicon|fonts\.|fontshare/.test(m.text()) && !(expect422 && /status of 422/.test(m.text())) && consoleErrors.push(`${who}: ${m.text()}`));
};

try {
  // --- customer: shop -> cart -> checkout
  const customer = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const c = await customer.newPage();
  watch(c, "customer");
  await c.goto(SHOP + "/shop.html");
  await c.locator('[data-add-to-cart="tote-bag"]').click();
  await c.locator('[data-add-to-cart="banneton"]').click();
  await c.waitForTimeout(300);
  check((await c.locator(".cart-link [data-cart-count]").textContent()).trim() === "2", "two items in the cart badge");
  await c.goto(SHOP + "/cart.html");
  await c.locator("#go-to-checkout").click();
  await c.waitForURL(/checkout\.html/);
  await c.waitForSelector("#checkout-summary .summary-line--total");
  check((await c.locator("#checkout-summary").textContent()).includes("$105.00"), "checkout shows the cart total $105.00");
  await c.fill("#name", "Sam Example");
  await c.fill("#email", "sam@example.com");
  await c.fill("#address", "local pickup");
  await c.waitForTimeout(3200); // the form's minimum time
  await c.screenshot({ path: `${SHOTS}/1-checkout.png`, fullPage: true });
  await c.locator('#checkout-form button[type="submit"]').click();
  await c.waitForSelector("[data-order-placed]", { timeout: 10000 });
  const number = await c.locator("[data-order-placed]").getAttribute("data-order-placed");
  log("order", number);
  check(c.url().endsWith("/checkout.html"), "fixi swapped in the received page without a page load");
  check((await c.locator("h1").textContent()).includes("Thank you, Sam"), "'Thank you, Sam!' shown");
  const emptied = await c
    .waitForFunction(() => Object.keys(JSON.parse(localStorage.getItem("rh_cart_v1") || "{}")).length === 0, null, { timeout: 5000 })
    .then(() => true, () => false);
  check(emptied, "the cart is emptied");
  check(emptied && (await c.locator(".cart-link [data-cart-count]").isHidden()), "the cart badge goes away");
  await c.screenshot({ path: `${SHOTS}/2-received.png`, fullPage: true });

  // --- admin: list, accept + invoice, comment, .eml
  const admin = await browser.newContext({ httpCredentials: ADMIN, acceptDownloads: true, viewport: { width: 1280, height: 900 } });
  const a = await admin.newPage();
  watch(a, "admin");
  await a.goto(SHOP + "/admin");
  check(await a.locator(`text=${number}`).isVisible(), "the order is on /admin");
  await a.locator(`a:has-text("${number}")`).click();
  // Accepted gates Invoiced.
  check(await a.locator('#status option[value="invoiced"]').isDisabled(), "Invoiced is greyed out until Accepted is ticked");
  await a.check("#accepted");
  check(!(await a.locator('#status option[value="invoiced"]').isDisabled()), "ticking Accepted enables Invoiced");
  await a.selectOption("#status", "invoiced");
  await a.locator('#status-form button[type="submit"]').click();
  await a.waitForLoadState();
  check((await a.locator(".detail-head .pill").textContent()).includes("Invoiced"), "status saved as Invoiced");
  await a.fill('.comment-form textarea[name="body"]', "Called Sam, pickup Saturday");
  await a.locator('.comment-form button[type="submit"]').click();
  await a.waitForSelector('.timeline :text("Called Sam, pickup Saturday")', { timeout: 5000 });
  check(true, "comment added through fixi");
  const cleared = await a
    .waitForFunction(() => document.querySelector('.comment-form textarea[name="body"]').value === "", null, { timeout: 5000 })
    .then(() => true, () => false);
  check(cleared, "the comment box is emptied");
  const [dl] = await Promise.all([a.waitForEvent("download"), a.locator('a[href$="/invoice.eml"]').click()]);
  const eml = fs.readFileSync(await dl.path(), "utf8");
  check(dl.suggestedFilename() === `invoice-${number}.eml` && eml.startsWith("X-Unsent: 1"), "Invoice downloads an unsent .eml draft");
  const payLink = await a.locator("button[data-copy]").getAttribute("data-copy");
  const payPath = new URL(payLink).pathname;
  await a.screenshot({ path: `${SHOTS}/3-admin-order.png`, fullPage: true });

  // --- admin: connect Monokulo
  await a.goto(SHOP + "/admin/monokulo");
  await a.locator('button:has-text("Connect to Monokulo")').click();
  await a.waitForURL(/\/admin\/monokulo\?done=connected/, { timeout: 10000 });
  check((await a.locator("main").textContent()).includes("pk_e2e"), "the store is connected (keys from the connect flow)");
  check(fake.finishes[0]?.webhook_url === SHOP + "/webhooks/monokulo", "finish registered the shop's webhook URL");
  await a.locator('button:has-text("Send a test webhook")').click();
  await a.waitForURL(/done=tested/);
  check(true, "test webhook button works");
  await a.screenshot({ path: `${SHOTS}/4-admin-monokulo.png`, fullPage: true });

  // --- customer: pay by Monero
  await c.goto(SHOP + payPath);
  await c.waitForSelector("#fake-mk", { timeout: 10000 });
  check((await c.locator("#fake-mk").textContent()).includes("pk_e2e") && (await c.locator("#fake-mk").textContent()).includes("refund form off"), "Monokulo's checkout is mounted with the connected key, its refund form off");
  check(fake.orders.at(-1)?.auth === "Bearer sk_e2e" && fake.orders.at(-1)?.amount === "105.00" && fake.orders.at(-1)?.currency === "AUD", "the Monokulo order was made server-side for 105.00 AUD with the secret key");
  const refund = "4" + "A".repeat(94);
  await c.fill("#refund", refund);
  await c.locator("#refund").blur();
  await c.waitForSelector('#refund-status:has-text("Saved.")', { timeout: 5000 });
  check(true, "refund address saved through fixi");
  expect422 = true;
  await c.fill("#refund", "not-an-address");
  await c.locator("#refund").blur();
  await c.waitForSelector('#refund-status:has-text("Monero address")', { timeout: 5000 });
  expect422 = false;
  check(true, "a bad refund address is refused with a reason");
  await c.fill("#refund", refund);
  await c.locator("#refund").blur();
  await c.waitForSelector('#refund-status:has-text("Saved.")', { timeout: 5000 });
  await c.screenshot({ path: `${SHOTS}/5-pay-monero.png`, fullPage: true });

  const status = await webhook({ api_version: 2, event_id: "evt_e2e_1", event: "order.paid", order_id: fake.orders.at(-1) ? "order_e2e_" + fake.orders.length : "x", merchant_order_id: number, amount: "105.00", currency: "AUD", xmr_amount: "0.121916000000", fx_rate: "861.26", fx_source: "haveno" });
  check(status === 200, "signed order.paid webhook accepted");
  await c.waitForSelector('h2:has-text("Paid, thank you!")', { timeout: 20000 });
  check(true, "the pay page moves to Paid by itself");
  await c.screenshot({ path: `${SHOTS}/6-paid.png`, fullPage: true });
  await a.goto(SHOP + "/admin");
  await a.locator(`a:has-text("${number}")`).click();
  const timeline = await a.locator(".timeline").textContent();
  check((await a.locator(".detail-head .pill").textContent()).includes("Paid") && timeline.includes(refund), "admin shows Paid, with the refund address in the timeline");

  // --- second order: bank transfer
  await c.goto(SHOP + "/shop.html");
  await c.locator('[data-add-to-cart="banneton"]').click();
  await c.goto(SHOP + "/checkout.html");
  await c.waitForSelector("#checkout-summary .summary-line--total");
  await c.fill("#name", "Sam Example");
  await c.fill("#email", "sam@example.com");
  await c.waitForTimeout(3200);
  await c.locator('#checkout-form button[type="submit"]').click();
  await c.waitForSelector("[data-order-placed]");
  const number2 = await c.locator("[data-order-placed]").getAttribute("data-order-placed");
  await a.goto(SHOP + "/admin");
  await a.locator(`a:has-text("${number2}")`).click();
  await a.check("#accepted");
  await a.selectOption("#status", "invoiced");
  await a.locator('#status-form button[type="submit"]').click();
  await a.waitForLoadState();
  const pay2 = new URL(await a.locator("button[data-copy]").getAttribute("data-copy")).pathname;
  await c.goto(SHOP + pay2 + "?method=bank");
  const ref = (await c.locator(".bank__row--ref dd").textContent()).trim();
  check(/^[23456789ABCDEFGHJKMNPQRSTVWXYZ]{5}$/.test(ref), `bank page shows the short reference ${ref}`);
  await c.screenshot({ path: `${SHOTS}/7-pay-bank.png`, fullPage: true });
  await c.locator('button:has-text("I\'ve sent the payment")').click();
  await c.waitForSelector("text=Thanks, Rachel's been told");
  check(true, "\"I've sent the payment\" shows the thanks page");
  await a.reload();
  check((await a.locator(".timeline").textContent()).includes("paid by PayID / bank transfer") && (await a.locator(".detail-head .pill").textContent()).includes("Invoiced"), "admin sees the claim, status unchanged");

  // --- phone width: received page fits
  const phone = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true });
  const p = await phone.newPage();
  await p.goto(SHOP + pay2 + "?method=bank");
  const overflow = await p.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  check(overflow <= 0, `pay page fits a phone (overflow ${overflow}px)`);
  await p.screenshot({ path: `${SHOTS}/8-pay-phone.png`, fullPage: true });
} catch (e) {
  failures++;
  console.log("NOT OK - flow broke:", e.message.split("\n")[0]);
} finally {
  check(consoleErrors.length === 0, `no JavaScript errors${consoleErrors.length ? ": " + consoleErrors.join(" | ") : ""}`);
  await browser.close();
  server.close();
  console.log(failures ? `${failures} FAILED` : "all passed");
  process.exit(failures ? 1 : 0);
}
