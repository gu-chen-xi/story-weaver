// 「开发者模式 · 请求快照」（2026-10-31 重构）。
//
// 只保留一件事：**只读查看“当时发给模型的原文”**。
// 为什么不是看剧情区 / prompt 预算：那两处都会按展示需要裁剪、分段；
// 只有 debug_requests 里的 system_prompt / history_json / user_text / tail_block 是原样。
//
// 数据本来就存了（每个请求一条，生成前自动只留最新 N 条），这一页不采集新东西。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { T } from "../theme";
import { useToast } from "../toast";
import type { Story } from "../types";

type Row = {
  id: number; created_at: string; model: string; node_id: number | null; status: string;
  prompt_tokens: number; completion_tokens: number; estimated_tokens: number;
  system_chars: number; history_chars: number; user_chars: number; tail_chars: number; preview: string;
};
type Detail = {
  id: number; created_at: string; model: string; node_id: number | null; status: string; error: string;
  prompt_tokens: number; completion_tokens: number;
  system_prompt: string; history_json: string; user_text: string; tail_block: string; ai_reply: string;
};

type Tab = "system" | "history" | "user" | "tail" | "reply";
const TABS: [Tab, string][] = [["system", "系统提示词"], ["history", "历史"], ["user", "本轮输入"], ["tail", "尾部协议"], ["reply", "AI 回复"]];

export function DevToolsPage({ currentStory }: { currentStory: Story | null }) {
  const toast = useToast();
  const [rows, setRows] = useState<Row[]>([]);
  const [sel, setSel] = useState<number | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [tab, setTab] = useState<Tab>("system");

  const load = async () => {
    if (!currentStory) { setRows([]); setDetail(null); return; }
    try {
      const list = await invoke<Row[]>("list_prompt_snapshots", { storyId: String(currentStory.id), limit: 50 });
      setRows(list);
      if (list.length && !list.some((r) => r.id === sel)) setSel(list[0].id);
    } catch (e) { toast("❌ 快照列表读取失败：" + String(e)); }
  };
  useEffect(() => { void load(); }, [currentStory?.id]);
  useEffect(() => {
    if (!currentStory || sel == null) { setDetail(null); return; }
    void invoke<Detail | null>("get_prompt_snapshot", { storyId: String(currentStory.id), snapshotId: sel })
      .then((d) => { setDetail(d); })
      .catch((e) => toast("❌ 快照读取失败：" + String(e)));
  }, [sel, currentStory?.id]);

  const text = detail
    ? tab === "system" ? detail.system_prompt
    : tab === "history" ? detail.history_json
    : tab === "user" ? detail.user_text
    : tab === "tail" ? detail.tail_block
    : detail.ai_reply
    : "";

  const copy = async () => {
    try { await navigator.clipboard.writeText(text); toast("📋 已复制（" + text.length + " 字符）"); }
    catch { toast("⚠ 复制失败：浏览器不允许剪贴板"); }
  };

  return (
    <div style={{ flex: 1, display: "flex", minWidth: 0, height: "100%" }}>
      {/* 左：请求列表 */}
      <div style={{ width: 320, borderRight: `1px solid ${T.line}`, display: "flex", flexDirection: "column", minHeight: 0 }}>
        <div style={{ padding: "12px 12px 8px", display: "flex", alignItems: "center", gap: 8 }}>
          <div style={{ fontSize: 13, fontWeight: 700 }}>请求快照</div>
          <div style={{ fontSize: 10.5, color: T.text3 }}>{rows.length} 条（最新在前）</div>
          <div style={{ flex: 1 }} />
          <button onClick={() => void load()}
            style={{ height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>刷新</button>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: "0 8px 12px" }}>
          {!currentStory && <div style={{ padding: 10, fontSize: 12, color: T.text3 }}>先选一个故事。</div>}
          {currentStory && rows.length === 0 && (
            <div style={{ padding: 10, fontSize: 12, color: T.text3 }}>还没有请求记录 —— 发一轮对话就会有一条。</div>
          )}
          {rows.map((r) => (
            <button key={r.id} onClick={() => setSel(r.id)}
              style={{ display: "block", width: "100%", textAlign: "left", marginBottom: 4, padding: "7px 9px", borderRadius: 8, cursor: "pointer", fontFamily: "inherit",
                border: `1px solid ${sel === r.id ? T.accent : T.lineSoft}`, background: sel === r.id ? T.accentSoft : "transparent", color: T.text2 }}>
              <div style={{ fontSize: 10.5, color: T.text3, display: "flex", gap: 6 }}>
                <span>#{r.id}</span><span>{r.created_at}</span>
                <span style={{ marginLeft: "auto" }}>{r.status}</span>
              </div>
              <div style={{ fontSize: 11.5, marginTop: 3, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {r.preview || "（空输入）"}
              </div>
              <div style={{ fontSize: 10, color: T.text3, marginTop: 2 }}>
                {r.prompt_tokens || r.estimated_tokens} tok ｜ 系统{r.system_chars} 历史{r.history_chars} 尾{r.tail_chars}
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* 右：原文 */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0 }}>
        <div style={{ padding: "10px 14px", borderBottom: `1px solid ${T.lineSoft}`, display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
          {TABS.map(([k, label]) => (
            <button key={k} onClick={() => setTab(k)}
              style={{ height: 24, padding: "0 10px", borderRadius: 6, cursor: "pointer", fontFamily: "inherit", fontSize: 11,
                border: `1px solid ${tab === k ? T.accent : T.line}`, background: tab === k ? T.accentSoft : "transparent",
                color: tab === k ? T.accent : T.text2 }}>
              {label}
            </button>
          ))}
          <div style={{ flex: 1 }} />
          <span style={{ fontSize: 10.5, color: T.text3 }}>{text.length} 字符</span>
          <button onClick={() => void copy()} disabled={!detail}
            style={{ height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>复制</button>
        </div>
        {detail && (
          <div style={{ padding: "6px 14px", fontSize: 10.5, color: T.text3, borderBottom: `1px solid ${T.lineSoft}` }}>
            {detail.model} ｜ 节点 {detail.node_id ?? "-"} ｜ prompt {detail.prompt_tokens} / completion {detail.completion_tokens}
            {detail.error ? ` ｜ ❌ ${detail.error}` : ""}
          </div>
        )}
        <pre style={{ flex: 1, margin: 0, padding: 14, overflow: "auto", whiteSpace: "pre-wrap", wordBreak: "break-word",
          fontSize: 11.5, lineHeight: 1.65, color: T.text1, fontFamily: "ui-monospace, Consolas, monospace" }}>
          {detail ? (text || "（这一段是空的）") : (currentStory ? "左边选一条请求" : "")}
        </pre>
      </div>
    </div>
  );
}
