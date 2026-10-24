// 断链修复的测试（用户实测踩到的坑）。
//
// 现象：图例显示「共 9 个节点 · 3 条分支」，树里只画了主线 4 个。
// 根因：分支节点的 `parent_id` 链没连到根 → 从根不可达 → 原实现只取第一个根，整棵丢掉。
//
// 现在：不可达节点按 `branch_id` 挂回该分支的**分叉点所在段**，并标记 `repaired`。

const test = require("node:test");
const assert = require("node:assert/strict");

const { buildSegments, layoutTree } = require("../.tmp/fe-tests/src/lib/timelineSegments.js");
const { buildDayTree, flattenDays } = require("../.tmp/fe-tests/src/lib/timelineNodes.js");

const node = (id, day, parent, branch = null) => ({
  id,
  date_label: `第 ${day} 天`,
  sort_order: id,
  node_type: "day",
  is_flashback: 0,
  tags: "",
  parent_id: parent,
  branch_id: branch,
});
const branch = (id, name, fork, color = "#8B5CF6") => ({
  id, story_id: "1", name, fork_node_id: fork, source_branch_id: null,
  color, created_at: "2026-10-24",
});

/** 复刻用户那种坏数据：4 个主线节点 + 5 个分支节点，分支节点父链断裂 */
function brokenData() {
  const nodes = [
    node(1, 1, null), node(2, 2, 1), node(3, 3, 2), node(4, 4, 3),
    // 分支 1/2/3 的节点：parent 指向不存在的 id（就是"断链"）
    node(10, 2, 999, "B1"), node(11, 3, 999, "B1"),
    node(20, 2, 999, "B2"),
    node(30, 5, 999, "B3"), node(31, 6, 999, "B3"),
  ];
  const branches = [branch("B1", "123", 2), branch("B2", "321", 2), branch("B3", "321", 3)];
  return { nodes, branches };
}

test("段框：断链节点必须仍然出现在树上（不能凭空消失）", () => {
  const { nodes, branches } = brokenData();
  const root = buildSegments(nodes, branches, null);
  // 统计树上所有段的节点总数
  const total = (() => {
    let n = 0;
    const walk = (s) => { n += s.nodes.length; s.children.forEach(walk); };
    walk(root);
    return n;
  })();
  assert.equal(total, nodes.length, `节点数必须守恒：树上 ${total} / 数据 ${nodes.length}`);

  // 三条分支都要出现
  const names = new Set();
  const walk = (s) => { names.add(s.lineName); s.children.forEach(walk); };
  walk(root);
  for (const b of ["123", "321"]) {
    assert.ok([...names].some((n) => n.startsWith(b)), `分支 ${b} 未出现：${[...names]}`);
  }
});

test("段框：修复出来的段带 repaired 标记（界面要能提示）", () => {
  const { nodes, branches } = brokenData();
  const root = buildSegments(nodes, branches, null);
  const repaired = [];
  const walk = (s) => { if (s.repaired) repaired.push(s.lineName); s.children.forEach(walk); };
  walk(root);
  assert.ok(repaired.length >= 2, `应有修复段：${repaired}`);
});

test("段框：坏数据也能量出布局（不返回 null、宽高为正）", () => {
  const { nodes, branches } = brokenData();
  const root = buildSegments(nodes, branches, null);
  const lay = layoutTree(root, { boxW: 180, rowH: 16, padY: 9, gapX: 18, gapY: 30, expanded: new Set() });
  assert.ok(lay.boxes.length >= 3, "至少有主线 + 两条修复段");
  assert.ok(lay.width > 0 && lay.height > 0);
});

test("逐日：断链节点也必须出现在药丸树里", () => {
  const { nodes, branches } = brokenData();
  const root = buildDayTree(nodes, branches, null, null);
  const all = flattenDays(root);
  assert.equal(all.length, nodes.length, `药丸数必须等于节点数：${all.length} / ${nodes.length}`);
  assert.ok(all.some((n) => n.repaired), "应有 repaired 标记");
});

test("正常数据不该被误标 repaired（不能把好数据也当断链）", () => {
  const nodes = [
    node(1, 1, null), node(2, 2, 1),
    node(3, 2, 2, "B1"),
  ];
  const branches = [branch("B1", "分支A", 2)];
  const seg = buildSegments(nodes, branches, null);
  const walk = (s) => { assert.equal(s.repaired, false, `${s.lineName} 不该被标成修复`); s.children.forEach(walk); };
  walk(seg);
  assert.equal(flattenDays(buildDayTree(nodes, branches, null, null)).length, nodes.length);
});
test("主线断链：接在主线末端，不能挂到根造成假分叉", () => {
  // 用户实测形态：7 个节点、0 条分支；第 5 天的 parent_id 断了
  const nodes = [
    node(1, 1, null), node(2, 2, 1), node(3, 3, 2), node(4, 4, 3),
    node(5, 5, 999), node(6, 6, 5), node(7, 7, 6),   // 5 的父链断（999 不存在）
  ];
  const seg = buildSegments(nodes, [], null);
  // 主线断链应**并进同一个段**：一个框、没有子段（不是两条从根分出的枝）
  assert.equal(seg.children.length, 0, `主线断链不该造出分叉：${seg.children.length} 个子段`);
  assert.equal(seg.repaired, true, "该段要标 repaired（界面要提示父链断开）");
  assert.deepEqual(seg.nodes.map((n) => n.id), [1, 2, 3, 4, 5, 6, 7], "第 5–7 天要接在主线后面");
  assert.equal(seg.nodes.length, nodes.length, "节点守恒");

  // 逐日模式同样必须是一条链（不是两根从根分出的枝）
  const day = buildDayTree(nodes, [], null, null);
  assert.equal(day.children.length, 1, "逐日：主线断链同样不该造出分叉");
  assert.deepEqual(flattenDays(day).map((n) => n.id), [1, 2, 3, 4, 5, 6, 7]);
});

test("分支断链：仍然挂到分叉点（不能接到主线末端）", () => {
  const nodes = [
    node(1, 1, null), node(2, 2, 1), node(3, 3, 2),
    node(10, 3, 999, "B1"),   // 分支节点父链断
  ];
  const branches = [branch("B1", "分支A", 2)];
  const seg = buildSegments(nodes, branches, null);
  const repaired = seg.children.find((c) => c.repaired);
  assert.ok(repaired, "分支断链也要挂回来");
  assert.equal(repaired.branchId, "B1");
  assert.equal(repaired.depth, seg.depth + 1, "分支应在分叉点下一层");
});