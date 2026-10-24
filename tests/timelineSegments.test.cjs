// `lib/timelineSegments.ts` 的布局单测（npm test）。
//
// 为什么测：这是"纯函数算坐标"的层 —— 段划分错了，界面上表现为
// "分支看不见/顺序怪/框重叠"，但它**不会报错**，只会让用户觉得时间线没法用。
// 文档明确点出的四类边界（长段/空段/倒挂/分叉归属）逐条锁住。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  buildSegments,
  layoutTree,
  dotsOf,
  dayOfLabel,
  rowsOf,
  dotsAria,
  DOT_CAP,
} = require("../.tmp/fe-tests/src/lib/timelineSegments.js");

// 造节点：id 自增，parent 显式给
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

const branch = (id, name, fork, color = "#8B5CF6") => ({
  id, story_id: "1", name, fork_node_id: fork, source_branch_id: null,
  color, created_at: "2026-10-24",
});

const OPTS = {
  boxW: 186, rowH: 16, padY: 9, gapX: 18, gapY: 30,
  expanded: new Set(),
};

test("dayOfLabel：只认「第 N 天」，不猜", () => {
  assert.equal(dayOfLabel("第 7 天"), 7);
  assert.equal(dayOfLabel("插曲 2"), -1);
  assert.equal(dayOfLabel(""), -1);
});

test("线性故事：只有一个段，无分叉", () => {
  const nodes = [node(1, 1, null), node(2, 2, 1), node(3, 3, 2)];
  const seg = buildSegments(nodes, [], null);
  assert.equal(seg.children.length, 0, "不该有子段");
  assert.equal(seg.nodes.length, 3);
  assert.equal(seg.hasFork, false);
  assert.equal(seg.rangeLabel, "第 1 天—第 3 天");
});

test("中部建分支：根段收在分叉点，分支与「主线续写」并列成兄弟", () => {
  // 1→2→3（主线），节点 2 处分叉出 A；主线从 2 继续 4
  const nodes = [
    node(1, 1, null), node(2, 2, 1),        // 根段：1 → 2
    node(3, 3, 2),                           // 主线续写（parent = 2）
    node(4, 2, 2, "BR-A"),                   // 分支 A（也从 2 分叉）
    node(5, 4, 3),                           // 主线再往后（挂在 3 上，不制造新分叉）
  ];
  const branches = [branch("BR-A", "分支A", 2)];
  const seg = buildSegments(nodes, branches, null);
  // 根段：1,2（2 是分叉点，归入本段末尾）
  assert.deepEqual(seg.nodes.map((n) => n.id), [1, 2]);
  assert.equal(seg.hasFork, true, "分叉点归入本段末尾");
  // 子段 = 主线续写 + 分支A，二者**并列**
  assert.equal(seg.children.length, 2);
  const lines = seg.children.map((c) => c.branchId).sort();
  assert.deepEqual(lines, [null, "BR-A"].sort(), "主线续写与分支必须是并列兄弟");
});

test("空段：分支已建但没有节点 → 出现占位框（不能过滤掉）", () => {
  const nodes = [node(1, 1, null), node(2, 2, 1)];
  const branches = [branch("BR-X", "空分支", 2)];
  const seg = buildSegments(nodes, branches, null);
  const empties = seg.children.filter((c) => c.empty);
  assert.equal(empties.length, 1, "必须保留空段占位");
  assert.equal(empties[0].rangeLabel, "尚未开始");
  assert.equal(empties[0].branchId, "BR-X");
});

test("范围倒挂：按父链顺序，标签允许「第 20 天—第 9 天」", () => {
  const nodes = [node(1, 20, null), node(2, 9, 1), node(3, 11, 2)];
  const seg = buildSegments(nodes, [], null);
  assert.equal(seg.rangeLabel, "第 20 天—第 11 天", "按父链，不按天数字段重排");
  assert.equal(dotsAria(seg).includes("倒挂"), true, "说明里应提示这是正常的");
});

