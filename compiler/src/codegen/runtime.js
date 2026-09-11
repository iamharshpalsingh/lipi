// ===== LiPi JavaScript runtime ======================================================
// Included in every `lipi build` bundle. It gives compiled programs the same
// behaviour as `lipi run`: Integers and Decimals, strict Booleans, structural
// equality, and the same error codes, messages, hints and source excerpts.
"use strict";

// ----- values -----------------------------------------------------------------------
class Dec { constructor(v) { this.v = v; } }
class LObj { constructor(f, t, m) { this.f = f || new Map(); this.t = t || null; this.m = m || null; } }
class LType { constructor(name, fields, methods) { this.name = name; this.fields = fields; this.methods = methods; } }
/// A JavaScript object reached through the `js` module.
class JsRef { constructor(v) { this.v = v; } }
class LipiError extends Error {
  constructor(value, diag, trace) { super(diag.message); this.lipi = value; this.diag = diag; this.trace = trace; }
}

const $rt = { target: "web", files: [], sites: [], frames: [], mods: [], cache: new Map(), loading: new Set() };
const MAX_DEPTH = 5000;

// Integers are JS numbers while they're exactly representable and BigInts
// beyond that, so they stay exact across the whole 64-bit range.
const isInt = (x) => typeof x === "number" || typeof x === "bigint";
const isDec = (x) => x instanceof Dec;
const isNum = (x) => isInt(x) || x instanceof Dec;
const nv = (x) => (x instanceof Dec ? x.v : typeof x === "bigint" ? Number(x) : x);
const $d = (v) => new Dec(v);
const MIN64 = -(2n ** 63n), MAX64 = 2n ** 63n - 1n;

function overflow(s) {
  $fail("LIP5009", "this Integer calculation overflowed", s, "Integers go up to 9223372036854775807. For bigger values, use Decimals (for example 1.0 * x).");
}

/// A BigInt result as an Integer: a plain number when it fits exactly.
function norm(b, s) {
  if (b < MIN64 || b > MAX64) overflow(s);
  return b >= -9007199254740991n && b <= 9007199254740991n ? Number(b) : b;
}

function iop(op, a, b, s) {
  if (typeof a === "number" && typeof b === "number") {
    const r = op === "+" ? a + b : op === "-" ? a - b : a * b;
    if (Number.isSafeInteger(r)) return r + 0;
  }
  const x = BigInt(a), y = BigInt(b);
  return norm(op === "+" ? x + y : op === "-" ? x - y : x * y, s);
}

function ipow(a, b, s) {
  if (typeof a === "number" && typeof b === "number") {
    const r = a ** b;
    if (Number.isSafeInteger(r)) return r;
  }
  const x = BigInt(a);
  let k = BigInt(b);
  if (x === 0n || x === 1n) return k === 0n ? 1 : Number(x);
  if (x === -1n) return k % 2n === 0n ? 1 : -1;
  let result = 1n, base = x;
  while (k > 0n) {
    if (k & 1n) { result *= base; if (result < MIN64 || result > MAX64) overflow(s); }
    k >>= 1n;
    if (k > 0n) { base *= base; if (base > MAX64 || base < MIN64) overflow(s); }
  }
  return norm(result, s);
}

function typeName(v) {
  if (v === null || v === undefined) return "Null";
  if (typeof v === "boolean") return "Boolean";
  if (typeof v === "number" || typeof v === "bigint") return "Integer";
  if (v instanceof Dec) return "Decimal";
  if (typeof v === "string") return "String";
  if (Array.isArray(v)) return "Array";
  if (v instanceof LObj) return v.t ? v.t.name : "Object";
  if (v instanceof LType) return "Type";
  if (typeof v === "function") return "Function";
  if (v instanceof Promise) return "Task";
  if (v instanceof JsRef) return "JsObject";
  return "Object";
}

function withArticle(t) { return /^[aeiou]/i.test(t) ? `an ${t}` : `a ${t}`; }

// ----- "did you mean" ---------------------------------------------------------------
/// Edit distance where swapping two neighbouring letters counts as one edit
/// ("nmae" is 1 away from "name"), the same as lipi_compiler::suggest.
function editDistance(a, b) {
  a = Array.from(a); b = Array.from(b);
  const d = Array.from({ length: a.length + 1 }, (_, i) => Array.from({ length: b.length + 1 }, (_, j) => (i === 0 ? j : j === 0 ? i : 0)));
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
    }
  }
  return d[a.length][b.length];
}
function toCamel(s) { return s.replace(/_+([a-z0-9])/g, (_, c) => c.toUpperCase()); }
function closest(name, candidates) {
  const n = Array.from(name).length;
  const limit = n <= 2 ? 1 : n <= 5 ? 2 : 3;
  const lower = name.toLowerCase(), camel = toCamel(name);
  let best = null;
  for (const c of candidates) {
    if (c === name) continue;
    const d = c === camel || c.toLowerCase() === lower ? 0 : editDistance(name, c);
    if (d <= limit && (!best || d < best[0] || (d === best[0] && c < best[1]))) best = [d, c];
  }
  return best ? best[1] : null;
}
function didYouMean(name, candidates) {
  const c = closest(name, candidates);
  if (!c) return null;
  return c === toCamel(name) && name.includes("_") ? `did you mean "${c}"? LiPi names use camelCase.` : `did you mean "${c}"?`;
}

// ----- errors and diagnostics ---------------------------------------------------------
const CATEGORIES = { 0: "Syntax", 1: "Name", 2: "Type", 3: "Module", 4: "Async", 5: "Runtime", 6: "Security", 7: "Package" };
const category = (code) => CATEGORIES[code ? code[3] : ""] || "Error";
const siteOf = (s) => (s === null || s === undefined ? null : $rt.sites[s]);
const fileOf = (site) => (site ? $rt.files[site.f].name : ($rt.files[0] ? $rt.files[0].name : "<main>"));
const traceNow = () => $rt.frames.map((f) => `${f.n}() called at ${f.file}:${f.line}`);

function errorObject(diag) {
  const site = siteOf(diag.site);
  return new LObj(new Map([
    ["message", diag.message], ["code", diag.code], ["category", category(diag.code)],
    ["hint", diag.hint === undefined ? null : diag.hint], ["line", site ? site.l : null], ["file", fileOf(site)],
  ]));
}

function $fail(code, message, site, hint) {
  const diag = { code, message, site, hint: hint === undefined ? null : hint };
  throw new LipiError(errorObject(diag), diag, traceNow());
}

function render(diag, trace) {
  let out = `ERROR ${diag.code}: ${diag.message}\n\n`;
  const site = siteOf(diag.site);
  if (site) {
    const file = $rt.files[site.f];
    out += `${file.name}:${site.l}:${site.c}\n`;
    const line = file.lines[site.l];
    if (line !== undefined) out += `    ${line.replace(/\t/g, "    ")}\n    ${" ".repeat(site.p)}${"^".repeat(site.w)}\n`;
  } else {
    out += `${fileOf(null)}\n`;
  }
  if (diag.hint) out += `\nHint: ${diag.hint}\n`;
  if (trace && trace.length) {
    out += "\n";
    for (const f of trace.slice().reverse().slice(0, 8)) out += `  in ${f}\n`;
    if (trace.length > 8) out += `  ... and ${trace.length - 8} more calls\n`;
  }
  return out;
}

function operandHint(site, ty, op) {
  const name = site && site.n;
  if (ty === "String" && op === "*") return 'To repeat text, use .repeat(n), for example: "-".repeat(20)';
  if (name && ty === "String") return `"${name}" is a String. Convert it to a number or use a numeric value.`;
  if (ty === "String") return "This is a String. Convert it with toNumber(...) first.";
  if (name && ty === "Null") return `"${name}" is null (it has no value yet). Give it a value first, or use ${name} ?? 0.`;
  if (name) return `"${name}" is ${withArticle(ty)}.`;
  return `This value is ${withArticle(ty)}.`;
}

const isNumName = (t) => t === "Integer" || t === "Decimal" || t === "Number";

function binaryError(op, l, r, s) {
  const S = siteOf(s), L = S.L, R = S.R;
  if (op === "+" && ((l === "String" && isNumName(r)) || (isNumName(l) && r === "String"))) {
    const t = l === "String" ? L : R;
    const msg = `cannot add ${l} and ${r}`;
    if (siteOf(t).n) $fail("LIP2001", msg, t, operandHint(siteOf(t), "String", null));
    $fail("LIP2001", msg, s, 'To put a value inside text, use interpolation, for example: "Total: {total}"');
  }
  if (op === "+") $fail("LIP2001", `cannot add ${l} and ${r}`, s, "+ works with two numbers, two Strings or two Arrays.");
  if (op === "-" || op === "*" || op === "/" || op === "%" || op === "**") {
    const msg = op === "-" ? `cannot subtract ${r} from ${l}` : op === "*" ? `cannot multiply ${l} by ${r}` : op === "**" ? `cannot raise ${l} to the power of ${r}` : `cannot divide ${l} by ${r}`;
    const leftBad = !isNumName(l);
    $fail("LIP2001", msg, leftBad ? L : R, operandHint(siteOf(leftBad ? L : R), leftBad ? l : r, op));
  }
  if (op === "<" || op === ">" || op === "<=" || op === ">=") $fail("LIP2001", `cannot compare ${l} with ${r}`, s, "< and > compare two numbers or two Strings.");
  $fail("LIP2001", `cannot check \`in\` ${withArticle(r)}`, R, "`in` works with Arrays, Strings and Objects.");
}

function $bool(v, s) {
  if (typeof v === "boolean") return v;
  const ty = typeName(v);
  const hints = {
    Null: "Compare with null explicitly, for example: if user != null",
    Array: "Check for items explicitly, for example: if not items.isEmpty()",
    String: 'Check the text explicitly, for example: if name != ""',
    Integer: "Compare it explicitly, for example: if count > 0",
    Decimal: "Compare it explicitly, for example: if count > 0",
    Object: "Compare with null or check a field, for example: if user != null",
  };
  $fail("LIP2005", `expected a Boolean (true or false), but this is ${withArticle(ty)}`, s, hints[ty] || "Conditions must be true or false.");
}

function expectBool(v, s, what) {
  if (typeof v === "boolean") return v;
  $fail("LIP2005", `${what} must return true or false, but it returned ${withArticle(typeName(v))}`, s, "Return a comparison, for example: items.filter(x => x > 3)");
}

// ----- equality and printing ---------------------------------------------------------
function eq(a, b) {
  if (a instanceof JsRef) return b instanceof JsRef && a.v === b.v;
  if (a === null || a === undefined) return b === null || b === undefined;
  if (typeof a === "boolean" || typeof a === "string") return a === b;
  if (isNum(a)) return isNum(b) && (isInt(a) && isInt(b) ? a == b : nv(a) === nv(b));
  if (Array.isArray(a)) return Array.isArray(b) && (a === b || (a.length === b.length && a.every((x, i) => eq(x, b[i]))));
  if (a instanceof LObj) {
    if (!(b instanceof LObj)) return false;
    if (a === b) return true;
    if (a.t !== b.t || a.f.size !== b.f.size) return false;
    for (const [k, v] of a.f) if (!b.f.has(k) || !eq(v, b.f.get(k))) return false;
    return true;
  }
  return a === b;
}

function fmtDec(n) {
  if (Number.isNaN(n)) return "NaN";
  if (!Number.isFinite(n)) return n > 0 ? "infinity" : "-infinity";
  if (Number.isInteger(n) && Math.abs(n) < 1e16) return (Object.is(n, -0) ? "-" : "") + n.toFixed(1);
  return String(n);
}

function quote(s) {
  let out = '"';
  for (const c of s) out += c === '"' ? '\\"' : c === "\\" ? "\\\\" : c === "\n" ? "\\n" : c === "\t" ? "\\t" : c === "\r" ? "\\r" : c;
  return out + '"';
}

