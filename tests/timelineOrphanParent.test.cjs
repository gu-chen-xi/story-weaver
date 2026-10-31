// 时间线树：孤儿节点不能变成新的根（2026-10-31 用户实测「两张主线卡」）。
const test = require("node:test");
const assert = require("node:assert");
const { buildDayTree } = require("../.tmp/fe-tests/src/lib/timelineNodes.js");

const node = (id, label, sort, parent, branch = null) => ({
  id, date_label: label, sort_order: sort, parent_id: parent,
  node_type: "day", is_flashback: 0, branch_id: branch, tags: "", day_summary: "",
  created_at: "", messages: 0, parent: null,
});

test("parent 为空的 orphan 挂回同分支前一个节点，不产生第二个根", () => {
  const nodes = [
    node(1, "第 10 天", 10, null),
    node(2, "第 11 天", 11, 1),
    node(3, "第 12 天", 13, null),   // 坏数据：parent 丢了
    node(4, "第 13 天", 14, 3),
  ];
  const root = buildDayTree(nodes, [], null, null);
  assert.equal(root.id, 1, "根应该是最早那天");
  const collect = (n, out = []) => { out.push(n.id); n.children.forEach((c) => collect(c, out)); return out; };
  assert.deepStrictEqual(collect(root), [1, 2, 3, 4], "第 12 天要挂在第 11 天后面，而不是另起一棵树");
});
