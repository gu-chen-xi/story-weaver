// 「🧩 上下文概览」：把"这一次请求到底装配了什么"压缩成一屏能看懂的卡片。
//
// 起因（2026-10-18）：多次排查（阶段摘要有没有进提示词、时间基准有没有生效、
// 原文保留几天）都只能翻「生成参数」的 JSON 或直接查库 —— 太慢。
// 这里把关键事实**先算好摆出来**，一眼就能判断"这次装配对不对"。
//
// 数据来源：debug_requests 最新一条（每次生成都会写）。
// 全部本地计算，不额外发请求。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { computeBudget } from "../lib/parse";
import type { DebugLike } from "../types";
import { T } from "../theme";

type DebugRow = {
  id: number;
  created_at?: string;
  system_prompt: string;
  tail_block?: string;
  history_json: string;
  user_text: string;
  estimated_tokens: number;
  input_budget: number;
  n_used: number;
  context_window: number;
  model?: string;
  status?: string;
};

type Facts = {
  at: string;
  model: string;
  sysTokens: number;
  histTokens: number;
  userTokens: number;
  usedTokens: number;
  inputBudget: number;
  pct: number;
  recallDays: number;      // 【前情提要】几行
  phaseSegments: number;   // 其中"（阶段摘要）"几段
  phaseRange: string;      // 例：1~9
  hasTimeBase: boolean;    // 有没有 【时间基准】
  timeBaseLine: string;    // 时间基准那行
  historyTurns: number;    // 历史条数
  historyDayRange: string; // 历史里出现的天范围（第 X~Y 天）
  hasRecallBlock: boolean; // 有没有【相关旧日记忆】
};

/// 从 system_prompt 里切出某个小节的内容（到下一个【 为止）
function section(text: string, title: string): string {
  const i = text.indexOf(title);
  if (i < 0) return "";
  const rest = text.slice(i + title.length);
  const j = rest.indexOf("【");
  return (j < 0 ? rest : rest.slice(0, j)).trim();
}

/// 从文本里抓出所有"第 N 天"
function dayNumbers(text: string): number[] {
  const out: number[] = [];
  const re = /第\s*(\d{1,4})\s*天/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) out.push(Number(m[1]));
  return out;
}

function buildFacts(dbg: DebugRow, ctxWindow: number): Facts {
  const like: DebugLike = {
    system_prompt: dbg.system_prompt,
    tail_block: dbg.tail_block ?? "",
    history_json: dbg.history_json,
    user_text: dbg.user_text,
    estimated_tokens: dbg.estimated_tokens,
    input_budget: dbg.input_budget,
    context_window: dbg.context_window,
  };
  const b = computeBudget(like, ctxWindow);

  const recap = section(dbg.system_prompt, "【前情提要】");
  const recapLines = recap.split("\n").filter((l) => l.trim().startsWith("- 第"));
  const phaseLines = recapLines.filter((l) => l.includes("（阶段摘要）"));
  let phaseRange = "";
  if (phaseLines.length) {
    const nums = dayNumbers(phaseLines.join("\n"));
    if (nums.length) phaseRange = `${Math.min(...nums)}~${Math.max(...nums)}`;
  }

  const tb = section(dbg.system_prompt, "【时间基准】");
  const tbLine = tb.split("\n").find((l) => l.includes("第 1 天") || l.includes("时代背景")) ?? "";

  const days = dayNumbers(dbg.history_json);
  const historyDayRange = days.length ? `${Math.min(...days)}~${Math.max(...days)}` : "";

  return {
    at: dbg.created_at ?? "",
    model: dbg.model ?? "—",
    sysTokens: b.sysTokens,
    histTokens: b.histTokens,
    userTokens: b.userTokens,
    usedTokens: b.usedTokens,
    inputBudget: b.inputBudget,
    pct: b.pctUsed,
    recallDays: recapLines.length,
    phaseSegments: phaseLines.length,
    phaseRange,
    hasTimeBase: tb.length > 0,
    timeBaseLine: tbLine.replace(/^-\s*/, "").slice(0, 42),
    historyTurns: b.history.length,
    historyDayRange,
    hasRecallBlock: dbg.system_prompt.includes("【相关旧日记忆】") || (dbg.tail_block ?? "").includes("【相关旧日记忆】"),
  };
}

