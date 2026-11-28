// 「StoryInput」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import type { AppView } from "../types";
import { computeBudget } from "../lib/parse";
import { directorGate } from "../lib/directorGate";
import { useGeneration } from "../hooks/useGeneration";
import { T, GRAD_ACCENT } from "../theme";
import { Icon } from "../components/icons";
import type { Character, DebugReq, DirectorLedger, Relation, RelationChange, Story } from "../types";
import { DirectorLedgerView } from "./DirectorLedger";
import { invoke } from "@tauri-apps/api/core";
import { createPortal } from "react-dom";
import { reportError } from "../lib/errors";
import { roleSwitchHint } from "../lib/roleHint";

export type ViewName = AppView;

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
  lenLevel: "简单" | "详细";
  loadChars: (storyId: number) => Promise<void>;
  loadDebug: () => Promise<void>;
  loadProposals: () => Promise<void>;
  /// 赦免请求变了之后让剧情区重新拉一次（盒子挂在 AI 回复下面）
  loadPardons: () => Promise<void>;
  /// 关系变化撤销后要让关系图谱重新拉一次
  loadRelations: (storyId: number) => Promise<void>;
  loadTimelineAndMessages: (storyId: number, nodeId?: number, branchOverride?: string | null) => Promise<void>;
  memoryCtx: boolean;
  /// 全局提示（浮在输入框上方 20px；不参与布局）
  msg: string;
  myRole: string;
  navMode: "旁白" | "对话";
  node: { id: number; date_label: string } | null;
  recallPick: number[];
  /// 关系（导演指令闸门要看 AI 扮演者 → 用户扮演者 的强度）
  relations: Relation[];
  recallDays: { node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; eligible: boolean; tags?: string[] }[];
  sensitiveFilter: boolean;
  setAiRole: Dispatch<SetStateAction<string>>;
  /// 改剧情约束（落盘在 hook 里，键名只有一处）
  updateConstraints: (list: string[]) => void;
  setGenRequestId: Dispatch<SetStateAction<string | null>>;
  setLenLevel: Dispatch<SetStateAction<"简单" | "详细">>;
  setMemoryCtx: Dispatch<SetStateAction<boolean>>;
  setMessages: Dispatch<SetStateAction<{ id: number; role: string; content: string; mode: string; created_at: string }[]>>;
  setMsg: Dispatch<SetStateAction<string>>;
  setMyRole: Dispatch<SetStateAction<string>>;
  setNavMode: Dispatch<SetStateAction<"旁白" | "对话">>;
  setRecallPick: Dispatch<SetStateAction<number[]>>;
  setRecallDays: Dispatch<SetStateAction<{ node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; eligible: boolean; tags?: string[] }[]>>;
  setShowRecallPicker: Dispatch<SetStateAction<boolean>>;
  updateDirectors: (list: string[]) => void;
};

// 2026-11-25：输入框「＋」上拉框的两个排版零件（分组标题 + 设置行）。
// 样式口径对齐设置弹窗：左标题/说明、右控件、展开区挂在该行下面。
const selStyle: React.CSSProperties = {
  height: 28, padding: "0 6px", borderRadius: 7, border: `1px solid ${T.line}`,
  background: T.fill, color: T.text2, fontSize: 11.5, fontFamily: "inherit",
  cursor: "pointer", outline: "none",
};
const rowBtn: React.CSSProperties = {
  height: 26, padding: "0 10px", borderRadius: 7, border: `1px solid ${T.line}`,
  background: "transparent", color: T.text2, fontSize: 11.5, cursor: "pointer", fontFamily: "inherit",
};

function ToolsSection(props: { title: string; children: React.ReactNode }) {
  return (
    <div style={{ marginTop: 10 }}>
      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.2, padding: "0 2px 6px" }}>{props.title}</div>
      <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: "rgba(0,0,0,.14)", padding: "0 10px" }}>
        {props.children}
      </div>
    </div>
  );
}

function ToolsRow(props: {
  title: string;
  desc?: string;
  right?: React.ReactNode;
  children?: React.ReactNode;
  dim?: boolean;
  last?: boolean;
}) {
  return (
    <div style={{ padding: "9px 0", borderBottom: props.last ? "none" : `1px solid ${T.lineSoft}` }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 12.5, color: props.dim ? T.text3 : T.text1, fontWeight: 500 }}>{props.title}</div>
          {props.desc ? <div style={{ fontSize: 10.5, color: T.text3, lineHeight: 1.55, marginTop: 2 }}>{props.desc}</div> : null}
        </div>
        {props.right ? <div style={{ flexShrink: 0 }}>{props.right}</div> : null}
      </div>
      {props.children ? <div style={{ marginTop: 8 }}>{props.children}</div> : null}
    </div>
  );
}

