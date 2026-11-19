// 自检脚本的报告落盘守卫（2026-11-19，错误合集 E-PROC-08）。
//
// 背景：3.16.0 发布说明里记着"控制台说报告已保存，但 debug-diagnostics\ 里没有落盘"。
// 根因是 Buffer 从没初始化 + 空 ArrayList 在 PowerShell 里是"假"。这里用源码断言把坑焊死。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const file = path.join(__dirname, "..", "scripts", "selfcheck", "selfcheck.ps1");
const src = fs.readFileSync(file, "utf8");

test("Buf 真的初始化成 ArrayList（不是 $null）", () => {
  assert.match(src, /\$script:Buf = New-Object System\.Collections\.ArrayList/, "Buf 没初始化，报告永远是空的");
});

test("不许再出现裸 if ($script:Buf)（空 ArrayList 为假）", () => {
  assert.ok(!/if \(\$script:Buf\)/.test(src), "守卫要写 $null -ne $script:Buf");
});

test("先写盘、再宣布「报告已保存」", () => {
  const write = src.indexOf("Out-File -LiteralPath $REPORT");
  const tell = src.indexOf('Write-Host ("  报告已保存');
  assert.ok(write > 0 && tell > 0, "缺少写盘或提示");
  assert.ok(write < tell, "提示写在写盘之前 —— 失败时会骗人");
});
