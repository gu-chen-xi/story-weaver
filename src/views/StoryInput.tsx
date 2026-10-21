// 「StoryInput」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { computeBudget } from "../lib/parse";
import { useGeneration } from "../hooks/useGeneration";
import { T } from "../theme";
import type { Character, DebugReq, Story } from "../types";
import { invoke } from "@tauri-apps/api/core";

export type ViewName =
  | "chars" | "world" | "relations" | "story" | "proposals" | "temp" | "timeline" | "stories" | "plugins" | "portrait" | "compliance" | "params" | "home";

export type StoryInputProps = {
  activeCleanupRef: { current: (() => void) | null };
  aiRole: string;
  autoAnalyze: boolean;
  autoSummarizeToday: (storyId: string, nodeId: number) => Promise<void>;
  backfillSummaries: (storyId: string, currentNodeId: number) => Promise<void>;
  chars: Character[];
  /** v3.4：导演监视器「当前角色卡」选中的角色 id（示例对话第③优先级用） */
  focusCharId?: number | null;
  constraints: string[];
  ctxWindow: number;
  currentStory: Story | null;
  dayDebug: DebugReq | null;
  directors: string[];
  genRequestId: string | null;
  genRequestRef: { current: string | null };
  input: string;
  lenLevel: "简单" | "详细";
  loadChars: (storyId: number) => Promise<void>;
  loadDebug: () => Promise<void>;
  loadEvals: () => Promise<void>;
  loadProposals: () => Promise<void>;
  loadTimelineAndMessages: (storyId: number, nodeId?: number, branchOverride?: string | null) => Promise<void>;
  memoryCtx: boolean;
  msg: string;
  myRole: string;
  navMode: "旁白" | "对话";
  node: { id: number; date_label: string } | null;
  pluginOn: (id: string) => boolean;
  recallDay: number | null;
  recallDays: { node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; tags?: string[] }[];
  sensitiveFilter: boolean;
  setAiRole: Dispatch<SetStateAction<string>>;
  setConstraints: Dispatch<SetStateAction<string[]>>;
  setGenRequestId: Dispatch<SetStateAction<string | null>>;
  setInput: Dispatch<SetStateAction<string>>;
  setLenLevel: Dispatch<SetStateAction<"简单" | "详细">>;
  setMemoryCtx: Dispatch<SetStateAction<boolean>>;
  setMessages: Dispatch<SetStateAction<{ id: number; role: string; content: string; mode: string; created_at: string }[]>>;
  setMsg: Dispatch<SetStateAction<string>>;
  setMyRole: Dispatch<SetStateAction<string>>;
  setNavMode: Dispatch<SetStateAction<"旁白" | "对话">>;
  setRecallDay: Dispatch<SetStateAction<number | null>>;
  setRecallDays: Dispatch<SetStateAction<{ node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; tags?: string[] }[]>>;
  setShowRecallPicker: Dispatch<SetStateAction<boolean>>;
  updateDirectors: (list: string[]) => void;
};