export function StoryInput(props: StoryInputProps) {
  const { activeCleanupRef, aiRole, autoAnalyze, autoSummarizeToday, backfillSummaries, chars, constraints, updateConstraints, focusCharId, ctxWindow, currentStory, dayDebug, directors, genRequestId, genRequestRef, lenLevel, loadChars, loadDebug, loadProposals, loadPardons, loadRelations, loadTimelineAndMessages, memoryCtx, msg, myRole, navMode, node, recallPick, recallDays, relations, sensitiveFilter, setAiRole, setGenRequestId, setLenLevel, setMemoryCtx, setMessages, setMsg, setMyRole, setNavMode, setRecallPick, setRecallDays, setShowRecallPicker, updateDirectors } = props;
  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const [showAt, setShowAt] = useState(false);   // @角色 菜单
  const [showTag, setShowTag] = useState(false); // #标签 菜单
  const [tagCustom, setTagCustom] = useState("");
  // 2026-11-07：底栏那排工具收进「＋」上拉框（用户要的：平时只留 ＋ / Token / 提交）
  const [toolsOpen, setToolsOpen] = useState(false);
  const [input, setInput] = useState("");
  // 2026-11-07：抽屉走 Portal 挂到 body 上 —— 放在流里会把输入框撑大（用户实测图）。
  // 位置按底栏与 ＋ 按钮的实测坐标算：贴在 ＋ 正上方，宽度跟输入卡片一致。
  const [toolsAnchor, setToolsAnchor] = useState<{ left: number; width: number; bottom: number } | null>(null);
  const barRef = useRef<HTMLDivElement | null>(null);
  const plusRef = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (!toolsOpen) return;
    const measure = () => {
      const bar = barRef.current?.getBoundingClientRect();
      const btn = plusRef.current?.getBoundingClientRect();
      if (!bar || !btn) return;
      setToolsAnchor({ left: bar.left, width: bar.width, bottom: window.innerHeight - btn.top + 8 });
    };
    measure();
    // 点抽屉外面就收起；输入框不算外面（点它只是继续打字）
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest("[data-tools-drawer]") || t.closest("[data-tools-toggle]")) return;
      setToolsOpen(false);
    };
    window.addEventListener("resize", measure);
    document.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("resize", measure);
      document.removeEventListener("mousedown", onDown);
    };
  }, [toolsOpen]);
  const PRESET_TAGS = ["主线", "日常", "回忆", "倒叙", "插曲", "战斗", "情感"];
  const [showConstraintInput, setShowConstraintInput] = useState(false);
  const [constraintDraft, setConstraintDraft] = useState("");
  const [showDirectorInput, setShowDirectorInput] = useState(false);
  const [directorDraft, setDirectorDraft] = useState("");
  // 导演账本（2026-10-27）：按「故事 + AI 扮演者」取账，累计到上限就锁导演指令。
  // 数据只有后端算（阈值也在后端），前端不另存一份常量 —— 两边各写一份迟早会漂。
  // 时段标记（2026-10-29）：早/午/晚，用户自己打；**只在变档时标**，默认「早」。
  // 换天（node.id 变）重置为「早」——新的一天从早上开始，符合直觉。
  const [tod, setTod] = useState<"早" | "午" | "晚">("早");
  useEffect(() => { setTod("早"); }, [node?.id]);
  const [ledger, setLedger] = useState<DirectorLedger | null>(null);
  const [showLedger, setShowLedger] = useState(false);
  const [costBusyId, setCostBusyId] = useState<number | null>(null);
  const loadLedger = useCallback(async () => {
    if (!currentStory) { setLedger(null); return; }
    try {
      setLedger(await invoke<DirectorLedger>("list_director_costs", {
        storyId: String(currentStory.id),
        characterName: aiRole,
        currentNodeId: node?.id ?? null, // 2026-10-30：旧账按「距今多少个故事日」衰减，需要当前节点
      }));
    } catch (e) {
      // 账本读不到不该挡住写作：清空即可（下一次生成后还会再拉）
      reportError("读取导演账本", e, { level: "warn" });
      setLedger(null);
    }
    // deps 里带 node.id：切分支 / 换天时读的是**另一个分支的账**，不重拉就会显示旧数字
  }, [currentStory, aiRole, node?.id]);
  useEffect(() => { void loadLedger(); }, [loadLedger]);
  // 账本打开时才拉关系变化（省一次 IPC；它只在这个面板里用）
  useEffect(() => { if (showLedger) void loadRelChanges(); }, [showLedger, currentStory?.id, node?.id]);
  const [msgFading, setMsgFading] = useState(false);
  // 剧情区提示**自动淡出**（2026-10-25 用户定：6 秒，不多不少）：
  // 前 5.4 秒正常显示，最后 0.6 秒透明度渐变到 0，然后清空。
  // 为什么放这里而不是 App：这条提示只在剧情页渲染，动画状态跟着它的生命周期走最省事。
  useEffect(() => {
    if (!msg) { setMsgFading(false); return; }
    setMsgFading(false);
    const t1 = window.setTimeout(() => setMsgFading(true), 5400);
    const t2 = window.setTimeout(() => { setMsg(""); setMsgFading(false); }, 6000);
    return () => { window.clearTimeout(t1); window.clearTimeout(t2); };
  }, [msg, setMsg]);
  // 「导演指令」闸门（v3.7.0）：AI 扮演者 → 用户扮演者 的关系强度 = 0（彻底决裂）时
  // 软件直接不给发。为什么不做成提示词规则：这属于"能不能做"，不是"怎么做"，
  // 交给 AI 判断它一定会滥用（模型对"可以拒绝"的理解是"尽量拒绝"）。
  // 判定逻辑在 lib/directorGate.ts（纯函数，有 tests/directorGate.test.cjs 盯着）。
  const gate = directorGate({
    mode: navMode, myRole, aiRole, chars, relations,
    // 第二条件（2026-10-27）：代价攒满也锁 —— 阈值来自后端账本，取不到账本时传 null（不拦）
    cost: ledger ? { total: ledger.total, limit: ledger.limit } : null,
  });
  // 强度掉到 0 时，已经展开的面板要收起来（不然输入框还在，用户以为能填）
  useEffect(() => {
    if (gate.blocked) setShowDirectorInput(false);
  }, [gate.blocked]);
  const openRecallPicker = async () => {
    if (!currentStory) return;
    try {
      setRecallDays(await invoke<{ node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; eligible: boolean; tags?: string[] }[]>(
        "list_recall_days", { storyId: String(currentStory.id), nodeId: node?.id ?? null }
      ));
      setShowRecallPicker(true);
    } catch (e) { setMsg(reportError("读取可回顾的天", e, { silent: true })); }
  };
  
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
    sensitiveFilter, myRole, aiRole, constraints, lenLevel, memoryCtx, recallPick,
    genRequestRef, setGenRequestId, activeCleanupRef, setMessages,
    loadTimelineAndMessages, loadChars, loadDebug, loadProposals,
    backfillSummaries, autoSummarizeToday, autoAnalyze,
    panelFocusName, directorBlocked: gate.blocked,
    tod,
    // 每轮生成后后端会结一笔账，账本要跟着刷新（不然累计停在旧数字上）
    reloadDirectorCosts: loadLedger,
  });

  // 关系变化账本（2026-10-29）：可查可撤；撤销会让关系图谱变，所以顺手重拉一次关系
  const [relChanges, setRelChanges] = useState<RelationChange[]>([]);
  const loadRelChanges = async () => {
    if (!currentStory) { setRelChanges([]); return; }
    try {
      setRelChanges(await invoke<RelationChange[]>("list_relation_changes", { storyId: String(currentStory.id) }));
    } catch (e) { reportError("载入关系变化", e, { level: "warn" }); }
  };
  const undoRelationChange = async (id: number) => {
    if (!currentStory) return;
    setCostBusyId(id);
    try {
      const r = await invoke<{ new: number }>("undo_relation_change", { storyId: String(currentStory.id), changeId: id });
      setMsg(`↩ 已撤销这条关系变化（强度回到 ${r.new}）`);
      await loadRelChanges();
      await loadLedger();
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg(reportError("撤销导演代价", e, { silent: true }));
    } finally {
      setCostBusyId(null);
    }
  };
  // 赦免（2026-10-27 用户定）：三段式 —— 请求（弹窗 + 可补一句）→ 她同意 → 确认。
  // 请求不是把请求写进剧情：后端只把它挂成盒子，回复也不进历史。
  const [pardonFor, setPardonFor] = useState<number | null>(null);   // 正在写补充的那笔账 id
  const [pardonExtra, setPardonExtra] = useState("");
  const [pardonBusy, setPardonBusy] = useState(false);

  const requestPardon = async () => {
    if (!currentStory || pardonFor === null) return;
    setPardonBusy(true);
    try {
      const r = await invoke<{ verdict: string; message_id: number }>("request_pardon", {
        storyId: String(currentStory.id), costId: pardonFor, extra: pardonExtra,
        aiName: aiRole, myName: myRole,
      });
      const said = r.verdict === "agreed" ? "她同意了 —— 现在可以点「确认赦免」"
        : r.verdict === "refused" ? "她不同意"
        : "她的回答没看懂（可以再问一次）";
      setMsg(`📨 已发送赦免请求：${said}（盒子在那条 AI 回复下面）`);
      setPardonFor(null);
      setPardonExtra("");
      await loadLedger();
      await loadPardons();
      // 盒子挂在产生这笔账的那条 AI 回复下面 —— 那可能不是最新一条，
      // 所以主动滚过去（2026-10-27 用户实测：发出去了但看不到盒子）。
      window.setTimeout(() => {
        document.getElementById(`msg-${r.message_id}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
      }, 250);
    } catch (e) {
      setMsg(reportError("请求赦免", e, { silent: true }));
    } finally {
      setPardonBusy(false);
    }
  };

  const applyPardon = async (id: number) => {
    if (!currentStory) return;
    setCostBusyId(id);
    try {
      const msg = await invoke<string>("apply_pardon", {
        storyId: String(currentStory.id), costId: id, aiName: aiRole, myName: myRole,
      });
      setMsg("✅ " + msg);
      await loadLedger();
      await loadPardons();
      // 2026-10-31：赦免会**回补关系强度**（rel_delta/2），不重拉监视器与闸门还拿旧数
      await loadRelations(currentStory.id);
    } catch (e) {
      setMsg(reportError("确认赦免", e, { silent: true }));
    } finally {
      setCostBusyId(null);
    }
  };

  const undoCost = async (id: number) => {
    if (!currentStory) return;
    setCostBusyId(id);
    try {
      await invoke("delete_director_cost", { storyId: String(currentStory.id), costId: id });
      setMsg("🗑 已撤销这一笔（只允许删已赦免的账）");
      await loadLedger();
      await loadPardons();
    } catch (e) {
      setMsg(reportError("撤销导演代价", e, { silent: true }));
    } finally {
      setCostBusyId(null);
    }
  };

  const dayBudget = computeBudget(dayDebug, ctxWindow);
  return (
        (

          <div style={{ borderTop: "none", background: "transparent", padding: "0 24px 14px", marginTop: 8, flexShrink: 0, position: "relative" }}>
            {/* v3.5.7：**提示浮层**（回到用户认可的那个版本）——
                绝对定位在输入框上方 20px：**不参与布局**，
                所以提示出现/消失都不会把上面的剧情头部顶下去。
                （v3.5.5 曾改成"标题行下面一条 bar"，它不滚动但占竖向空间，
                  头部会跟着上下跳 —— 用户实测后改回。）*/}
            {msg && (
              <div
                aria-live="polite"
                style={{
                  position: "absolute",
                  left: 24,
                  right: 24,
                  bottom: "calc(100% + 20px)",
                  display: "flex",
                  justifyContent: "flex-end",
                  pointerEvents: "none",
                  zIndex: 8,
                  opacity: msgFading ? 0 : 1,
                  transition: "opacity .6s ease",
                }}
              >
                <div
                  style={{
                    pointerEvents: "auto",
                    maxWidth: "70%",
                    fontSize: 12,
                    lineHeight: 1.6,
                    padding: "6px 12px",
                    borderRadius: 10,
                    background: T.panel2,
                    border: `1px solid ${/^[❌⚠🛡]/.test(msg) ? T.warn : T.line}`,
                    color: /^[❌⚠🛡]/.test(msg) ? T.warn : T.text2,
                    boxShadow: "0 4px 14px rgba(0,0,0,.35)",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                  }}
                >
                  {msg}
                </div>
              </div>
            )}

            <div style={{ border: `1px solid ${T.line}`, background: T.panel2, borderRadius: 12, paddingBottom: 4 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px 0", flexWrap: "wrap" }}>
                <div style={{ display: "inline-flex", background: T.fill, border: `1px solid ${T.line}`, borderRadius: 8, padding: 2 }}>
                  {/* 2026-11-07：两个按钮合成一个**点击切换**；提示语挪进 title（原来那一行太占地方） */}
                  <button onClick={() => setNavMode(navMode === "旁白" ? "对话" : "旁白")}
                    title="旁白＝上帝扩写 · 对话＝沉浸扮演（点一下切换）"
                    style={{ border: "none", background: "transparent", color: T.accent, fontSize: 11.5,
                      padding: "4px 11px", borderRadius: 6, cursor: "pointer", fontWeight: 600, fontFamily: "inherit" }}>
                    {navMode} <span style={{ opacity: 0.75, fontSize: 10.5, marginLeft: 2 }}>⇄</span>
                  </button>
                </div>
                {navMode === "对话" && (
                  <div style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                    <span style={{ fontSize: 11, color: T.text3 }}>我扮演</span>
                    <select value={myRole} onChange={(e) => {
                      // 2026-11-11：换扮演者 = 换一段关系的视角（配对视角），旧对话不会消失，只是不在这个视角里
                      const next = e.target.value;
                      const hint = roleSwitchHint({ ai: aiRole, my: myRole }, { ai: aiRole, my: next });
                      setMyRole(next);
                      if (hint) setMsg(hint);
                    }} style={{ background: T.fill, border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, fontSize: 11.5, padding: "3px 6px", outline: "none", fontFamily: "inherit" }}>
                      <option value="自由">自由</option>
                      {chars.map((cc) => <option key={cc.id} value={cc.name}>{cc.name}</option>)}
                    </select>
                    <span style={{ fontSize: 11, color: T.text3 }}>AI 扮演</span>
                    <select value={aiRole} onChange={(e) => {
                      const next = e.target.value;
                      const hint = roleSwitchHint({ ai: aiRole, my: myRole }, { ai: next, my: myRole });
                      setAiRole(next);
                      if (hint) setMsg(hint);
                    }} style={{ background: T.fill, border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, fontSize: 11.5, padding: "3px 6px", outline: "none", fontFamily: "inherit" }}>
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
                        <span key={cc.id} onClick={() => { setInput((v) => (v ? v + " " : "") + "@" + cc.name + " "); setShowAt(false); }} style={{ fontSize: 11.5, color: T.text1, background: T.fill, border: `1px solid ${T.lineSoft}`, borderRadius: 6, padding: "3px 9px", cursor: "pointer" }}>{cc.name}</span>
                      ))}
                    </div>
                  )}
                </span>
                <span style={{ position: "relative" }}>
                  <span onClick={() => setShowTag((v) => !v)} title="给本节点打标签" style={{ fontSize: 11, color: showTag ? T.accent : T.text3, border: showTag ? `1px solid ${T.accent}` : `1px solid ${T.line}`, borderRadius: 6, padding: "3px 8px", cursor: "pointer", background: showTag ? T.accentSoft : "transparent" }}><span style={{ color: T.accent }}>#</span> 标签</span>
                  {showTag && (
                    <div style={{ position: "absolute", top: "100%", left: 0, zIndex: 30, marginTop: 4, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 8, padding: 8, minWidth: 210, boxShadow: "0 10px 28px rgba(0,0,0,.45)", display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                      {PRESET_TAGS.map((t) => (
                        <span key={t} onClick={() => { setInput((v) => (v ? v + " " : "") + "#" + t + " "); setShowTag(false); }} style={{ fontSize: 11.5, color: T.text1, background: T.fill, border: `1px solid ${T.lineSoft}`, borderRadius: 6, padding: "3px 9px", cursor: "pointer" }}>#{t}</span>
                      ))}
                      <input
                        value={tagCustom}
                        onChange={(e) => setTagCustom(e.target.value)}
                        onKeyDown={(e) => { if (e.key === "Enter" && tagCustom.trim()) { setInput((v) => (v ? v + " " : "") + "#" + tagCustom.trim() + " "); setTagCustom(""); setShowTag(false); } }}
                        placeholder="自定义标签"
                        style={{ width: 90, fontSize: 11.5, background: T.fill, border: `1px solid ${T.line}`, color: T.text1, borderRadius: 6, padding: "3px 8px", outline: "none" }}
                      />
                    </div>
                  )}
                </span>
              </div>
              <textarea
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendMessage(); } }}
                placeholder="描述你想要的剧情走向、语气或镜头感…（Shift+Enter 换行）"
                style={{ width: "100%", boxSizing: "border-box", border: "none", background: "transparent", resize: "none", outline: "none", color: T.text1, fontSize: 13.5, lineHeight: 1.7, padding: "10px 14px 4px", minHeight: 56, fontFamily: "inherit" }}
              />
              {toolsOpen && toolsAnchor && createPortal(
                <div data-tools-drawer style={{ position: "fixed", left: toolsAnchor!.left, width: toolsAnchor!.width,
                  bottom: toolsAnchor!.bottom, maxHeight: "58vh", overflowY: "auto", overflowX: "hidden", zIndex: 70,
                  border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel2, padding: "4px 14px 12px",
                  boxShadow: "0 14px 38px rgba(0,0,0,.55)" }}>
                  <div
                    onMouseDown={(e) => {
                      const tag = (e.target as HTMLElement).tagName;
                      if (tag !== "SELECT" && tag !== "INPUT" && tag !== "TEXTAREA") e.preventDefault();
                    }}>
                {/* 2026-11-25：重排成「分组 + 设置行」——左标题/说明、右控件、展开区挂在该行下面。
                    原来是一排同尺寸的小胶囊靠 CSS order 堆着，扫一眼分不出开关/下拉/可展开。 */}
                <ToolsSection title="回复">
                  <ToolsRow
                    title="长度"
                    desc="简单＝五到十行，动作神态择要、心理不省；详细＝四项充分描写"
                    right={
                      <select value={lenLevel} onChange={(e) => setLenLevel(e.target.value as "简单" | "详细")}
                        title="回复长度：简单＝五到十行；详细＝四项充分描写" style={selStyle}>
                        <option value="简单">简单 · 精简</option>
                        <option value="详细">详细 · 展开</option>
                      </select>
                    }
                  />
                  <ToolsRow
                    title="时段"
                    desc="只影响日结怎么判作息，不写进正文"
                    last
                    right={
                      <select value={tod} onChange={(e) => setTod(e.target.value as "早" | "午" | "晚")}
                        title="这一轮的时段（日结判作息用）：早 00:00–11:59 / 午 12:00–17:59 / 晚 18:00–23:59"
                        style={selStyle}>
                        <option value="早">早</option>
                        <option value="午">午</option>
                        <option value="晚">晚</option>
                      </select>
                    }
                  />
                </ToolsSection>

                <ToolsSection title="上下文与回忆">
                  <ToolsRow
                    title="记忆"
                    desc="生成时把关键记忆（含她的 + 无归属的）拼进本次请求"
                    right={
                      <button onClick={() => setMemoryCtx(!memoryCtx)} role="switch" aria-checked={memoryCtx}
                        title="把关键记忆拼进本次请求"
                        style={{ background: "none", border: "none", padding: 0, cursor: "pointer", display: "inline-flex" }}>
                        <span style={{ width: 34, height: 19, borderRadius: 10, background: memoryCtx ? "rgba(55,201,120,.65)" : T.line, position: "relative", display: "inline-block", transition: "background .2s" }}>
                          <span style={{ position: "absolute", top: 2, left: memoryCtx ? 17 : 2, width: 15, height: 15, borderRadius: "50%", background: memoryCtx ? "#1c1f26" : "#22262f", transition: "left .2s" }} />
                        </span>
                      </button>
                    }
                  />
                  <ToolsRow
                    title="回顾"
                    desc="指定某一天，把那天的原始对话拼进本次提示词（不依赖自动识别）"
                    last
                    right={
                      <button onClick={() => void openRecallPicker()} title="选一天，把那天聊过的原始对话拼进本次提示词"
                        style={{ ...rowBtn, display: "inline-flex", alignItems: "center", gap: 5, borderColor: recallPick.length ? T.accent : T.line, color: recallPick.length ? T.accent : T.text2, background: recallPick.length ? T.accentSoft : "transparent" }}>
                        {recallPick.length > 0 ? `已选 ${recallPick.length} 天` : <><Icon name="recall" size={12} /> 选择…</>}
                      </button>
                    }
                  >
                    {recallPick.length > 0 && (
                      <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                        {recallPick.map((id) => (
                          <span key={id} style={{ display: "inline-flex", alignItems: "center", gap: 5, padding: "2px 8px", borderRadius: 10, border: `1px solid ${T.accent}55`, background: T.accentSoft, color: T.accent, fontSize: 11 }}>
                            {recallDays.find((d) => d.node_id === id)?.date_label ?? `节点 #${id}`}
                            <button onClick={() => setRecallPick(recallPick.filter((x) => x !== id))} title="取消这一天"
                              style={{ background: "none", border: "none", color: T.accent, cursor: "pointer", fontSize: 11, padding: 0, fontFamily: "inherit" }}>✕</button>
                          </span>
                        ))}
                      </div>
                    )}
                  </ToolsRow>
                </ToolsSection>

                {navMode === "对话" && (
                  <ToolsSection title="导演">
                    <ToolsRow
                      title="导演指令"
                      desc={gate.blocked ? `已锁：${gate.reason}` : "强制剧情怎么走，只对下一次发送生效"}
                      dim={gate.blocked}
                      right={
                        <button
                          onClick={() => {
                            // 锁定态**仍然可点**：点了就把原因说出来，比一个点不动的灰按钮清楚
                            if (gate.blocked) {
                              const queued = directors.length > 0 ? `（已排队的 ${directors.length} 条不会随本次发送）` : "";
                              setMsg("🚫 " + gate.reason + queued);
                              return;
                            }
                            setShowDirectorInput(!showDirectorInput);
                          }}
                          title={gate.blocked ? gate.reason : "导演指令：强制剧情这样走（一次性，附加到本次发送）"}
                          style={{ ...rowBtn, cursor: gate.blocked ? "not-allowed" : "pointer",
                            borderColor: !gate.blocked && (directors.length > 0 || showDirectorInput) ? "rgba(255,122,24,.42)" : T.line,
                            color: gate.blocked ? T.text3 : directors.length > 0 || showDirectorInput ? T.accent : T.text2,
                            background: !gate.blocked && directors.length > 0 ? T.accentSoft : "transparent",
                            opacity: gate.blocked ? 0.7 : 1 }}>
                          {gate.blocked ? "🔒 已锁" : showDirectorInput ? "收起" : `展开${directors.length > 0 ? ` · ${directors.length} 条` : ""}`}
                        </button>
                      }
                    >
                      {showDirectorInput && !gate.blocked && (
                        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                          <div style={{ display: "flex", gap: 6 }}>
                            <input
                              value={directorDraft}
                              onChange={(e) => setDirectorDraft(e.target.value)}
                              onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); const v = directorDraft.trim(); if (v) { updateDirectors([...directors, v]); setDirectorDraft(""); } } }}
                              placeholder="例如：强制让夏婉琴接受告白（回车添加，发送后自动清空）"
                              style={{ flex: 1, padding: "7px 10px", borderRadius: 7, border: `1px solid rgba(255,122,24,.35)`, background: T.fill, color: T.text1, fontSize: 12, outline: "none", fontFamily: "inherit" }}
                            />
                            <button onClick={() => { const v = directorDraft.trim(); if (v) { updateDirectors([...directors, v]); setDirectorDraft(""); } }}
                              style={{ padding: "0 12px", border: "none", borderRadius: 7, background: T.accent, color: T.onAccent, fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>添加</button>
                          </div>
                          {directors.length > 0 && (
                            <div style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                              {directors.map((d, i) => (
                                <span key={i} title={d} style={{ display: "inline-flex", alignItems: "center", gap: 5, maxWidth: 320, padding: "3px 8px", borderRadius: 10, border: `1px solid rgba(255,122,24,.42)`, background: T.accentSoft, color: T.accent, fontSize: 11 }}>
                                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{d}</span>
                                  <button onClick={() => updateDirectors(directors.filter((_, j) => j !== i))} style={{ color: T.danger, background: "none", border: "none", cursor: "pointer", fontSize: 11, padding: 0, fontFamily: "inherit" }}>✕</button>
                                </span>
                              ))}
                              <span style={{ fontSize: 10.5, color: T.text3 }}>只在下次发送时生效，发完自动清空</span>
                            </div>
                          )}
                        </div>
                      )}
                    </ToolsRow>
                    <ToolsRow
                      title="导演账本"
                      desc={ledger ? `已用 ${ledger.total}/${ledger.limit} 分；累计到上限会锁住导演指令` : "每次导演指令都要结账；累计到上限会锁住导演指令"}
                      right={
                        <button onClick={() => setShowLedger((v) => !v)} title="导演账本：看每次指令的代价，可撤销 / 申请赦免"
                          style={{ ...rowBtn, borderColor: showLedger ? T.accent : T.line, color: showLedger ? T.accent : T.text2, background: showLedger ? T.accentSoft : "transparent" }}>
                          {showLedger ? "收起" : "展开"}
                        </button>
                      }
                    >
                      {showLedger && (
                        <DirectorLedgerView
                          ledger={ledger}
                          characterName={aiRole}
                          busyId={costBusyId}
                          onRequestPardon={(id) => { setPardonFor(id); setPardonExtra(""); }}
                          onApplyPardon={(id) => void applyPardon(id)}
                          onUndo={(id) => void undoCost(id)}
                          relationChanges={relChanges}
                          onUndoRelation={(id) => void undoRelationChange(id)}
                        />
                      )}
                    </ToolsRow>
                    <ToolsRow
                      title="剧情约束"
                      desc="硬性限制，比如「角色不会死亡」；每次生成都会带上"
                      last
                      right={
                        <button onClick={() => setShowConstraintInput(!showConstraintInput)}
                          style={{ ...rowBtn, borderColor: constraints.length > 0 || showConstraintInput ? T.ok : T.line, color: constraints.length > 0 || showConstraintInput ? T.ok : T.text2 }}>
                          {showConstraintInput ? "收起" : constraints.length > 0 ? `编辑 · ${constraints.length} 条` : "添加"}
                        </button>
                      }
                    >
                      {showConstraintInput && (
                        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                          <div style={{ display: "flex", gap: 6 }}>
                            <input
                              value={constraintDraft}
                              onChange={(e) => setConstraintDraft(e.target.value)}
                              onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); const v = constraintDraft.trim(); if (v) { updateConstraints([...constraints, v]); setConstraintDraft(""); } } }}
                              placeholder="添加剧情约束，如：角色不会死亡（回车添加）"
                              style={{ flex: 1, padding: "7px 10px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.fill, color: T.text1, fontSize: 12, outline: "none", fontFamily: "inherit" }}
                            />
                            <button onClick={() => { const v = constraintDraft.trim(); if (v) { updateConstraints([...constraints, v]); setConstraintDraft(""); } }}
                              style={{ padding: "0 12px", border: "none", borderRadius: 7, background: T.accent, color: T.onAccent, fontSize: 12, fontWeight: 600, cursor: "pointer", fontFamily: "inherit" }}>添加</button>
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
                    </ToolsRow>
                  </ToolsSection>
                )}

                <ToolsSection title="内置">
                  <ToolsRow
                    title="敏感词过滤"
                    desc="内置常开：输入与输出都会过滤（本软件不具备内容审查能力，内容合法性仍由你自行判断）"
                    last
                    right={<span style={{ fontSize: 11.5, color: T.ok }}>✓ 已内置</span>}
                  />
                </ToolsSection>

                {/* 赦免请求弹窗（2026-10-27）：确认 + 可补一句话；请求不进剧情历史 */}
                {pardonFor !== null && (
                  <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 60 }}>
                    <div style={{ width: 420, background: T.panel2, border: `1px solid ${T.line}`, borderRadius: 12, padding: 16, boxShadow: "0 18px 50px rgba(0,0,0,.5)" }}>
                      <div style={{ fontSize: 13, fontWeight: 600, color: T.text1, marginBottom: 6 }}>是否发送请求赦免？</div>
                      <div style={{ fontSize: 11.5, color: T.text3, lineHeight: 1.7, marginBottom: 8 }}>
                        这会把一条「请求她原谅」的盒子挂到那条 AI 回复下面（不进剧情历史）。她同意之后「赦免」才会变亮。
                      </div>
                      <textarea
                        value={pardonExtra}
                        onChange={(e) => setPardonExtra(e.target.value)}
                        placeholder="想补充的话（可留空）"
                        rows={3}
                        style={{ width: "100%", boxSizing: "border-box", padding: "8px 10px", borderRadius: 8, border: `1px solid ${T.line}`, background: T.fill, color: T.text1, fontSize: 12, resize: "vertical", outline: "none", fontFamily: "inherit" }}
                      />
                      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 10 }}>
                        <button onClick={() => { setPardonFor(null); setPardonExtra(""); }} disabled={pardonBusy}
                          style={{ height: 30, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, fontSize: 12, cursor: "pointer", fontFamily: "inherit" }}>取消</button>
                        <button onClick={() => void requestPardon()} disabled={pardonBusy}
                          style={{ height: 30, padding: "0 16px", borderRadius: 8, border: "none", background: T.accent, color: T.onAccent, fontSize: 12, fontWeight: 650, cursor: "pointer", fontFamily: "inherit" }}>{pardonBusy ? "发送中…" : "是，发送"}</button>
                      </div>
                    </div>
                  </div>
                )}
                  </div>
                </div>,
                document.body,
              )}

              <div ref={barRef} style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px 10px" }}>
                {/* 2026-11-07：上拉框开关（用户给的图二那个圆形 ＋）。开着时转成 ×，一眼看出还能收回去 */}
                <button ref={plusRef} data-tools-toggle onClick={() => setToolsOpen((v) => !v)}
                  title={toolsOpen ? "收起工具栏" : "展开工具栏（敏感词过滤 / 剧情约束 / 时段 / 导演指令 / 账本 / 长度 / 记忆 / 回顾）"}
                  style={{ width: 30, height: 30, flexShrink: 0, borderRadius: "50%", cursor: "pointer",
                    border: `1px solid ${toolsOpen ? T.accent : "rgba(200,168,106,.75)"}`,
                    background: toolsOpen ? T.accentSoft : "#131a2a",
                    color: toolsOpen ? T.accent : T.gold, fontSize: 16, lineHeight: 1, fontFamily: "inherit",
                    display: "inline-flex", alignItems: "center", justifyContent: "center",
                    transform: toolsOpen ? "rotate(45deg)" : "none", transition: "transform .18s" }}>
                  +
                </button>
                <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 10 }}>
                  {/* 2026-11-07：圆点 + 长条 → 一个**圆环**（直径跟原来的点一样 12px），文字只留百分比 */}
                  <span title={dayDebug ? `当前节点 ${node?.date_label ?? "—"} · 已用 ${dayBudget.usedTokens.toLocaleString("en-US")} / ${dayBudget.inputBudget.toLocaleString("en-US")}（上限）· ${dayBudget.pctUsed}%` : "本节点用量：还没有可统计的内容"}
                    style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
                    <svg width={12} height={12} viewBox="0 0 12 12" style={{ transform: "rotate(-90deg)", flexShrink: 0 }}>
                      <circle cx={6} cy={6} r={4.6} fill="none" stroke={T.inset} strokeWidth={2.4} />
                      <circle cx={6} cy={6} r={4.6} fill="none" strokeWidth={2.4} strokeLinecap="round"
                        stroke={dayDebug ? (dayBudget.pctUsed > 85 ? T.danger : dayBudget.pctUsed > 60 ? T.warn : T.ok) : T.line}
                        strokeDasharray={`${2 * Math.PI * 4.6}`}
                        strokeDashoffset={`${(1 - Math.min(100, dayDebug ? dayBudget.pctUsed : 0) / 100) * 2 * Math.PI * 4.6}`}
                        style={{ transition: "stroke-dashoffset .3s" }} />
                    </svg>
                    <span style={{ fontSize: 10.5, color: T.text3, display: "inline-block", minWidth: 30, textAlign: "right" }}>{dayDebug ? `${dayBudget.pctUsed}%` : "—"}</span>
                  </span>
                  <span style={{ fontSize: 10.5, color: T.text3, display: "inline-block", minWidth: 66, textAlign: "right" }}>{input.length} / 2000</span>
                  {/* 第五批（《现状说明》5.2）：生成中显示「停止」——
                      以前没有取消入口，上游不响应时界面永远转圈，只能杀进程。 */}
                  {genRequestId ? (
                    <button onClick={() => void stopGeneration()}
                      title="停止本次生成（已生成的部分会保留，并标记为未完成）"
                      style={{ width: 34, height: 34, padding: 0, border: `1px solid ${T.danger}`, borderRadius: "50%", cursor: "pointer", background: "transparent", color: T.danger, fontSize: 14, fontWeight: 650, fontFamily: "inherit", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>■</button>
                  ) : (
                    <button onClick={sendMessage} title="提交（Enter）" style={{ width: 34, height: 34, padding: 0, border: "none", borderRadius: "50%", cursor: "pointer", background: GRAD_ACCENT, color: T.onAccent, fontSize: 15, fontWeight: 650, fontFamily: "inherit", display: "inline-flex", alignItems: "center", justifyContent: "center", boxShadow: "0 4px 14px rgba(255,122,24,.26)" }}>➤</button>
                  )}
                </div>
              </div>
            </div>
          </div>
        )
  );
}
