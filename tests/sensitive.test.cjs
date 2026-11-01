// 敏感词过滤的单元测试（2026-11-01 新增，此前这块**一条用例都没有**）。
//
// 要守住的三件事：
//   ① 该拦的拦住 —— 大小写、全角、插入空格/符号、常见谐音变体；
//   ② 不该拦的别拦 —— "妈妈的""狗日子""体操你们""USB""自我操作"；
//   ③ 流式跨 chunk 不漏 —— 中文流常按字切，"傻" 和 "逼" 分两个 delta 到。
//
// 运行：npm test

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  SENSITIVE_RULES,
  STREAM_HOLDBACK,
  findSensitive,
  hasSensitive,
  normalizeForMatch,
  sanitizeStream,
  sanitizeText,
} = require("../.tmp/fe-tests/src/lib/sensitive.js");

test("原有词表全部仍然拦得住", () => {
  for (const w of ["傻逼", "操你妈", "他妈的", "混蛋", "王八蛋", "婊子", "杂种", "死全家", "草泥马", "废物点心"]) {
    assert.equal(hasSensitive(w), true, w + " 应该命中");
  }
});

test("归一化：大小写 / 全角 / 插入分隔符都拦得住", () => {
  for (const s of ["你个傻b", "傻 逼", "傻-逼", "傻·逼", "操 你 妈", "ＣＮＭ", "ｓｂ"]) {
    assert.equal(hasSensitive(s), true, s + " 应该命中");
  }
});

test("谐音与字母缩写", () => {
  for (const s of ["傻屄", "沙比", "煞笔", "SB", "sb", "nmsl", "tmd", "wcnm"]) {
    assert.equal(hasSensitive(s), true, s + " 应该命中");
  }
});

test("放行：妈妈的 / 狗日子 / 体操你们（原来的误伤）", () => {
  for (const s of ["她回到妈妈的房间", "妈妈的怀抱", "你妈妈的身体好点了吗", "狗日子不好过", "把体操你们练完"]) {
    assert.equal(hasSensitive(s), false, s + " 不该命中");
  }
});

test("放行：USB / 操场 / 自我操作", () => {
  for (const s of ["USB 接口坏了", "她在操场上站着", "自我操作并不容易"]) {
    assert.equal(hasSensitive(s), false, s + " 不该命中");
  }
});

test("放行：牛杂碎 / 杂种优势（食物与生物学，不是骂人）", () => {
  for (const s of ["牛杂碎汤真好喝", "杂种优势是生物学概念", "夜市那家卖羊杂碎"]) {
    assert.equal(hasSensitive(s), false, s + " 不该命中");
  }
  // 但骂人的形态照拦
  for (const s of ["你个杂碎", "那畜牲跑了", "他妈的杂种"]) {
    assert.equal(hasSensitive(s), true, s + " 应该命中");
  }
});

test("findSensitive 回显原文里的那一段（给人看）", () => {
  const hit = findSensitive("你这个 傻B 东西");
  assert.ok(hit);
  assert.equal(hit.word, "傻B");
  assert.equal(findSensitive("今天天气不错"), null);
});

test("sanitizeText 只换命中部分，其余一字不动", () => {
  const src = "她说：你个傻逼，然后转身走了。";
  const out = sanitizeText(src);
  assert.ok(out.includes("***"));
  assert.ok(!out.includes("傻逼"));
  assert.ok(out.startsWith("她说：你个"));
  assert.ok(out.endsWith("，然后转身走了。"));
});

test("sanitizeText 对没有命中的长文本原样返回", () => {
  const src = "他在窗边站了很久，风把窗帘吹起来，像一面慢慢鼓起的帆。";
  assert.equal(sanitizeText(src), src);
  assert.equal(sanitizeText(""), "");
});

test("流式：词被拆到两个 chunk 也不漏", () => {
  let carry = "";
  let out = "";
  for (const ch of "你个傻逼啊") {
    const r = sanitizeStream(carry, ch);
    carry = r.carry;
    out += r.out;
  }
  out += sanitizeStream(carry, "", true).out;
  assert.ok(out.includes("***"), "应该出现掩码，实际：" + out);
  assert.ok(!out.includes("傻逼"), "原文不该漏出去，实际：" + out);
});

test("流式：正常文本一个字都不能丢", () => {
  const src = "今天下雨了，她在窗边站了很久，一句话也没说。";
  let carry = "";
  let out = "";
  for (const ch of src) {
    const r = sanitizeStream(carry, ch);
    carry = r.carry;
    out += r.out;
  }
  out += sanitizeStream(carry, "", true).out;
  assert.equal(out, src);
});

test("流式：留字长度要够覆盖最长的词", () => {
  const longest = Math.max(...SENSITIVE_RULES.map((r) => r.w.length));
  assert.ok(STREAM_HOLDBACK >= longest, `留字 ${STREAM_HOLDBACK} 小于最长词 ${longest}`);
});

test("归一化会剥掉分隔符但保留下标映射", () => {
  const { text, map } = normalizeForMatch("傻 逼");
  assert.equal(text, "傻逼");
  assert.deepEqual(map, [0, 2]);
});

test("词表本身：没有空词、没有重复", () => {
  const seen = new Set();
  for (const r of SENSITIVE_RULES) {
    assert.ok(r.w.length > 0);
    assert.equal(r.w, r.w.normalize("NFKC").toLowerCase(), r.w + " 应该是归一化后的形态");
    assert.equal(seen.has(r.w), false, r.w + " 重复了");
    seen.add(r.w);
  }
  assert.ok(SENSITIVE_RULES.length >= 60, "词表不应少于 60 条");
});
