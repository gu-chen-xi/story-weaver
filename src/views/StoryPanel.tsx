// 「StoryPanel」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import { useEffect, useRef, type Dispatch, type SetStateAction } from "react";
import { createPortal } from "react-dom";
import type { AppView } from "../types";
import { parseDirector, parseScriptBlocks, stripDirector, stripStanceMarker } from "../lib/parse";
import { T } from "../theme";
import { dayNoOf, pickGap } from "../lib/timelineDates";
import { DayStrip } from "./DayStrip";
import type { BranchDto, Character, Story, TimelineNodeDto } from "../types";
import { invoke } from "@tauri-apps/api/core";
import { useConfirm } from "../confirm";

export type ViewName = AppView;

export type StoryPanelProps = {
  branches: BranchDto[];
  cancelEdit: () => void;
  chars: Character[];
  currentBranchId: string | null;
  currentStory: Story | null;
  editText: string;
  editingId: number | null;
  loadBranches: (storyId: number) => Promise<void>;
  loadChars: (storyId: number) => Promise<void>;
  loadRelations: (storyId: number) => Promise<void>;
  loadTimelineAndMessages: (storyId: number, nodeId?: number, branchOverride?: string | null) => Promise<void>;
  loadWorlds: (storyId: number) => Promise<void>;
  messages: { id: number; role: string; content: string; mode: string; created_at: string }[];
  node: { id: number; date_label: string } | null;
  setBranchDialog: Dispatch<SetStateAction<{ open: boolean; name: string; forkNodeId: number | null; busy: boolean }>>;
  setBranchEdit: Dispatch<SetStateAction<{ id: string; name: string; note: string; busy: boolean } | null>>;
  setCurrentBranchId: Dispatch<SetStateAction<string | null>>;
  setEditText: Dispatch<SetStateAction<string>>;
  setEditingId: Dispatch<SetStateAction<number | null>>;
  setMsg: Dispatch<SetStateAction<string>>;
  switchBranch: (branchId: string | null) => Promise<void>;
  timelineNodes: TimelineNodeDto[];
  /// 头部要 portal 出去的宿主节点（App 提供，位于滚动容器外面）
  headerEl: HTMLDivElement | null;
  /// 空档（相邻两天之间的"无事件"区间）：`{ from: 前一天, to: 后一天 }`
  timeGaps: () => { from: number; to: number }[];
  /// 删除某一天（连带它的消息；内部有 confirm）
  removeNode: (nodeId: number, label: string) => Promise<void>;
  /// 打开「在这段里插入事件」弹窗
  setGapPrompt: Dispatch<SetStateAction<{ from: number; to: number; day: string; title: string } | null>>;
};