export function StoryInput(props: StoryInputProps) {
  const { activeCleanupRef, aiRole, autoAnalyze, autoSummarizeToday, backfillSummaries, chars, constraints, focusCharId, ctxWindow, currentStory, dayDebug, directors, genRequestId, genRequestRef, input, lenLevel, loadChars, loadDebug, loadEvals, loadProposals, loadTimelineAndMessages, memoryCtx, msg, myRole, navMode, node, pluginOn, recallDay, recallDays, sensitiveFilter, setAiRole, setConstraints, setGenRequestId, setInput, setLenLevel, setMemoryCtx, setMessages, setMsg, setMyRole, setNavMode, setRecallDay, setRecallDays, setShowRecallPicker, updateDirectors } = props;
  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const [showAt, setShowAt] = useState(false);   // @角色 菜单
  const [showTag, setShowTag] = useState(false); // #标签 菜单
  const [tagCustom, setTagCustom] = useState("");
  const [showLenMenu, setShowLenMenu] = useState(false); // 📏 长度 下拉
  // 点别处自动收起（window mousedown；下拉项自己 stopPropagation）
  useEffect(() => {
    if (!showLenMenu) return;
    const close = () => setShowLenMenu(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [showLenMenu]);
  const PRESET_TAGS = ["主线", "日常", "回忆", "倒叙", "插曲", "战斗", "情感"];
  const [showConstraintInput, setShowConstraintInput] = useState(false);
  const [constraintDraft, setConstraintDraft] = useState("");
  const [showDirectorInput, setShowDirectorInput] = useState(false);
  const [directorDraft, setDirectorDraft] = useState("");
  const openRecallPicker = async () => {
    if (!currentStory) return;
    try {
      setRecallDays(await invoke<{ node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; tags?: string[] }[]>(
        "list_recall_days", { storyId: String(currentStory.id) }
      ));
      setShowRecallPicker(true);
    } catch (e) { setMsg("❌ 读取可回顾的天失败：" + String(e)); }
  };
  const updateConstraints = (list: string[]) => { setConstraints(list); localStorage.setItem("ai.constraints", JSON.stringify(list)); };
  // 生成流全部在 hooks/useGeneration.ts（C1）：这里只保留渲染 + 本页小菜单状态
  // v3.4：导演监视器「当前角色卡」的焦点角色名。
  // 回落规则与 DirectorPanel 里一致（选中 → 有当前状态的 → 第一个角色），
  // 保证"界面上看到的焦点角色"就是"提示词里用其示例对话的角色"。
  const panelFocusName = (() => {
    const picked = chars.find((c) => c.id === focusCharId)
      ?? chars.find((x) => x.current_status)
      ?? chars[0];
    return picked?.name ?? "";
  })();

  const { sendMessage, stopGeneration } = useGeneration({
    currentStory, node, input, setInput, setMsg, navMode, directors, updateDirectors,
    sensitiveFilter, myRole, aiRole, constraints, lenLevel, memoryCtx, recallDay,
    genRequestRef, setGenRequestId, activeCleanupRef, setMessages,
    loadTimelineAndMessages, loadChars, loadDebug, loadProposals, loadEvals,
    backfillSummaries, autoSummarizeToday, autoAnalyze, pluginOn,
    panelFocusName,
  });

  const dayBudget = computeBudget(dayDebug, ctxWindow);
  return (
        (
          <div style={{ borderTop: `1px solid ${T.line}`, background: T.panel, padding: "12px 24px 14px", flexShrink: 0 }}>
            <div style={{ border: `1px solid ${T.line}`, background: T.panel2, borderRadius: 12, paddingBottom: 4 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px 0", flexWrap: "wrap" }}>
                <div style={{ display: "inline-flex", background: "#191c22", border: `1px solid ${T.line}`, borderRadius: 8, padding: 2 }}>
                  <button onClick={() => setNavMode("旁白")} style={{ border: "none", background: "transparent", color: navMode === "旁白" ? T.accent : T.text3, fontSize: 11.5, padding: "4px 11px", borderRadius: 6, cursor: "pointer", fontWeight: navMode === "旁白" ? 600 : 400, fontFamily: "inherit" }}>🎙 旁白</button>
                  <button onClick={() => setNavMode("对话")} style={{ border: "none", background: "transparent", color: navMode === "对话" ? T.accent : T.text3, fontSize: 11.5, padding: "4px 11px", borderRadius: 6, cursor: "pointer", fontWeight: navMode === "对话" ? 600 : 400, fontFamily: "inherit" }}>💬 对话</button>
                </div>
                {navMode === "对话" && (
                  <div style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                    <span style={{ fontSize: 11, color: T.text3 }}>我扮演</span>
                    <select value={myRole} onChange={(e) => setMyRole(e.target.value)} style={{ background: "#191c22", border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, fontSize: 11.5, padding: "3px 6px", outline: "none", fontFamily: "inherit" }}>
                      <option value="自由">自由</option>
                      {chars.map((cc) => <option key={cc.id} value={cc.name}>{cc.name}</option>)}
                    </select>
                    <span style={{ fontSize: 11, color: T.text3 }}>AI 扮演</span>
                    <select value={aiRole} onChange={(e) => setAiRole(e.target.value)} style={{ background: "#191c22", border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, fontSize: 11.5, padding: "3px 6px", outline: "none", fontFamily: "inherit" }}>
                      <option value="自由">自由</option>
                      {chars.map((cc) => <option key={cc.id} value={cc.name}>{cc.name}</option>)}
                    </select>
                  </div>
                )}
                <span style={{ position: "relative" }}>
                  <span onClick={() => setShowAt((v) => !v)} title="绑定本次聚焦角色" style={{ fontSize: 11, color: showAt ? T.accent : T.text3, border: showAt ? `1px solid ${T.accent}` : `1px solid ${T.line}`, borderRadius: 6, padding: "3px 8px", cursor: "pointer", background: showAt ? T.accentSoft : "transparent" }}><span style={{ color: T.accent }}>@</span> 角色</span>
                  {showAt && (
                    <div style={{ position: "absolute", top: "100%", left: 0, zIndex: 30, marginTop: 4, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8, padding: 8, minWidth: 150, boxShadow: "0 10px 28px rgba(0,0,0,.45)", display: "flex", flexWrap: "wrap", gap: 6 }}>
                      {chars.length === 0 && <div style={{ fontSize: 11, color: T.text3, padding: 4 }}>暂无角色，先到角色管理创建</div>}
                      {chars.map((cc) => (
                        <span key={cc.id} onClick={() => { setInput((v) => (v ? v + " " : "") + "@" + cc.name + " "); setShowAt(false); }} style={{ fontSize: 11.5, color: T.text1, background: "#191c22", border: `1px solid ${T.lineSoft}`, borderRadius: 6, padding: "3px 9px", cursor: "pointer" }}>{cc.name}</span>
                      ))}
                    </div>
                  )}
                </span>
                <span style={{ position: "relative" }}>
                  <span onClick={() => setShowTag((v) => !v)} title="给本节点打标签" style={{ fontSize: 11, color: showTag ? T.accent : T.text3, border: showTag ? `1px solid ${T.accent}` : `1px solid ${T.line}`, borderRadius: 6, padding: "3px 8px", cursor: "pointer", background: showTag ? T.accentSoft : "transparent" }}><span style={{ color: T.accent }}>#</span> 标签</span>
                  {showTag && (
                    <div style={{ position: "absolute", top: "100%", left: 0, zIndex: 30, marginTop: 4, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8, padding: 8, minWidth: 210, boxShadow: "0 10px 28px rgba(0,0,0,.45)", display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                      {PRESET_TAGS.map((t) => (
                        <span key={t} onClick={() => { setInput((v) => (v ? v + " " : "") + "#" + t + " "); setShowTag(false); }} style={{ fontSize: 11.5, color: T.text1, background: "#191c22", border: `1px solid ${T.lineSoft}`, borderRadius: 6, padding: "3px 9px", cursor: "pointer" }}>#{t}</span>
                      ))}
                      <input
                        value={tagCustom}
                        onChange={(e) => setTagCustom(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter" && tagCustom.trim()) { setInput((v) => (v ? v + " " : "") + "#" + tagCustom.trim() + " "); setTagCustom(""); setShowTag(false); } }}
                        placeholder="自定义标签"
                        style={{ width: 90, fontSize: 11.5, background: "#191c22", border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, padding: "3px 8px", outline: "none" }}
                      />
                    </div>
                  )}
                </span>
                <span style={{ marginLeft: "auto", fontSize: 11, color: T.text3 }}>旁白＝上帝扩写 · 对话＝沉浸扮演</span>
              </div>
              <textarea
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendMessage(); } }}
                placeholder="描述你想要的剧情走向、语气或镜头感…（Shift+Enter 换行）"
                style={{ width: "100%", boxSizing: "border-box", border: "none", background: "transparent", resize: "none", outline: "none", color: T.text1, fontSize: 13.5, lineHeight: 1.7, padding: "10px 14px 4px", minHeight: 56, fontFamily: "inherit" }}
              />
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px 10px", flexWrap: "wrap" }}>
                <span title="内置常开：拦截输入中的常见辱骂词，并把 AI 输出中的同词替换为 ***（本软件不具备内容审查能力，内容合法性仍由你自行判断）"
                  style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.ok, fontSize: 11.5, fontFamily: "inherit", cursor: "default" }}>🛡 敏感词过滤 · 内置开启</span>
                <button onClick={() => setShowConstraintInput(!showConstraintInput)} style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: constraints.length > 0 || showConstraintInput ? T.ok : T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>🔒 剧情约束{constraints.length > 0 ? `(${constraints.length})` : ""}</button>
                {navMode === "对话" && (
                  <button onClick={() => setShowDirectorInput(!showDirectorInput)} title="导演指令：强制剧情这样走（一次性，附加到本次发送）" style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${directors.length > 0 ? "rgba(255,122,24,.42)" : T.line}`, background: directors.length > 0 ? T.accentSoft : "#191c22", color: directors.length > 0 || showDirectorInput ? T.accent : T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>🎬 导演指令{directors.length > 0 ? `(${directors.length})` : ""}</button>
                )}
                {/* 📏 长度：下拉框，**向上展开**（2026-10-18 用户要求：
                    它在输入框底栏，向下弹会被窗口边缘切掉） */}
                <span style={{ position: "relative", display: "inline-flex" }}>
                  <button
                    onClick={(e) => { e.stopPropagation(); setShowLenMenu((v) => !v); }}
                    title="回复长度：简单＝五到十行；详细＝四项充分描写"
                    style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px",
                      borderRadius: 7, border: `1px solid ${showLenMenu ? T.accent : T.line}`, background: "#191c22",
                      color: T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>
                    📏 长度
                    <b style={{ color: T.accent, fontWeight: 600 }}>{lenLevel}</b>
                    <span style={{ fontSize: 9, color: T.text3 }}>{showLenMenu ? "▾" : "▴"}</span>
                  </button>
                  {showLenMenu && (
                    <div
                      onMouseDown={(e) => e.stopPropagation()}
                      style={{ position: "absolute", bottom: "100%", left: 0, marginBottom: 6, zIndex: 40,
                        minWidth: 215, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 9,
                        boxShadow: "0 10px 26px rgba(0,0,0,.45)", overflow: "hidden" }}>
                      {([["简单", "篇幅精简，五到十行，动作神态择要，心理不省，环境不展开"],
                         ["详细", "篇幅不限，动作/神态/心理/环境四项充分描写"]] as const).map(([k, tip]) => (
                        <div key={k}
                          onClick={() => { setLenLevel(k); setShowLenMenu(false); }}
                          title={tip}
                          style={{ padding: "8px 11px", fontSize: 11.5, cursor: "pointer",
                            background: lenLevel === k ? T.accentSoft : "transparent",
                            color: lenLevel === k ? T.accent : T.text2,
                            fontWeight: lenLevel === k ? 600 : 400 }}>
                          {k === "简单" ? "简单 · 精简" : "详细 · 展开"}
                          <div style={{ fontSize: 10, color: T.text3, fontWeight: 400, marginTop: 2, lineHeight: 1.5 }}>
                            {tip}
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </span>
                <span onClick={() => setMemoryCtx(!memoryCtx)} style={{ display: "inline-flex", alignItems: "center", gap: 7, fontSize: 11.5, color: T.text2, cursor: "pointer" }}>
                  <span style={{ width: 30, height: 17, borderRadius: 9, background: memoryCtx ? "rgba(55,201,120,.6)" : T.line, position: "relative", display: "inline-block", transition: "background .2s" }}>
                    <span style={{ position: "absolute", top: 2, left: memoryCtx ? 15 : 2, width: 13, height: 13, borderRadius: "50%", background: memoryCtx ? "#1c1f26" : "#22262f", transition: "left .2s" }} />
                  </span>记忆
                </span>
                {/* RAG ①：手动回顾 —— 指定某一天，直接把它拼进提示词 */}
                <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                  <button onClick={() => void openRecallPicker()} title="选一天，把那天聊过的原始对话拼进本次提示词（不依赖自动识别）"
                    style={{ height: 26, padding: "0 10px", borderRadius: 7, border: `1px solid ${recallDay ? T.accent : T.line}`, background: recallDay ? T.accentSoft : "transparent", color: recallDay ? T.accent : T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit" }}>🔍 回顾</button>
                  {recallDay !== null && (
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 5, padding: "2px 8px", borderRadius: 10, border: `1px solid ${T.accent}55`, background: T.accentSoft, color: T.accent, fontSize: 11 }}>
                      {recallDays.find((d) => d.node_id === recallDay)?.date_label ?? `节点 #${recallDay}`}
                      <button onClick={() => setRecallDay(null)} title="取消本次回顾" style={{ background: "none", border: "none", color: T.accent, cursor: "pointer", fontSize: 11, padding: 0, fontFamily: "inherit" }}>✕</button>
                    </span>
                  )}
                </span>
                {showConstraintInput && (
                  <div style={{ width: "100%", display: "flex", flexDirection: "column", gap: 6, marginTop: 2 }}>
                    <div style={{ display: "flex", gap: 6 }}>
                      <input
                        value={constraintDraft}
                        onChange={(e) => setConstraintDraft(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); const v = constraintDraft.trim(); if (v) { updateConstraints([...constraints, v]); setConstraintDraft(""); } } }}
                        placeholder="添加剧情约束，如：角色不会死亡（回车添加）"
                        style={{ flex: 1, padding: "7px 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12, outline: "none", fontFamily: "inherit" }}
                      />
                      <button onClick={() => { const v = constraintDraft.trim(); if (v) { updateConstraints([...constraints, v]); setConstraintDraft(""); } }} style={{ padding: "0 12px", border: "none", borderRadius: 7, background: T.accent, color: "#231505", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>添加</button>
                    </div>
                    {constraints.length > 0 && (
                      <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                        {constraints.map((cst, i) => (
                          <span key={i} style={{ display: "inline-flex", alignItems: "center", gap: 5, padding: "3px 8px", borderRadius: 10, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, fontSize: 11 }}>
                            {cst}
                            <button onClick={() => updateConstraints(constraints.filter((_, j) => j !== i))} style={{ color: T.danger, background: "none", border: "none", cursor: "pointer", fontSize: 11, padding: 0, fontFamily: "inherit" }}>✕</button>
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                )}
                {showDirectorInput && navMode === "对话" && (
                  <div style={{ width: "100%", display: "flex", flexDirection: "column", gap: 6, marginTop: 2 }}>
                    <div style={{ display: "flex", gap: 6 }}>
                      <input
                        value={directorDraft}
                        onChange={(e) => setDirectorDraft(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); const v = directorDraft.trim(); if (v) { updateDirectors([...directors, v]); setDirectorDraft(""); } } }}
                        placeholder="导演指令：强制剧情这样走，如：强制让夏婉琴接受告白（回车添加，发送后自动清空）"
                        style={{ flex: 1, padding: "7px 10px", borderRadius: 7, border: `1px solid rgba(255,122,24,.35)`, background: "#191c22", color: T.text1, fontSize: 12, outline: "none", fontFamily: "inherit" }}
                      />
                      <button onClick={() => { const v = directorDraft.trim(); if (v) { updateDirectors([...directors, v]); setDirectorDraft(""); } }} style={{ padding: "0 12px", border: "none", borderRadius: 7, background: T.accent, color: "#231505", fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>添加</button>
                    </div>
                    {directors.length > 0 && (
                      <div style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                        {directors.map((d, i) => (
                          <span key={i} title={d} style={{ display: "inline-flex", alignItems: "center", gap: 5, maxWidth: 320, padding: "3px 8px", borderRadius: 10, border: `1px solid rgba(255,122,24,.42)`, background: T.accentSoft, color: T.accent, fontSize: 11 }}>
                            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>🎬 {d}</span>
                            <button onClick={() => updateDirectors(directors.filter((_, j) => j !== i))} style={{ color: T.danger, background: "none", border: "none", cursor: "pointer", fontSize: 11, padding: 0, fontFamily: "inherit" }}>✕</button>
                          </span>
                        ))}
                        <span style={{ fontSize: 10.5, color: T.text3 }}>这些指令只在下次发送时生效，发完自动清空</span>
                      </div>
                    )}
                  </div>
                )}
                <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 10 }}>
                  <span title={dayDebug ? `当前节点 ${node?.date_label ?? "—"} · 已用 ${dayBudget.usedTokens.toLocaleString("en-US")} / ${dayBudget.inputBudget.toLocaleString("en-US")} token · 剩余 ${dayBudget.remain.toLocaleString("en-US")}` : "当前节点还没有请求记录"} style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 10.5, color: T.text3, cursor: "default" }}>
                    <span style={{ width: 7, height: 7, borderRadius: "50%", background: dayDebug ? (dayBudget.pctUsed > 85 ? T.danger : dayBudget.pctUsed > 60 ? T.warn : T.ok) : T.line }} />
                    <span>Token {dayDebug ? `${dayBudget.pctUsed}%` : "—"}</span>
                    <span style={{ width: 46, height: 5, borderRadius: 3, background: "#12141a", border: `1px solid ${T.lineSoft}`, overflow: "hidden", display: "inline-block" }}>
                      <span style={{ display: "block", height: "100%", width: `${dayBudget.pctUsed}%`, background: dayDebug ? (dayBudget.pctUsed > 85 ? T.danger : dayBudget.pctUsed > 60 ? T.warn : T.ok) : "transparent" }} />
                    </span>
                  </span>
                  {/* 第五批：**把提示放到输入框旁边**。
                      原来 msg 只渲染在中间工作区顶部（滚动区里），用户在底部打字时
                      根本看不到「正在生成中，不能重复发送」这类提示（用户实测反馈）。 */}
                  {msg && (
                    <span style={{
                      fontSize: 11, maxWidth: 520, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
                      color: /^[❌⚠🛡]/.test(msg) ? T.warn : T.text2,
                    }} title={msg}>{msg}</span>
                  )}
                  <span style={{ fontSize: 10.5, color: T.text3 }}>{input.length} / 2000</span>
                  {/* 第五批（《现状说明》5.2）：生成中显示「停止」——
                      以前没有取消入口，上游不响应时界面永远转圈，只能杀进程。 */}
                  {genRequestId ? (
                    <button onClick={() => void stopGeneration()}
                      title="停止本次生成（已生成的部分会保留，并标记为未完成）"
                      style={{ height: 32, padding: "0 18px", border: `1px solid ${T.danger}`, borderRadius: 8, cursor: "pointer", background: "transparent", color: T.danger, fontSize: 13, fontWeight: 650, fontFamily: "inherit" }}>⏹ 停止</button>
                  ) : (
                    <button onClick={sendMessage} style={{ height: 32, padding: "0 18px", border: "none", borderRadius: 8, cursor: "pointer", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontSize: 13, fontWeight: 650, fontFamily: "inherit", boxShadow: "0 4px 14px rgba(255,122,24,.26)" }}>➤ 提交</button>
                  )}
                </div>
              </div>
            </div>
          </div>
        )
  );
}
