// Stride: a tiny single-page shop with a few deliberate UX problems for Telltale to find.
//
//  1. /products: product images look clickable (pointer, hover zoom) but do nothing  → dead clicks
//  2. /checkout: "Pay now" takes 3 s and gives no feedback                           → rage clicks
//  3. /pricing:  the "Annual" toggle throws a TypeError                              → JS errors
//  4. /signup:   a long form that wipes every field when the phone number is wrong   → form drop-off
"use strict";

const PRODUCTS = [
  { id: "shoe-1", name: "Tempo Racer", price: 129, color: "#e0482d" },
  { id: "shoe-2", name: "Trail Ridge", price: 149, color: "#3a7d44" },
  { id: "shoe-3", name: "Cloud Daily", price: 109, color: "#3d6fd6" },
  { id: "shoe-4", name: "Night Pacer", price: 139, color: "#22223b" },
  { id: "shoe-5", name: "Sprint Lite", price: 99, color: "#e9a23b" },
];
const SHIPPING = 7.5;

const app = document.getElementById("app");
const cart = JSON.parse(sessionStorage.getItem("stride_cart") || "[]");

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const money = (n) => `€${n.toFixed(2)}`;

function shoeImage(color) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300"><rect width="400" height="300" fill="${color}" opacity=".12"/><path d="M60 200c40-10 70-40 95-80 20 10 35 25 60 30 40 8 90 10 120 40 10 10 8 30-10 32H75c-20 0-30-20-15-22z" fill="${color}"/><path d="M60 222h290" stroke="${color}" stroke-width="10" stroke-linecap="round" opacity=".5"/></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

function saveCart() {
  sessionStorage.setItem("stride_cart", JSON.stringify(cart));
  document.getElementById("cart-count").textContent = cart.length;
}

