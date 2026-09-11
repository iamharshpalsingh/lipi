// A tiny stand-in for the browser DOM: just enough to run a LiPi web build
// under Node in tests, click through it and print what it drew.
//
//   node ui_dom.js dist/app.js "click:Add to cart" "type:Search=chai" "go:/checkout"
//
// After each step it prints the HTML inside #app.
"use strict";

class FakeNode {
  constructor() { this.childNodes = []; this.parentNode = null; }
  appendChild(c) { if (c.parentNode) c.parentNode.removeChild(c); c.parentNode = this; this.childNodes.push(c); return c; }
  removeChild(c) { const i = this.childNodes.indexOf(c); if (i >= 0) this.childNodes.splice(i, 1); c.parentNode = null; return c; }
  replaceChild(n, o) {
    if (n.parentNode) n.parentNode.removeChild(n);
    const i = this.childNodes.indexOf(o);
    this.childNodes[i] = n;
    n.parentNode = this;
    o.parentNode = null;
    return o;
  }
  insertBefore(n, ref) {
    if (!ref) return this.appendChild(n);
    if (n.parentNode) n.parentNode.removeChild(n);
    this.childNodes.splice(this.childNodes.indexOf(ref), 0, n);
    n.parentNode = this;
    return n;
  }
  get firstChild() { return this.childNodes[0] || null; }
}

class FakeText extends FakeNode {
  constructor(t) { super(); this.nodeValue = t; }
  get textContent() { return this.nodeValue; }
}

class FakeElement extends FakeNode {
  constructor(tag) { super(); this.tagName = tag.toUpperCase(); this.attributes = new Map(); this.listeners = {}; this.value = ""; this.checked = false; }
  setAttribute(k, v) { this.attributes.set(k, String(v)); }
  removeAttribute(k) { this.attributes.delete(k); }
  getAttribute(k) { return this.attributes.has(k) ? this.attributes.get(k) : null; }
  get id() { return this.getAttribute("id") || ""; }
  set id(v) { this.setAttribute("id", v); }
  addEventListener(type, f) { (this.listeners[type] = this.listeners[type] || []).push(f); }
  dispatch(type) { for (const f of this.listeners[type] || []) f({ type, preventDefault() {} }); }
  get textContent() { return this.childNodes.map((c) => c.textContent).join(""); }
  set textContent(v) { this.childNodes = []; this.appendChild(new FakeText(v)); }
}

function all(n, out = []) {
  for (const c of n.childNodes) if (c instanceof FakeElement) { out.push(c); all(c, out); }
  return out;
}

const windowListeners = {};
const documentNode = new FakeElement("html");
const head = documentNode.appendChild(new FakeElement("head"));
const body = documentNode.appendChild(new FakeElement("body"));
const app = body.appendChild(new FakeElement("div"));
app.id = "app";
globalThis.document = {
  head, body,
  createElement: (t) => new FakeElement(t),
  createTextNode: (t) => new FakeText(t),
  getElementById: (id) => all(documentNode).find((e) => e.id === id) || null,
};
globalThis.window = { addEventListener(type, f) { (windowListeners[type] = windowListeners[type] || []).push(f); } };
let hash = "";
globalThis.location = {
  get hash() { return hash; },
  set hash(v) { hash = v.startsWith("#") ? v : "#" + v; for (const f of windowListeners.hashchange || []) f(); },
};

function html(n) {
  if (n instanceof FakeText) return n.nodeValue;
  const tag = n.tagName.toLowerCase();
  const attrs = Array.from(n.attributes).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, v]) => (v === "" ? ` ${k}` : ` ${k}="${v}"`)).join("");
  let props = "";
  if (tag === "input") props = n.getAttribute("type") === "checkbox" ? (n.checked ? " [checked]" : "") : ` [value=${JSON.stringify(n.value)}]`;
  return `<${tag}${attrs}${props}>${n.childNodes.map(html).join("")}</${tag}>`;
}

const settle = () => new Promise((r) => setTimeout(r, 0)).then(() => new Promise((r) => setTimeout(r, 0)));

async function main() {
  const [bundle, ...steps] = process.argv.slice(2);
  require(require("path").resolve(bundle));
  await settle();
  console.log("--- start\n" + html(app));
  for (const step of steps) {
    const at = step.indexOf(":");
    const [action, arg] = [step.slice(0, at), step.slice(at + 1)];
    if (action === "click") {
      const el = all(app).find((e) => (e.tagName === "BUTTON" || e.tagName === "A") && e.textContent === arg);
      if (!el) throw new Error(`nothing to click called "${arg}"`);
      if (el.tagName === "A") location.hash = el.getAttribute("href");
      else el.dispatch("click");
    } else if (action === "type") {
      const [placeholder, value] = arg.split("=");
      const el = all(app).find((e) => e.getAttribute("placeholder") === placeholder);
      if (!el) throw new Error(`no field with placeholder "${placeholder}"`);
      el.value = value;
      el.dispatch("input");
    } else if (action === "check") {
      const label = all(app).find((e) => e.tagName === "LABEL" && e.textContent === arg);
      if (!label) throw new Error(`no checkbox labelled "${arg}"`);
      const box = label.childNodes[0];
      box.checked = !box.checked;
      box.dispatch("change");
    } else if (action === "go") {
      location.hash = "#" + arg;
    }
    await settle();
    console.log(`--- ${step}\n` + html(app));
  }
  const rules = document.getElementById("lipi-rules");
  if (rules) console.log("--- rules\n" + rules.textContent);
  const errors = all(body).filter((e) => e.getAttribute("class") === "lipi-error");
  for (const e of errors) console.log("--- error on the page\n" + e.textContent);
}

main().catch((e) => { console.error(e); process.exitCode = 1; });