test("长段：点阵封顶，中间用 ⋯（框宽与节点数解耦）", () => {
  const many = Array.from({ length: 100 }, (_, i) => node(i + 1, i + 1, i === 0 ? null : i));
  const seg = buildSegments(many, [], null);
  const dots = dotsOf(seg.nodes);
  assert.equal(dots.length, DOT_CAP, "点阵长度固定");
  assert.equal(dots[5], "⋯", "省略号在第 6 位");
  assert.equal(dots.filter((d) => d === "●").length, DOT_CAP - 1);
});

test("分叉点用 ★ 标在点阵里", () => {
  const nodes = [node(1, 1, null), node(2, 2, 1), node(3, 3, 1, "BR-A")];
  const seg = buildSegments(nodes, [branch("BR-A", "A", 1)], null);
  assert.equal(seg.nodes[0].isFork, true, "节点 1 有两个后继");
  assert.equal(dotsOf(seg.nodes)[0], "★");
});

test("布局：叶按宽度累加，父居中于子，层间不重叠", () => {
  // 根 1 分叉出三条分支（都在节点 1 之后）→ 三个叶
  const nodes = [
    node(1, 1, null),
    node(2, 2, 1, "BR-A"),
    node(3, 2, 1, "BR-B"),
    node(4, 2, 1, "BR-C"),
  ];
  const branches = [branch("BR-A", "A", 1), branch("BR-B", "B", 1), branch("BR-C", "C", 1)];
  const seg = buildSegments(nodes, branches, null);
  const layout = layoutTree(seg, OPTS);
  const leaves = layout.boxes.filter((b) => b.seg.depth === 1);
  assert.equal(leaves.length, 3);
  const xs = leaves.map((b) => b.x).sort((a, b) => a - b);
  for (let i = 1; i < xs.length; i++) {
    assert.ok(xs[i] - xs[i - 1] >= OPTS.boxW, `兄弟框重叠：${xs[i - 1]} vs ${xs[i]}`);
  }
  const root = layout.boxes.find((b) => b.seg.depth === 0);
  const mid = (xs[0] + xs[xs.length - 1]) / 2 + OPTS.boxW / 2;
  assert.ok(Math.abs(root.x + root.w / 2 - mid) < 1, "父框应居中于子");
  // 层：根的底边在子框顶边之上
  for (const l of leaves) {
    assert.ok(l.y >= root.y + root.h, "子框必须在父框下方");
  }
});

test("展开态：框变高，且不会压到下一层", () => {
  const nodes = [node(1, 1, null), node(2, 2, 1), node(3, 3, 1, "BR-A")];
  const seg = buildSegments(nodes, [branch("BR-A", "A", 1)], null);
  const collapsed = layoutTree(seg, OPTS);
  const expanded = layoutTree(seg, { ...OPTS, expanded: new Set([seg.key]) });
  const cRoot = collapsed.boxes.find((b) => b.seg.depth === 0);
  const eRoot = expanded.boxes.find((b) => b.seg.depth === 0);
  assert.ok(eRoot.h > cRoot.h, "展开后框更高");
  const child = expanded.boxes.find((b) => b.seg.depth === 1);
  assert.ok(child.y >= eRoot.y + eRoot.h, "下一层要被推下去，不能压住");
});

test("空数据 / 成环数据都不炸", () => {
  assert.equal(buildSegments([], [], null), null);
  // 1 → 2 → 1 的环（脏数据）
  const cyc = [node(1, 1, 2), node(2, 2, 1)];
  const seg = buildSegments(cyc, [], null);
  assert.ok(seg !== null, "成环也要给出结果，不能返回 null 让界面空白");
  assert.ok(seg.nodes.length >= 1);
});

test("rowsOf：基准 3 行，分叉 +1，当前 +1", () => {
  const nodes = [node(1, 1, null), node(2, 2, 1), node(3, 3, 2)];
  const plain = buildSegments(nodes, [], "none");
  assert.equal(rowsOf(plain, new Set()), 3);
  const cur = buildSegments(nodes, [], null);
  assert.equal(cur.isCurrent, true);
  assert.equal(rowsOf(cur, new Set()), 4, "当前线 +1 行");
});
