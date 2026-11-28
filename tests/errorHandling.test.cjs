const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

function walk(dir, out = []) {
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, ent.name);
    if (ent.isDirectory()) walk(p, out);
    else if (/\.(ts|tsx)$/.test(ent.name)) out.push(p);
  }
  return out;
}

function catchBodies(text) {
  const out = [];
  const re = /catch\s*(?:\([^)]*\))?\s*\{/g;
  let m;
  while ((m = re.exec(text))) {
    let i = m.index + m[0].length;
    let depth = 1;
    for (; i < text.length && depth > 0; i++) {
      if (text[i] === "{") depth++;
      else if (text[i] === "}") depth--;
    }
    out.push({ at: m.index, body: text.slice(m.index + m[0].length, i - 1) });
  }
  return out;
}

test("错误收口：src 里不允许无说明的静默 catch", () => {
  const root = path.join(__dirname, "..", "src");
  const errors = [];
  for (const file of walk(root)) {
    const rel = path.relative(path.join(__dirname, ".."), file).replaceAll("\\", "/");
    const text = fs.readFileSync(file, "utf8");

    for (const { body } of catchBodies(text)) {
      // errors.ts 里 JSON.stringify 的兜底是错误工具自身的最后退路，不调用自己。
      if (rel === "src/lib/errors.ts") continue;
      if (!/(reportError|expectedError|\bthrow\b)/.test(body)) {
        errors.push(`${rel}: catch 必须调用 reportError / expectedError / throw，实际：${body.trim().slice(0, 120)}`);
      }
    }

    if (/\.catch\(\s*\(\s*\)\s*=>/.test(text)) {
      errors.push(`${rel}: 不允许 .catch(() => ...) 无说明吞错`);
    }
  }
  assert.deepEqual(errors, []);
});