// 分叉判定：`branches.fork_node_id` 也算分叉（用户实测：嵌套分支只有一条孩子链）
//
// 现场：从6分支的第 8 天(id13) 只连了一个孩子 id25（嵌套子分支），
// 所以"多孩子"判据不触发 → 9/10 天被并进父段（父段显示成"第7—10天 4 个节点"）。

const test = require("node:test");
const assert = require("node:assert/strict");

const { buildSegments } = require("../.tmp/fe-tests/src/lib/timelineSegments.js");
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
const branch = (id, name, fork) => ({
  id, story_id: "1", name, fork_node_id: fork, source_branch_id: null,
  color: "#3B82F6", created_at: "2026-10-24",
});

/** 复刻用户库里的形态：父分支在 13 只连了一个孩子（嵌套子分支） */
function nested() {
  const nodes = [
    node(1, 1, null), node(2, 2, 1), node(3, 3, 2), node(4, 4, 3),
    node(5, 5, 4), node(6, 6, 5),          // 主线 1..6
    node(12, 7, 6, "PARENT"),              // 父分支：第 7 天
    node(13, 8, 12, "PARENT"),             // 父分支：第 8 天（嵌套分支的分叉点）
    node(25, 9, 13, "CHILD"),              // 嵌套子分支：第 9 天
    node(26, 10, 25, "CHILD"),             // 嵌套子分支：第 10 天
  ];
  const branches = [branch("PARENT", "从6分支", 6), branch("CHILD", "从6分支的嵌套", 13)];
  return { nodes, branches };
}

test("父分支不该把嵌套子分支的节点吞进自己的段", () => {
  const { nodes, branches } = nested();
  const seg = buildSegments(nodes, branches, null);
  // 找父分支的段
  const all = [];
  const walk = (s) => { all.push(s); s.children.forEach(walk); };
  walk(seg);
  const parent = all.find((s) => s.branchId === "PARENT");
  assert.ok(parent, "父分段的段应存在");
  assert.deepEqual(parent.nodes.map((n) => n.id), [12, 13], "父段只该有第 7、8 天");
  assert.equal(parent.hasFork, true, "第 8 天是嵌套分支的分叉点，父段应标 hasFork");
  const child = all.find((s) => s.branchId === "CHILD");
  assert.ok(child, "嵌套子分支应有自己的段");
  assert.deepEqual(child.nodes.map((n) => n.id), [25, 26], "子段是第 9、10 天");
});

test("节点守恒：分段后一个都不少", () => {
  const { nodes, branches } = nested();
  const seg = buildSegments(nodes, branches, null);
  let n = 0;
  const walk = (s) => { n += s.nodes.length; s.children.forEach(walk); };
  walk(seg);
  assert.equal(n, nodes.length);
});

test("逐日模式：第 8 天要标成 ★ 分叉点", () => {
  const { nodes, branches } = nested();
  const root = buildDayTree(nodes, branches, null, null);
  const byId = new Map(flattenDays(root).map((n) => [n.id, n]));
  assert.equal(byId.get(13).isFork, true, "只有一条孩子链也要标分叉点");
  assert.equal(byId.get(25).isLineStart, true, "子分支第一个节点是线起点");
  assert.equal(byId.get(25).lineName.includes("从6分支的嵌套"), true);
});