function repr(v, depth = 0) {
  if (depth > 40) return "...";
  if (v === null || v === undefined) return "null";
  if (typeof v === "boolean") return v ? "true" : "false";
  if (isInt(v)) return String(v);
  if (v instanceof Dec) return fmtDec(v.v);
  if (typeof v === "string") return quote(v);
  if (Array.isArray(v)) return `[${v.map((x) => repr(x, depth + 1)).join(", ")}]`;
  if (v instanceof LObj) {
    if (v.m) return `<module ${v.m}>`;
    const prefix = v.t ? `${v.t.name} ` : "";
    if (v.f.size === 0) return `${prefix}{}`;
    const parts = [];
    for (const [k, x] of v.f) parts.push(`${/^[A-Za-z_][A-Za-z0-9_]*$/.test(k) ? k : quote(k)}: ${repr(x, depth + 1)}`);
    return `${prefix}{${parts.join(", ")}}`;
  }
  if (v instanceof LType) return `<type ${v.name}>`;
  if (typeof v === "function") {
    if (v.$bm) return `<method ${v.$bm}>`;
    if (v.$m) return v.$m.l ? "<function>" : `<function ${v.$m.n}>`;
    return `<function ${v.$native || v.name || ""}>`;
  }
  if (v instanceof Promise) return v.$done ? "<task done>" : "<task running>";
  if (v instanceof JsRef) return jsDescribe(v.v);
  return String(v);
}

const display = (v) => (typeof v === "string" ? v : repr(v));
const $tpl = (parts) => parts.map(display).join("");

function $show(values) {
  const line = values.map(display).join(" ");
  if ($rt.target === "node") process.stdout.write(line + "\n");
  else {
    console.log(line);
    const out = typeof document !== "undefined" && document.getElementById("lipi-output");
    if (out) { out.hidden = false; out.textContent += line + "\n"; }
  }
}

// ----- arithmetic and operators --------------------------------------------------------
function divZero(s) { $fail("LIP5002", "cannot divide by zero", siteOf(s).R, "Check that the number you divide by isn't 0 first."); }

function $bin(op, a, b, s) {
  switch (op) {
    case "==": return eq(a, b);
    case "!=": return !eq(a, b);
    case "+":
      if (isInt(a) && isInt(b)) return iop("+", a, b, s);
      if (isNum(a) && isNum(b)) return new Dec(nv(a) + nv(b));
      if (typeof a === "string" && typeof b === "string") return a + b;
      if (Array.isArray(a) && Array.isArray(b)) return a.concat(b);
      break;
    case "-":
    case "*":
      if (isInt(a) && isInt(b)) return iop(op, a, b, s);
      if (isNum(a) && isNum(b)) return new Dec(op === "-" ? nv(a) - nv(b) : nv(a) * nv(b));
      break;
    case "/":
      if (isNum(a) && isNum(b)) { if (nv(b) === 0) divZero(s); return new Dec(nv(a) / nv(b)); }
      break;
    case "%":
      if (isNum(a) && isNum(b)) {
        if (nv(b) === 0) divZero(s);
        if (typeof a === "number" && typeof b === "number") { const m = a % b; return m !== 0 && m < 0 !== b < 0 ? m + b : m + 0; }
        if (isInt(a) && isInt(b)) {
          const x = BigInt(a), y = BigInt(b);
          let m = x % y;
          if (m !== 0n && m < 0n !== y < 0n) m += y;
          return norm(m, s);
        }
        const x = nv(a), y = nv(b);
        return new Dec(x - y * Math.floor(x / y));
      }
      break;
    case "**":
      if (isInt(a) && isInt(b) && b >= 0) return ipow(a, b, s);
      if (isNum(a) && isNum(b)) return new Dec(Math.pow(nv(a), nv(b)));
      break;
    case "<": case ">": case "<=": case ">=":
      if ((isNum(a) && isNum(b)) || (typeof a === "string" && typeof b === "string")) {
        const x = isNum(a) ? nv(a) : a, y = isNum(b) ? nv(b) : b;
        return op === "<" ? x < y : op === ">" ? x > y : op === "<=" ? x <= y : x >= y;
      }
      break;
    case "in": case "not in": {
      let r;
      if (Array.isArray(b)) r = b.some((x) => eq(x, a));
      else if (typeof a === "string" && typeof b === "string") r = b.includes(a);
      else if (typeof a === "string" && b instanceof LObj) r = b.f.has(a);
      else break;
      return op === "in" ? r : !r;
    }
  }
  binaryError(op, typeName(a), typeName(b), s);
}

function $neg(v, s) {
  if (typeof v === "number") return 0 - v;
  if (typeof v === "bigint") return norm(-v, s);
  if (isDec(v)) return new Dec(-v.v);
  const ty = typeName(v);
  $fail("LIP2001", `cannot negate ${withArticle(ty)}`, s, operandHint(siteOf(s), ty, null));
}

const $nn = (a, b) => (a === null || a === undefined ? b() : a);

function $obj(pairs) { return new LObj(new Map(pairs)); }

function rangeInt(v, s) {
  if (isInt(v)) return Number(v);
  $fail("LIP2001", "ranges need Integers", s, operandHint(siteOf(s), typeName(v), null));
}

function $rangeParts(a, b, step, sa, sb, ss) {
  const from = rangeInt(a, sa), to = rangeInt(b, sb);
  let st = to >= from ? 1 : -1;
  if (step !== null && step !== undefined) {
    st = rangeInt(step, ss);
    if (st === 0) $fail("LIP5008", "a range's step can't be 0", ss);
  }
  return [from, to, st];
}

function $range(a, b, step, sa, sb, ss, s) {
  const [from, to, st] = $rangeParts(a, b, step, sa, sb, ss);
  if ((to - from) / st > 10000000) $fail("LIP5008", "this range is too big to turn into an Array", s, "Loop over it directly with `for i in a to b` instead.");
  const out = [];
  for (let i = from; st > 0 ? i <= to : i >= to; i += st) out.push(i);
  return out;
}

function $inr(v, a, b) {
  if (!isNum(v) || !isNum(a) || !isNum(b)) return false;
  const x = nv(v), lo = Math.min(nv(a), nv(b)), hi = Math.max(nv(a), nv(b));
  return x >= lo && x <= hi;
}

function $count(v, s) {
  if (isInt(v) && v >= 0) return Number(v);
  if (isInt(v)) $fail("LIP5008", "`repeat` needs an Integer that isn't negative", s);
  $fail("LIP2001", "`repeat` needs an Integer", s, operandHint(siteOf(s), typeName(v), null));
}

function $pairs(v, s) {
  if (Array.isArray(v)) return { keyed: false, items: v.map((x, i) => [i, x]) };
  if (typeof v === "string") return { keyed: false, items: Array.from(v).map((c, i) => [i, c]) };
  if (v instanceof LObj) return { keyed: true, items: Array.from(v.f.entries()) };
  $fail("LIP2001", `cannot loop over ${withArticle(typeName(v))}`, s, "Loop over an Array, a String, an Object or a range like 1 to 10.");
}

// ----- types -------------------------------------------------------------------------
function typeStr(t) {
  if (typeof t === "string") return t;
  if (t.list) return `Array[${typeStr(t.list)}]`;
  return `${typeStr(t.opt)}?`;
}

function matches(v, t) {
  if (typeof t !== "string") {
    if (t.opt) return v === null || matches(v, t.opt);
    return Array.isArray(v) && v.every((x) => matches(x, t.list));
  }
  switch (t) {
    case "Any": return true;
    case "Integer": return isInt(v);
    case "Decimal": case "Number": return isNum(v);
    case "String": return typeof v === "string";
    case "Boolean": return typeof v === "boolean";
    case "Null": return v === null;
    case "Array": return Array.isArray(v);
    case "Object": return v instanceof LObj;
    case "Function": return typeof v === "function" || v instanceof LType;
    case "Task": return v instanceof Promise;
    case "JsObject": return v instanceof JsRef;
    default: return v instanceof LObj && v.t !== null && v.t.name === t;
  }
}

function $chk(v, t, name, s) {
  if (matches(v, t)) return v;
  $fail("LIP2002", `"${name}" should be ${withArticle(typeStr(t))}, but this is ${withArticle(typeName(v))}`, s, `"${name}" was declared as ${typeStr(t)}.`);
}

function $type(name, fields, methods) { return new LType(name, fields, methods); }

function construct(t, pos, named, s) {
  const list = () => t.fields.map((f) => f.n).join(", ");
  if (pos.length > t.fields.length) {
    const n = t.fields.length;
    $fail("LIP2003", `"${t.name}" has ${n} field${n === 1 ? "" : "s"}, but ${pos.length} values were given`, s, `Its fields are: ${list()}`);
  }
  const values = pos.slice();
  if (named) for (const [k, v] of Object.entries(named)) {
    const i = t.fields.findIndex((f) => f.n === k);
    if (i < 0) $fail("LIP1007", `"${t.name}" has no field "${k}"`, s, didYouMean(k, t.fields.map((f) => f.n)) || `Its fields are: ${list()}`);
    values[i] = v;
  }
  const fields = new Map();
  t.fields.forEach((f, i) => {
    let v = values[i];
    if (v === undefined) {
      if (f.d) v = f.d();
      else if (f.o) v = null;
      else $fail("LIP2003", `missing "${f.n}" when creating ${withArticle(t.name)}`, s, `For example: ${t.name}(${f.n}: ...)`);
    }
    if (f.t && !matches(v, f.t)) $fail("LIP2002", `"${f.n}" should be ${withArticle(typeStr(f.t))}, but this is ${withArticle(typeName(v))}`, s, `"${f.n}" was declared as ${typeStr(f.t)}.`);
    fields.set(f.n, v);
  });
  return new LObj(fields, t);
}

// ----- functions and calls -----------------------------------------------------------
function $fn(meta, impl) { impl.$m = meta; return impl; }
function nat(name, f) { f.$native = name; return f; }
const sig = (m) => `${m.n}(${m.p.map((p) => p[0]).join(", ")})`;
const fname = (m) => (m.l ? "this function" : `"${m.n}"`);

function $call(f, pos, named, s) {
  if (typeof f === "function" && f.$m) return callFn(f, pos, named, s);
  if (typeof f === "function" && f.$native) return f(pos, named || null, s);
  if (f instanceof LType) return construct(f, pos, named, s);
  const site = siteOf(s);
  const what = site && site.n ? `"${site.n}"` : "this value";
  $fail("LIP2008", `${what} is ${withArticle(typeName(f))}, not a function`, site && site.C !== undefined ? site.C : s, "Only functions can be called with ( ).");
}

