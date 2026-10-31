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
  id: number; created_at: string; model: string; node_id: number | null; node_label: string; status: string;
  prompt_tokens: number; completion_tokens: number; estimated_tokens: number;
  system_chars: number; history_chars: number; user_chars: number; tail_chars: number; preview: string;
};
type Detail = {
  id: number; created_at: string; model: string; node_id: number | null; status: string; error: string;
  prompt_tokens: number; completion_tokens: number;
  system_prompt: string; history_json: string; user_text: string; tail_block: string; ai_reply: string;
};

type Tab = "system" | "history" | "user" | "tail" | "reply" | "memory" | "recall";
const TABS: [Tab, string][] = [
  ["system", "系统提示词"], ["tail", "尾部协议"], ["memory", "关键记忆"], ["recall", "回顾检索"],
  ["history", "历史"], ["user", "本轮输入"], ["reply", "AI 回复"],
];


/// 把一段文本里的 【…】 段标出来（协议段/日期头都长这样），用来在快照里一眼找到分段。
function RichText({ text }: { text: string }) {
  if (!text) return <span style={{ color: T.text3 }}>（空）</span>;
  const parts = text.split(/(【[^】]*】)/g);
  return (
    <>
      {parts.map((p, i) =>
        /^【.*】$/.test(p) ? (
          <span key={i} style={{ color: T.accent, fontWeight: 700 }}>{p}</span>
        ) : (
          <span key={i}>{p}</span>
        )
      )}
    </>
  );
}

/// 从整段文本里切出一段【标题…】块（到下一个行首【 或文本结尾）。
function extractBlock(text: string, header: string): string {
  const i = text.indexOf(header);
  if (i < 0) return "";
  const rest = text.slice(i);
  const m = rest.slice(header.length).search(/\n【/);
  return (m < 0 ? rest : rest.slice(0, header.length + m)).trim();
}

/// 历史：JSON 字符串 → 每条一块（角色 chip + 真换行）。
/// 后端存的是 JSON，\n 是转义；这里只在渲染时还原，不动存储、不动请求。
function HistoryView({ json }: { json: string }) {
  let rows: [string, string][] | null = null;
  try {
    const parsed = JSON.parse(json);
    if (Array.isArray(parsed)) {
      rows = parsed.map((r) => (Array.isArray(r)
        ? [String(r[0] ?? ""), String(r[1] ?? "")] as [string, string]
        : ["", String(r)] as [string, string]));
    }
  } catch { rows = null; }
  if (!rows) {
    return <div style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}><RichText text={json} /></div>;
  }
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      {rows.map(([role, content], i) => (
        <div key={i} style={{ borderLeft: "2px solid " + (role === "user" ? T.accent : T.line), paddingLeft: 10 }}>
          <div style={{ fontSize: 10, color: T.text3, marginBottom: 2 }}>
            #{i + 1} {role === "user" ? "用户" : role === "assistant" ? "AI" : (role || "?")} · {content.length} 字符
          </div>
          <div style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}><RichText text={content} /></div>
        </div>
      ))}
    </div>
  );
}
export function DevToolsPage({ currentStory }: { currentStory: Story | null }) {
  const toast = useToast();
  const [rows, setRows] = useState<Row[]>([]);
  const [sel, setSel] = useState<number | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [tab, setTab] = useState<Tab>("system");
  const [nodeFilter, setNodeFilter] = useState<number | "all">("all");

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

  // 下拉按「第 X 天」显示（label 由后端从 timeline_nodes 带回来）；同名标签只保留一个
  const nodeOpts = Array.from(
    new Map(
      rows
        // 只列时间线上还存在的节点：被删掉的节点（label 为空）不出现，免得下拉里冒出 #13/#15
        .filter((r): r is Row & { node_id: number } => r.node_id != null && !!r.node_label)
        .map((r) => [r.node_id, r.node_label || `#${r.node_id}`]),
    ),
  ).sort((a, b) => a[0] - b[0]);
  const shown = nodeFilter === "all" ? rows : rows.filter((r) => r.node_id === nodeFilter);
  const text = detail
    ? tab === "system" ? detail.system_prompt
    : tab === "history" ? detail.history_json
    : tab === "user" ? detail.user_text
    : tab === "tail" ? detail.tail_block
    : tab === "memory" ? extractBlock(detail.tail_block, "【关键记忆】")
    : tab === "recall" ? extractBlock(detail.system_prompt, "【相关旧日记忆】")
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
          <div style={{ fontSize: 10.5, color: T.text3 }}>
            {shown.length}/{rows.length} 条
          </div>
          <div style={{ flex: 1 }} />
          <select value={nodeFilter} onChange={(e) => setNodeFilter(e.target.value === "all" ? "all" : Number(e.target.value))}
            title="只看某一天的请求"
            style={{ height: 24, borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, fontFamily: "inherit", fontSize: 10.5, maxWidth: 110 }}>
            <option value="all">全部节点</option>
            {nodeOpts.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
          </select>
          <button onClick={() => void load()}
            style={{ height: 24, padding: "0 10px", borderRadius: 6, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>刷新</button>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: "0 8px 12px" }}>
          {!currentStory && <div style={{ padding: 10, fontSize: 12, color: T.text3 }}>先选一个故事。</div>}
          {currentStory && rows.length === 0 && (
            <div style={{ padding: 10, fontSize: 12, color: T.text3 }}>还没有请求记录 —— 发一轮对话就会有一条。</div>
          )}
          {shown.map((r) => (
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
        <div style={{ flex: 1, overflow: "auto", padding: 14, fontSize: 11.5, lineHeight: 1.7,
          color: T.text1, fontFamily: "ui-monospace, Consolas, monospace", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
          {!detail
            ? (currentStory ? "左边选一条请求" : "")
            : tab === "history"
              ? <HistoryView json={detail.history_json} />
              : (text ? <RichText text={text} />
                  : tab === "memory" ? "这一轮没有注入【关键记忆】（当前分支暂无记忆，或预算被裁掉）"
                  : tab === "recall" ? "这一轮没有触发回顾检索（没命中回忆意图，也没手动指定某天）"
                  : "（这一段是空的）")}
        </div>
      </div>
    </div>
  );
}