function navigate(path) {
  history.pushState({}, "", path);
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

const pages = {
  "/": () => `
    <section class="hero">
      <div>
        <h1>Run further, feel lighter.</h1>
        <p class="lead">Five shoes, designed by runners, for every kind of run: tempo days, trails, recovery jogs and race day.</p>
        <a class="btn" href="/products" data-link>Shop shoes</a>
        <a class="btn secondary" href="/pricing" data-link>Stride+ membership</a>
      </div>
      <img src="${shoeImage("#e0482d")}" alt="Tempo Racer">
    </section>
    <h2>Popular this week</h2>
    <div class="grid">${PRODUCTS.slice(0, 3).map(card).join("")}</div>`,

  "/products": () => `
    <h1>All shoes</h1>
    <p class="lead">Free returns within 30 days.</p>
    <div class="grid">${PRODUCTS.map(card).join("")}</div>`,

  "/cart": () => {
    const items = cart.map((id) => PRODUCTS.find((p) => p.id === id)).filter(Boolean);
    const subtotal = items.reduce((s, p) => s + p.price, 0);
    if (!items.length) return `<h1>Your cart</h1><p class="lead">Your cart is empty.</p><a class="btn" href="/products" data-link>Shop shoes</a>`;
    return `
      <h1>Your cart</h1>
      <div class="panel">
        ${items.map((p) => `<div class="row"><span>${esc(p.name)}</span><span class="price">${money(p.price)}</span></div>`).join("")}
        <div class="row"><span class="muted">Shipping</span><span>${money(SHIPPING)}</span></div>
        <div class="row"><strong>Total</strong><strong>${money(subtotal + SHIPPING)}</strong></div>
      </div>
      <p><button class="btn" id="checkout">Checkout</button></p>`;
  },

  "/checkout": () => {
    const items = cart.map((id) => PRODUCTS.find((p) => p.id === id)).filter(Boolean);
    const total = items.reduce((s, p) => s + p.price, 0) + SHIPPING;
    return `
      <h1>Checkout</h1>
      <div class="panel">
        <div class="row"><span>Card</span><span class="muted">•••• 4242 (demo)</span></div>
        <div class="row"><span>Deliver to</span><span class="muted">1 Example Street</span></div>
        <div class="row"><strong>Total</strong><strong>${money(total)}</strong></div>
      </div>
      <p><button class="btn" id="pay">Pay now</button></p>`;
  },

  "/checkout/success": () => `
    <h1>Thanks for your order!</h1>
    <p class="lead">We'll email you when your shoes ship.</p>
    <a class="btn secondary" href="/products" data-link>Keep shopping</a>`,

  "/pricing": () => `
    <h1>Stride+ membership</h1>
    <p class="lead">Free express shipping, early access to drops and a training plan built for you.</p>
    <div class="toggle"><button class="on" id="monthly">Monthly</button><button id="annual">Annual (save 20%)</button></div>
    <div class="plans">
      <div class="panel"><h3>Stride+</h3><p class="price" id="plan-price">€6 / month</p><a class="btn" href="/signup" data-link>Join</a></div>
      <div class="panel"><h3>Stride+ Coach</h3><p class="price" id="coach-price">€14 / month</p><a class="btn" href="/signup" data-link>Join</a></div>
    </div>`,

  "/signup": () => `
    <h1>Create your account</h1>
    <p class="lead">Join Stride+ in a minute.</p>
    <form class="signup" id="signup-form" novalidate>
      <p class="error" id="signup-error" hidden></p>
      <label>First name <input name="first" autocomplete="off"></label>
      <label>Last name <input name="last" autocomplete="off"></label>
      <label>Email <input name="email" type="text" autocomplete="off"></label>
      <label>Phone (format: +44 0000 000000) <input name="phone" autocomplete="off"></label>
      <label>Date of birth <input name="dob" placeholder="DD/MM/YYYY"></label>
      <label>Shoe size <select name="size"><option></option>${[38, 39, 40, 41, 42, 43, 44, 45].map((s) => `<option>${s}</option>`).join("")}</select></label>
      <label>How did you hear about us? <input name="source"></label>
      <label>Password <input name="password" type="password"></label>
      <button class="btn" type="submit">Create account</button>
    </form>`,
};

function card(p) {
  return `
    <div class="product-card">
      <img src="${shoeImage(p.color)}" alt="${esc(p.name)}">
      <div class="info">
        <h3><a href="/products/${p.id}" data-link>${esc(p.name)}</a></h3>
        <span class="price">${money(p.price)}</span>
      </div>
    </div>`;
}

function productPage(id) {
  const p = PRODUCTS.find((x) => x.id === id);
  if (!p) return `<h1>Not found</h1>`;
  return `
    <div class="detail">
      <img src="${shoeImage(p.color)}" alt="${esc(p.name)}">
      <div>
        <h1>${esc(p.name)}</h1>
        <p class="price">${money(p.price)}</p>
        <p class="muted">Lightweight mesh upper, responsive foam and a grippy outsole.</p>
        <div class="sizes">${[39, 40, 41, 42, 43, 44].map((s, i) => `<button class="${i === 2 ? "on" : ""}">${s}</button>`).join("")}</div>
        <button class="btn" id="add-to-cart">Add to cart</button>
      </div>
    </div>`;
}

function render() {
  const path = location.pathname;
  const page = path.startsWith("/products/") ? () => productPage(path.split("/")[2]) : pages[path] || (() => `<h1>Not found</h1>`);
  app.innerHTML = page();
  document.title = `Stride: ${path === "/" ? "running shoes" : path.slice(1)}`;
  bind(path);
}

function bind(path) {
  if (path.startsWith("/products/")) {
    app.querySelectorAll(".sizes button").forEach((b) =>
      b.addEventListener("click", () => {
        app.querySelectorAll(".sizes button").forEach((x) => x.classList.remove("on"));
        b.classList.add("on");
      })
    );
    app.querySelector("#add-to-cart")?.addEventListener("click", () => {
      cart.push(path.split("/")[2]);
      saveCart();
      window.telltale?.track("add_to_cart", { product: path.split("/")[2] });
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
        window.telltale?.track("purchase", { items: cart.length });
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
      window.telltale?.track("signup_completed");
      app.innerHTML = `<h1>Welcome to Stride+!</h1><p class="lead">Check your inbox to confirm your email.</p>`;
    });
  }
}

saveCart();
render();