function callFn(f, pos, named, s) {
  const m = f.$m, params = m.p;
  // `key:` gives a component its identity on the page; it isn't a parameter.
  let key;
  if (m.c && named && "key" in named && !params.some((p) => p[0] === "key")) {
    key = named.key;
    named = { ...named };
    delete named.key;
  }
  if ($rt.frames.length >= MAX_DEPTH) $fail("LIP5005", `too much recursion: ${fname(m)} called itself too many times`, s, "Make sure the recursion has a case where it stops calling itself.");
  if (pos.length > params.length) {
    const n = params.length;
    $fail("LIP2003", `${fname(m)} takes ${n} argument${n === 1 ? "" : "s"}, but ${pos.length} were given`, s, `It is defined as ${sig(m)}.`);
  }
  const args = pos.slice();
  while (args.length < params.length) args.push(undefined);
  if (named) for (const [k, v] of Object.entries(named)) {
    const i = params.findIndex((p) => p[0] === k);
    if (i < 0) $fail("LIP1007", `${fname(m)} has no parameter named "${k}"`, s, didYouMean(k, params.map((p) => p[0])) || `It is defined as ${sig(m)}.`);
    if (args[i] !== undefined) $fail("LIP2003", `the argument "${k}" was given twice`, s);
    args[i] = v;
  }
  for (let i = 0; i < params.length; i++) {
    const [pn, hasDefault, ty] = params[i];
    if (args[i] === undefined) {
      if (!hasDefault) $fail("LIP2003", `missing argument "${pn}" for ${fname(m)}`, s, `It is defined as ${sig(m)}.`);
    } else if (ty && !matches(args[i], ty)) {
      $fail("LIP2004", `"${pn}" should be ${withArticle(typeStr(ty))}, but this is ${withArticle(typeName(args[i]))}`, s, `${fname(m)} expects "${pn}" to be ${typeStr(ty)}.`);
    }
  }
  const site = siteOf(s);
  $rt.frames.push({ n: m.n === "<block>" ? "<block>" : m.l ? "<function>" : m.n, file: fileOf(site), line: site ? site.l : 0 });
  if (m.c) $ui.nextKey = key;
  let result;
  try {
    result = f.apply(null, args);
  } catch (e) {
    if (e instanceof RangeError && !(e instanceof LipiError)) $fail("LIP5005", `too much recursion: ${fname(m)} called itself too many times`, s, "Make sure the recursion has a case where it stops calling itself.");
    throw e;
  } finally {
    $rt.frames.pop();
  }
  if (m.a) {
    if (!m.r || !(result instanceof Promise)) return track(result);
    return track(result.then((v) => {
      if (!matches(v, m.r)) $fail("LIP2007", `${fname(m)} should return ${withArticle(typeStr(m.r))}, but it returned ${withArticle(typeName(v))}`, m.rs, `The definition says it returns ${typeStr(m.r)}.`);
      return v;
    }));
  }
  if (m.r && !matches(result, m.r)) $fail("LIP2007", `${fname(m)} should return ${withArticle(typeStr(m.r))}, but it returned ${withArticle(typeName(result))}`, m.rs, `The definition says it returns ${typeStr(m.r)}.`);
  return result;
}

/// Call a callback, dropping arguments it doesn't declare (like `lipi run`).
function cb(f, args, s) {
  if (typeof f === "function" && f.$m) args = args.slice(0, f.$m.p.length);
  return $call(f, args, null, s);
}

function bound(recv, name) {
  const f = nat(name, (pos, named, s) => $mc(recv, name, pos, named, s, false));
  f.$bm = name;
  return f;
}

function bindMethod(obj, name, impl) {
  const m = impl.$m;
  return $fn({ ...m }, (...args) => impl(obj, ...args));
}

// ----- tasks -----------------------------------------------------------------------
function track(p) {
  if (p instanceof Promise && p.$done === undefined) {
    p.$done = false;
    p.then(() => { p.$done = true; }, () => { p.$done = true; });
  }
  return p;
}

// ----- fields, indexes and methods ------------------------------------------------------
const STRING_MEMBERS = ["length", "upper", "lower", "trim", "trimStart", "trimEnd", "split", "contains", "startsWith", "endsWith", "replace", "indexOf", "slice", "repeat", "chars", "lines", "isEmpty", "padStart", "padEnd", "reverse", "toNumber"];
const LIST_MEMBERS = ["length", "first", "last", "push", "pop", "insert", "removeAt", "remove", "contains", "indexOf", "join", "map", "filter", "reduce", "each", "find", "any", "all", "sort", "sortBy", "reverse", "slice", "sum", "min", "max", "isEmpty", "copy", "unique", "flat", "count"];
const OBJECT_MEMBERS = ["keys", "values", "entries", "has", "get", "remove", "copy", "length", "isEmpty"];
const NUMBER_MEMBERS = ["round", "floor", "ceil", "abs", "toString"];
const TASK_MEMBERS = ["cancel", "isDone"];
const STD_MODULES = ["math", "json", "fs", "env", "http", "time", "process", "server", "crypto", "database"];

function membersFor(v) {
  if (typeof v === "string") return ["String", STRING_MEMBERS];
  if (Array.isArray(v)) return ["Array", LIST_MEMBERS];
  if (isInt(v)) return ["Integer", NUMBER_MEMBERS];
  if (isDec(v)) return ["Decimal", NUMBER_MEMBERS];
  if (v instanceof Promise) return ["Task", TASK_MEMBERS];
  if (v instanceof LObj) return ["Object", OBJECT_MEMBERS];
  return null;
}

function unknownMember(ty, name, s, members) {
  $fail("LIP1004", `${ty}s don't have "${name}"`, s, didYouMean(name, members) || `Available: ${members.join(", ")}`);
}

function property(v, name) {
  if (typeof v === "string" && name === "length") return { v: Array.from(v).length };
  if (Array.isArray(v)) {
    if (name === "length") return { v: v.length };
    if (name === "first") return { v: v.length ? v[0] : null };
    if (name === "last") return { v: v.length ? v[v.length - 1] : null };
  }
  return null;
}

function nullHint(site, what) {
  const o = site && site.o;
  return o ? `"${o}" is null. Use ${o}?${what} to get null instead of an error.` : `Use ?${what} to get null instead of an error when the value is null.`;
}

function missingField(o, name, s) {
  const available = Array.from(o.f.keys());
  if (o.t) available.push(...Object.keys(o.t.methods));
  const suggestion = didYouMean(name, available);
  if (o.m && STD_MODULES.includes(o.m)) $fail("LIP1004", `the ${o.m} module has no "${name}"`, s, suggestion || `Available: ${available.join(", ")}`);
  if (o.m) $fail("LIP3003", `module "${o.m}" doesn't export "${name}"`, s, suggestion || `If "${name}" is defined in ${o.m}.lipi, add: export ${name}`);
  if (o.t) $fail("LIP5004", `"${o.t.name}" has no field or method "${name}"`, s, suggestion || `Available: ${available.join(", ")}`);
  $fail("LIP5004", `this Object has no field "${name}"`, s, suggestion || `If the field may be missing, use obj.get("${name}") or obj["${name}"], which give null instead of an error.`);
}

function $get(obj, name, s, optional) {
  if (obj === null || obj === undefined) {
    if (optional) return null;
    $fail("LIP5003", `cannot read ".${name}" of null`, s, nullHint(siteOf(s), `.${name}`));
  }
  if (obj instanceof LObj) {
    if (obj.f.has(name)) return obj.f.get(name);
    if (obj.t && obj.t.methods[name]) return bindMethod(obj, name, obj.t.methods[name]);
    if (!obj.m) {
      if (name === "length") return obj.f.size;
      if (OBJECT_MEMBERS.includes(name)) return bound(obj, name);
    }
    missingField(obj, name, s);
  }
  if (obj instanceof JsRef) return jsGet(obj, name, s);
  if (obj instanceof LType) $fail("LIP5004", `"${obj.name}" is a type; create one first to use ".${name}"`, s, `For example: item = ${obj.name}(...) and then item.${name}`);
  const p = property(obj, name);
  if (p) return p.v;
  const m = membersFor(obj);
  if (m && m[1].includes(name)) return bound(obj, name);
  if (m) unknownMember(m[0], name, s, m[1]);
  $fail("LIP1004", `${withArticle(typeName(obj))} has no ".${name}"`, s);
}

function $getl(obj, name, s, optional) {
  if (obj instanceof LObj && !obj.t && !obj.m && !obj.f.has(name)) return null;
  return $get(obj, name, s, optional);
}

function $set(obj, name, v, s) {
  if (obj instanceof LObj) {
    if (obj.m) $fail("LIP5008", "modules can't be changed from outside", s);
    if (obj.t) {
      const f = obj.t.fields.find((x) => x.n === name);
      if (!f) {
        const names = obj.t.fields.map((x) => x.n);
        $fail("LIP5004", `"${obj.t.name}" has no field "${name}"`, s, didYouMean(name, names) || `Its fields are: ${names.join(", ")}`);
      }
      if (f.t) $chk(v, f.t, name, siteOf(s).V ?? s);
    }
    obj.f.set(name, v);
    $rt.changed();
    return;
  }
  if (obj instanceof JsRef) return jsSet(obj, name, v, s);
  if (obj === null || obj === undefined) $fail("LIP5003", `cannot set ".${name}" on null`, s, nullHint(siteOf(s), `.${name}`));
  $fail("LIP5008", `cannot set a field on ${withArticle(typeName(obj))}`, s);
}

function position(index, len, s) {
  if (!isInt(index)) $fail("LIP2001", "positions in Arrays and Strings must be Integers", s, operandHint(siteOf(s), typeName(index), null));
  const i = typeof index === "bigint" ? -1 : index < 0 ? len + index : index;
  if (i < 0 || i >= len) {
    const hint = len === 0 ? "The Array is empty. Add items with .push(item) first." : `Positions start at 0, so the last one is ${len - 1}. Negative positions count from the end: -1 is the last item.`;
    $fail("LIP5001", `index ${index} is outside array length ${len}`, s, hint);
  }
  return i;
}

function $idx(obj, index, s) {
  if (obj instanceof JsRef) return jsGet(obj, index, s);
  if (Array.isArray(obj)) return obj[position(index, obj.length, s)];
  if (typeof obj === "string") { const chars = Array.from(obj); return chars[position(index, chars.length, s)]; }
  if (obj instanceof LObj) {
    if (typeof index === "string") return obj.f.has(index) ? obj.f.get(index) : null;
    $fail("LIP2001", `Object keys are Strings, not ${withArticle(typeName(index))}`, s);
  }
  if (obj === null || obj === undefined) $fail("LIP5003", "cannot read an item from null", s, nullHint(siteOf(s), "[...]"));
  $fail("LIP2001", `cannot use [ ] on ${withArticle(typeName(obj))}`, s);
}

function $seti(obj, index, v, s) {
  if (obj instanceof JsRef) return jsSet(obj, index, v, s);
  if (Array.isArray(obj)) {
    if (index === obj.length) $fail("LIP5001", `index ${obj.length} is just past the end of the array`, s, "To add an item to the end, use .push(item).");
    obj[position(index, obj.length, s)] = v;
    $rt.changed();
    return;
  }
  if (obj instanceof LObj) {
    if (typeof index !== "string") $fail("LIP2001", `Object keys are Strings, not ${withArticle(typeName(index))}`, s);
    if (obj.t || obj.m) return $set(obj, index, v, s);
    obj.f.set(index, v);
    $rt.changed();
    return;
  }
  if (typeof obj === "string") $fail("LIP5008", "Strings can't be changed in place", s, "Build a new String instead, for example with .replace() or .slice().");
  if (obj === null || obj === undefined) $fail("LIP5003", "cannot set an item on null", s, nullHint(siteOf(s), "[...]"));
  $fail("LIP2001", `cannot use [ ] on ${withArticle(typeName(obj))}`, s);
}

function $exp(module, name, source, s) {
  if (module instanceof LObj && module.f.has(name)) return module.f.get(name);
  const available = module instanceof LObj ? Array.from(module.f.keys()) : [];
  $fail("LIP3003", `"${source}" doesn't export "${name}"`, s, didYouMean(name, available) || `If "${name}" is defined there, add \`export ${name}\` to that file.`);
}

// Argument helpers for built-ins.
function arg(a, named, i, name) { return named && name in named ? named[name] : a[i]; }
function need(fn, a, named, i, name, s) {
  const v = arg(a, named, i, name);
  if (v === undefined) $fail("LIP5008", `${fn}() needs "${name}"`, s);
  return v;
}
function wrong(fn, name, expected, got, s) {
  $fail("LIP5008", `${fn}() expects "${name}" to be ${withArticle(expected)}, but got ${withArticle(typeName(got))}`, s);
}
function argInt(fn, a, named, i, name, s) { const v = need(fn, a, named, i, name, s); if (!isInt(v)) wrong(fn, name, "Integer", v, s); return Number(v); }
function optInt(fn, a, named, i, name, s) { const v = arg(a, named, i, name); if (v === undefined || v === null) return null; if (!isInt(v)) wrong(fn, name, "Integer", v, s); return Number(v); }
function argNum(fn, a, named, i, name, s) { const v = need(fn, a, named, i, name, s); if (!isNum(v)) wrong(fn, name, "Number", v, s); return nv(v); }
function optNum(fn, a, named, i, name, s) { const v = arg(a, named, i, name); if (v === undefined || v === null) return null; if (!isNum(v)) wrong(fn, name, "Number", v, s); return nv(v); }
function argStr(fn, a, named, i, name, s) { const v = need(fn, a, named, i, name, s); if (typeof v !== "string") wrong(fn, name, "String", v, s); return v; }
function optStr(fn, a, named, i, name, s) { const v = arg(a, named, i, name); if (v === undefined || v === null) return null; if (typeof v !== "string") wrong(fn, name, "String", v, s); return v; }
function argFn(fn, a, named, i, name, s) {
  const v = need(fn, a, named, i, name, s);
  if (typeof v === "function" || v instanceof LType) return v;
  $fail("LIP5008", `${fn}() expects a function, but got ${withArticle(typeName(v))}`, s, `For example: items.${fn}(item => item * 2)`);
}

