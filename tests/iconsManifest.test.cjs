// 图标集「创作说明 ↔ 代码」一致性守卫（2026-11-23，配 docs/design/icon-set-AUTHORSHIP.md）。
//
// 为什么要这个测试：那份说明是要拿给别人看"这套图标是我们自己画的"，如果清单和代码
// 对不上（加/删图标忘了同步），它就从证据变成笑话。这里只读源码，不跑 DOM。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const read = (p) => fs.readFileSync(path.join(ROOT, p), "utf8");

const src = read("src/components/icons.tsx");
const doc = read("docs/design/icon-set-AUTHORSHIP.md");

const codeNames = [...src.match(/export type IconName =([\s\S]*?);/)[1].matchAll(/"([a-z]+)"/g)].map((m) => m[1]);
const docNames = [...doc.matchAll(/^\| \d+ \| `([a-z]+)` \|/gm)].map((m) => m[1]);

test("清单与代码一一对应（50 个，不重不漏）", () => {
  assert.equal(codeNames.length, 50, `代码里应有 50 个图标名，实际 ${codeNames.length}`);
  assert.equal(docNames.length, 50, `创作说明里应有 50 行清单，实际 ${docNames.length}`);
  assert.deepEqual([...docNames].sort(), [...codeNames].sort(), "清单与代码的图标名不一致");
});

test("创作说明里记了 v3.17.0 的 blob 哈希与「50 个」", () => {
  assert.ok(doc.includes("FC95E24E9F8C00B3E8246DB3145775A89B3861F6FA0905E567F7EF27D91058C8"), "缺少 v3.17.0 的 blob 哈希");
  assert.ok(doc.includes("50 个"), "缺少「50 个」这个数量口径");
});

test("项目没有引入图标库（不然「自己画的」站不住）", () => {
  const pkg = JSON.parse(read("package.json"));
  const deps = { ...(pkg.dependencies || {}), ...(pkg.devDependencies || {}) };
  for (const lib of ["lucide-react", "lucide", "react-icons", "heroicons", "@heroicons/react", "feather-icons", "react-feather", "tabler-icons", "@tabler/icons-react", "@fortawesome/fontawesome-free", "bootstrap-icons", "@mui/icons-material"]) {
    assert.ok(!(lib in deps), `不该出现图标库依赖：${lib}`);
  }
});
