// 「ParamsPage」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import { useState, type Dispatch, type SetStateAction } from "react";
import type { AppView } from "../types";
import { computeBudget, estimateTokens, sectionColor , fmtNum} from "../lib/parse";
import { diffLines, diffStat, type DiffRow } from "../lib/diff";
import { T } from "../theme";
import type { DebugReq, PortraitItem, Story, TimelineNodeDto } from "../types";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";

/// 把一条请求记录拼成"完整提示词"（与后端装配顺序一致：system → 历史 → 尾部块 → 用户输入）。
///
/// 为什么在前端拼而不是问后端要：后端只存分段（system_prompt / tail_block / history_json），
/// 拼起来是纯字符串操作，前端做零成本，也不给后端加一条只为复制的命令。
function fullPrompt(d: DebugReq | null): string {
  if (!d) return "";
  const parts: string[] = [d.system_prompt || ""];
  try {
    const hist = JSON.parse(d.history_json || "[]") as { role?: string; content?: string }[];
    for (const h of hist) {
      if (!h?.content) continue;
      parts.push(`[${h.role === "user" ? "用户" : "AI"}] ${h.content}`);
    }
  } catch { /* 历史坏了就不拼，别让整页崩 */ }
  if (d.tail_block) parts.push(d.tail_block);
  if (d.user_text) parts.push(`[本次输入] ${d.user_text}`);
  return parts.filter(Boolean).join("\n\n");
}

export type ViewName = AppView;

export type ParamsPageProps = {
  ctxWindow: number;
  currentStory: Story | null;
  debugList: DebugReq[];
  loadDebug: () => Promise<void>;
  loadPortraits: () => Promise<void>;
  modelName: string;
  nRecentInput: string;
  node: { id: number; date_label: string } | null;
  pDebugId: number | null;
  pParamForm: { prompt: string; negative: string; seed: string; steps: string; cfg: string; model: string };
  paramPortrait: PortraitItem | undefined;
  portraits: PortraitItem[];
  setDebugList: Dispatch<SetStateAction<DebugReq[]>>;
  setMsg: Dispatch<SetStateAction<string>>;
  setPDebugId: Dispatch<SetStateAction<number | null>>;
  setPParamForm: Dispatch<SetStateAction<{ prompt: string; negative: string; seed: string; steps: string; cfg: string; model: string }>>;
  setPParamId: Dispatch<SetStateAction<number | null>>;
  setSelectedDebug: Dispatch<SetStateAction<DebugReq | null>>;
  setView: Dispatch<SetStateAction<ViewName>>;
  timelineNodes: TimelineNodeDto[];
};