function bounds(len, start, end) {
  const norm = (x) => { const y = x < 0 ? len + x : x; return Math.max(0, Math.min(len, y)); };
  const s = norm(start === null ? 0 : start), e = norm(end === null ? len : end);
  return [s, Math.max(s, e)];
}

/// A whole-number Decimal result (from floor, round...) as an Integer when it fits.
function whole(x) {
  if (!Number.isFinite(x) || !Number.isInteger(x) || Math.abs(x) >= 9.2e18) return new Dec(x);
  return Number.isSafeInteger(x) ? x + 0 : BigInt(x);
}

function compareValues(x, y) {
  if (typeof x === "string" && typeof y === "string") return x < y ? -1 : x > y ? 1 : 0;
  const a = nv(x), b = nv(y);
  return a < b ? -1 : a > b ? 1 : 0;
}

function checkSortable(keys, s) {
  if (keys.every(isNum) || keys.every((k) => typeof k === "string")) return;
  const kinds = Array.from(new Set(keys.map(typeName))).sort();
  $fail("LIP5008", `cannot sort values that are ${kinds.join(" and ")}`, s, "sort() works on all-number or all-String Arrays. Use sortBy(item => ...) to choose what to sort by.");
}

function listPos(a, named, i, name, len, allowEnd, s, fn) {
  const idx = argInt(fn, a, named, i, name, s);
  const j = idx < 0 ? len + idx : idx;
  const max = allowEnd ? len : len - 1;
  if (j < 0 || j > max) $fail("LIP5001", `index ${idx} is outside array length ${len}`, s, "Positions start at 0. Negative positions count from the end: -1 is the last item.");
  return j;
}

function stringMethod(str, name, a, n, s) {
  const chars = () => Array.from(str);
  switch (name) {
    case "upper": return str.toUpperCase();
    case "lower": return str.toLowerCase();
    case "trim": return str.trim();
    case "trimStart": return str.trimStart();
    case "trimEnd": return str.trimEnd();
    case "split": {
      const sep = optStr(name, a, n, 0, "separator", s);
      if (sep === null) return str.split(/\s+/).filter((x) => x !== "");
      if (sep === "") return chars();
      return str.split(sep);
    }
    case "contains": return str.includes(argStr(name, a, n, 0, "text", s));
    case "startsWith": return str.startsWith(argStr(name, a, n, 0, "text", s));
    case "endsWith": return str.endsWith(argStr(name, a, n, 0, "text", s));
    case "replace": return str.split(argStr(name, a, n, 0, "old", s)).join(argStr(name, a, n, 1, "new", s));
    case "indexOf": { const i = str.indexOf(argStr(name, a, n, 0, "text", s)); return i < 0 ? null : Array.from(str.slice(0, i)).length; }
    case "slice": { const c = chars(); const [st, en] = bounds(c.length, optInt(name, a, n, 0, "start", s), optInt(name, a, n, 1, "end", s)); return c.slice(st, en).join(""); }
    case "repeat": { const k = argInt(name, a, n, 0, "count", s); if (k < 0) $fail("LIP5008", "repeat() needs a count that isn't negative", s); return str.repeat(k); }
    case "chars": return chars();
    case "lines": { const ls = str.split("\n"); if (ls.length && ls[ls.length - 1] === "") ls.pop(); return ls.map((l) => l.replace(/\r$/, "")); }
    case "isEmpty": return str.length === 0;
    case "padStart": case "padEnd": {
      const w = Math.max(0, argInt(name, a, n, 0, "width", s));
      const fill = optStr(name, a, n, 1, "fill", s) ?? " ";
      const len = chars().length;
      if (len >= w || fill === "") return str;
      const pad = Array.from(fill.repeat(w)).slice(0, w - len).join("");
      return name === "padStart" ? pad + str : str + pad;
    }
    case "reverse": return chars().reverse().join("");
    case "toNumber": return toNumber(str);
  }
  return undefined;
}

function listMethod(l, name, a, n, s) {
  const what = `the function given to ${name}()`;
  switch (name) {
    case "push": if (a.length === 0) $fail("LIP5008", "push() needs an item to add", s); l.push(...a); $rt.changed(); return null;
    case "pop": if (l.length === 0) $fail("LIP5001", "cannot pop from an empty Array", s, "Check .isEmpty() first."); { const v = l.pop(); $rt.changed(); return v; }
    case "insert": { const i = listPos(a, n, 0, "position", l.length, true, s, name); l.splice(i, 0, need(name, a, n, 1, "item", s)); $rt.changed(); return null; }
    case "removeAt": { const i = listPos(a, n, 0, "position", l.length, false, s, name); const v = l.splice(i, 1)[0]; $rt.changed(); return v; }
    case "remove": { const item = need(name, a, n, 0, "item", s); const i = l.findIndex((x) => eq(x, item)); if (i < 0) return false; l.splice(i, 1); $rt.changed(); return true; }
    case "contains": { const item = need(name, a, n, 0, "item", s); return l.some((x) => eq(x, item)); }
    case "indexOf": { const item = need(name, a, n, 0, "item", s); const i = l.findIndex((x) => eq(x, item)); return i < 0 ? null : i; }
    case "join": return l.map(display).join(optStr(name, a, n, 0, "separator", s) ?? "");
    case "map": { const f = argFn(name, a, n, 0, "function", s); return l.slice().map((x, i) => cb(f, [x, i], s)); }
    case "filter": { const f = argFn(name, a, n, 0, "function", s); return l.slice().filter((x, i) => expectBool(cb(f, [x, i], s), s, what)); }
    case "each": { const f = argFn(name, a, n, 0, "function", s); l.slice().forEach((x, i) => cb(f, [x, i], s)); return null; }
    case "reduce": {
      const f = argFn(name, a, n, 0, "function", s);
      const items = l.slice();
      let acc;
      const start = arg(a, n, 1, "start");
      if (start !== undefined) acc = start;
      else { if (items.length === 0) return null; acc = items.shift(); }
      for (const x of items) acc = cb(f, [acc, x], s);
      return acc;
    }
    case "find": { const f = argFn(name, a, n, 0, "function", s); for (const x of l.slice()) if (expectBool(cb(f, [x], s), s, what)) return x; return null; }
    case "any": case "all": {
      const f = argFn(name, a, n, 0, "function", s);
      const wantAny = name === "any";
      for (const x of l.slice()) if (expectBool(cb(f, [x], s), s, what) === wantAny) return wantAny;
      return !wantAny;
    }
    case "count": {
      if (a.length === 0 && !n) return l.length;
      const f = argFn(name, a, n, 0, "function", s);
      return l.slice().filter((x) => expectBool(cb(f, [x], s), s, what)).length;
    }
    case "sort": { const items = l.slice(); checkSortable(items, s); return items.sort(compareValues); }
    case "sortBy": {
      const f = argFn(name, a, n, 0, "function", s);
      const pairs = l.slice().map((x) => [cb(f, [x], s), x]);
      checkSortable(pairs.map((p) => p[0]), s);
      return pairs.sort((p, q) => compareValues(p[0], q[0])).map((p) => p[1]);
    }
    case "reverse": return l.slice().reverse();
    case "slice": { const [st, en] = bounds(l.length, optInt(name, a, n, 0, "start", s), optInt(name, a, n, 1, "end", s)); return l.slice(st, en); }
    case "sum": {
      let total = 0, dec = false, f = 0;
      l.forEach((x, i) => {
        if (!isNum(x)) $fail("LIP5008", `sum() needs an Array of numbers, but item ${i} is ${withArticle(typeName(x))}`, s);
        if (dec || isDec(x)) {
          if (!dec) { dec = true; f = nv(total); }
          f += nv(x);
        } else total = iop("+", total, x, s);
      });
      return dec ? new Dec(f) : total;
    }
    case "min": case "max": {
      checkSortable(l, s);
      if (l.length === 0) return null;
      return l.reduce((x, y) => { const c = compareValues(y, x); return (name === "min" && c < 0) || (name === "max" && c > 0) ? y : x; });
    }
    case "isEmpty": return l.length === 0;
    case "copy": return l.slice();
    case "unique": { const out = []; for (const x of l) if (!out.some((y) => eq(x, y))) out.push(x); return out; }
    case "flat": return l.flatMap((x) => (Array.isArray(x) ? x : [x]));
  }
  return undefined;
}

function numberMethod(v, name, a, n, s) {
  const x = nv(v);
  switch (name) {
    case "round": {
      if (isInt(v) && a.length === 0) return v;
      const d = optInt(name, a, n, 0, "digits", s);
      if (d === null) return whole(Math.round(x));
      const f = Math.pow(10, d);
      return new Dec(Math.round(x * f) / f);
    }
    case "floor": return isInt(v) ? v : whole(Math.floor(x));
    case "ceil": return isInt(v) ? v : whole(Math.ceil(x));
    case "abs": return isInt(v) ? (v < 0 ? $neg(v, s) : v) : new Dec(Math.abs(x));
    case "toString": return display(v);
  }
  return undefined;
}

function objectMethod(o, name, a, n, s) {
  switch (name) {
    case "keys": return Array.from(o.f.keys());
    case "values": return Array.from(o.f.values());
    case "entries": return Array.from(o.f.entries()).map(([k, v]) => [k, v]);
    case "has": return o.f.has(argStr(name, a, n, 0, "key", s));
    case "get": { const k = argStr(name, a, n, 0, "key", s); if (o.f.has(k)) return o.f.get(k); const d = arg(a, n, 1, "default"); return d === undefined ? null : d; }
    case "remove": {
      const k = argStr(name, a, n, 0, "key", s);
      if (o.t) $fail("LIP5008", "cannot remove a field from a typed Object", s, "Set it to null instead.");
      const v = o.f.has(k) ? o.f.get(k) : null;
      o.f.delete(k);
      $rt.changed();
      return v;
    }
    case "copy": return new LObj(new Map(o.f), o.t);
    case "isEmpty": return o.f.size === 0;
  }
  return undefined;
}

function taskMethod(t, name) {
  if (name === "cancel") { const running = !t.$done; t.$cancelled = true; return running; }
  if (name === "isDone") return !!t.$done;
  return undefined;
}

function $mc(obj, name, pos, named, s, optional) {
  const site = siteOf(s);
  const nameSite = site && site.N !== undefined ? site.N : s;
  if (obj === null || obj === undefined) {
    if (optional) return null;
    $fail("LIP5003", `cannot call ".${name}()" on null`, nameSite, nullHint(siteOf(nameSite), `.${name}()`));
  }
  if (obj instanceof LObj) {
    if (obj.f.has(name)) return $call(obj.f.get(name), pos, named, s);
    if (obj.t && obj.t.methods[name]) return callFn(bindMethod(obj, name, obj.t.methods[name]), pos, named, s);
    if (!obj.m) {
      if (name === "length") $fail("LIP5008", `"${name}" is a property, not a method`, s, `Write it without parentheses: .${name}`);
      const r = objectMethod(obj, name, pos, named, s);
      if (r !== undefined) return r;
    }
    missingField(obj, name, nameSite);
  }
  if (property(obj, name)) $fail("LIP5008", `"${name}" is a property, not a method`, s, `Write it without parentheses: .${name}`);
  let r;
  if (typeof obj === "string") r = stringMethod(obj, name, pos, named, s);
  else if (Array.isArray(obj)) r = listMethod(obj, name, pos, named, s);
  else if (isNum(obj)) r = numberMethod(obj, name, pos, named, s);
  else if (obj instanceof Promise) r = taskMethod(obj, name);
  else if (obj instanceof JsRef) return jsCall(obj, name, pos, named, s);
  if (r !== undefined) return r;
  const m = membersFor(obj);
  if (m) unknownMember(m[0], name, nameSite, m[1]);
  $fail("LIP1004", `${withArticle(typeName(obj))} has no method "${name}"`, nameSite);
}

