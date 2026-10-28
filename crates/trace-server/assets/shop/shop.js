// Stride: a tiny single-page shop with a few deliberate UX problems for Trace to find.
//
//  1. /products: product images look clickable (pointer, hover zoom) but do nothing  → dead clicks
//  2. /checkout: "Pay now" takes 3 s and gives no feedback                           → rage clicks
//  3. /pricing:  the "Annual" toggle throws a TypeError                              → JS errors
//  4. /signup:   a long form that wipes every field when the phone number is wrong   → form drop-off
//  5. /products/:id: the reviews API fails ~30% of the time                         → network + console errors
"use strict";

const PRODUCTS = [
  { id: "shoe-1", name: "Tempo Racer", price: 129, color: "#ff5a26" },
  { id: "shoe-2", name: "Trail Ridge", price: 149, color: "#4f7a3a" },
  { id: "shoe-3", name: "Cloud Daily", price: 109, color: "#3d6fd6" },
  { id: "shoe-4", name: "Night Pacer", price: 139, color: "#2b2d4a" },
  { id: "shoe-5", name: "Sprint Lite", price: 99, color: "#e9a23b" },
];
const SHIPPING = 7.5;

// Where the shop lives: "" at a server's root, "/demo" on tracetells.com.
const BASE = document.documentElement.dataset.base || "";
// "api" asks the server's flaky reviews API; "static" (GitHub Pages) reads
// JSON files and sometimes asks for one that doesn't exist.
const REVIEWS = document.documentElement.dataset.reviews || "api";

// Links are written from the root; under a base path they get it added.
function rebase(root) {
  if (!BASE) return;
  root.querySelectorAll('a[href^="/"]').forEach((a) => {
    const href = a.getAttribute("href");
    if (href !== BASE && !href.startsWith(`${BASE}/`)) a.setAttribute("href", BASE + href);
  });
}

const app = document.getElementById("app");
const cart = JSON.parse(sessionStorage.getItem("stride_cart") || "[]");

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const money = (n) => `€${n.toFixed(2)}`;

