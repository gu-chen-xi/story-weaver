// 「导演代价」协议行的剥离（2026-10-27）。
//
// 为什么值得单测：这条协议行是**软件对账用的**，用户不该看见；而它又必须只剥"最后一行"，
// 剥多了就是改用户的剧情（正文里真的可能写到这个词）。剥少了 → 气泡里露出 〔导演代价〕。

const test = require("node:test");
const assert = require("node:assert/strict");

const { stripCostMarker } = require("../.tmp/fe-tests/src/lib/parse.js");

test("剥掉最后一行协议行，正文原样保留", () => {
  const got = stripCostMarker("（她别过脸）「别。」\n〔导演代价〕条件：她要他先道歉");
  assert.equal(got, "（她别过脸）「别。」");
});

test("末尾有多余空行也认（模型常在最后多打一个换行）", () => {
  const got = stripCostMarker("「行吧。」\n〔导演代价〕白给：她懒得争\n\n");
  assert.equal(got, "「行吧。」");
});

test("正文中间的同名文本不剥（那是剧情，不是协议）", () => {
  const body = "（她在账本上写〔导演代价〕三个字）\n「你看。」";
  assert.equal(stripCostMarker(body), body);
});

test("只剩协议行 → 退回原文（不生成空气泡）", () => {
  const only = "〔导演代价〕交换：他答应帮一次忙";
  assert.equal(stripCostMarker(only), only);
});

test("没有协议行 → 原样返回", () => {
  assert.equal(stripCostMarker("普通回复"), "普通回复");
  assert.equal(stripCostMarker(""), "");
});

test("半角冒号 / 类型前有空格 也照样剥", () => {
  assert.equal(stripCostMarker("「嗯。」\n〔导演代价〕时限: 只试三天"), "「嗯。」");
  assert.equal(stripCostMarker("「嗯。」\n〔导演代价〕 压力 ： 被逼的"), "「嗯。」");
});

test("表态标记不受影响（两条协议各管各的）", () => {
  const got = stripCostMarker("「我答应你。」\n〔态度:接受〕");
  assert.equal(got, "「我答应你。」\n〔态度:接受〕", "没有导演代价行就别动它");
});


// ===== 两条协议共存（2026-10-27 实测：收据被态度标记挤到上一行，账就漏了）=====

test("收据后面还跟着态度标记 → 收据照样剥掉，标记留着", () => {
  const got = stripCostMarker("正文\n〔导演代价〕条件：他交出钥匙\n〔态度:拒绝〕");
  assert.equal(got, "正文\n〔态度:拒绝〕");
});

test("收据在态度标记之后（顺序反了）也剥", () => {
  const got = stripCostMarker("正文\n〔态度:接受〕\n〔导演代价〕白给：她没反抗");
  assert.equal(got, "正文\n〔态度:接受〕");
});

test("收据后面还接着正文 → 不在协议区，不剥（正文可能真的写到这个词）", () => {
  const body = "〔导演代价〕白给：她没反抗\n（她转身走了）";
  assert.equal(stripCostMarker(body), body);
});
test("多行正文只删最后一行，其它行一行不少", () => {
  const got = stripCostMarker("第一行\n第二行\n第三行\n〔导演代价〕心软：她撑不住了");
  assert.equal(got, "第一行\n第二行\n第三行");
});