// ----- errors in user code -------------------------------------------------------------
/// A variable read before anything was assigned to it. The message and hint
/// were worked out by the compiler and stored on the site.
function $u(s) { const S = siteOf(s); $fail("LIP1002", S.m, s, S.h); }

/// `process.exit(code)`: unwinds the program (running `finally` blocks) and sets the exit code.
class ExitSignal { constructor(code) { this.code = code; } }

function $throw(v, s) {
  if (typeof v === "string") {
    const diag = { code: "LIP5006", message: v, site: s, hint: null };
    return new LipiError(errorObject(diag), diag, traceNow());
  }
  const message = v instanceof LObj && v.f.has("message") ? display(v.f.get("message")) : repr(v);
  return new LipiError(v, { code: "LIP5006", message, site: s, hint: null }, traceNow());
}

/// The value a `catch` receives.
function $caught(e) {
  if (e instanceof ExitSignal) throw e;
  if (e instanceof LipiError) return e.lipi;
  const diag = { code: "LIP5000", message: String(e && e.message ? e.message : e), site: null, hint: null };
  return errorObject(diag);
}

// ----- standard library --------------------------------------------------------------
function toNumber(v) {
  if (isNum(v)) return v;
  if (typeof v !== "string") return null;
  const t = v.trim().replace(/_/g, "");
  if (/^[+-]?\d+$/.test(t)) {
    const b = BigInt(t);
    if (b < MIN64 || b > MAX64) return new Dec(Number(t));
    return b >= -9007199254740991n && b <= 9007199254740991n ? Number(b) : b;
  }
  if (t !== "" && /^[0-9.+\-eE]+$/.test(t)) { const n = Number(t); if (Number.isFinite(n)) return new Dec(n); }
  return null;
}

function module(name, entries) { return new LObj(new Map(Object.entries(entries)), null, name); }

function fromJson(v) {
  if (v === null) return null;
  if (typeof v === "number") return Number.isInteger(v) && Number.isSafeInteger(v) ? v : new Dec(v);
  if (Array.isArray(v)) return v.map(fromJson);
  if (typeof v === "object") return new LObj(new Map(Object.entries(v).map(([k, x]) => [k, fromJson(x)])));
  return v;
}

function toJsonText(v, pretty, s, indent = "") {
  const inner = indent + "  ";
  const nl = pretty ? "\n" : "", sp = pretty ? " " : "";
  if (v === null || v === undefined) return "null";
  if (typeof v === "boolean") return String(v);
  if (isInt(v)) return String(v);
  if (isDec(v)) { if (!Number.isFinite(v.v)) $fail("LIP5000", `can't convert to JSON: ${fmtDec(v.v)} can't be written as JSON`, s); return Number.isInteger(v.v) ? v.v.toFixed(1) : String(v.v); }
  if (typeof v === "string") return JSON.stringify(v);
  if (Array.isArray(v)) {
    if (v.length === 0) return "[]";
    return `[${nl}${v.map((x) => (pretty ? inner : "") + toJsonText(x, pretty, s, inner)).join("," + nl)}${nl}${pretty ? indent : ""}]`;
  }
  if (v instanceof LObj) {
    const parts = [];
    for (const [k, x] of v.f) if (typeof x !== "function") parts.push(`${pretty ? inner : ""}${JSON.stringify(k)}:${sp}${toJsonText(x, pretty, s, inner)}`);
    if (parts.length === 0) return "{}";
    return `{${nl}${parts.join("," + nl)}${nl}${pretty ? indent : ""}}`;
  }
  $fail("LIP5000", `can't convert to JSON: ${withArticle(typeName(v))} can't be turned into JSON`, s);
}

function jsonParse(text, s) {
  try { return fromJson(JSON.parse(text)); }
  catch (e) { $fail("LIP5000", `this isn't valid JSON (${e.message})`, s, `JSON looks like {"name": "Dezy", "age": 25}. Keys and text need double quotes. Tip: write JSON in 'single quotes'.`); }
}

function civil(ms) {
  const d = new Date(ms);
  return { y: d.getUTCFullYear(), mo: d.getUTCMonth() + 1, d: d.getUTCDate(), h: d.getUTCHours(), mi: d.getUTCMinutes(), s: d.getUTCSeconds(), wd: d.getUTCDay() };
}
const pad2 = (n) => String(n).padStart(2, "0");

let rng = (Date.now() ^ 0x9e3779b9) >>> 0 || 1;
function random() {
  if (typeof crypto !== "undefined" && crypto.getRandomValues) { const b = new Uint32Array(2); crypto.getRandomValues(b); return (b[0] * 2 ** 21 + (b[1] >>> 11)) / 2 ** 53; }
  rng ^= rng << 13; rng ^= rng >>> 17; rng ^= rng << 5; rng >>>= 0;
  return rng / 2 ** 32;
}

function sleepTask(ms) { return track(new Promise((resolve) => setTimeout(() => resolve(null), Math.max(0, ms)))); }

function randomBytes(n) { const b = new Uint8Array(n); crypto.getRandomValues(b); return b; }
const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

