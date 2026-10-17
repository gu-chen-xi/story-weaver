// 时间线树扁平化的单元测试（`buildTreeRows`）。
//
// 为什么单独测它：树状图的"收起/展开"曾经因为**用错了数据源**而完全不生效
// （表格用折叠无关的那份，区间条用折叠感知的那份）——
// 现在只有一个函数、一个参数控制，语义必须钉死。

const test = require("node:test");
const assert = require("node:assert/strict");

const { buildTreeRows } = require("../.tmp/fe-tests/src/lib/timelineTree.js");

/** 造一个节点（只填 buildTreeRows 关心的字段） */
function nd(id, parent, sort, extra) {
  return Object.assign(
    { id, parent_id: parent, sort_order: sort, date_label: `第 ${id} 天`, node_type: "day", branch_id: null },
    extra || {},
  );
}

test("buildTreeRows：单链展开成 1..N 行，深度依次递增", () => {
  const nodes = [nd(1, null, 1), nd(2, 1, 2), nd(3, 2, 3)];
  const rows = buildTreeRows(nodes, null);
  assert.deepEqual(rows.map((r) => r.n.id), [1, 2, 3]);
  assert.deepEqual(rows.map((r) => r.depth), [0, 1, 2]);
  assert.deepEqual(rows.map((r) => r.hasKids), [true, true, false]);
});

test("buildTreeRows：collapsed 里的节点 → 它和它的子孙都不出现", () => {
  const nodes = [nd(1, null, 1), nd(2, 1, 2), nd(3, 2, 3)];
  const rows = buildTreeRows(nodes, new Set([2]));
  assert.deepEqual(rows.map((r) => r.n.id), [1, 2], "第 3 天是第 2 天的子节点，应被收起");
});

test("buildTreeRows：collapsed=null → 无视折叠（区间条用这个）", () => {
  const nodes = [nd(1, null, 1), nd(2, 1, 2), nd(3, 2, 3)];
  const rows = buildTreeRows(nodes, null);
  assert.equal(rows.length, 3, "传 null 时必须完全展开，否则用户会丢日期按钮");
});

test("buildTreeRows：分叉 —— 收起分叉点会同时藏掉两条子线", () => {
  //   1
  //   └ 2 (分叉)
  //     ├ 3
  //     └ 4
  const nodes = [nd(1, null, 1), nd(2, 1, 2), nd(3, 2, 3), nd(4, 2, 4)];
  const all = buildTreeRows(nodes, null);
  assert.deepEqual(all.map((r) => r.n.id), [1, 2, 3, 4]);

  const collapsed = buildTreeRows(nodes, new Set([2]));
  assert.deepEqual(collapsed.map((r) => r.n.id), [1, 2], "分叉点收起后两条子线都不显示");
  assert.equal(collapsed[1].hasKids, true, "收起的分叉点仍应标记 hasKids（界面要画 ▸）");
});

test("buildTreeRows：父节点不存在 → 当作根节点，不丢节点", () => {
  const nodes = [nd(1, null, 1), nd(9, 999, 2)];
  const rows = buildTreeRows(nodes, null);
  assert.equal(rows.length, 2, "父 id 指向不存在的节点时不能把该节点丢掉");
  assert.deepEqual(rows.map((r) => r.n.id), [1, 9]);
  assert.deepEqual(rows.map((r) => r.depth), [0, 0]);
});

test("buildTreeRows：脏数据成环时不死循环，且不会渲染成空白", () => {
  // 1 → 2 → 1（互为父）：没有任何节点是根
  const nodes = [nd(1, 2, 1), nd(2, 1, 2)];
  const rows = buildTreeRows(nodes, null);
  // 关键：不能是 0 行 —— 空白树比"显示得不对"更难排查
  assert.equal(rows.length, 2, "成环时应兜底把所有节点当根，实际 " + rows.length + " 行");
  assert.deepEqual(rows.map((r) => r.n.id), [1, 2]);
});

test("buildTreeRows：同级按 sort_order 排，order 相同再按 id", () => {
  const nodes = [nd(1, null, 1), nd(3, 1, 5), nd(2, 1, 5), nd(4, 1, 2)];
  const rows = buildTreeRows(nodes, null);
  assert.deepEqual(rows.map((r) => r.n.id), [1, 4, 2, 3], "sort_order 5 的两条按 id 升序");
});

test("buildTreeRows：空输入 → 空数组", () => {
  assert.deepEqual(buildTreeRows([], null), []);
  assert.deepEqual(buildTreeRows([], new Set()), []);
});