export function StoryPanel(props: StoryPanelProps) {
  const confirm = useConfirm();
  const { branches, cancelEdit, chars, currentBranchId, currentStory, editText, editingId, loadBranches, loadChars, loadRelations, loadTimelineAndMessages, loadWorlds, messages, node, setBranchDialog, setBranchEdit, setCurrentBranchId, setEditText, setEditingId, setMsg, switchBranch, timelineNodes , headerEl, timeGaps, setGapPrompt, removeNode,
} = props;
  // 「编辑」框按内容自适应高度（2026-10-26 用户要求"正常大小"）：
  // 短消息就一行高，长消息最多长到窗口 40%，再长就在框里滚 —— 不再是固定 80px 起步的大空块。
  const editRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    const el = editRef.current;
    if (!el) return;
    el.style.height = "auto";
    const cap = Math.round(window.innerHeight * 0.4);
    el.style.height = Math.min(el.scrollHeight, cap) + "px";
  }, [editText, editingId]);

  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const removeBranch = async (b: BranchDto) => {
    if (!currentStory) return;
    if (!(await confirm({
      title: "删除分支",
      body: `删除分支「${b.name}」？\n\n· 会删掉这条分支自己的 ${b.node_count ?? 0} 个节点及其对话\n· 分叉点及之前的共享历史不受影响\n· 主线不受影响\n\n此操作不可撤销。`,
      danger: true,
    }))) return;
    try {
      const msg = await invoke<string>("delete_branch", {
        storyId: String(currentStory.id), branchId: b.id,
      });
      setMsg("🗑 " + msg);
      // 若删的正是当前分支 → 后端已把记录归主线，这里同步界面
      if (currentBranchId === b.id) setCurrentBranchId(null);
      await loadBranches(currentStory.id);
      await loadChars(currentStory.id);
      await loadWorlds(currentStory.id);
      await loadRelations(currentStory.id);
      await loadTimelineAndMessages(currentStory.id, undefined, currentBranchId === b.id ? null : currentBranchId);
    } catch (e) {
      setMsg("❌ 删除分支失败：" + String(e));
    }
  };
  const openBranchDialog = (forkNodeId?: number) => {
    setBranchDialog({ open: true, name: "", forkNodeId: forkNodeId ?? node?.id ?? null, busy: false });
  };
  const makeDaySummary = async (force = false) => {
    if (!currentStory || !node) return;
    try {
      setMsg("📝 正在生成日摘要…");
      const s = await invoke<string | null>("generate_day_summary", {
        storyId: String(currentStory.id), nodeId: node.id, force,
      });
      if (s) {
        setMsg("📝 日摘要已生成");
        await loadTimelineAndMessages(currentStory.id, node.id);
      } else {
        setMsg("ℹ 这天已有摘要（生成一次即固化，不再回改）");
      }
    } catch (e) {
      setMsg("❌ 日摘要失败: " + String(e));
    }
  };
  const addNextDay = async () => {
    if (!currentStory) return;
    try {
      const n = await invoke<{ id: number; date_label: string }>("create_next_day_node", {
        storyId: String(currentStory.id),
        // 两个都传：**branchId 是准的**（用户当前所在分支），afterNodeId 只是"父节点提示"。
        //
        // 后端规则（2026-10-17 修）：只有 afterNodeId **确实挂在某条分支上**时才用它推导分支；
        // 若它是**共享历史**（branch_id IS NULL，例如刚建好的分支，其"最后一个节点"就是分叉点），
        // 一律采用 branchId —— 否则新分会建到主线里（用户实测：建了分支，点「下一天」主线多一天）。
        afterNodeId: node?.id ?? null,
        branchId: currentBranchId,
      });
      setMsg(`✅ 已创建 ${n.date_label}`);
      // 新节点一定属于当前分支，按它切过去（loadTimelineAndMessages 会校验归属）
      await loadTimelineAndMessages(currentStory.id, n.id, currentBranchId);
    } catch (e) {
      setMsg("❌ 创建下一天失败: " + String(e));
    }
  };
  /// 删除一组「我 + 紧随其后的 AI 回复」（2026-10-18 用户定的删除单位）。
  ///
  /// 为什么按一组删：单删用户消息会留下"没有提问的回复"，单删 AI 回复会留下"没人回答的提问"，
  /// 两种都会让历史错乱。所以以**一轮对话**为单位。
  ///
  /// 边界：用户消息后面可能跟多条 AI 回复（重生成/中断续写）→ 一并删；
  ///       后面没有 AI 回复（请求失败被撤销过）→ 只删这一条；永远不动下一轮。
  const deletePair = async (m: { id: number; role: string }) => {
    if (!currentStory || !node) return;
    const idx = messages.findIndex((x) => x.id === m.id);
    if (idx < 0) return;
    const ids: number[] = [m.id];
    for (let i = idx + 1; i < messages.length; i++) {
      if (messages[i].role === "user") break;   // 到了下一轮 → 停
      ids.push(messages[i].id);
    }
    const aiCount = ids.length - 1;
    const tip = aiCount > 0
      ? `删除这一组对话？\n\n· 你的这条消息 + 紧随其后的 ${aiCount} 条 AI 回复\n· 不可撤销（后面的对话不受影响）`
      : `删除这条消息？\n\n· 它后面没有 AI 回复\n· 不可撤销`;
    if (!(await confirm({ title: "删除消息", body: tip, danger: true }))) return;
    try {
      for (const id of ids) {
        await invoke("delete_message", { storyId: String(currentStory.id), messageId: id });
      }
      await loadTimelineAndMessages(currentStory.id, node.id);
      setMsg(aiCount > 0 ? `🗑 已删除 1 组对话（${ids.length} 条）` : "🗑 已删除 1 条消息");
    } catch (e) {
      setMsg("❌ 删除失败：" + String(e));
    }
  };


  const startEdit = (m: { id: number; content: string }) => {
    setEditingId(m.id);
    setEditText(stripStanceMarker(m.content));
  };
  const saveEdit = async () => {
    if (!currentStory || editingId == null) return;
    try {
      await invoke("update_message", {
        storyId: String(currentStory.id),
        messageId: editingId,
        content: editText,
      });
      setMsg("✅ 已修改历史剧情（后续续写将基于修改后的内容）");
      cancelEdit();
      if (node) await loadTimelineAndMessages(currentStory.id, node.id);
    } catch (e) {
      setMsg("❌ 修改失败: " + String(e));
    }
  };
  const addInterlude = async () => {
    if (!currentStory) return;
    try {
      const n = await invoke<{ id: number; date_label: string }>("create_interlude_node", {
        storyId: String(currentStory.id), branchId: currentBranchId,
      });
      setMsg(`🎭 已创建插曲：${n.date_label}`);
      await loadTimelineAndMessages(currentStory.id, n.id, currentBranchId);
    } catch (e) {
      setMsg("❌ 创建插曲失败: " + String(e));
    }
  };
  /**
   * 「时间流逝」里插入事件（v3.5.7 补回入口）。
   *
   * 2026-10-24 撤「天数排」时，能点的那条"⏳ 时间流逝 · 第 N~M 天"分隔被一起删了 ——
   * 逻辑（`timeGaps` / `insertEventAtDay` / 弹窗 / 后端 `create_node_at_day`）全在，
   * 只是**没有入口**。这里把它放回「+ 下一天」旁边。
   *
   * 选哪段空档：
   *   1. 当前这天**落在某段空档里** → 就那段（最符合"我在第 4 天，想补第 4 天的事"）；
   *   2. 否则取**当前天之后的第一段**；
   *   3. 都没有 → 提示"没有空白期"（时间连续时不该给用户一个打不开的弹窗）。
   */
  const openGapInsert = () => {
    const gaps = timeGaps();
    if (gaps.length === 0) {
      setMsg("⚠ 这条线的时间是连续的，没有空白期 —— 直接用「+ 下一天」加新的一天");
      return;
    }
    const pick = pickGap(gaps, node ? dayNoOf(node) : 0);
    if (!pick) return;
    setGapPrompt({ from: pick.from + 1, to: pick.to - 1, day: String(pick.from + 1), title: "" });
  };

  const storyHeader = (
        <>
          {/* v3.5.4：**固定头部**（用户 2026-10-24 要求）——
              第一排：分支条 + 下一天 / 插入插曲；第二排：这条线的天数药丸（可左右滑动）。
              它固定在对话区上方；下面的日摘要与消息照常跟着页面滚动。 */}
          <div
            style={{
              background: T.panel,
              paddingTop: 8,
              paddingBottom: 8,
              marginBottom: 10,
              borderBottom: `1px solid ${T.lineSoft}`,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap", marginBottom: 6 }}>
                {branches.length > 0 && <span style={{ fontSize: 11, color: T.text3, marginRight: 2 }}>分支：</span>}
                {/* 主线永远是第一项（只有主线时不显示它，避免"只有一个选项"的噪音） */}
                {(branches.length > 0 ? [{ id: null as string | null, name: "主线", color: "#6B7280" }, ...branches] : []).map((b) => {
                  const active = currentBranchId === b.id;
                  return (
                    <button key={b.id ?? "main"} onClick={() => void switchBranch(b.id)}
                      title={b.id === null
                        ? "主线（branch_id IS NULL）"
                        : `${(b as BranchDto).note ? (b as BranchDto).note + "\n" : ""}分叉节点 id=${(b as BranchDto).fork_node_id} · 该分支 ${(b as BranchDto).node_count ?? 0} 个节点`}
                      style={{
                        display: "inline-flex", alignItems: "center", gap: 6, fontSize: 12,
                        padding: "3px 10px", borderRadius: 12, cursor: "pointer", fontFamily: "inherit",
                        border: `1px solid ${active ? b.color : T.line}`,
                        background: active ? "rgba(255,255,255,.06)" : "transparent",
                        color: active ? T.text1 : T.text2,
                        fontWeight: active ? 650 : 400,
                        borderBottom: active ? `2px solid ${b.color}` : `1px solid ${active ? b.color : T.line}`,
                      }}>
                      <span style={{ width: 8, height: 8, borderRadius: 4, background: b.color, flexShrink: 0 }} />
                      {b.name}{b.id !== null && (b as BranchDto).node_count ? <span style={{ color: T.text3, fontSize: 10 }}>({(b as BranchDto).node_count})</span> : null}
                      {/* 改名 / 备注（第五批 ④）—— 同样不要触发"切换分支" */}
                      {b.id !== null && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            const bd = b as BranchDto;
                            setBranchEdit({ id: bd.id, name: bd.name, note: bd.note ?? "", busy: false });
                          }}
                          title={`改名 / 写备注「${b.name}」`}
                          style={{ marginLeft: 2, padding: "0 5px", borderRadius: 6, border: "none",
                            background: "transparent", color: T.text3, fontSize: 12, lineHeight: 1,
                            cursor: "pointer", fontFamily: "inherit" }}>
                          ✏
                        </button>
                      )}
                      {/* 删除分支（主线不可删）—— 点它不要触发"切换分支" */}
                      {b.id !== null && (
                        <button
                          onClick={(e) => { e.stopPropagation(); void removeBranch(b as BranchDto); }}
                          title={`删除分支「${b.name}」（只删它自己的节点，共享历史与主线不受影响）`}
                          style={{ marginLeft: 2, padding: "0 5px", borderRadius: 6, border: "none",
                            background: "transparent", color: T.text3, fontSize: 12, lineHeight: 1,
                            cursor: "pointer", fontFamily: "inherit" }}>
                          🗑
                        </button>
                      )}
                    </button>
                  );
                })}
                <button onClick={() => openBranchDialog()}
                  title="从某个节点分叉出一条新的故事线（会复制当前世界状态）"
                  style={{ marginLeft: branches.length > 0 ? 4 : 0, fontSize: 12, padding: "3px 12px", borderRadius: 12,
                    cursor: "pointer", fontFamily: "inherit", border: `1px dashed ${T.accent}`,
                    background: "transparent", color: T.accent }}>
                  ＋ 新建分支
                </button>
              <div style={{ flex: 1 }} />
              <button onClick={addNextDay} style={{ padding: "3px 12px", fontSize: 12, borderRadius: 14, border: `1px dashed ${T.text3}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit" }}>+ 下一天</button>
              <button
                onClick={openGapInsert}
                title="在这条线没有剧情的空白期里新建一天（比如第 4~6 天没写，就在中间补一天）"
                style={{ padding: "3px 12px", fontSize: 12, borderRadius: 14, border: `1px dashed ${T.text3}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit" }}
              >
                ⏳ 插入事件
              </button>
              <button onClick={addInterlude} style={{ padding: "3px 12px", fontSize: 12, borderRadius: 14, border: `1px dashed ${T.danger}`, background: "transparent", color: "#c98a80", cursor: "pointer", fontFamily: "inherit" }}>🎭 插入插曲</button>
              {/* v3.5.7：删除**当前这一天**（旧「天数排」每颗药丸上的 × 撤掉后，
                  删除入口就没了）。放这里是因为：目标唯一、位置固定、不会误点；
                  想删任意一天 → 右键药丸。 */}
              {node && (
                <button
                  onClick={() => void removeNode(node.id, node.date_label)}
                  title={`删除「${node.date_label}」及其全部消息（删中间一天时，后面的天会自动接回去）`}
                  style={{ padding: "3px 12px", fontSize: 12, borderRadius: 14, border: `1px dashed ${T.danger}`,
                    background: "transparent", color: T.danger, cursor: "pointer", fontFamily: "inherit" }}
                >
                  🗑 删除第 {dayNoOf(node)} 天
                </button>
              )}
            </div>
            <DayStrip
              nodes={timelineNodes}
              branches={branches}
              currentBranchId={currentBranchId}
              currentNodeId={node?.id ?? null}
              onPick={(id) => { if (currentStory) void loadTimelineAndMessages(currentStory.id, id); }}
              // 右键药丸 → 删那一天（旧「天数排」的 × 的能力，收进右键以免误点）
              onDelete={(id, label) => void removeNode(id, label)}
            />
          </div>
        </>
  );

  return (
      (
        <>
          {/* v3.5.5：头部内容改为 portal 到滚动容器外面（见 App 的 storyHeaderSlot） */}
          {headerEl ? createPortal(storyHeader, headerEl) : null}

          <div style={{ display: "flex", flexDirection: "column", gap: 18, minHeight: 200 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, color: T.text3, fontSize: 11 }}>
              <span style={{ flex: 1, height: 1, background: T.lineSoft }} />
              {node ? node.date_label : "—"} · {messages.length} 条消息
              <span style={{ flex: 1, height: 1, background: T.lineSoft }} />
            </div>
            {/* D 子项：当前这天的日摘要（生成一次即固化；没有就提示可生成） */}
            {(() => {
              const cur = timelineNodes.find((n) => n.id === node?.id);
              if (!cur) return null;
              return (
                <div style={{ border: `1px ${cur.day_summary ? "solid" : "dashed"} ${cur.day_summary ? "rgba(55,201,120,.35)" : T.lineSoft}`, borderRadius: 10, background: cur.day_summary ? "rgba(55,201,120,.05)" : "transparent", padding: "10px 14px" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: cur.day_summary ? T.ok : T.text3, marginBottom: cur.day_summary ? 6 : 0, flexWrap: "wrap" }}>
                    <b>📝 日摘要（这天发生了什么）</b>
                    {cur.day_summary
                      ? <span style={{ color: T.text3, fontWeight: 400 }}>已固化，不会自动回改；点右侧可覆盖</span>
                      : <span style={{ fontWeight: 400 }}>还没生成 —— 翻篇后会自动补；跨天回读时更早的日子只带摘要</span>}
                    <button
                      onClick={async () => {
                        if (cur.day_summary) {
                          const ok = await confirm({
                            title: "覆盖日摘要",
                            body: "这天已有日摘要，重新生成会覆盖它。\n\n摘要一旦生成就不再回改，覆盖后与历史对话的一致性由你把握。",
                            okText: "覆盖",
                            danger: true,
                          });
                          if (!ok) return;
                          makeDaySummary(true);
                        } else makeDaySummary(false);
                      }}
                      style={{ marginLeft: "auto", padding: "2px 10px", fontSize: 11, borderRadius: 7, border: `1px solid ${T.ok}`, background: "rgba(55,201,120,.08)", color: T.ok, cursor: "pointer" }}
                    >{cur.day_summary ? "重新生成" : "📝 生成日摘要"}</button>
                  </div>
                  {cur.day_summary && <div style={{ fontSize: 12, color: T.text2, lineHeight: 1.75 }}>{cur.day_summary}</div>}
                </div>
              );
            })()}
            {messages.length === 0 && (
              <p style={{ color: T.text3, fontSize: 13 }}>这一天还没有剧情。在下面输入开始。</p>
            )}
            {messages.map((m) => (
              <div key={m.id} style={{ display: "flex", gap: 10, maxWidth: "78%", alignSelf: m.role === "user" ? "flex-end" : "flex-start", flexDirection: m.role === "user" ? "row-reverse" : "row" }}>
                <div style={{ width: 30, height: 30, flex: "0 0 30px", borderRadius: 9, marginTop: 2, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 11, fontWeight: 600, background: m.role === "user" ? "linear-gradient(135deg,#5b8cff,#8f6bff)" : "linear-gradient(135deg,#FF7A18,#ffc078)", color: m.role === "user" ? "#fff" : "#231505" }}>
                  {m.role === "user" ? "我" : "✦"}
                </div>
                <div style={{ minWidth: 0 }}>
                  <div style={{ fontSize: 11, color: T.text3, marginBottom: 5, display: "flex", gap: 8, alignItems: "center", justifyContent: m.role === "user" ? "flex-end" : "flex-start" }}>
                    <span style={{ color: T.text2, fontWeight: 600 }}>{m.role === "user" ? "我" : "导演模式"}</span>
                    <span>{m.created_at}</span>
                    {editingId !== m.id && (
                      <button onClick={() => startEdit(m)} style={{ fontSize: 11, background: "none", border: "none", color: T.text3, cursor: "pointer", padding: 0 }}>✏️ 编辑</button>
                    )}
                      {/* 删除：只在「我」的消息上给入口，按"我 + 紧随的 AI 回复"成组删 */}
                      {m.role === "user" && editingId !== m.id && (
                        <button onClick={() => void deletePair(m)} title="删除这一组（我的消息 + 后面的 AI 回复）"
                          style={{ fontSize: 11, background: "none", border: "none", color: T.text3, cursor: "pointer", padding: 0 }}>🗑 删除这组</button>
                      )}
                  </div>
                  {editingId === m.id ? (
                    <div>
                      <textarea value={editText} onChange={(e) => setEditText(e.target.value)} ref={editRef} rows={1} style={{ width: "100%", boxSizing: "border-box", minHeight: 34, maxHeight: "40vh", padding: "6px 8px", fontSize: 13, lineHeight: 1.6, fontFamily: "inherit", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, resize: "none", overflowY: "auto", outline: "none" }} />
                      <div style={{ marginTop: 6, display: "flex", gap: 8 }}>
                        <button onClick={saveEdit} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, borderRadius: 6 }}>保存</button>
                        <button onClick={cancelEdit} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, borderRadius: 6 }}>取消</button>
                      </div>
                    </div>
                  ) : (
                    <>
                      {/* 导演指令：独立小条显示在气泡上方，正文里不重复出现 */}
                      {m.role === "user" && parseDirector(m.content) && (
                        <div style={{ marginBottom: 5, padding: "5px 11px", borderRadius: 9, border: `1px dashed rgba(255,122,24,.45)`, background: T.accentSoft, color: T.accent, fontSize: 11, lineHeight: 1.6, whiteSpace: "pre-wrap", textAlign: "left" }}>
                          🎬 导演指令：{parseDirector(m.content)}
                        </div>
                      )}
                      {(m.role !== "user" || stripDirector(m.content)) && (
                    <div style={{ padding: "11px 14px", borderRadius: 14, fontSize: 13.5, lineHeight: 1.75, wordBreak: "break-word", position: "relative", background: m.role === "user" ? "linear-gradient(135deg,#3573ee,#2b5fd0)" : T.aiBubble, border: m.role === "user" ? "none" : `1px solid ${T.lineSoft}`, borderTopLeftRadius: m.role === "user" ? 14 : 5, borderTopRightRadius: m.role === "user" ? 5 : 14, color: m.role === "user" ? "#fff" : "#dfe3ec", boxShadow: m.role === "user" ? "0 4px 14px rgba(47,111,235,.24)" : "none" }}>
                      {m.role === "user" ? (
                      <span style={{ whiteSpace: "pre-wrap" }}>{stripDirector(stripStanceMarker(m.content))}</span>
                    ) : (
                      <div>
                        {parseScriptBlocks(stripStanceMarker(m.content), chars.map((cc) => cc.name)).map((b, bi) =>
                          b.type === "speaker" ? (
                            <div key={bi} style={{ fontSize: 12, fontWeight: 700, color: T.accent, margin: "10px 0 2px", letterSpacing: 1 }}>{b.text}</div>
                          ) : b.type === "narr" ? (
                            <div key={bi} style={{ color: "#9aa3b2", lineHeight: 1.8, marginBottom: 8 }}>{b.text}</div>
                          ) : b.type === "action" ? (
                            <div key={bi} style={{ color: "#8a91a1", fontStyle: "italic", lineHeight: 1.8, marginBottom: 8 }}>{b.text}</div>
                          ) : b.type === "inner" ? (
                            /* 沉浸式心声（角色内心原声）：斜体 + 暖玫瑰色 + 左侧竖线，与旁白/动作区分 */
                            <div key={bi} style={{ color: "#d9a7bd", fontStyle: "italic", lineHeight: 1.8, marginBottom: 8, paddingLeft: 10, borderLeft: "2px solid rgba(217,167,189,.45)" }}>{b.text}</div>
                          ) : (
                            <div key={bi} style={{ marginBottom: 8, lineHeight: 1.8 }}>
                              <span style={{ display: "inline-block", fontSize: 11, fontWeight: 600, color: T.accent, background: T.accentSoft, padding: "1px 8px", borderRadius: 9, marginRight: 8, verticalAlign: 1 }}>{b.name}</span>
                              <span style={{ color: "#dfe3ec" }}>{b.text}</span>
                            </div>
                          )
                        )}
                      </div>
                    )}
                      {m.id < 0 && (
                        <span style={{ display: "inline-block", width: 7, height: 15, background: T.accent, verticalAlign: -2, marginLeft: 2, animation: "dsh-blink 1s steps(2) infinite", borderRadius: 1 }} />
                      )}
                    </div>
                      )}
                    </>
                  )}
                </div>
              </div>
            ))}
          </div>

        </>
      )
  );
}