const $g = {};
function installStd() {
  const def = (name, f) => { $g[name] = nat(name, f); };
  def("toNumber", (a) => toNumber(a[0] ?? null));
  def("toInteger", (a) => { const v = toNumber(a[0] ?? null); return isDec(v) ? (Number.isFinite(v.v) ? Math.trunc(v.v) : null) : v; });
  def("toDecimal", (a) => { const v = toNumber(a[0] ?? null); return isInt(v) ? new Dec(v) : v; });
  def("toString", (a) => display(a[0] ?? null));
  def("typeOf", (a) => typeName(a[0] ?? null));
  def("assert", (a, n, s) => {
    const c = need("assert", a, n, 0, "condition", s);
    if (expectBool(c, s, "the condition given to assert()")) return null;
    const m = arg(a, n, 1, "message");
    $fail("LIP5010", m === undefined ? "assertion failed" : `assertion failed: ${display(m)}`, s);
  });
  def("assertEqual", (a, n, s) => {
    const actual = need("assertEqual", a, n, 0, "actual", s), expected = need("assertEqual", a, n, 1, "expected", s);
    if (!eq(actual, expected)) $fail("LIP5010", `expected ${repr(expected)}, but got ${repr(actual)}`, s);
    return null;
  });
  def("sleep", (a, n, s) => sleepTask(argNum("sleep", a, n, 0, "milliseconds", s)));
  def("all", (a, n, s) => {
    const items = need("all", a, n, 0, "tasks", s);
    if (!Array.isArray(items)) wrong("all", "tasks", "Array", items, s);
    return track(Promise.all(items));
  });
  def("timeout", (a, n, s) => {
    const task = need("timeout", a, n, 0, "task", s), ms = argNum("timeout", a, n, 1, "milliseconds", s);
    if (!(task instanceof Promise)) return task;
    return track(Promise.race([task, new Promise((_, reject) => setTimeout(() => {
      try { $fail("LIP4002", `timed out after ${Math.round(ms)} ms`, s, "The task took too long and was cancelled. Allow more time if it needs longer."); } catch (e) { reject(e); }
    }, ms))]));
  });

  const decimal = (f) => (a, n, s) => new Dec(f(argNum("math", a, n, 0, "x", s)));
  const toInt = (f) => (a, n, s) => { const v = need("math", a, n, 0, "x", s); if (isInt(v)) return v; return whole(f(argNum("math", a, n, 0, "x", s))); };
  const numbers = (fn, a, s) => {
    const items = a.length === 1 && Array.isArray(a[0]) ? a[0] : a;
    for (const v of items) if (!isNum(v)) $fail("LIP5008", `${fn}() works with numbers, but got ${withArticle(typeName(v))}`, s);
    return items;
  };
  $g.math = module("math", {
    pi: new Dec(Math.PI), e: new Dec(Math.E), infinity: new Dec(Infinity),
    sqrt: nat("sqrt", decimal(Math.sqrt)), sin: nat("sin", decimal(Math.sin)), cos: nat("cos", decimal(Math.cos)), tan: nat("tan", decimal(Math.tan)),
    exp: nat("exp", decimal(Math.exp)), log10: nat("log10", decimal(Math.log10)),
    floor: nat("floor", toInt(Math.floor)), ceil: nat("ceil", toInt(Math.ceil)),
    abs: nat("abs", (a, n, s) => { const v = need("abs", a, n, 0, "x", s); return isInt(v) ? (v < 0 ? $neg(v, s) : v) : new Dec(Math.abs(argNum("abs", a, n, 0, "x", s))); }),
    sign: nat("sign", (a, n, s) => { const x = argNum("sign", a, n, 0, "x", s); return x > 0 ? 1 : x < 0 ? -1 : 0; }),
    round: nat("round", (a, n, s) => {
      const x = argNum("round", a, n, 0, "x", s), d = optInt("round", a, n, 1, "digits", s);
      if (d === null) return isInt(a[0]) ? a[0] : whole(Math.round(x));
      const f = Math.pow(10, d);
      return new Dec(Math.round(x * f) / f);
    }),
    pow: nat("pow", (a, n, s) => { const b = need("pow", a, n, 0, "base", s), e = need("pow", a, n, 1, "exponent", s); if (isInt(b) && isInt(e) && e >= 0) return ipow(b, e, s); return new Dec(Math.pow(argNum("pow", a, n, 0, "base", s), argNum("pow", a, n, 1, "exponent", s))); }),
    log: nat("log", (a, n, s) => { const x = argNum("log", a, n, 0, "x", s), b = optNum("log", a, n, 1, "base", s); return new Dec(b === null ? Math.log(x) : Math.log(x) / Math.log(b)); }),
    atan2: nat("atan2", (a, n, s) => new Dec(Math.atan2(argNum("atan2", a, n, 0, "y", s), argNum("atan2", a, n, 1, "x", s)))),
    clamp: nat("clamp", (a, n, s) => {
      const x = need("clamp", a, n, 0, "x", s), lo = need("clamp", a, n, 1, "min", s), hi = need("clamp", a, n, 2, "max", s);
      if (isInt(x) && isInt(lo) && isInt(hi)) return x < lo ? lo : x > hi ? hi : x;
      return new Dec(Math.min(Math.max(nv(x), nv(lo)), nv(hi)));
    }),
    min: nat("min", (a, n, s) => { const v = numbers("min", a, s); return v.length ? v.reduce((x, y) => (nv(y) < nv(x) ? y : x)) : null; }),
    max: nat("max", (a, n, s) => { const v = numbers("max", a, s); return v.length ? v.reduce((x, y) => (nv(y) > nv(x) ? y : x)) : null; }),
    random: nat("random", () => new Dec(random())),
    randomInt: nat("randomInt", (a, n, s) => {
      const lo = argInt("randomInt", a, n, 0, "min", s), hi = argInt("randomInt", a, n, 1, "max", s);
      if (hi < lo) $fail("LIP5008", "randomInt() needs min to be less than or equal to max", s);
      return lo + Math.floor(random() * (hi - lo + 1));
    }),
  });
  $g.json = module("json", {
    parse: nat("parse", (a, n, s) => jsonParse(argStr("parse", a, n, 0, "text", s), s)),
    stringify: nat("stringify", (a, n, s) => { const p = arg(a, n, 1, "pretty"); return toJsonText(need("stringify", a, n, 0, "value", s), p !== undefined && p !== null && p !== false, s); }),
  });
  const timeArg = (a, n, s) => { const t = optNum("time", a, n, 0, "time", s); return t === null ? Date.now() : Math.trunc(t); };
  const DAYS = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
  $g.time = module("time", {
    now: nat("now", () => Date.now()),
    date: nat("date", (a, n, s) => { const c = civil(timeArg(a, n, s)); return $obj([["year", c.y], ["month", c.mo], ["day", c.d], ["hour", c.h], ["minute", c.mi], ["second", c.s], ["weekday", DAYS[c.wd]]]); }),
    iso: nat("iso", (a, n, s) => { const c = civil(timeArg(a, n, s)); return `${String(c.y).padStart(4, "0")}-${pad2(c.mo)}-${pad2(c.d)}T${pad2(c.h)}:${pad2(c.mi)}:${pad2(c.s)}Z`; }),
  });
  const request = async (method, url, body, options, s) => {
    if (!/^https?:\/\//.test(url) && $rt.target === "node") $fail("LIP5000", `"${url}" isn't a full web address`, s, 'HTTP requests need a full URL that starts with https:// or http://, like "https://api.example.com/users".');
    const headers = {};
    let timeout = 30000;
    if (options instanceof LObj) {
      const h = options.f.get("headers");
      if (h instanceof LObj) for (const [k, v] of h.f) headers[k] = display(v);
      const t = options.f.get("timeout");
      if (isNum(t)) timeout = nv(t);
      const q = options.f.get("query");
      if (q instanceof LObj) {
        const qs = Array.from(q.f).map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(display(v))}`).join("&");
        if (qs) url += (url.includes("?") ? "&" : "?") + qs;
      }
    }
    let payload;
    const hasType = Object.keys(headers).some((k) => k.toLowerCase() === "content-type");
    if (body !== null && body !== undefined) {
      if (typeof body === "string") { payload = body; if (!hasType) headers["Content-Type"] = "text/plain; charset=utf-8"; }
      else { payload = toJsonText(body, false, s); if (!hasType) headers["Content-Type"] = "application/json"; }
    }
    const controller = typeof AbortController !== "undefined" ? new AbortController() : null;
    const timer = controller ? setTimeout(() => controller.abort(), timeout) : null;
    let res;
    try { res = await fetch(url, { method, headers, body: payload, signal: controller ? controller.signal : undefined }); }
    catch (e) {
      if (e && e.name === "AbortError") $fail("LIP4004", `the request to ${url} timed out after ${timeout} ms`, s);
      $fail("LIP4004", `couldn't reach ${url}: ${e.message}`, s);
    } finally { if (timer) clearTimeout(timer); }
    const text = await res.text();
    const h = new Map();
    res.headers.forEach((v, k) => h.set(k.toLowerCase(), v));
    return $obj([["status", res.status], ["ok", res.ok], ["url", res.url || url], ["headers", new LObj(h)], ["body", text], ["json", nat("json", (a, n, s2) => jsonParse(text, s2))]]);
  };
  const http = (method, hasBody) => (a, n, s) => {
    const url = argStr(method.toLowerCase(), a, n, 0, "url", s);
    const body = hasBody ? arg(a, n, 1, "body") : null;
    const options = arg(a, n, hasBody ? 2 : 1, "options");
    return track(request(method, url, body, options, s));
  };
  $g.http = module("http", {
    get: nat("get", http("GET", false)), delete: nat("delete", http("DELETE", false)),
    post: nat("post", http("POST", true)), put: nat("put", http("PUT", true)), patch: nat("patch", http("PATCH", true)),
    request: nat("request", (a, n, s) => {
      const o = need("request", a, n, 0, "options", s);
      if (!(o instanceof LObj)) $fail("LIP5008", "http.request() needs an Object", s);
      const method = o.f.has("method") ? display(o.f.get("method")).toUpperCase() : "GET";
      return track(request(method, display(o.f.get("url")), o.f.get("body") ?? null, o, s));
    }),
  });
  const serverOnly = (fn) => (a, n, s) => $fail("LIP6001", `crypto.${fn}() only works on the server`, s, "Do password hashing and similar work in server code (lipi run), not in the browser.");
  $g.crypto = module("crypto", {
    randomToken: nat("randomToken", (a, n, s) => {
      const k = Math.min(1024, Math.max(1, Math.trunc(optNum("randomToken", a, n, 0, "bytes", s) ?? 32)));
      let bin = "";
      for (const b of randomBytes(k)) bin += String.fromCharCode(b);
      return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    }),
    uuid: nat("uuid", () => {
      const b = randomBytes(16);
      b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
      const h = hex(b);
      return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
    }),
    sha256: nat("sha256", serverOnly("sha256")), hmacSha256: nat("hmacSha256", serverOnly("hmacSha256")),
    hashPassword: nat("hashPassword", serverOnly("hashPassword")), verifyPassword: nat("verifyPassword", serverOnly("verifyPassword")),
  });
}
installStd();
$rt.changed = () => {};

/// The server-side modules, for `lipi build --target node`.
function installNode() {
  const fs = require("fs");
  const ioError = (path, e, s) => {
    if (e && e.code === "ENOENT") $fail("LIP5007", `there's no file or folder at "${path}"`, s, `Paths are relative to the folder you ran lipi from (${process.cwd()}).`);
    if (e && (e.code === "EACCES" || e.code === "EPERM")) $fail("LIP5007", `not allowed to access "${path}"`, s);
    $fail("LIP5007", `couldn't access "${path}": ${e && e.message}`, s);
  };
  const io = (path, s, work) => { try { return work(); } catch (e) { if (e instanceof LipiError) throw e; ioError(path, e, s); } };
  $g.fs = module("fs", {
    read: nat("read", (a, n, s) => { const p = argStr("read", a, n, 0, "path", s); return io(p, s, () => fs.readFileSync(p, "utf8")); }),
    write: nat("write", (a, n, s) => {
      const p = argStr("write", a, n, 0, "path", s), text = need("write", a, n, 1, "text", s);
      if (typeof text !== "string") $fail("LIP5008", `fs.write() writes a String, but got ${withArticle(typeName(text))}`, s, "Convert it first, for example with json.stringify(value) or toString(value).");
      io(p, s, () => fs.writeFileSync(p, text));
      return null;
    }),
    append: nat("append", (a, n, s) => { const p = argStr("append", a, n, 0, "path", s), text = argStr("append", a, n, 1, "text", s); io(p, s, () => fs.appendFileSync(p, text)); return null; }),
    exists: nat("exists", (a, n, s) => fs.existsSync(argStr("exists", a, n, 0, "path", s))),
    isDir: nat("isDir", (a, n, s) => { const p = argStr("isDir", a, n, 0, "path", s); try { return fs.statSync(p).isDirectory(); } catch { return false; } }),
    list: nat("list", (a, n, s) => { const p = optStr("list", a, n, 0, "path", s) ?? "."; return io(p, s, () => fs.readdirSync(p).sort()); }),
    makeDir: nat("makeDir", (a, n, s) => { const p = argStr("makeDir", a, n, 0, "path", s); io(p, s, () => fs.mkdirSync(p, { recursive: true })); return null; }),
    delete: nat("delete", (a, n, s) => { const p = argStr("delete", a, n, 0, "path", s); io(p, s, () => (fs.statSync(p).isDirectory() ? fs.rmSync(p, { recursive: true }) : fs.unlinkSync(p))); return null; }),
  });
  $g.env = module("env", {
    get: nat("get", (a, n, s) => { const k = argStr("get", a, n, 0, "name", s); if (k in process.env) return process.env[k]; const d = arg(a, n, 1, "default"); return d === undefined ? null : d; }),
    has: nat("has", (a, n, s) => argStr("has", a, n, 0, "name", s) in process.env),
    all: nat("all", () => new LObj(new Map(Object.keys(process.env).sort().map((k) => [k, process.env[k]])))),
    load: nat("load", (a, n, s) => {
      const p = optStr("load", a, n, 0, "path", s) ?? ".env";
      let count = 0;
      for (let line of io(p, s, () => fs.readFileSync(p, "utf8")).split("\n")) {
        line = line.trim();
        if (line === "" || line.startsWith("#")) continue;
        if (line.startsWith("export ")) line = line.slice(7);
        const eqAt = line.indexOf("=");
        if (eqAt < 0) continue;
        let v = line.slice(eqAt + 1).trim();
        if (v.length >= 2 && v.startsWith('"') && v.endsWith('"')) v = v.slice(1, -1);
        process.env[line.slice(0, eqAt).trim()] = v;
        count++;
      }
      return count;
    }),
  });
  const platforms = { win32: "windows", darwin: "macos" };
  $g.process = module("process", {
    args: process.argv.slice(2),
    platform: platforms[process.platform] || process.platform,
    exit: nat("exit", (a, n, s) => { throw new ExitSignal(optInt("exit", a, n, 0, "code", s) ?? 0); }),
    cwd: nat("cwd", () => process.cwd()),
    run: nat("run", (a, n, s) => {
      const command = argStr("run", a, n, 0, "command", s);
      const r = require("child_process").spawnSync(command, { shell: true, encoding: "utf8" });
      if (r.error) $fail("LIP5007", `couldn't run "${command}": ${r.error.message}`, s);
      return $obj([["code", r.status ?? -1], ["output", r.stdout || ""], ["error", r.stderr || ""]]);
    }),
  });
}

// ----- JavaScript interop -------------------------------------------------------------------
// The `js` module is the explicit boundary to JavaScript: js.global, js.import,
// js.new and js.value. Values crossing it are converted both ways:
//   JS number   -> Integer when it's a whole number that fits exactly, else Decimal
//   JS array    -> a new LiPi Array (a copy)
//   JS function -> a LiPi function (a LiPi function passed to JS becomes a JS function)
//   JS promise  -> a Task (use await)
//   other JS objects -> JsObject, whose fields and methods work with . and ( )
//   null/undefined -> null
// Errors thrown by JavaScript become LiPi errors with code LIP5012.

function jsDescribe(v) {
  if (typeof v === "function") return `<js function ${v.name || ""}>`.replace(" >", ">");
  const name = v && v.constructor && v.constructor.name;
  return `<js ${name || "object"}>`;
}

function jsError(e, s) {
  if (e instanceof LipiError || e instanceof ExitSignal) throw e;
  $fail("LIP5012", `JavaScript error: ${e && e.message ? e.message : String(e)}`, s, "The error came from JavaScript code called through `js`.");
}

function toJs(v) {
  if (v === null || v === undefined) return null;
  if (typeof v === "bigint") return Number(v);
  if (v instanceof Dec) return v.v;
  if (v instanceof JsRef) return v.v;
  if (Array.isArray(v)) return v.map(toJs);
  if (v instanceof Promise) return v.then(toJs);
  if (v instanceof LObj) {
    const o = {};
    for (const [k, x] of v.f) if (typeof x !== "function") o[k] = toJs(x);
    return o;
  }
  if (typeof v === "function") {
    if (v.$js) return v.$js;
    return function (...args) {
      const r = cb(v, args.map((x) => fromJs(x)), null);
      $rt.changed();
      return toJs(r);
    };
  }
  return v;
}

function fromJs(v, self) {
  if (v === null || v === undefined) return null;
  switch (typeof v) {
    case "number": return Number.isSafeInteger(v) ? v + 0 : new Dec(v);
    case "bigint": return v < MIN64 || v > MAX64 ? new Dec(Number(v)) : v >= -9007199254740991n && v <= 9007199254740991n ? Number(v) : v;
    case "string": case "boolean": return v;
    case "function": {
      if (v.$m || v.$native) return v;
      const f = nat(v.name || "function", (pos, named, s) => {
        let r;
        try { r = v.apply(self, jsArgs(pos, named)); } catch (e) { jsError(e, s); }
        return fromJs(r);
      });
      f.$js = v;
      return f;
    }
    case "object":
      if (Array.isArray(v)) return v.map((x) => fromJs(x));
      if (typeof v.then === "function") return track(Promise.resolve(v).then((x) => fromJs(x), (e) => jsError(e, null)));
      return new JsRef(v);
  }
  return new JsRef(v);
}

function jsArgs(pos, named) {
  const out = pos.map(toJs);
  if (named) out.push(Object.fromEntries(Object.entries(named).map(([k, x]) => [k, toJs(x)])));
  return out;
}

function jsKey(key, s) {
  if (typeof key === "string") return key;
  if (isInt(key)) return Number(key);
  $fail("LIP2001", `JavaScript properties are Strings or Integers, not ${withArticle(typeName(key))}`, s);
}

function jsGet(ref, key, s) {
  let v;
  try { v = ref.v[jsKey(key, s)]; } catch (e) { jsError(e, s); }
  return fromJs(v, ref.v);
}

function jsSet(ref, key, v, s) {
  try { ref.v[jsKey(key, s)] = toJs(v); } catch (e) { jsError(e, s); }
  $rt.changed();
}

function jsCall(ref, name, pos, named, s) {
  let f;
  try { f = ref.v[name]; } catch (e) { jsError(e, s); }
  if (typeof f !== "function") {
    $fail("LIP2008", `"${name}" isn't a function on this JavaScript object`, s,
      f === undefined ? "It doesn't exist. Check the spelling: JavaScript names are case-sensitive." : "It's a value; read it without ( ).");
  }
  let r;
  try { r = f.apply(ref.v, jsArgs(pos, named)); } catch (e) { jsError(e, s); }
  return fromJs(r);
}

/// js.value(x): plain JavaScript data (objects and arrays, deeply) as LiPi values.
function jsValue(v, depth = 0) {
  if (v instanceof JsRef) v = v.v;
  if (v === null || v === undefined || depth > 100) return null;
  if (Array.isArray(v)) return v.map((x) => jsValue(x, depth + 1));
  if (typeof v === "object" && typeof v.then !== "function") {
    const m = new Map();
    for (const k of Object.keys(v)) m.set(k, jsValue(v[k], depth + 1));
    return new LObj(m);
  }
  return fromJs(v);
}

function installJs() {
  $g.js = module("js", {
    global: new JsRef(globalThis),
    import: nat("import", (a, n, s) => {
      const spec = argStr("import", a, n, 0, "module", s);
      return track(import(spec).then((m) => new JsRef(m), (e) => jsError(e, s)));
    }),
    new: nat("new", (a, n, s) => {
      const C = toJs(need("new", a, n, 0, "constructor", s));
      if (typeof C !== "function") $fail("LIP5008", "js.new() needs a JavaScript class or constructor", s, "For example: js.new(js.global.Date)");
      let r;
      try { r = Reflect.construct(C, jsArgs(a.slice(1), n)); } catch (e) { jsError(e, s); }
      return fromJs(r);
    }),
    value: nat("value", (a, n, s) => jsValue(need("value", a, n, 0, "value", s))),
    typeOf: nat("typeOf", (a, n, s) => {
      const v = toJs(need("typeOf", a, n, 0, "value", s));
      return v === null ? "null" : Array.isArray(v) ? "array" : typeof v;
    }),
  });
}
installJs();

// ----- modules and program start --------------------------------------------------------
function $use(id) {
  if (!$rt.cache.has(id)) $rt.cache.set(id, $rt.mods[id]());
  return $rt.cache.get(id);
}

function $module(name, pairs) { return new LObj(new Map(pairs), null, name); }

function $uncaught(e) {
  if (e instanceof ExitSignal) {
    if ($rt.target === "node") process.exitCode = e.code;
    return;
  }
  const text = e instanceof LipiError ? render(e.diag, e.trace) : render({ code: "LIP5000", message: `internal error: ${e && e.stack ? e.stack : e}`, site: null, hint: "This is a bug in LiPi's JavaScript runtime. Please report it." }, []);
  if ($rt.target === "node") { process.stderr.write(text); process.exitCode = 1; }
  else {
    console.error(text);
    if (typeof document !== "undefined") {
      const box = document.createElement("pre");
      box.className = "lipi-error";
      box.textContent = text;
      document.body.appendChild(box);
    }
  }
}

/// `await task`: a cancelled task has no result.
function $aw(v, s) {
  if (v instanceof Promise && v.$cancelled) $fail("LIP4003", "this task was cancelled", s, "A cancelled task has no result, so it can't be awaited.");
  return v;
}

/// Run the main module. Modules without top-level `await` run synchronously.
/// If the program declared pages, the app starts drawing when it finishes.
function $start(entry) {
  try {
    const r = $use(entry);
    if (r instanceof Promise) r.then(uiAfterMain, $uncaught);
    else uiAfterMain();
  } catch (e) {
    $uncaught(e);
  }
}

// ----- LiPi UI ----------------------------------------------------------------------------
// Drawing is immediate-mode: a page's block runs from the top on every redraw
// and each element call adds a node to the element being drawn. The new tree
// is then patched into the DOM, so text boxes keep their focus and cursor.
// Redraws happen after every event handler, after state changes, and when an
// awaited handler finishes.

const $ui = { pages: [], stack: null, instances: new Map(), seen: null, path: [], root: null, old: [], scheduled: false, started: false, rules: new Set() };
/// Style options that become CSS rules (inline styles can't express them).
const UI_STATE_STYLES = new Set(["hover", "focus", "mobile", "desktop"]);
const MOBILE_MAX = 720;

/// `hover: "..."`, `focus:`, `mobile:` and `desktop:`: a class for the element
/// plus one CSS rule per distinct style, kept in a <style id="lipi-rules">.
/// Declarations get !important so they win over the element's inline `style:`.
function uiRuleClass(kind, css, s) {
  if (/[{}<>]/.test(css)) $fail("LIP6003", `${kind} styles can't contain { } < or >`, s, `Write plain declarations, for example: ${kind}: "color: #B83A22;"`);
  const decls = css.split(";").map((d) => d.trim()).filter((d) => d !== "").map((d) => (/!important$/i.test(d) ? d : d + " !important")).join("; ");
  let h = 0;
  for (const c of kind + decls) h = (Math.imul(h, 31) + c.charCodeAt(0)) >>> 0;
  const cls = `lipi-${kind}-${h.toString(36)}`;
  if (!$ui.rules.has(cls)) {
    $ui.rules.add(cls);
    const sel = kind === "hover" ? `.${cls}:hover` : kind === "focus" ? `.${cls}:focus-visible` : `.${cls}`;
    const rule = kind === "mobile" ? `@media (max-width: ${MOBILE_MAX}px) { ${sel} { ${decls} } }`
      : kind === "desktop" ? `@media (min-width: ${MOBILE_MAX + 1}px) { ${sel} { ${decls} } }`
      : `${sel} { ${decls} }`;
    if (typeof document !== "undefined") {
      let el = document.getElementById("lipi-rules");
      if (!el) { el = document.createElement("style"); el.id = "lipi-rules"; document.head.appendChild(el); }
      el.textContent += rule + "\n";
    }
  }
  return cls;
}
const BLOCKED_TAGS = new Set(["script", "style", "iframe", "object", "embed", "link", "meta", "base", "frame", "frameset", "template"]);
const UI_OPTIONS = { heading: ["level"], button: ["disabled"], link: ["to"], image: ["alt"], field: ["placeholder", "type", "disabled"], checkbox: ["disabled"] };
const UI_CSS = `
#app { max-width: 60rem; margin: 0 auto; }
.lipi-card { background: #fff; border: 1px solid #E8E1D6; border-radius: 12px; padding: 1rem 1.25rem; margin: .75rem 0; box-shadow: 0 1px 2px rgba(23,18,14,.06); }
.lipi-row { display: flex; gap: .75rem; align-items: center; flex-wrap: wrap; margin: .35rem 0; }
.lipi-column { display: flex; flex-direction: column; gap: .5rem; }
.lipi-heading { margin: .25em 0 .5em; line-height: 1.2; }
.lipi-text { margin: .35em 0; line-height: 1.55; }
.lipi-button { font: inherit; background: #D2452A; color: #fff; border: 0; border-radius: 8px; padding: .45rem 1rem; cursor: pointer; transition: background .15s, color .15s, transform .15s, box-shadow .15s; }
.lipi-link { transition: background .15s, color .15s; }
.lipi-button:hover { background: #B83A22; }
.lipi-button:disabled { opacity: .5; cursor: default; }
.lipi-button:focus-visible, .lipi-field:focus-visible, .lipi-link:focus-visible { outline: 2px solid #17120E; outline-offset: 2px; }
.lipi-field { font: inherit; padding: .45rem .65rem; border: 1px solid #CFC6B8; border-radius: 8px; background: #fff; min-width: 14rem; }
.lipi-link { color: #B83A22; }
.lipi-image { max-width: 100%; border-radius: 8px; }
.lipi-checkbox { display: inline-flex; gap: .5rem; align-items: center; cursor: pointer; }
`;

class Instance {
  constructor() { this.store = new Map(); }
  init(name, make) {
    if (!this.store.has(name)) this.store.set(name, make());
    return this.store.get(name);
  }
  put(name, value) {
    this.store.set(name, value);
    $rt.changed();
  }
}

/// Entering a component while drawing. Instances are identified by their
/// position (the Nth ProductCard inside its parent), so state stays with them.
$ui.enter = (name, s) => {
  if ($ui.stack === null) $fail("LIP6002", `the component "${name}" can only be used while drawing a page`, s, "Use it inside a `page` block or another component.");
  const parent = $ui.path[$ui.path.length - 1];
  // A component called with `key:` is identified by that key, so its state
  // follows its item when a list is filtered or reordered.
  const given = $ui.nextKey;
  $ui.nextKey = undefined;
  let key;
  if (given === undefined || given === null) {
    const n = parent.counts.get(name) || 0;
    parent.counts.set(name, n + 1);
    key = `${parent.key}/${name}#${n}`;
  } else {
    key = `${parent.key}/${name}=${repr(given)}`;
    if ($ui.seen.has(key)) $fail("LIP5008", `two "${name}" components have the same key ${repr(given)}`, s, "Each item in a list needs its own key, such as its id.");
  }
  $ui.seen.add(key);
  let inst = $ui.instances.get(key);
  if (!inst) { inst = new Instance(); $ui.instances.set(key, inst); }
  $ui.path.push({ key, counts: new Map() });
  return inst;
};
$ui.leave = () => { $ui.path.pop(); };

function uiCheck(name, s) {
  if ($ui.stack === null) $fail("LIP6002", `"${name}" can only be used while drawing a page`, s, "Put it inside a `page` block or a `component`.");
}
function emit(v) { $ui.stack[$ui.stack.length - 1].push(v); }
const vnode = (tag, a, k, on, p) => ({ tag, a, k: k || [], on: on || {}, p: p || {} });
const texts = (values) => (values.length ? [{ t: values.map(display).join(" ") }] : []);

/// A trailing block arrives as the last positional argument.
function splitBlock(pos) {
  const last = pos[pos.length - 1];
  return pos.length && typeof last === "function" ? [pos.slice(0, -1), last] : [pos, null];
}

function drawInside(block, s) {
  const list = [];
  if (!block) return list;
  $ui.stack.push(list);
  try { cb(block, [], s); } finally { $ui.stack.pop(); }
  return list;
}

/// The attributes every element accepts (class, id, style, title), plus checks for its own options.
function common(kind, named, s) {
  const a = { class: "lipi-" + kind };
  if (!named) return a;
  const own = UI_OPTIONS[kind] || [];
  for (const [k, v] of Object.entries(named)) {
    if (k === "class") a.class += " " + display(v);
    else if (k === "id" || k === "style" || k === "title") a[k] = display(v);
    else if (UI_STATE_STYLES.has(k)) a.class += " " + uiRuleClass(k, display(v), s);
    else if (!own.includes(k)) {
      const all = ["class", "id", "style", "title", "hover", "focus", "mobile", "desktop", ...own];
      $fail("LIP5008", `${kind} has no option "${k}"`, s, didYouMean(k, all) || `Its options are: ${all.join(", ")}`);
    }
  }
  return a;
}

function optBool(named, key, s) { return named && named[key] !== undefined ? $bool(named[key], s) : false; }

function uiSafeUrl(url, s, what) {
  if (/^\s*(javascript|data|vbscript):/i.test(url)) $fail("LIP6003", `${what} can't use "${url.split(":")[0]}:" addresses`, s, "They could run code the page didn't write.");
}

function installUI() {
  const def = (name, f) => { $g[name] = nat(name, f); };
  const container = (kind, tag) => def(kind, (pos, named, s) => {
    uiCheck(kind, s);
    const [values, block] = splitBlock(pos);
    emit(vnode(tag, common(kind, named, s), texts(values).concat(drawInside(block, s))));
    return null;
  });
  container("card", "div");
  container("row", "div");
  container("column", "div");
  container("section", "section");
  def("heading", (pos, named, s) => {
    uiCheck("heading", s);
    const level = named && named.level !== undefined ? named.level : 2;
    if (!isInt(level) || level < 1 || level > 6) $fail("LIP5008", "a heading's level must be an Integer from 1 to 6", s);
    const [values, block] = splitBlock(pos);
    emit(vnode("h" + level, common("heading", named, s), texts(values).concat(drawInside(block, s))));
    return null;
  });
  def("text", (pos, named, s) => {
    uiCheck("text", s);
    const [values, block] = splitBlock(pos);
    emit(vnode("p", common("text", named, s), texts(values).concat(drawInside(block, s))));
    return null;
  });
  def("button", (pos, named, s) => {
    uiCheck("button", s);
    const [values, block] = splitBlock(pos);
    const a = common("button", named, s);
    a.type = "button";
    a.disabled = optBool(named, "disabled", s);
    emit(vnode("button", a, texts(values), block ? { click: block } : {}));
    return null;
  });
  def("link", (pos, named, s) => {
    uiCheck("link", s);
    const target = named && named.to !== undefined ? named.to : pos[1];
    if (typeof target !== "string") $fail("LIP5008", "a link needs to say where it goes", s, 'For example: link "About", to: "/about"');
    const a = common("link", named, s);
    if (target.startsWith("/")) a.href = "#" + target;
    else if (/^(https?:|mailto:|tel:)/i.test(target)) { a.href = target; a.target = "_blank"; a.rel = "noopener noreferrer"; }
    else $fail("LIP6003", `"${target}" isn't a link LiPi can open safely`, s, 'Link to a page of this app ("/about") or to an address that starts with https://, http://, mailto: or tel:.');
    emit(vnode("a", a, texts(pos[0] === undefined ? [target] : [pos[0]])));
    return null;
  });
  def("image", (pos, named, s) => {
    uiCheck("image", s);
    const src = pos[0];
    if (typeof src !== "string") wrong("image", "source", "String", src === undefined ? null : src, s);
    uiSafeUrl(src, s, "images");
    const a = common("image", named, s);
    a.src = src;
    a.alt = named && named.alt !== undefined ? display(named.alt) : "";
    emit(vnode("img", a));
    return null;
  });
  def("field", (pos, named, s) => {
    uiCheck("field", s);
    const [values, block] = splitBlock(pos);
    const a = common("field", named, s);
    a.type = named && named.type !== undefined ? display(named.type) : "text";
    if (named && named.placeholder !== undefined) a.placeholder = display(named.placeholder);
    a.disabled = optBool(named, "disabled", s);
    const v = values[0];
    emit(vnode("input", a, [], block ? { input: block } : {}, { value: v === undefined || v === null ? "" : display(v) }));
    return null;
  });
  def("checkbox", (pos, named, s) => {
    uiCheck("checkbox", s);
    const [values, block] = splitBlock(pos);
    const checked = values[0] === undefined ? false : $bool(values[0], s);
    const box = vnode("input", { type: "checkbox", disabled: optBool(named, "disabled", s) }, [], block ? { change: block } : {}, { checked });
    emit(vnode("label", common("checkbox", named, s), [box].concat(texts(values.slice(1)))));
    return null;
  });
  def("element", (pos, named, s) => {
    uiCheck("element", s);
    const [values, block] = splitBlock(pos);
    const tag = values[0];
    if (typeof tag !== "string" || !/^[a-z][a-z0-9-]*$/.test(tag)) $fail("LIP5008", "element needs a tag name", s, 'For example: element "ul"');
    if (BLOCKED_TAGS.has(tag)) $fail("LIP6003", `element can't create <${tag}>`, s, "Scripts, styles and embedded pages could run code the page didn't write.");
    emit(vnode(tag, common("element", named, s), texts(values.slice(1)).concat(drawInside(block, s))));
    return null;
  });
  def("page", (pos, named, s) => {
    if ($ui.stack !== null) $fail("LIP6002", "`page` declares a page, so it can't be used while drawing one", s, "Put `page` blocks at the top level of the file.");
    const [path, block] = pos;
    if (typeof path !== "string" || !path.startsWith("/")) $fail("LIP5008", "a page needs a path that starts with /", s, 'For example: page "/about"');
    if (typeof block !== "function") $fail("LIP5008", "a page needs an indented block that draws it", s);
    if ($ui.pages.some((p) => p.path === path)) $fail("LIP5008", `there's already a page for "${path}"`, s);
    $ui.pages.push({ path, parts: path.split("/").filter((x) => x !== ""), block, s });
    return null;
  });
  def("navigate", (pos, named, s) => {
    const path = pos[0];
    if (typeof path !== "string" || !path.startsWith("/")) $fail("LIP5008", "navigate needs the path of a page", s, 'For example: navigate("/cart")');
    if (typeof location !== "undefined") location.hash = "#" + path;
    return null;
  });
  $rt.changed = () => { if ($ui.started && $ui.stack === null) uiSchedule(); };
}

function uiDecode(x) { try { return decodeURIComponent(x); } catch { return x; } }

/// The page for the current address (`#/products/7`) and its route object.
function uiRoute() {
  const raw = typeof location !== "undefined" ? location.hash.replace(/^#/, "") : "";
  const q = raw.indexOf("?");
  let path = q < 0 ? raw : raw.slice(0, q);
  if (!path.startsWith("/")) path = "/" + path;
  const parts = path.split("/").filter((x) => x !== "").map(uiDecode);
  const query = new Map();
  for (const kv of (q < 0 ? "" : raw.slice(q + 1)).split("&")) {
    if (!kv) continue;
    const eqAt = kv.indexOf("=");
    query.set(uiDecode(eqAt < 0 ? kv : kv.slice(0, eqAt)), uiDecode(eqAt < 0 ? "" : kv.slice(eqAt + 1)));
  }
  for (const p of $ui.pages) {
    const params = new Map();
    let ok = true;
    for (let i = 0; i < Math.max(p.parts.length, parts.length); i++) {
      const want = p.parts[i];
      if (want === "*") { params.set("rest", parts.slice(i).join("/")); break; }
      if (want === undefined || parts[i] === undefined) { ok = false; break; }
      if (want.startsWith(":")) params.set(want.slice(1), parts[i]);
      else if (want !== parts[i]) { ok = false; break; }
    }
    if (ok) return { page: p, path, route: $obj([["path", path], ["params", new LObj(params)], ["query", new LObj(query)]]) };
  }
  return { page: null, path };
}

function uiSchedule() {
  if ($ui.scheduled) return;
  $ui.scheduled = true;
  queueMicrotask(uiRender);
}

function uiRender() {
  $ui.scheduled = false;
  const m = uiRoute();
  const list = [];
  $ui.stack = [list];
  $ui.seen = new Set();
  $ui.path = [{ key: "", counts: new Map() }];
  try {
    if (m.page) {
      const r = cb(m.page.block, [m.route], m.page.s);
      if (r instanceof Promise) $fail("LIP4001", "a page draws right away, so its block can't use `await`", m.page.s, "Load data in top-level code or in a button's block, keep it in `state`, and draw it here.");
    } else {
      emit(vnode("h1", { class: "lipi-heading" }, [{ t: "Page not found" }]));
      emit(vnode("p", { class: "lipi-text" }, [{ t: `There's no page for ${m.path}.` }]));
    }
  } catch (e) {
    $ui.stack = null;
    $uncaught(e);
    return;
  }
  $ui.stack = null;
  for (const k of Array.from($ui.instances.keys())) if (!$ui.seen.has(k)) $ui.instances.delete(k);
  patchChildren($ui.root, $ui.old, list);
  $ui.old = list;
}

/// Run an event handler, then redraw (again when an async handler finishes).
function uiFire(el, type) {
  const h = el.$h && el.$h[type];
  if (!h) return;
  const args = type === "input" ? [el.value] : type === "change" ? [el.checked] : [];
  let r;
  try {
    r = cb(h, args, null);
  } catch (e) {
    $uncaught(e);
  }
  if (r instanceof Promise) r.then(uiSchedule, (e) => { $uncaught(e); uiSchedule(); });
  uiSchedule();
}

function uiSetAttr(el, k, v) {
  if (v === false || v === null || v === undefined) el.removeAttribute(k);
  else el.setAttribute(k, v === true ? "" : String(v));
}

function uiListen(el, on) {
  if (!el.$types) el.$types = new Set();
  for (const type of Object.keys(on)) {
    if (el.$types.has(type)) continue;
    el.$types.add(type);
    el.addEventListener(type, () => uiFire(el, type));
  }
}

function uiCreate(v) {
  if (v.t !== undefined) return document.createTextNode(v.t);
  const el = document.createElement(v.tag);
  for (const [k, x] of Object.entries(v.a)) uiSetAttr(el, k, x);
  for (const [k, x] of Object.entries(v.p)) el[k] = x;
  el.$h = v.on;
  uiListen(el, v.on);
  for (const c of v.k) el.appendChild(uiCreate(c));
  return el;
}

function uiPatch(parent, node, o, n) {
  if (n.t !== undefined && o.t !== undefined) {
    if (o.t !== n.t) node.nodeValue = n.t;
    return;
  }
  if (n.t !== undefined || o.t !== undefined || o.tag !== n.tag) {
    parent.replaceChild(uiCreate(n), node);
    return;
  }
  for (const k of Object.keys(o.a)) if (!(k in n.a)) node.removeAttribute(k);
  for (const [k, x] of Object.entries(n.a)) if (o.a[k] !== x) uiSetAttr(node, k, x);
  for (const [k, x] of Object.entries(n.p)) if (node[k] !== x) node[k] = x;
  node.$h = n.on;
  uiListen(node, n.on);
  patchChildren(node, o.k, n.k);
}

function patchChildren(dom, olds, news) {
  const nodes = Array.from(dom.childNodes);
  for (let i = 0; i < news.length; i++) {
    if (i < olds.length && nodes[i]) uiPatch(dom, nodes[i], olds[i], news[i]);
    else dom.appendChild(uiCreate(news[i]));
  }
  for (let i = nodes.length - 1; i >= news.length; i--) dom.removeChild(nodes[i]);
}

function uiStart() {
  $ui.started = true;
  if (!document.getElementById("lipi-style")) {
    const style = document.createElement("style");
    style.id = "lipi-style";
    style.textContent = UI_CSS;
    document.head.appendChild(style);
  }
  $ui.root = document.getElementById("app");
  if (!$ui.root) {
    $ui.root = document.createElement("div");
    $ui.root.id = "app";
    document.body.insertBefore($ui.root, document.body.firstChild);
  }
  if (typeof window !== "undefined") window.addEventListener("hashchange", uiSchedule);
  uiRender();
}

function uiAfterMain() {
  if ($ui.pages.length && typeof document !== "undefined") uiStart();
}

installUI();
