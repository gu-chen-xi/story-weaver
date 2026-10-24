// 逐日节点树 + 通用布局的单测（npm test）。
//
// 为什么单独测：逐日模式（参考图二）是"每个节点一个药丸"，段框那套断言覆盖不到它；
// 而两者共用 `layoutUnits`，坐标错了会表现成"药丸叠在一起 / 线连错父亲"，
// 界面上不报错、只是难看和难用。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  buildDayTree,
  dayTreeToUnits,
  flattenDays,
  layoutUnits,
  pillWidth,
} = require("../.tmp/fe-tests/src/lib/timelineNodes.js");

const node = (id, day, parent, branch = null, opts = {}) => ({
  id,
  date_label: `第 ${day} 天`,
  sort_order: opts.sort ?? id,
  node_type: opts.type ?? "day",
  is_flashback: opts.flashback ?? 0,
  tags: "",
  parent_id: parent,
  branch_id: branch,
});

const branch = (id, name, fork, note = "") => ({
  id, story_id: "1", name, fork_node_id: fork, source_branch_id: null,
  color: "#8B5CF6", created_at: "2026-10-24", note,
});

// 造参考图那棵树：主线 1..7 在第 7 天分叉 → 分支A(8)、分支B(8,再在 11 天分叉出 B1/B2)
function refTree() {
  const nodes = [
    node(1, 1, null), node(2, 2, 1), node(3, 3, 2), node(4, 4, 3),
    node(5, 5, 4), node(6, 6, 5), node(7, 7, 6),          // 第 7 天是分叉点
    node(8, 8, 7, "BR-A"),
    node(9, 8, 7, "BR-B"), node(10, 9, 9, "BR-B"), node(11, 10, 10, "BR-B"),
    node(12, 11, 11, "BR-B"),                              // 第 11 天再分叉
    node(13, 12, 12, "BR-B1"), node(14, 12, 12, "BR-B2"),
  ];
  const branches = [branch("BR-A", "分支A", 7, "活着"), branch("BR-B", "分支B", 7, "牺牲"),
                    branch("BR-B1", "子分支B1", 11), branch("BR-B2", "子分支B2", 11)];
  return { nodes, branches };
}

test("逐日树：分叉点/线起点/线名（带备注）都对", () => {
  const { nodes, branches } = refTree();
  const root = buildDayTree(nodes, branches, null, 5);
  const all = flattenDays(root);
  const byId = new Map(all.map((n) => [n.id, n]));

  assert.equal(byId.get(7).isFork, true, "第 7 天有两个后继");
  assert.equal(byId.get(12).isFork, true, "第 11 天（id 12）再分叉");
  assert.equal(byId.get(8).isLineStart, true, "分支A 第一个节点应标记为线起点");
  assert.equal(byId.get(9).isLineStart, true, "分支B 第一个节点也应标记");
  assert.ok(byId.get(8).lineName.includes("分支A"), "线名应含分支名");
  assert.ok(byId.get(8).lineName.includes("活着"), "线名应带分支备注（参考图「分支A · 活着」）");
  assert.equal(byId.get(5).isCurrent, true, "传入的当前节点要被标出来");
});

test("逐日树：主线节点颜色是浅色、分支用自己的颜色", () => {
  const { nodes, branches } = refTree();
  const root = buildDayTree(nodes, branches, null, null);
  const byId = new Map(flattenDays(root).map((n) => [n.id, n]));
  assert.equal(byId.get(1).color, "#cfd6e4", "主线用浅色");
  assert.equal(byId.get(8).color, "#8B5CF6", "分支用分支色");
});

test("逐日树：深度正确（主线一路到底 + 分支再分叉）", () => {
  const { nodes, branches } = refTree();
  const root = buildDayTree(nodes, branches, null, null);
  const depth = new Map();
  const walk = (n, d) => { depth.set(n.id, d); n.children.forEach((c) => walk(c, d + 1)); };
  walk(root, 0);
  assert.equal(depth.get(7), 6, "第 7 天在第 7 层（0 起算）");
  assert.equal(depth.get(12), 10, "第 11 天更深");
});

test("药丸宽度：按字数算，不测 DOM；分叉点更宽（要放 ★）", () => {
  assert.ok(pillWidth("第 1 天", false) >= 44, "有最小宽度");
  assert.ok(pillWidth("第 123 天", false) > pillWidth("第 1 天", false), "字数多应更宽");
  assert.ok(pillWidth("第 7 天", true) > pillWidth("第 7 天", false), "分叉点更宽");
});

test("通用布局：兄弟不重叠、父居中于子、层间不压", () => {
  // 根 + 3 个孩子
  const u = (key, depth, w, h, children = []) => ({ key, depth, w, h, children });
  const root = u("r", 0, 100, 40, [u("a", 1, 80, 30), u("b", 1, 120, 30), u("c", 1, 60, 30)]);
  const lay = layoutUnits(root, 10, 20);
  const kids = lay.placed.filter((p) => p.key !== "r").sort((x, y) => x.x - y.x);
  assert.equal(kids.length, 3);
  for (let i = 1; i < kids.length; i++) {
    const prev = kids[i - 1];
    assert.ok(kids[i].x >= prev.x + prev.w, `重叠：${prev.key} 结束于 ${prev.x + prev.w}，${kids[i].key} 起于 ${kids[i].x}`);
  }
  const r = lay.placed.find((p) => p.key === "r");
  const mid = (kids[0].x + kids[0].w / 2 + kids[kids.length - 1].x + kids[kids.length - 1].w / 2) / 2;
  assert.ok(Math.abs(r.x + r.w / 2 - mid) < 1, "根应居中于子的整体跨度");
  kids.forEach((k) => assert.ok(k.y >= r.y + r.h, "子必须在下层"));
});

test("逐日单位的尺寸：线起点额外留一行放标签", () => {
  const { nodes, branches } = refTree();
  const root = buildDayTree(nodes, branches, null, null);
  const depth = new Map();
  const walk = (n, d) => { depth.set(n.id, d); n.children.forEach((c) => walk(c, d + 1)); };
  walk(root, 0);
  const units = dayTreeToUnits(root, 22, (n) => depth.get(n.id) ?? 0);
  const findUnit = (key, u) => (u.key === key ? u : u.children.map((c) => findUnit(key, c)).find(Boolean));
  const a = findUnit("d8", units);   // 分支A 起点
  const b = findUnit("d9", units);   // 分支B 起点
  const m = findUnit("d1", units);   // 主线起点
  assert.ok(a && b && m);
  assert.ok(a.h >= 22 + 16, "线起点要高出一行（标签）");
  assert.ok(m.h >= 22, "主线起点也要有标签行");
});
