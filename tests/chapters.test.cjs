// 章节纯逻辑（对应 src/lib/chapters.ts）：重点是**按钮传参**别出错。
const test = require("node:test");
const assert = require("node:assert");
const { toggleNodeId, selectionSummary, canCreateChapter, ownerChapterOf, wordsLabel } = require("../.tmp/fe-tests/src/lib/chapters.js");

test("toggleNodeId：加入保序、再点移除、不重复", () => {
  let s = [];
  s = toggleNodeId(s, 3);
  s = toggleNodeId(s, 1);
  s = toggleNodeId(s, 2);
  assert.deepStrictEqual(s, [3, 1, 2], "按点击顺序，不排序");
  s = toggleNodeId(s, 1);
  assert.deepStrictEqual(s, [3, 2]);
  s = toggleNodeId(s, 3);
  assert.deepStrictEqual(s, [2], "不会重复添加");
});

test("selectionSummary：标签按节点顺序取，缺失用 #id", () => {
  const nodes = [{ id: 1, date_label: "第 1 天" }, { id: 2, date_label: "第 2 天" }];
  const r = selectionSummary([2, 1, 9], nodes);
  assert.equal(r.count, 3);
  assert.deepStrictEqual(r.labels, ["第 2 天", "第 1 天", "#9"]);
  assert.match(r.text, /已选 3 天/);
  assert.equal(selectionSummary([], nodes).text, "还没选天");
});

test("canCreateChapter：标题和天数都要有", () => {
  assert.equal(canCreateChapter("", [1]).ok, false);
  assert.equal(canCreateChapter("  ", [1]).ok, false);
  assert.equal(canCreateChapter("序雾", []).ok, false);
  assert.equal(canCreateChapter("序雾", [1]).ok, true);
});

test("ownerChapterOf：多对多允许重复，但要能查出来", () => {
  const chs = [{ order_no: 1, node_ids: [1, 2] }, { order_no: 2, node_ids: [3] }];
  assert.equal(ownerChapterOf(2, chs), 1);
  assert.equal(ownerChapterOf(3, chs), 2);
  assert.equal(ownerChapterOf(4, chs), null);
});

test("wordsLabel：万为单位", () => {
  assert.equal(wordsLabel(0), "0 字");
  assert.equal(wordsLabel(9999), "9999 字");
  assert.equal(wordsLabel(12345), "1.23 万字");
});