export function ContextPanel(props: { storyId: string | null; ctxWindow: number }) {
  const { storyId, ctxWindow } = props;
  const [f, setF] = useState<Facts | null>(null);
  const [err, setErr] = useState("");

  const load = async () => {
    if (!storyId) { setF(null); return; }
    try {
      const list = await invoke<DebugRow[]>("list_debug_requests", { storyId });
      if (!list.length) { setF(null); setErr(""); return; }
      // 取最新一条（命令已按时间倒序；保险起见按 id 取最大）
      const latest = list.reduce((a, b) => (b.id > a.id ? b : a), list[0]);
      setF(buildFacts(latest, ctxWindow));
      setErr("");
    } catch (e) {
      setErr(String(e));
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [storyId]);

  const row = (k: string, v: React.ReactNode, ok?: boolean) => (
    <div style={{ display: "flex", gap: 6, fontSize: 11, lineHeight: 1.65 }}>
      <span style={{ width: 74, color: T.text3, flexShrink: 0 }}>{k}</span>
      <span style={{ color: ok === false ? T.warn : T.text2, flex: 1, minWidth: 0 }}>{v}</span>
    </div>
  );

  return (
    <div>
      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 8, display: "flex", alignItems: "center", gap: 6 }}>
        <span>🧩 上下文概览</span>
        <span style={{ marginLeft: "auto" }} title="每次生成后自动更新">本次请求</span>
        <span onClick={() => void load()} title="重新读取最新一次请求" style={{ cursor: "pointer" }}>↻</span>
      </div>

      {err && <div style={{ fontSize: 11, color: T.danger }}>读取失败：{err}</div>}
      {!f && !err && (
        <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.7 }}>
          {storyId ? "还没有生成记录 —— 发一条消息后这里会显示这次请求的装配情况。" : "先选一个故事。"}
        </div>
      )}

      {f && (
        <div style={{ display: "flex", flexDirection: "column", gap: 1 }}>
          {row("用量", <>
            <b style={{ color: f.pct > 85 ? T.warn : T.text1 }}>{f.usedTokens.toLocaleString()}</b>
            <span style={{ color: T.text3 }}> / {f.inputBudget.toLocaleString()} token（{f.pct}%）</span>
          </>)}
          {row("分项", <span style={{ color: T.text3 }}>
            系统 {f.sysTokens} · 历史 {f.histTokens} · 本次 {f.userTokens}
          </span>)}
          {row("模型", <span>{f.model}</span>)}
          {row("前情提要",
            <>
              {f.recallDays} 天
              {f.phaseSegments > 0
                ? <span style={{ color: T.accent }}> · 其中 {f.phaseSegments} 段阶段摘要{f.phaseRange ? `（第 ${f.phaseRange} 天）` : ""}</span>
                : <span style={{ color: T.text3 }}> · 全部为日摘要</span>}
            </>,
            f.recallDays > 0)}
          {row("时间基准",
            f.hasTimeBase
              ? <span style={{ color: T.text2 }}>{f.timeBaseLine || "已注入"}</span>
              : <span>未注入（故事没填时间起点？）</span>,
            f.hasTimeBase)}
          {row("历史", <>
            {f.historyTurns} 条
            {f.historyDayRange && <span style={{ color: T.text3 }}> · 第 {f.historyDayRange} 天</span>}
          </>)}
          {row("回顾注入", f.hasRecallBlock
            ? <span style={{ color: T.accent }}>有【相关旧日记忆】</span>
            : <span style={{ color: T.text3 }}>无</span>)}
          {f.at && row("记录时间", <span style={{ color: T.text3 }}>{f.at}</span>)}
        </div>
      )}
    </div>
  );
}