// A side-on running shoe on a tile tinted with its colour; `bare` leaves the tile out.
function shoeImage(color, bare) {
  const tile = bare ? "" : `<rect width="400" height="300" fill="${color}" opacity=".16"/>`;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300">${tile}<g transform="translate(0 24)">
    <ellipse cx="205" cy="236" rx="165" ry="10" fill="#15130f" opacity=".08"/>
    <path d="M60 182C60 150 72 118 96 98c16-12 38-14 50-2 14 14 22 28 44 36 32 12 72 14 110 22 30 6 52 16 56 30H60z" fill="${color}"/>
    <path d="M300 154c30 6 52 16 56 30h-40c0-12-6-22-16-30z" fill="#15130f" opacity=".14"/>
    <path d="M96 98c8-14 26-20 42-14" stroke="#15130f" stroke-width="7" fill="none" stroke-linecap="round"/>
    <path d="M112 168c52-20 120-36 190-10" stroke="#fff" stroke-width="9" fill="none" stroke-linecap="round" opacity=".9"/>
    <path d="M178 124l14-15M198 134l14-15M218 141l14-15" stroke="#fff" stroke-width="5" stroke-linecap="round"/>
    <path d="M48 190c0-4 4-8 12-8h284c16 0 26 10 24 24H46c0-6 0-12 2-16z" fill="#fff" stroke="#15130f" stroke-opacity=".14" stroke-width="2"/>
    <path d="M46 206h322c0 12-8 18-22 18H70c-14 0-24-6-24-18z" fill="#15130f"/></g></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const ICONS = {
  arrow: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6"/></svg>`,
  truck: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7h11v9H3zM14 10h4l3 3v3h-7"/><circle cx="7" cy="17.5" r="1.8"/><circle cx="17" cy="17.5" r="1.8"/></svg>`,
  returns: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12a8 8 0 1 0 2.3-5.7L4 8.6"/><path d="M4 4v4.6h4.6"/></svg>`,
  check: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>`,
  lock: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>`,
};

function saveCart() {
  sessionStorage.setItem("stride_cart", JSON.stringify(cart));
  document.getElementById("cart-count").textContent = cart.length;
}

function navigate(path) {
  history.pushState({}, "", path === BASE || path.startsWith(`${BASE}/`) ? path : BASE + path);
  render();
  window.scrollTo(0, 0);
}

document.addEventListener("click", (e) => {
  const link = e.target.closest("a[data-link]");
  if (!link || e.metaKey || e.ctrlKey) return;
  e.preventDefault();
  navigate(link.getAttribute("href"));
});
window.addEventListener("popstate", render);

const cartItems = () => cart.map((id) => PRODUCTS.find((p) => p.id === id)).filter(Boolean);

function cartItem(p) {
  return `
    <div class="cart-item">
      <img src="${shoeImage(p.color)}" alt="">
      <div><a href="/products/${p.id}" data-link>${esc(p.name)}</a><span class="muted">1 pair</span></div>
      <span class="price">${money(p.price)}</span>
    </div>`;
}

const pages = {
  "/": () => `
    <section class="hero">
      <div>
        <span class="chip"><span class="dot"></span>Designed by runners</span>
        <h1>Run further,<br><span class="accent">feel lighter.</span></h1>
        <p class="lead">Five shoes, designed by runners, for every kind of run: tempo days, trails, recovery jogs and race day.</p>
        <div class="actions">
          <a class="btn" href="/products" data-link>Shop shoes ${ICONS.arrow}</a>
          <a class="btn secondary" href="/pricing" data-link>Stride+ membership</a>
        </div>
        <div class="perks"><span>${ICONS.truck}Free shipping over €100</span><span>${ICONS.returns}Free returns within 30 days</span></div>
      </div>
      <div class="hero-art">
        <span class="ghost" aria-hidden="true">Tempo</span>
        <img src="${shoeImage(PRODUCTS[0].color, true)}" alt="Tempo Racer">
        <div class="hero-tag">
          <div><strong>Tempo Racer</strong><span class="muted">Popular this week</span></div>
          <a href="/products/shoe-1" data-link>${money(129)} ${ICONS.arrow}</a>
        </div>
      </div>
    </section>
    <div class="section-head">
      <h2 class="display-2">Popular this week</h2>
      <a class="more" href="/products" data-link>All shoes ${ICONS.arrow}</a>
    </div>
    <div class="grid">${PRODUCTS.slice(0, 3).map(card).join("")}</div>
    <section class="banner">
      <div>
        <span class="eyebrow">Stride+ membership</span>
        <h2 class="display-2">Run with a plan</h2>
        <p>Free express shipping, early access to drops and a training plan built for you.</p>
      </div>
      <div class="banner-cta">
        <span>From <strong>€6</strong> / month</span>
        <a class="btn secondary" href="/pricing" data-link>See membership</a>
      </div>
    </section>`,

  "/products": () => `
    <h1>All shoes</h1>
    <div class="page-meta"><p class="lead">Free returns within 30 days.</p><span>${PRODUCTS.length} shoes</span></div>
    <div class="grid">${PRODUCTS.map(card).join("")}</div>`,

  "/cart": () => {
    const items = cartItems();
    const subtotal = items.reduce((s, p) => s + p.price, 0);
    if (!items.length) return `<h1>Your cart</h1><p class="lead">Your cart is empty.</p><a class="btn" href="/products" data-link>Shop shoes</a>`;
    return `
      <h1>Your cart</h1>
      <div class="cart-layout">
        <div class="cart-items">
          ${items.map(cartItem).join("")}
          <a class="more" href="/products" data-link>Keep shopping</a>
        </div>
        <aside class="panel" aria-label="Order summary">
          <h2>Summary</h2>
          <div class="row"><span>Subtotal</span><span>${money(subtotal)}</span></div>
          <div class="row"><span>Shipping</span><span>${money(SHIPPING)}</span></div>
          <div class="row total"><span>Total</span><span>${money(subtotal + SHIPPING)}</span></div>
          <button class="btn block" id="checkout">Checkout</button>
        </aside>
      </div>`;
  },

  "/checkout": () => {
    const items = cartItems();
    const total = items.reduce((s, p) => s + p.price, 0) + SHIPPING;
    return `
      <h1>Checkout</h1>
      <div class="cart-layout">
        <div class="cart-items">${items.map(cartItem).join("")}</div>
        <aside class="panel" aria-label="Payment">
          <h2>Payment</h2>
          <div class="row"><span>Card</span><span class="muted">•••• 4242 (demo)</span></div>
          <div class="row"><span>Deliver to</span><span class="muted">1 Example Street</span></div>
          <div class="row total"><span>Total</span><span>${money(total)}</span></div>
          <button class="btn block" id="pay">Pay now</button>
          <p class="note">${ICONS.lock}Demo checkout: no payment is taken.</p>
        </aside>
      </div>`;
  },

  "/help": () => `
    <h1>Shipping &amp; returns</h1>
    <p class="lead">Free shipping over €100, otherwise €7.50. Returns are free within 30 days: drop the box at any parcel shop.</p>
    <a class="btn secondary" href="/products" data-link>Shop shoes</a>`,

  "/checkout/success": () => `
    <h1>Thanks for your order!</h1>
    <p class="lead">We'll email you when your shoes ship.</p>
    <a class="btn secondary" href="/products" data-link>Keep shopping</a>`,

  "/pricing": () => `
    <div class="pricing-head">
      <span class="eyebrow">Stride+ membership</span>
      <h1>Run with a plan</h1>
      <p class="lead">Free express shipping, early access to drops and a training plan built for you.</p>
      <div class="toggle"><button class="on" id="monthly">Monthly</button><button id="annual">Annual <span class="save">save 20%</span></button></div>
    </div>
    <div class="plans">
      <div class="plan">
        <h3>Stride+</h3>
        <p class="plan-price" id="plan-price">€6 / month</p>
        <ul><li>${ICONS.check}Free express shipping</li><li>${ICONS.check}Early access to drops</li></ul>
        <a class="btn secondary" href="/signup" data-link>Join Stride+</a>
      </div>
      <div class="plan featured">
        <h3>Stride+ Coach</h3>
        <p class="plan-price" id="coach-price">€14 / month</p>
        <ul><li>${ICONS.check}Everything in Stride+</li><li>${ICONS.check}A training plan built for you</li></ul>
        <a class="btn" href="/signup" data-link>Join Stride+ Coach</a>
      </div>
    </div>`,

  "/signup": () => `
    <div class="signup-layout">
      <div class="signup-intro">
        <h1>Create your account</h1>
        <p class="lead">Join Stride+ in a minute.</p>
        <img src="${shoeImage(PRODUCTS[0].color)}" alt="">
      </div>
      <form class="signup" id="signup-form" novalidate>
        <p class="error" id="signup-error" hidden></p>
        <div class="pair">
          <label>First name <input name="first" autocomplete="off"></label>
          <label>Last name <input name="last" autocomplete="off"></label>
        </div>
        <label>Email <input name="email" type="text" autocomplete="off"></label>
        <label>Phone (format: +44 0000 000000) <input name="phone" autocomplete="off"></label>
        <div class="pair">
          <label>Date of birth <input name="dob" placeholder="DD/MM/YYYY"></label>
          <label>Shoe size <select name="size"><option></option>${[38, 39, 40, 41, 42, 43, 44, 45].map((s) => `<option>${s}</option>`).join("")}</select></label>
        </div>
        <label>How did you hear about us? <input name="source"></label>
        <label>Password <input name="password" type="password"></label>
        <button class="btn block" type="submit">Create account</button>
      </form>
    </div>`,
};

function card(p) {
  return `
    <div class="product-card">
      <img src="${shoeImage(p.color)}" alt="${esc(p.name)}">
      <div class="info">
        <div>
          <h3><a href="/products/${p.id}" data-link>${esc(p.name)}</a></h3>
          <span class="swatch-line"><span class="swatch" style="background:${p.color}"></span>Running shoe</span>
        </div>
        <span class="price">${money(p.price)}</span>
      </div>
    </div>`;
}

function productPage(id) {
  const p = PRODUCTS.find((x) => x.id === id);
  if (!p) return `<h1>Not found</h1>`;
  return `
    <nav class="breadcrumb" aria-label="Breadcrumb"><a href="/products" data-link>All shoes</a><span aria-hidden="true">/</span><span>${esc(p.name)}</span></nav>
    <div class="detail">
      <img src="${shoeImage(p.color)}" alt="${esc(p.name)}">
      <div>
        <h1>${esc(p.name)}</h1>
        <div class="detail-meta"><span class="price">${money(p.price)}</span><span class="muted" id="reviews">Loading reviews…</span></div>
        <p class="desc">Lightweight mesh upper, responsive foam and a grippy outsole.</p>
        <span class="field-label">Size (EU)</span>
        <div class="sizes">${[39, 40, 41, 42, 43, 44].map((s, i) => `<button class="${i === 2 ? "on" : ""}">${s}</button>`).join("")}</div>
        <button class="btn block" id="add-to-cart" data-trace-event="add_to_cart" data-trace-props='{"product":"${p.id}"}'>Add to cart</button>
        <div class="info-list">
          <div>${ICONS.truck}<span>Free shipping over €100, otherwise €7.50.</span></div>
          <div>${ICONS.returns}<span>Free returns within 30 days: drop the box at any parcel shop.</span></div>
        </div>
      </div>
    </div>`;
}

function render() {
  const path = (BASE && location.pathname.startsWith(BASE) ? location.pathname.slice(BASE.length) : location.pathname) || "/";
  const page = path.startsWith("/products/") ? () => productPage(path.split("/")[2]) : pages[path] || (() => `<h1>Not found</h1>`);
  app.innerHTML = page();
  rebase(app);
  document.title = `Stride: ${path === "/" ? "running shoes" : path.slice(1)}`;
  bind(path);
}

function bind(path) {
  if (path.startsWith("/products/")) {
    const reviewId = 101 + PRODUCTS.findIndex((p) => p.id === path.split("/")[2]);
    const reviews = app.querySelector("#reviews");
    const url =
      REVIEWS === "static"
        ? `${BASE}/api/reviews/${reviewId}${Math.random() < 0.3 ? "-unavailable" : ""}.json`
        : `/api/reviews/${reviewId}`;
    fetch(url)
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((r) => (reviews.textContent = `★ ${r.rating} · ${r.count} reviews`))
      .catch((err) => {
        // Planted problem: a flaky API, reported the way most apps do it.
        console.error("Reviews failed to load: %s", err.message);
        reviews.textContent = "Reviews are unavailable right now.";
      });
    app.querySelectorAll(".sizes button").forEach((b) =>
      b.addEventListener("click", () => {
        app.querySelectorAll(".sizes button").forEach((x) => x.classList.remove("on"));
        b.classList.add("on");
      })
    );
    app.querySelector("#add-to-cart")?.addEventListener("click", () => {
      // Tracked by data-trace-event on the button: no analytics code needed here.
      cart.push(path.split("/")[2]);
      saveCart();
      navigate("/cart");
    });
  }
  if (path === "/cart") app.querySelector("#checkout")?.addEventListener("click", () => navigate("/checkout"));

  if (path === "/checkout") {
    // Planted problem: payment takes 3 seconds and the button gives no feedback at all.
    let paying = false;
    app.querySelector("#pay").addEventListener("click", () => {
      if (paying) return;
      paying = true;
      setTimeout(() => {
        window.trace?.track("purchase", { items: cart.length });
        cart.length = 0;
        saveCart();
        navigate("/checkout/success");
      }, 3000);
    });
  }

  if (path === "/pricing") {
    const pricing = { monthly: { plus: 6, coach: 14 } };
    app.querySelector("#monthly").addEventListener("click", () => {
      app.querySelector("#plan-price").textContent = `€${pricing.monthly.plus} / month`;
    });
    // Planted problem: annual prices were never added, so this throws.
    app.querySelector("#annual").addEventListener("click", () => {
      app.querySelector("#plan-price").textContent = `€${pricing.yearly.annual} / year`;
    });
  }

  if (path === "/signup") {
    const form = app.querySelector("#signup-form");
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const phone = form.phone.value.trim();
      // Planted problem: strict phone format, and a failed submit wipes every field.
      if (!/^\+44 \d{4} \d{6}$/.test(phone)) {
        form.reset();
        const err = app.querySelector("#signup-error");
        err.textContent = "Something went wrong. Please try again.";
        err.hidden = false;
        return;
      }
      window.trace?.track("signup_completed");
      app.innerHTML = `<h1>Welcome to Stride+!</h1><p class="lead">Check your inbox to confirm your email.</p>`;
    });
  }
}

rebase(document);
saveCart();
render();
