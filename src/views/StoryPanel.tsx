// 「StoryPanel」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import { type Dispatch, type SetStateAction } from "react";
import type { AppView } from "../types";
import { parseDirector, parseScriptBlocks, stripDirector } from "../lib/parse";
import { T } from "../theme";
import type { BranchDto, Character, Story, TimelineNodeDto } from "../types";
import { invoke } from "@tauri-apps/api/core";

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
};

export function StoryPanel(props: StoryPanelProps) {
  const { branches, cancelEdit, chars, currentBranchId, currentStory, editText, editingId, loadBranches, loadChars, loadRelations, loadTimelineAndMessages, loadWorlds, messages, node, setBranchDialog, setBranchEdit, setCurrentBranchId, setEditText, setEditingId, setMsg, switchBranch, timelineNodes } = props;
  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const removeBranch = async (b: BranchDto) => {
    if (!currentStory) return;
    if (!confirm(`删除分支「${b.name}」？\n\n· 会删掉这条分支自己的 ${b.node_count ?? 0} 个节点及其对话\n· 分叉点及之前的共享历史**不受影响**\n· 主线不受影响\n\n此操作不可撤销。`)) return;
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
    if (!confirm(tip)) return;
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
    setEditText(m.content);
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
  return (
      (
        <>
          {/* 第五批 ③ 第 5 步：顶部分支切换条。
              - 只有主线时整条隐藏（没有分支就不打扰界面）
              - 当前项加粗 + 下划线；圆点用分支自己的颜色（主线用中性灰）
              - 「＋ 新建分支」是主入口（另一入口是时间维节点右键，第 6 步做） */}
          {/* 第五批 ③ 第 5 步：**始终显示**（用户 2026-10-16 定）——
              若"有分支才显示"，第一条分支就没有入口了（先有鸡还是先有蛋）。
              没有分支时只显示「＋ 新建分支」一个按钮。 */}
          {true && (
            <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap", marginBottom: 12,
              padding: branches.length > 0 ? "6px 10px" : "0",
              border: branches.length > 0 ? `1px solid ${T.line}` : "none",
              borderRadius: 10, background: branches.length > 0 ? T.panel2 : "transparent" }}>
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
            </div>
          )}

          {/* 第五批 ①：这里原来还有一条「时间线」横条，与下面的「时间维」显示同一批节点（重复）。
              已合并：只保留下面那一条 —— 它同时承担 ① 显示时间维（含空白期）② 切换当前节点 ③ 删除节点。
              分叉后（第 ③ 步分支系统）由树控件在**同一条**上做缩进与折叠，不另起一条。 */}
          {/* 时间维操作区：与"时间维（点「时间流逝」…）"那行合并为一条 —— 操作与它作用的对象放在一起 */}
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 8 }}>
            <span style={{ fontSize: 11, color: T.text3 }}>时间维（点「时间流逝」可在空白期插入事件）</span>
            <button onClick={addNextDay} style={{ padding: "3px 12px", fontSize: 12, borderRadius: 14, border: `1px dashed ${T.text3}`, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit" }}>+ 下一天</button>
            <button onClick={addInterlude} style={{ padding: "3px 12px", fontSize: 12, borderRadius: 14, border: `1px dashed ${T.danger}`, background: "transparent", color: "#c98a80", cursor: "pointer", fontFamily: "inherit" }}>🎭 插入插曲</button>
          </div>
          {/* v3.5.2：时间维下面那排节点胶囊先撤掉，等树状图稳定后重做 */}

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
                      onClick={() => { if (cur.day_summary) { if (confirm("这天已有日摘要，重新生成会覆盖它。\n（摘要一旦生成就不再回改，覆盖后与历史对话的一致性由你把握）\n\n确定覆盖？")) makeDaySummary(true); } else makeDaySummary(false); }}
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
                      <textarea value={editText} onChange={(e) => setEditText(e.target.value)} style={{ width: "100%", minHeight: 80, padding: 6, fontSize: 13, fontFamily: "inherit", borderRadius: 4, border: `1px solid ${T.line}`, background: T.panel2, color: T.text1 }} />
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
                      <span style={{ whiteSpace: "pre-wrap" }}>{stripDirector(m.content)}</span>
                    ) : (
                      <div>
                        {parseScriptBlocks(m.content, chars.map((cc) => cc.name)).map((b, bi) =>
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
