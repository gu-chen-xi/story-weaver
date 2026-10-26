// DayStrip 的数据筛选（v3.5.4）：显示"这条线能看到的天"。
//
// 规则（与时间线树状图一致）：
//   * 按**父链顺序**展平（不是按天数数字排序 —— 倒挂/插曲会乱）；
//   * 只留 `共享历史（沿父链上溯到分叉点及之前）` + `当前分支自己的节点`；
//   * 主线时只留主线节点。
//
// 组件本身不导出筛选函数（它是 hooks + JSX），所以这里直接复算同一套组合，
// 锁住"用 buildDayTree + flattenDays + 这个过滤条件"这个契约。

const test = require("node:test");
const assert = require("node:assert/strict");

const { buildDayTree, flattenDays } = require("../.tmp/fe-tests/src/lib/timelineNodes.js");
const { branchPathIds } = require("../.tmp/fe-tests/src/lib/branchView.js");

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

/** 与 DayStrip 里的筛选完全一致（2026-10-26：共享历史按父链取，不是"所有主线天"） */
function stripDays(nodes, branches, currentBranchId, currentNodeId) {
  const root = buildDayTree(nodes, branches, currentBranchId, currentNodeId);
  if (!root) return [];
  const visible = branchPathIds(nodes, branches, currentBranchId);
  return flattenDays(root).filter((n) => visible.has(n.id));
}

function fixture() {
  const nodes = [
    node(1, 1, null), node(2, 2, 1), node(3, 3, 2),
    node(10, 4, 3, "B1"), node(11, 5, 10, "B1"),
    node(20, 4, 3, "B2"),
  ];
  const branches = [branch("B1", "分支A", 3), branch("B2", "分支B", 3)];
  return { nodes, branches };
}

test("主线：只显示主线自己的天", () => {
  const { nodes, branches } = fixture();
  const ids = stripDays(nodes, branches, null, 2).map((n) => n.id);
  assert.deepEqual(ids, [1, 2, 3], `主线不该混进分支的天：${ids}`);
});

test("分支：显示共享历史 + 自己那条线的天（不含兄弟分支）", () => {
  const { nodes, branches } = fixture();
  const ids = stripDays(nodes, branches, "B1", 11).map((n) => n.id);
  assert.deepEqual(ids, [1, 2, 3, 10, 11], `分支应含共享历史 + 自己：${ids}`);
  assert.ok(!ids.includes(20), "兄弟分支 B2 的天不该出现");
});

test("顺序按父链（不是按天数数字）—— 倒挂也不重排", () => {
  // 第 20 天 → 第 9 天（倒挂）
  const nodes = [node(1, 20, null), node(2, 9, 1), node(3, 11, 2)];
  const ids = stripDays(nodes, [], null, 1).map((n) => n.id);
  assert.deepEqual(ids, [1, 2, 3], "必须保持父链顺序，不能按天数字排序");
});

test("空数据返回空数组（组件会整条不渲染）", () => {
  assert.deepEqual(stripDays([], [], null, null), []);
});

/**
 * **回归锁（2026-10-26 用户实测）**：主线 1~17 天、从第 6 天分叉出分支（分支只有第 7、8 天）。
 * 切到这个分支时，药丸条必须是「1 2 3 4 5 6 + 分叉后的 7 8（分支色）」，
 * **不能**把主线自己的 7~17 天也画进来（截图里出现过 16、17、7、8 这种错序）。
 */
test("分支：只带分叉点之前的主线天，分叉之后的主线天不算共享历史", () => {
  const nodes = [];
  for (let d = 1; d <= 17; d++) nodes.push(node(100 + d, d, d === 1 ? null : 100 + d - 1));
  // 从第 6 天分叉：分支自己的第 7、8 天
  nodes.push(node(700, 7, 106, "B1"), node(701, 8, 700, "B1"));
  const branches = [branch("B1", "六日分支", 106)];

  const ids = stripDays(nodes, branches, "B1", 701).map((n) => n.id);
  assert.deepEqual(
    ids,
    [101, 102, 103, 104, 105, 106, 700, 701],
    `应为主线的 1~6 天 + 分支的 7、8 天，实际：${ids}`
  );
  assert.ok(!ids.includes(107), "分叉之后的主线第 7 天不该出现");
  assert.ok(!ids.includes(117), "主线第 17 天更不该出现");
});