// v3.4：本页小按钮样式（与页面其它处保持一致的 32px 高度）
const btnS: React.CSSProperties = {
  height: 32, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`,
  background: "transparent", color: T.text2, fontSize: 12, cursor: "pointer",
};
const selS: React.CSSProperties = {
  height: 32, padding: "0 8px", borderRadius: 8, border: `1px solid ${T.line}`,
  background: "#191c22", color: T.text1, fontSize: 12, outline: "none", maxWidth: 220,
};

export function ParamsPage(props: ParamsPageProps) {
  const { ctxWindow, currentStory, debugList, loadDebug, loadPortraits, modelName, nRecentInput, node, pDebugId, pParamForm, paramPortrait, portraits, setDebugList, setMsg, setPDebugId, setPParamForm, setPParamId, setSelectedDebug, setView, timelineNodes } = props;
  // v3.4：Prompt 对比（选一条历史请求，和当前这条做行级对比）
  const [cmpId, setCmpId] = useState<number | null>(null);
  const [diffRows, setDiffRows] = useState<DiffRow[] | null>(null);
  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const [paramsTab, setParamsTab] = useState<"budget" | "preview" | "image" | "json">("budget");
  const [pUnopenOpen, setPUnopenOpen] = useState(false);
  const clearDebug = async () => {
    if (!currentStory) return;
    if (!confirm("清空所有请求记录？")) return;
    try {
      await invoke("clear_debug_requests", { storyId: String(currentStory.id) });
      setDebugList([]);
      setSelectedDebug(null);
      setMsg("🗑️ 已清空请求记录");
    } catch (e) {
      setMsg("❌ 清空失败: " + String(e));
    }
  };
  const saveParamPortrait = async () => {
    if (!paramPortrait) return;
    try {
      await invoke("portrait_update_params", { id: paramPortrait.id, prompt: pParamForm.prompt, negative: pParamForm.negative, seed: pParamForm.seed, steps: pParamForm.steps, cfg: pParamForm.cfg, model: pParamForm.model });
      setMsg("✅ 生成参数已保存");
      await loadPortraits();
    } catch (e) { setMsg("❌ " + String(e)); }
  };
  const rollParamSeed = () => setPParamForm((f) => ({ ...f, seed: String(Math.floor(Math.random() * 1000000)) }));
  return (
      (() => {
        const nodeLabel = (id: number | null) => {
          const t = timelineNodes.find((n2) => n2.id === id);
          return t ? t.date_label : (id != null ? `节点 #${id}` : "未知节点");
        };
        const dbg = (pDebugId != null ? debugList.find((d) => d.id === pDebugId) : undefined)
          ?? (node ? debugList.find((d) => d.node_id === node.id) : undefined)
          ?? debugList[0]
          ?? null;
        // v3.4：这条记录是不是"最新一条"？
        // 改过设定（物品 / 术语 / 世界书 / 角色字段）之后，旧记录里当然看不到新内容 ——
        // 用户很容易以为"没生效"。这里显式提示，而不是让人对着旧记录猜。
        const newestDbgId = debugList.length > 0 ? debugList[0].id : null;
        const isStaleRecord = dbg != null && newestDbgId != null && dbg.id !== newestDbgId;
        const { sysSections, tailSection, history, lastUser, priorHistory, sysTokens, histTokens, userTokens, usedTokens, usedRecalculated, inputBudget, remain, pctUsed } = computeBudget(dbg, ctxWindow);
        // 数字千分位统一走 lib/parse 的 fmtNum（原来是 en-US 硬编码，与其它页不一致）
  const fmt = fmtNum;
        const paramP = paramPortrait;
        const budgetRows: { name: string; note: string; tokens: number; color: string; cut: string; cutColor: string }[] = [
          ...sysSections.map((s) => ({ name: s.name, note: s.text.slice(0, 24).replace(/\n/g, " "), tokens: estimateTokens(s.text), color: sectionColor(s.name), cut: "不可裁剪", cutColor: T.ok })),
          ...(tailSection ? [{ name: tailSection.name, note: tailSection.text.slice(0, 24).replace(/\n/g, " "), tokens: estimateTokens(tailSection.text), color: "#ff9a45", cut: "不可裁剪", cutColor: T.ok }] : []),
          { name: "历史上下文", note: priorHistory.length ? `${priorHistory.length} 条（不含本次输入）` : "本次未携带历史", tokens: histTokens, color: T.accent, cut: priorHistory.length ? "第 1 顺位裁剪" : "—", cutColor: priorHistory.length ? T.danger : T.text3 },
          { name: "本次输入", note: lastUser ? lastUser.content.slice(0, 24).replace(/\n/g, " ") : "当前用户输入", tokens: userTokens, color: "#ffd7ad", cut: "不可裁剪", cutColor: T.ok },
        ];
        const windowFor = (n: number) => `${Math.min(100, (n / inputBudget) * 100)}%`;
        return (
          <div style={{ maxWidth: 1120, margin: "0 auto" }}>
            <div style={{ display: "flex", alignItems: "flex-start", gap: 12, marginBottom: 14 }}>
              <div>
                <div style={{ fontSize: 15, fontWeight: 650, display: "flex", alignItems: "center", gap: 8 }}>🎛 生成参数</div>
                <div style={{ fontSize: 11.5, color: T.text3, marginTop: 4 }}>
                  {currentStory ? `《${currentStory.title}》 · 模型：${modelName || "未配置"} · 上下文窗口 ${fmt(ctxWindow)} token` : "尚未进入故事"}
                  {dbg ? ` · 请求记录 #${dbg.id} · ${nodeLabel(dbg.node_id)} · N=${dbg.n_used} 条（${dbg.created_at}）` : " · 暂无请求记录"}
                </div>
              </div>
              <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
                {debugList.length > 0 && (
                  <select value={dbg?.id ?? ""} onChange={(e) => setPDebugId(Number(e.target.value))} title="选择请求记录（默认跟随当前时间线节点）" style={{ height: 32, padding: "0 10px", maxWidth: 330 }}>
                    {debugList.map((d) => (
                      <option key={d.id} value={d.id}>#{d.id} · {nodeLabel(d.node_id)} · {d.created_at.slice(5, 16)}</option>
                    ))}
                  </select>
                )}
                <button onClick={() => { if (currentStory) loadDebug(); }} disabled={!currentStory} style={{ height: 32, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: currentStory ? "pointer" : "not-allowed", fontSize: 12.5 }}>🔄 刷新记录</button>
                {/* 原「调试」页删掉后，把它的「清空请求记录」能力搬到这里（否则这个命令就没人用了） */}
                <button onClick={() => { void clearDebug(); }} disabled={!currentStory} title="清空本故事的所有请求记录（只影响调试数据，不影响剧情）" style={{ height: 32, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text3, cursor: currentStory ? "pointer" : "not-allowed", fontSize: 12 }}>🗑 清空记录</button>
              </div>
            </div>
            <div style={{ display: "flex", gap: 4, padding: 3, border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel2, width: "fit-content", marginBottom: 16 }}>
              {([["budget", "📊 Token 预算"], ["preview", "🧾 Prompt 预览"], ["image", "🎨 立绘参数"], ["json", "🧩 JSON"]] as const).map(([k, lb]) => (
                <button key={k} onClick={() => setParamsTab(k)} style={{ border: "none", borderRadius: 7, padding: "7px 14px", fontSize: 12.5, cursor: "pointer", background: paramsTab === k ? T.accentSoft : "transparent", color: paramsTab === k ? T.accent : T.text2, fontWeight: paramsTab === k ? 600 : 400 }}>{lb}</button>
              ))}
            </div>
            {currentStory && isStaleRecord && dbg && (
              <div style={{ border: `1px solid ${T.accent}55`, background: "rgba(255,122,24,.08)", borderRadius: 10, padding: "8px 12px", marginBottom: 12, fontSize: 11.5, color: "#ffb877", lineHeight: 1.7 }}>
                ⚠ <b>这是历史记录</b>（#{dbg.id} · {nodeLabel(dbg.node_id)} · {dbg.created_at}），不是最新一条。
                你之后改过的设定（物品 / 术语 / 世界书 / 角色字段）<b>不会</b>出现在这条里 ——
                想看最新的，请发一条消息，或在上面的下拉里选 <b>#{newestDbgId}</b>。
              </div>
            )}
            {!currentStory && (
              <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "52px 20px", textAlign: "center", color: T.text3 }}>
                <div style={{ fontSize: 26, marginBottom: 8 }}>🎛</div>
                <div style={{ fontSize: 13 }}>请先进入一个故事，再查看该故事的生成参数。</div>
                <button onClick={() => setView("stories")} style={{ marginTop: 12, height: 32, padding: "0 16px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650 }}>去故事列表</button>
              </div>
            )}
            {currentStory && paramsTab === "budget" && (!dbg ? (
              <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>
                还没有请求记录。回到剧情区发送一条消息，或点右上角「刷新记录」。
              </div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 16 }}>
                  <div style={{ display: "flex", alignItems: "flex-end", gap: 14, flexWrap: "wrap" }}>
                    <div style={{ fontSize: 30, fontWeight: 800, letterSpacing: 0.5 }}>{fmt(usedTokens)}<span style={{ fontSize: 12, color: T.text3, fontWeight: 400, marginLeft: 6 }}>/ {fmt(inputBudget)} token 输入预算</span></div>
                    <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.7, paddingBottom: 3 }}>
                      已用 <b style={{ color: pctUsed > 85 ? T.warn : T.text1 }}>{pctUsed}%</b> · 剩余 {fmt(remain)} token
                      {usedRecalculated && (
                        <span title="这条记录生成时还没有 estimated_tokens 字段，用量是按当前配置对同一段提示词重算的，不是当时的真实值"
                          style={{ marginLeft: 6, fontSize: 10, padding: "1px 6px", borderRadius: 5,
                            border: `1px solid ${T.line}`, color: T.warn }}>老快照按当前口径重算</span>
                      )}
                      <br />
                {/* v3.4：Prompt 三件套 —— 复制整段 / 下载 .txt / 对比两次请求 */}
                <button
                  onClick={() => { void navigator.clipboard.writeText(fullPrompt(dbg)); setMsg("✅ 已复制完整提示词"); }}
                  disabled={!dbg}
                  title="把 system + 历史 + 尾部块 + 本次输入 拼成一整段复制"
                  style={btnS}
                >📋 复制整段</button>
                <button
                  onClick={() => {
                    const txt = fullPrompt(dbg);
                    if (!txt) return;
                    const blob = new Blob([txt], { type: "text/plain;charset=utf-8" });
                    const a = document.createElement("a");
                    a.href = URL.createObjectURL(blob);
                    a.download = `prompt-${dbg?.id ?? 0}-${(dbg?.created_at || "").replace(/[: ]/g, "-")}.txt`;
                    a.click();
                    URL.revokeObjectURL(a.href);
                    setMsg("✅ 已下载 .txt");
                  }}
                  disabled={!dbg}
                  title="下载成 .txt（本地生成，不经后端）"
                  style={btnS}
                >⬇ 导出 .txt</button>
                <select
                  value={cmpId ?? ""}
                  onChange={(e) => {
                    const id = e.target.value ? Number(e.target.value) : null;
                    setCmpId(id);
                    const other = debugList.find((d) => d.id === id) ?? null;
                    setDiffRows(id && dbg ? diffLines(fullPrompt(other), fullPrompt(dbg)) : null);
                  }}
                  disabled={debugList.length < 2}
                  title="选一条历史请求，和当前这条做行级对比（左=旧，右=当前）"
                  style={selS}
                >
                  <option value="">对比：{debugList.length < 2 ? "（只有一条记录）" : "选一条…"}</option>
                  {debugList.filter((d) => d.id !== dbg?.id).map((d) => (
                    <option key={d.id} value={d.id}>#{d.id} · {nodeLabel(d.node_id)} · {d.created_at.slice(5, 16)}</option>
                  ))}
                </select>

                      上下文窗口 {fmt(dbg.context_window > 0 ? dbg.context_window : ctxWindow)} × 系数 {dbg.reserve_ratio > 0 ? dbg.reserve_ratio : 0.8} − 输出预留 {fmt((dbg.max_tokens > 0 ? dbg.max_tokens : 4000) + (dbg.safety_margin > 0 ? dbg.safety_margin : 800))}<br />
                      记录节点：{nodeLabel(dbg.node_id)} · 实际带 {dbg.n_used} 条消息（含本次输入）
                    </div>
                  </div>
                  <div style={{ display: "flex", height: 20, borderRadius: 6, overflow: "hidden", border: `1px solid ${T.line}`, background: "#12141a", marginTop: 13 }}>
                    {budgetRows.map((r, i) => r.tokens > 0 && (
                      <div key={`${r.name}-${i}`} title={`${r.name} ${fmt(r.tokens)} token`} style={{ width: windowFor(r.tokens), background: r.color, height: "100%", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 9, color: "rgba(0,0,0,.65)", fontWeight: 700, whiteSpace: "nowrap", overflow: "hidden" }}>{r.tokens > inputBudget * 0.08 ? `${r.name} ${fmt(r.tokens)}` : ""}</div>
                    ))}
                    {remain > 0 && <div style={{ flex: 1, background: "#262a33" }} />}
                  </div>
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginTop: 12, paddingTop: 12, borderTop: `1px dashed ${T.lineSoft}` }}>
                    {budgetRows.map((r, i) => (
                      <span key={`${r.name}-legend-${i}`} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10.5, color: T.text2 }}>
                        <i style={{ width: 9, height: 9, borderRadius: 3, background: r.color, display: "block" }} />{r.name} <b style={{ color: T.text1, fontFamily: "monospace", fontSize: 10.5 }}>{fmt(r.tokens)}</b>
                      </span>
                    ))}
                  </div>
                </div>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 130px 90px 110px", gap: 10, padding: "9px 14px", background: T.panel2, fontSize: 10.5, color: T.text3, letterSpacing: 0.5 }}>
                    <span>注入内容</span><span>占用比例</span><span style={{ textAlign: "right" }}>Token</span><span style={{ textAlign: "right" }}>裁剪优先级</span>
                  </div>
                  {budgetRows.map((r, i) => (
                    <div key={`${r.name}-row-${i}`} style={{ display: "grid", gridTemplateColumns: "1fr 130px 90px 110px", gap: 10, alignItems: "center", padding: "9px 14px", borderTop: `1px solid ${T.lineSoft}`, fontSize: 11.5 }}>
                      <span style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
                        <i style={{ width: 8, height: 8, borderRadius: 3, background: r.color, flex: "0 0 8px" }} />
                        <span style={{ fontWeight: 600, color: T.text1 }}>{r.name}</span>
                        <span style={{ color: T.text3, fontSize: 10.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.note}</span>
                      </span>
                      <span style={{ height: 6, borderRadius: 3, background: "#12141a", border: `1px solid ${T.lineSoft}`, overflow: "hidden", display: "block" }}><i style={{ display: "block", height: "100%", width: windowFor(r.tokens), background: r.color }} /></span>
                      <span style={{ textAlign: "right", fontFamily: "monospace", color: T.text2 }}>{fmt(r.tokens)}</span>
                      <span style={{ textAlign: "right" }}><span style={{ fontSize: 9.5, padding: "2px 7px", borderRadius: 5, color: r.cutColor, background: `${r.cutColor}22`, whiteSpace: "nowrap" }}>{r.cut}</span></span>
                    </div>
                  ))}
                </div>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: "12px 16px", fontSize: 11.5, color: T.text2, lineHeight: 1.9 }}>
                  <div style={{ fontWeight: 650, color: T.text1, marginBottom: 6 }}>🧮 本次预算口径（后端算一次，前端直接读记录）</div>
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(215px, 1fr))", gap: "2px 18px", fontFamily: "monospace", fontSize: 11 }}>
                    <span>上下文窗口　　{fmt(dbg.context_window > 0 ? dbg.context_window : ctxWindow)}</span>
                    <span>预算系数　　　× {dbg.reserve_ratio > 0 ? dbg.reserve_ratio : 0.8}</span>
                    <span>输出预留　　　− {fmt((dbg.max_tokens > 0 ? dbg.max_tokens : 4000) + (dbg.safety_margin > 0 ? dbg.safety_margin : 800))}（输出 {fmt(dbg.max_tokens > 0 ? dbg.max_tokens : 4000)} + 余量 {fmt(dbg.safety_margin > 0 ? dbg.safety_margin : 800)}）</span>
                    <span style={{ color: T.accent }}>= 输入预算　　{fmt(inputBudget)}</span>
                    <span>system 段合计　{fmt(sysTokens)}</span>
                    <span>其中：历史上下文 {fmt(histTokens)} · 本次输入 {fmt(userTokens)}</span>
                  </div>
                  <div style={{ marginTop: 8, paddingTop: 8, borderTop: `1px dashed ${T.lineSoft}`, fontSize: 11, color: T.text3 }}>
                    system 分段占用：
                    {sysSections.map((s) => ` ${s.name} ${fmt(estimateTokens(s.text))}`).join(" ｜")}
                    {tailSection ? ` ｜ 上下文更新（尾部） ${fmt(estimateTokens(tailSection.text))}` : ""}
                  </div>
                </div>
                {/* B 子项：每日 N 值（由预算倒推，不写死）—— 同时给出"当前设置"与"32k 默认档"两种口径 */}
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: "12px 16px", fontSize: 11.5, color: T.text2, lineHeight: 1.9 }}>
                  <div style={{ fontWeight: 650, color: T.text1, marginBottom: 8 }}>📐 每日 N 值（由预算倒推，不写死）</div>
                  {(() => {
                    const outRes = (dbg.max_tokens > 0 ? dbg.max_tokens : 4000) + (dbg.safety_margin > 0 ? dbg.safety_margin : 800);
                    const ratio = dbg.reserve_ratio > 0 ? dbg.reserve_ratio : 0.8;
                    const days = dbg.day_window ?? 0;
                    // 单条均 token：后端已有 400 的下限（短消息不再把 N 撑爆），这里兜底同样用 400
  const avg = dbg.avg_msg_tokens && dbg.avg_msg_tokens > 0 ? dbg.avg_msg_tokens : 400;
                    const cap = Number(nRecentInput) || 10;
                    const win = dbg.context_window > 0 ? dbg.context_window : ctxWindow;
                    // 按给定窗口算：N = (窗口×系数 − 输出预留 − 固定区 − 尾部块 − 本次输入) ÷ 保留天数 ÷ 单条均token
                    const nFor = (w: number) => {
                      if (days <= 0) return { win: w, perDay: 0, nRaw: 0, nFinal: 0, overflow: false, used: 0 };
                      const scaled = Math.round(w * ratio);
                      const used = outRes + sysTokens + estimateTokens(tailSection?.text || "") + userTokens;
                      const hist = scaled - used;
                      // 预算被固定区/预留吃光（小窗口 + 厚重设定时会发生）→ N 只能是 0，单独标出来
                      const overflow = hist <= 0;
                      const safeHist = Math.max(hist, 0);
                      const perDay = Math.floor(safeHist / days);
                      const nRaw = Math.floor(perDay / avg);
                      return { win: w, perDay, nRaw, nFinal: Math.min(nRaw, cap), overflow, used };
                    };
                    const cur = nFor(win);
                    const base = nFor(32768);
                    const perDay = (dbg.day_n || "").split(",").filter(Boolean);
                    const rows: { k: string; a: string; b: string }[] = [
                      { k: "上下文窗口", a: fmt(base.win), b: fmt(cur.win) },
                      { k: "× 安全系数", a: `${ratio}`, b: `${ratio}` },
                      { k: "− 输出预留", a: fmt(outRes), b: fmt(outRes) },
                      { k: "− 固定区（实测）", a: fmt(sysTokens), b: fmt(sysTokens) },
                      { k: "− 尾部块 + 本次输入", a: fmt(estimateTokens(tailSection?.text || "") + userTokens), b: fmt(estimateTokens(tailSection?.text || "") + userTokens) },
                      { k: "÷ 保留天数", a: `${days || "—"}`, b: `${days || "—"}` },
                      { k: "= 每日基准预算", a: fmt(base.perDay), b: fmt(cur.perDay) },
                    ];
                    return (
                      <>
                        <div style={{ display: "grid", gridTemplateColumns: "minmax(150px, 1.4fr) 1fr 1fr", gap: "0 10px", fontFamily: "monospace", fontSize: 11 }}>
                          <span style={{ color: T.text3, borderBottom: `1px solid ${T.lineSoft}`, paddingBottom: 4 }}>推导项</span>
                          <span style={{ color: T.text3, textAlign: "right", borderBottom: `1px solid ${T.lineSoft}`, paddingBottom: 4 }}>默认档 32k</span>
                          <span style={{ color: T.accent, textAlign: "right", borderBottom: `1px solid ${T.lineSoft}`, paddingBottom: 4 }}>当前设置 {fmt(cur.win)}</span>
                          {rows.flatMap((r) => [
                            <span key={`${r.k}-k`} style={{ borderBottom: `1px dashed ${T.lineSoft}`, padding: "2px 0" }}>{r.k}</span>,
                            <span key={`${r.k}-a`} style={{ textAlign: "right", borderBottom: `1px dashed ${T.lineSoft}`, padding: "2px 0" }}>{r.a}</span>,
                            <span key={`${r.k}-b`} style={{ textAlign: "right", borderBottom: `1px dashed ${T.lineSoft}`, padding: "2px 0", color: T.text1 }}>{r.b}</span>,
                          ])}
                          <span style={{ paddingTop: 4 }}>÷ 单条均 token（实测，下限 400）</span>
                          <span style={{ textAlign: "right", paddingTop: 4 }}>{fmt(avg)}</span>
                          <span style={{ textAlign: "right", paddingTop: 4, color: T.text1 }}>{fmt(avg)}</span>
                        </div>
                        {(base.overflow || cur.overflow) && (
                          <div style={{ marginTop: 6, padding: "6px 9px", borderRadius: 7, background: "rgba(240,85,77,.08)", border: "1px solid rgba(240,85,77,.3)", fontSize: 11, color: "#ff8c85", lineHeight: 1.7 }}>
                            ⚠ {base.overflow ? `默认档 32k 已被固定区吃满（固定区 + 预留 ≈ ${fmt(base.used)} token &gt; ${fmt(Math.round(base.win * ratio))}）：这种设定量下 32k 窗口放不下任何历史，至少要 ${fmt(Math.ceil(base.used / ratio / 1000) * 1000)} 左右才装得下。` : ""}
                            {cur.overflow ? "当前设置的窗口也已被固定区吃满，历史无法携带 —— 请调大上下文窗口。" : ""}
                          </div>
                        )}
                        <div style={{ marginTop: 6, fontSize: 10.5, color: T.text3, lineHeight: 1.7 }}>
                          「固定区」包含：基础指令 / 输出排版 / 数据字典（这三项<b>不变</b>）＋ 世界观 / 角色 / 方向和关系（这三项<b>不一定会变</b>，改动设定后会跟着变）
                        </div>
                        <div style={{ display: "grid", gridTemplateColumns: "minmax(150px, 1.4fr) 1fr 1fr", gap: "0 10px", marginTop: 8, paddingTop: 8, borderTop: `1px solid ${T.line}`, fontSize: 12 }}>
                          <span style={{ fontWeight: 650 }}>N 值</span>
                          <span style={{ textAlign: "right" }}>
                            <b style={{ color: T.text2 }}>{fmt(base.nRaw)}</b>
                            <span style={{ fontSize: 10.5, color: T.text3 }}> → 封顶后 <b style={{ color: T.text2 }}>{fmt(base.nFinal)}</b></span>
                          </span>
                          <span style={{ textAlign: "right" }}>
                            <b style={{ color: T.ok, fontSize: 13 }}>{fmt(cur.nRaw)}</b>
                            <span style={{ fontSize: 10.5, color: T.text3 }}> → 当前设置下取 <b style={{ color: T.ok }}>{fmt(cur.nFinal)}</b> 条/天</span>
                          </span>
                        </div>
                        <div style={{ marginTop: 8, fontSize: 11, color: T.text3, lineHeight: 1.8 }}>
                          · <b style={{ color: T.text2 }}>N 值理论上限 = {fmt(cur.nRaw)} 条/天</b>（当前窗口下，预算能装下的条数；未受设置封顶）
                          <br />· <b style={{ color: T.text2 }}>N 值当前设置 = {fmt(cur.nFinal)} 条/天</b>（再按「每日上限 {cap} 条/天」封顶后的实际取值）
                          <br />· <b style={{ color: T.text2 }}>记录当时</b>的分配（后端当轮实际执行）：{days > 0 ? `原文保留 ${days} 天` : "未带历史"}
                          {perDay.length ? ` · 每天 ${perDay.join(" / ")} 条` : ""}
                          {dbg.n_used > 0 ? ` · 实发 ${dbg.n_used} 条` : ""}
                          {days > 0 ? " · 更早的天用【前情提要】摘要" : ""}
                          {(() => {
                            // 记录是"当时"的结果：若设置改过（口径/上限/窗口），这里用**当前设置**重算一遍做对照
                            const wouldN = cur.nFinal;
                            const maxPerDay = perDay.length ? Math.max(...perDay.map((x) => Number(x) || 0)) : 0;
                            const changed = wouldN !== maxPerDay;
                            return (
                              <>
                                <br />· <b style={{ color: T.text2 }}>按当前设置</b>的分配：{days > 0 ? `原文保留 ${days} 天 · 每天最多 ${wouldN} 条` : "未带历史"}
                                {changed && (
                                  <span style={{ color: T.warn }}>
                                    　⚠ 与记录不同（记录当时每日上限为 {maxPerDay || "—"} 条）：<b>下一条消息</b>才会按 {wouldN} 条/天生效
                                  </span>
                                )}
                              </>
                            );
                          })()}
                          <br />· <b style={{ color: T.text2 }}>口径提醒</b>：N 是**按消息条数**算的 ——「用户」与「AI」各算 1 条，所以<b>一轮对话 = 2 条 = 2 个 N</b>（例：N=50 条/天 ≈ 25 轮问答/天；想看"几轮"就把 N 除以 2）。
                          <br />· 提示：{cur.nRaw > cap
                            ? `预算还能装 ${fmt(cur.nRaw)} 条/天，但现在被「每日上限」（设置 › API，${cap} 条/天）挡住 —— 想让 AI 看到更多历史就调大它。`
                            : `预算已成瓶颈：调大「每日上限」不会再增加条数，需要调大上下文窗口或降低保留天数。`}
                        </div>
                      </>
                    );
                  })()}
                </div>
                <div style={{ fontSize: 11, color: T.text3, lineHeight: 1.8, display: "flex", gap: 7 }}>
                  <span>💡</span><span>裁剪规则：system 提示词（基础指令 / 输出排版 / 数据字典 / 世界观 / 角色 / 方向和关系）与本次输入绝不截断；只截断历史上下文，超限时自动向下调整 N（本次实际带 {dbg.n_used} 条消息，含本次输入）。</span>
                </div>
              </div>
            ))}
            {currentStory && paramsTab === "preview" && diffRows && (() => {
              const st = diffStat(diffRows);
              return (
                <div style={{ border: `1px solid ${T.accent}55`, borderRadius: 12, background: "#12141a", marginBottom: 14, overflow: "hidden" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 13px", borderBottom: `1px solid ${T.line}`, fontSize: 12 }}>
                    <b>与 #{cmpId} 对比</b>
                    <span style={{ color: "#69c46b" }}>+{st.added}</span>
                    <span style={{ color: T.danger }}>-{st.removed}</span>
                    <span style={{ color: T.text3, fontSize: 11.5 }}>（左=旧，右=当前；未变化的行也保留，方便看上下文）</span>
                    <button onClick={() => { setCmpId(null); setDiffRows(null); }} style={{ ...btnS, marginLeft: "auto", height: 26 }}>关闭对比</button>
                  </div>
                  <div style={{ maxHeight: 420, overflow: "auto", padding: "8px 0", fontFamily: "ui-monospace, Consolas, monospace", fontSize: 11.5, lineHeight: 1.65 }}>
                    {diffRows.map((r, idx) => (
                      <div key={idx} style={{
                        padding: "1px 13px",
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-all",
                        background: r.kind === "add" ? "rgba(105,196,107,.10)" : r.kind === "del" ? "rgba(240,85,77,.10)" : "transparent",
                        color: r.kind === "add" ? "#9ad99b" : r.kind === "del" ? "#ff9d96" : T.text3,
                      }}>
                        <span style={{ opacity: 0.55, marginRight: 8 }}>{r.kind === "add" ? "+" : r.kind === "del" ? "-" : " "}</span>
                        {r.text || " "}
                      </div>
                    ))}
                  </div>
                </div>
              );
            })()}
            {currentStory && paramsTab === "preview" && (!dbg ? (
              <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>暂无请求记录，先回剧情区发一条消息。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                  <span style={{ fontSize: 12, color: T.text2 }}>模型 {dbg.model || "—"} · {nodeLabel(dbg.node_id)} · N={dbg.n_used} 条 · 输入 {fmt(usedTokens)} token</span>
                  <button onClick={() => { const full = [dbg.system_prompt, ...history.map((m) => `[${m.role === "user" ? "用户" : "AI"}] ${m.content}`), tailSection ? `[上下文更新]\n${tailSection.text}` : ""].filter(Boolean).join("\n\n"); navigator.clipboard.writeText(full).then(() => setMsg("📋 已复制完整 Prompt")).catch(() => {}); }} style={{ marginLeft: "auto", height: 30, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>📋 复制完整 Prompt</button>
                </div>
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", overflow: "hidden" }}>
                  {sysSections.map((s, i) => (
                    <div key={`${s.name}-${i}`}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: i ? `1px solid ${T.lineSoft}` : "none", fontSize: 10.5, color: T.text2 }}>
                        <i style={{ width: 8, height: 8, borderRadius: 2, background: sectionColor(s.name) }} />
                        <b>{s.name}</b>
                        <span style={{ color: T.text3 }}>{fmt(estimateTokens(s.text))} token</span>
                      </div>
                      <div style={{ padding: "10px 16px", fontFamily: "monospace", fontSize: 11, lineHeight: 1.9, whiteSpace: "pre-wrap", color: sectionColor(s.name), borderLeft: `2px solid ${sectionColor(s.name)}`, maxHeight: 240, overflowY: "auto" }}>{s.text}</div>
                    </div>
                  ))}
                  <div>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10.5, color: T.text2 }}>
                      <i style={{ width: 8, height: 8, borderRadius: 2, background: T.accent }} />历史上下文 <span style={{ color: T.text3 }}>{priorHistory.length} 条 · {fmt(histTokens)} token</span>
                    </div>
                    <div style={{ padding: "10px 16px", fontSize: 11, lineHeight: 1.8, maxHeight: 220, overflowY: "auto", borderLeft: `2px solid ${T.accent}`, color: "#c8ccd6", whiteSpace: "pre-wrap" }}>
                      {priorHistory.length === 0 ? "（本次未携带历史）" : priorHistory.map((m, i) => (
                        <div key={i} style={{ marginBottom: 6 }}><span style={{ color: m.role === "user" ? "#ffd7ad" : "#8ab4ff" }}>[{m.role === "user" ? "用户" : "AI"}]</span> {m.content.length > 600 ? m.content.slice(0, 600) + "…" : m.content}</div>
                      ))}
                    </div>
                  </div>
                  {tailSection && (
                    <div>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10.5, color: T.text2 }}>
                        <i style={{ width: 8, height: 8, borderRadius: 2, background: "#ff9a45" }} />上下文更新（尾部） <span style={{ color: T.text3 }}>{fmt(estimateTokens(tailSection.text))} token · 每轮单独拼装，不落库</span>
                      </div>
                      <div style={{ padding: "10px 16px", fontFamily: "monospace", fontSize: 11, lineHeight: 1.9, whiteSpace: "pre-wrap", color: "#ffc078", borderLeft: "2px solid #ff9a45", maxHeight: 240, overflowY: "auto" }}>{tailSection.text}</div>
                    </div>
                  )}
                  <div>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, borderTop: `1px solid ${T.lineSoft}`, fontSize: 10.5, color: T.text2 }}>
                      <i style={{ width: 8, height: 8, borderRadius: 2, background: "#ffd7ad" }} />本次输入 <span style={{ color: T.text3 }}>{fmt(userTokens)} token</span>
                    </div>
                    <div style={{ padding: "10px 16px", fontFamily: "monospace", fontSize: 11, lineHeight: 1.9, whiteSpace: "pre-wrap", color: "#ffd7ad", borderLeft: "2px solid #ffd7ad" }}>{lastUser?.content || dbg.user_text}</div>
                  </div>
                  <div style={{ display: "flex", gap: 14, flexWrap: "wrap", padding: "10px 14px", borderTop: `1px solid ${T.line}`, background: T.panel2, fontSize: 10.5, color: T.text3 }}>
                    <span>输入 {fmt(usedTokens)}</span>
                    <span>预计输出上限 4,000</span>
                    <span>状态 {dbg.status === "ok" ? "✅ ok" : "❌ " + (dbg.error || "error")}</span>
                    <span style={{ marginLeft: "auto" }}>请求记录 #{dbg.id} · {dbg.created_at}</span>
                  </div>
                </div>
              </div>
            ))}
            {currentStory && paramsTab === "json" && (!dbg ? (
              <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>暂无请求记录。</div>
            ) : (() => {
              const jsonText = JSON.stringify({ id: dbg.id, node_id: dbg.node_id, model: dbg.model, estimated_tokens: dbg.estimated_tokens, n_used: dbg.n_used, status: dbg.status, created_at: dbg.created_at, system_prompt: dbg.system_prompt, tail_block: dbg.tail_block, messages: history, ai_reply: dbg.ai_reply }, null, 2);
              return (
                <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: "#12141a", overflow: "hidden" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 14px", background: T.panel2, fontSize: 11.5, color: T.text2 }}>
                    <b>本次请求 JSON</b><span style={{ color: T.text3, fontSize: 10.5 }}>system_prompt / tail_block / messages / 元信息</span>
                    <button onClick={() => { navigator.clipboard.writeText(jsonText).then(() => setMsg("📋 已复制 JSON")).catch(() => {}); }} style={{ marginLeft: "auto", height: 28, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel, color: T.text2, cursor: "pointer", fontSize: 11.5 }}>复制</button>
                  </div>
                  <pre style={{ margin: 0, padding: 14, maxHeight: 460, overflow: "auto", fontFamily: "monospace", fontSize: 11, lineHeight: 1.7, color: "#c8ccd6", whiteSpace: "pre-wrap" }}>{jsonText}</pre>
                </div>
              );
            })())}
            {currentStory && paramsTab === "image" && (!paramP ? (
              <div style={{ border: `1px dashed ${T.line}`, borderRadius: 12, padding: "46px 20px", textAlign: "center", color: T.text3, fontSize: 12.5 }}>
                立绘素材库还没有图片。可先到「立绘素材」导入并绑定角色。
                <button onClick={() => setView("portrait")} style={{ marginLeft: 10, height: 30, padding: "0 12px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12 }}>去立绘素材</button>
              </div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                  <span style={{ fontSize: 12, color: T.text3 }}>参数对象</span>
                  <select value={paramP.id} onChange={(e) => setPParamId(Number(e.target.value))} style={{ height: 32, padding: "0 10px", minWidth: 280 }}>
                    {portraits.map((p) => <option key={p.id} value={p.id}>{p.name}{p.bound_character ? ` · ${p.bound_character}` : ""}{p.ver ? ` · ${p.ver}` : ""}</option>)}
                  </select>
                  <span style={{ fontSize: 11, color: T.text3 }}>保存后写入该素材的生成参数（素材详情页同步显示）</span>
                </div>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1.1fr 260px", gap: 14, alignItems: "start" }}>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "11px 13px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}` }}>
                      <span style={{ width: 26, height: 26, borderRadius: 8, background: "linear-gradient(135deg,#7b5cff,#3a2b6b)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 14 }}>🧠</span>
                      <div><b style={{ fontSize: 12.5 }}>模型与采样</b><div style={{ fontSize: 10.5, color: T.text3, marginTop: 2 }}>底模 / LoRA / 采样策略</div></div>
                    </div>
                    <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 12 }}>
                      <label style={{ fontSize: 11.5, color: T.text2 }}>底模 / 模型名
                        <input value={pParamForm.model} onChange={(e) => setPParamForm({ ...pParamForm, model: e.target.value })} placeholder="例如 sd-xl-1.0-base" style={{ display: "block", width: "100%", marginTop: 6, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} />
                      </label>
                      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                        <label style={{ fontSize: 11.5, color: T.text2 }}>Steps<input value={pParamForm.steps} onChange={(e) => setPParamForm({ ...pParamForm, steps: e.target.value })} placeholder="32" style={{ display: "block", width: "100%", marginTop: 6, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} /></label>
                        <label style={{ fontSize: 11.5, color: T.text2 }}>CFG Scale<input value={pParamForm.cfg} onChange={(e) => setPParamForm({ ...pParamForm, cfg: e.target.value })} placeholder="7.5" style={{ display: "block", width: "100%", marginTop: 6, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} /></label>
                      </div>
                      <label style={{ fontSize: 11.5, color: T.text2 }}>Seed
                        <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
                          <input value={pParamForm.seed} onChange={(e) => setPParamForm({ ...pParamForm, seed: e.target.value })} placeholder="88214" style={{ flex: 1, minWidth: 0, padding: "7px 10px", boxSizing: "border-box", fontFamily: "monospace", fontSize: 11.5 }} />
                          <button onClick={rollParamSeed} title="随机种子" style={{ width: 34, borderRadius: 6, border: `1px solid ${T.line}`, background: T.panel2, cursor: "pointer" }}>🎲</button>
                        </div>
                      </label>
                    </div>
                  </div>
                  <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, overflow: "hidden" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "11px 13px", background: T.panel2, borderBottom: `1px solid ${T.lineSoft}` }}>
                      <span style={{ width: 26, height: 26, borderRadius: 8, background: "linear-gradient(135deg,#ff7a18,#ffb066)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 14 }}>✍️</span>
                      <div><b style={{ fontSize: 12.5 }}>提示词与一致性</b><div style={{ fontSize: 10.5, color: T.text3, marginTop: 2 }}>正面 / 负面 / 参考图锁定</div></div>
                    </div>
                    <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 12 }}>
                      <label style={{ fontSize: 11.5, color: T.text2 }}>正面提示词
                        <textarea value={pParamForm.prompt} onChange={(e) => setPParamForm({ ...pParamForm, prompt: e.target.value })} rows={4} placeholder="描述角色外观与画面..." style={{ display: "block", width: "100%", marginTop: 6, padding: "8px 10px", boxSizing: "border-box", resize: "vertical", fontFamily: "monospace", fontSize: 11, lineHeight: 1.7, color: "#9fb0c9" }} />
                      </label>
                      <label style={{ fontSize: 11.5, color: T.text2 }}>负面提示词
                        <textarea value={pParamForm.negative} onChange={(e) => setPParamForm({ ...pParamForm, negative: e.target.value })} rows={3} placeholder="blurry, lowres, watermark..." style={{ display: "block", width: "100%", marginTop: 6, padding: "8px 10px", boxSizing: "border-box", resize: "vertical", fontFamily: "monospace", fontSize: 11, lineHeight: 1.7, color: "#ff9d97" }} />
                      </label>
                      <div>
                        <div style={{ fontSize: 11.5, color: T.text2, marginBottom: 6 }}>尺寸与比例</div>
                        <div style={{ display: "flex", gap: 7, flexWrap: "wrap" }}>
                          <span style={{ fontSize: 11, padding: "5px 10px", borderRadius: 7, border: `1px solid ${T.accent}55`, background: T.accentSoft, color: T.accent }}>{paramP.ratio || "未设置"}{paramP.width && paramP.height ? ` · ${paramP.width}×${paramP.height}` : ""}</span>
                        </div>
                      </div>
                      <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: "#191c22", padding: "10px 11px" }}>
                        <div style={{ fontSize: 11, color: T.text2, marginBottom: 8 }}>🧩 参考立绘（已绑定，只读预览）</div>
                        <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                          {portraits.filter((p) => p.character_id && p.character_id === paramP.character_id).slice(0, 6).map((p) => (
                            <span key={p.id} style={{ width: 40, height: 40, borderRadius: 8, overflow: "hidden", border: `1px solid ${p.id === paramP.id ? T.accent : T.line}`, background: "#22262f", display: "flex", alignItems: "center", justifyContent: "center" }}>
                              {p.path ? <img src={convertFileSrc(p.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "cover" }} /> : <span>🧑</span>}
                            </span>
                          ))}
                          {portraits.filter((p) => p.character_id && p.character_id === paramP.character_id).length === 0 && <span style={{ fontSize: 10.5, color: T.text3 }}>该素材未绑定角色</span>}
                        </div>
                        <div style={{ fontSize: 10, color: T.text3, marginTop: 8 }}>参考强度 denoise：AI 生成立绘接入后开放</div>
                      </div>

                    </div>
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                    <div style={{ border: `1px solid ${T.line}`, borderRadius: 12, background: paramP.path ? "#191c22" : "linear-gradient(160deg,#2a3140,#191c22)", minHeight: 300, display: "flex", alignItems: "center", justifyContent: "center", position: "relative", overflow: "hidden" }}>
                      <span style={{ position: "absolute", top: 9, left: 10, fontSize: 9.5, padding: "2px 7px", borderRadius: 5, background: "rgba(0,0,0,.5)", color: "#dfe3ec" }}>{paramP.ratio || "预览"}{paramP.width && paramP.height ? ` · ${paramP.width}×${paramP.height}` : ""}</span>
                      {paramP.path ? <img src={convertFileSrc(paramP.path)} alt="" style={{ width: "100%", height: "100%", objectFit: "contain", position: "absolute", inset: 0 }} /> : <div style={{ textAlign: "center", color: T.text3 }}><div style={{ fontSize: 40 }}>🧑</div><div style={{ fontSize: 11.5, marginTop: 8 }}>预览区</div><small style={{ fontSize: 9.5 }}>生成后在此显示候选</small></div>}
                      <span style={{ position: "absolute", bottom: 9, left: 10, fontSize: 9.5, color: "#8a91a1", fontFamily: "monospace" }}>seed {pParamForm.seed || "—"} · steps {pParamForm.steps || "—"} · cfg {pParamForm.cfg || "—"}</span>
                    </div>
                    <button disabled title="规划中：AI 生成立绘" style={{ height: 34, borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontWeight: 650, fontSize: 12.5, opacity: 0.45, cursor: "not-allowed"}}>✨ 生成 4 张<span style={{ marginLeft: 6, fontSize: 10, opacity: 0.9 }}>规划中</span></button>
                    <button onClick={saveParamPortrait} style={{ height: 32, borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5 }}>💾 保存参数</button>
                    <button onClick={() => paramP && setPParamForm({ prompt: paramP.prompt || "", negative: paramP.negative || "", seed: paramP.seed || "", steps: paramP.steps || "", cfg: paramP.cfg || "", model: paramP.model || "" })} style={{ height: 32, borderRadius: 8, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontSize: 12.5 }}>↩ 恢复当前值</button>
                    <div style={{ fontSize: 10.5, color: T.text3, lineHeight: 1.8, padding: "2px 4px" }}>
                      素材：{paramP.name} · {paramP.ver || "v1"}<br />
                      绑定角色：{paramP.bound_character || paramP.role || "未绑定"}<br />
                      更新：{paramP.date || "—"}
                    </div>
                  </div>
                </div>
                {/* 未开放的生成参数：原来是 5 个散落的虚线框，收进折叠区，减少视觉噪音 */}
                <div style={{ marginTop: 14, border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel2, overflow: "hidden" }}>
                  <button onClick={() => setPUnopenOpen(!pUnopenOpen)}
                    style={{ width: "100%", textAlign: "left", padding: "9px 13px", border: "none", background: "transparent", color: T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit", display: "flex", alignItems: "center", gap: 8 }}>
                    <span>{pUnopenOpen ? "▾" : "▸"}</span>
                    <span>⚙ 未开放的生成参数（5 项）</span>
                    <span style={{ marginLeft: "auto", fontSize: 10.5, color: T.text3 }}>依赖生图插件 / AI 生成接入后开放</span>
                  </button>
                  {pUnopenOpen && (
                    <div style={{ padding: "0 13px 12px", display: "flex", flexWrap: "wrap", gap: 6, borderTop: `1px solid ${T.lineSoft}`, paddingTop: 10 }}>
                      {["LoRA / 挂载权重", "采样器", "其他尺寸比例", "输出格式（透明底）", "单次张数"].map((x) => (
                        <span key={x} style={{ fontSize: 10.5, padding: "3px 9px", borderRadius: 11, border: `1px dashed ${T.line}`, color: T.text3 }}>{x}</span>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        );
      })()
  );
}
