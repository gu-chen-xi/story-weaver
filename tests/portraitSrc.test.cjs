// portrait:// 地址拼装（2026-11-05）：把立绘从 asset 协议换出来之后，
// 前端只负责「文件名 + 平台前缀」，路径安全校验在后端（portrait_protocol::file_from_path）。
const test = require("node:test");
const assert = require("node:assert/strict");
const { portraitSrc } = require("../.tmp/fe-tests/src/lib/portraitSrc.js");

const WIN = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)";

test("Windows 用 http://portrait.localhost", () => {
  assert.equal(portraitSrc("D:\\a\\b\\x.png", WIN), "http://portrait.localhost/x.png");
});

test("mac/linux 用 portrait://localhost", () => {
  assert.equal(portraitSrc("/home/a/x.png", MAC), "portrait://localhost/x.png");
});

test("文件名里的空格与中文要编码", () => {
  assert.equal(portraitSrc("C:\\p\\夏 婉.png", WIN), "http://portrait.localhost/%E5%A4%8F%20%E5%A9%89.png");
});

test("只取文件名（路径前缀不进 URL）", () => {
  assert.equal(portraitSrc("C:\\deep\\dir\\a.png", WIN), "http://portrait.localhost/a.png");
});
