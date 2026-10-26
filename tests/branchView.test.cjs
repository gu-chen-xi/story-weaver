// `lib/branchView.ts` 的单测（`npm test`）。
//
// 为什么值得测：`branchPathIds` 出过两次真事故 ——
//   ① 把"共享历史"当成别分支整段跳过（时间维显示"第 2~11 天"一行带过）；
//   ② 按 branch_id 批量取，把"同父的兄弟分支"也算进共享历史。
// 这两种错在界面上都表现为"时间线少几天"，很容易被当成数据问题。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  visibleForBranch,
  branchColor,
  branchLabelOf,
  branchPathIds,
  forkPickerGroups,
} = require("../.tmp/fe-tests/src/lib/branchView.js");

const node = (id, sort, parent, branch) => ({
  id, sort_order: sort, parent_id: parent, branch_id: branch ?? null,
});

test("visibleForBranch：主线 null 与分支 id 各自隔离", () => {
  const n = node(1, 1, null, null);
  assert.equal(visibleForBranch(n, null), true);
  assert.equal(visibleForBranch(n, "b1"), false);
  const m = node(2, 2, 1, "b1");
  assert.equal(visibleForBranch(m, "b1"), true);
  assert.equal(visibleForBranch(m, null), false);
});

test("branchColor / branchLabelOf：主线与未知分支都有回退", () => {
  const branches = [{ id: "b1", name: "支线一", color: "#123456" }];
  assert.equal(branchColor(branches, null), "#6B7280");
  assert.equal(branchColor(branches, "b1"), "#123456");
  assert.equal(branchColor(branches, "不存在"), "#8B5CF6");
  assert.equal(branchLabelOf(branches, null), "主线");
  assert.equal(branchLabelOf(branches, "b1"), "支线一");
  assert.equal(branchLabelOf(branches, "abcdefghij"), "abcdefgh…");
});

test("branchPathIds：主线 = 全部主线节点 + 父链（不含兄弟分支）", () => {
  // 主线 1→2→3；分支 b1 在 2 处分叉，有自己的 4
  const nodes = [node(1, 1, null, null), node(2, 2, 1, null), node(3, 3, 2, null), node(4, 4, 2, "b1")];
  const branches = [{ id: "b1", fork_node_id: 2 }];
  const ids = branchPathIds(nodes, branches, null);
  // 主线：从最后一个主线节点 3 上溯 → {3,2,1}；不该含 4
  assert.deepEqual([...ids].sort((a, b) => a - b), [1, 2, 3]);
});

test("branchPathIds：分支 = 自己的节点 + 分叉点及以上的共享历史", () => {
  const nodes = [node(1, 1, null, null), node(2, 2, 1, null), node(3, 3, 2, null), node(4, 4, 2, "b1")];
  const branches = [{ id: "b1", fork_node_id: 2 }];
  const ids = branchPathIds(nodes, branches, "b1");
  // 分支：自己的 4 + 从分叉点 2 上溯的 {2,1}；**不该含 3**（那是主线的后续）
  assert.deepEqual([...ids].sort((a, b) => a - b), [1, 2, 4]);
});

test("branchPathIds：空数据 / 分叉点缺失都不炸", () => {
  assert.equal(branchPathIds([], [], null).size, 0);
  const ids = branchPathIds([node(1, 1, null, "b1")], [{ id: "b1", fork_node_id: null }], "b1");
  assert.deepEqual([...ids], [1], "分叉点缺失时至少含本分支自己的节点");
});

test("branchPathIds：成环不死循环（防环守卫）", () => {
  // 1→2→1 的环
  const nodes = [node(1, 1, 2, null), node(2, 2, 1, null)];
  const ids = branchPathIds(nodes, [], null);
  assert.ok(ids.size <= 2, "成环时最多走一圈就停");
});

// ── 分叉节点下拉的分组（2026-10-26：平铺清单里好几个「第 7 天（分支内）」分不清）──

const withLabel = (id, sort, branch, label) => ({
  ...node(id, sort, null, branch),
  date_label: label,
  node_type: "day",
});

test("forkPickerGroups：主线一组、每条分支一组，组内按时间排", () => {
  const nodes = [
    withLabel(3, 3, null, "第 3 天"),
    withLabel(1, 1, null, "第 1 天"),
    withLabel(2, 2, null, "第 2 天"),
    withLabel(12, 12, "b1", "第 7 天"),
    withLabel(11, 11, "b1", "第 6 天"),
    withLabel(21, 21, "b2", "第 7 天"),
  ];
  const branches = [{ id: "b1", name: "角色A线" }, { id: "b2", name: "角色B线" }];
  const groups = forkPickerGroups(nodes, branches);
  assert.deepEqual(groups.map((g) => g.name), ["主线", "角色A线", "角色B线"]);
  assert.deepEqual(groups[0].nodes.map((n) => n.id), [1, 2, 3], "主线组内按 sort_order");
  assert.deepEqual(groups[1].nodes.map((n) => n.id), [11, 12]);
  assert.deepEqual(groups[2].nodes.map((n) => n.id), [21]);
});

test("forkPickerGroups：不同分支同名天数各在各的组（不再混成一片）", () => {
  const nodes = [
    withLabel(11, 11, "b1", "第 7 天"),
    withLabel(21, 11, "b2", "第 7 天"),
    withLabel(31, 11, "b3", "第 7 天"),
  ];
  const branches = [{ id: "b1", name: "甲" }, { id: "b2", name: "乙" }, { id: "b3", name: "" }];
  const groups = forkPickerGroups(nodes, branches);
  assert.equal(groups.length, 3, "三条分支三个组");
  assert.deepEqual(groups.map((g) => g.name), ["甲", "乙", "b3…"], "空名字回退成 id 前 8 位");
});

test("forkPickerGroups：没有节点的主线 / 分支不出空组；分支记录没了的归「未知分支」", () => {
  const nodes = [withLabel(1, 1, null, "第 1 天"), withLabel(9, 9, "gone", "第 9 天")];
  const groups = forkPickerGroups(nodes, [{ id: "b1", name: "空分支" }]);
  assert.deepEqual(groups.map((g) => g.name), ["主线", "未知分支（分支记录已不在）"]);
  assert.deepEqual(groups[1].nodes.map((n) => n.id), [9], "找不到归属也不能丢");
});