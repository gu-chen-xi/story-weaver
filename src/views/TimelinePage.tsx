// 「📅 时间线」页（列表 / 日历 / 树状图三种视图 + 区间条 + 分支切换）。
//
// 拆分说明（2026-10-17）：从 App.tsx 整体下沉。**本组件不持有状态** ——
// 所有 state 仍由 App 持有，通过 props 传入；因此行为与拆分前完全一致。
// 依赖 24 项（8 个 state + 4 个 setter + 12 个辅助），props 类型逐一手写，
// 由 tsc 保证与 App 侧一致（漏传/类型错了会直接编译失败）。

import { useEffect, useState, type Dispatch, type ReactElement, type ReactNode, type SetStateAction } from "react";
import type { AppView } from "../types";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { KeyMemories } from "./KeyMemories";
import { TimelineTree } from "./TimelineTree";
import type { BranchDto, Story, TimelineNodeDto } from "../types";

/// 顶部导航的视图名（与 App 的 `view` state 保持一致）
export type ViewName = AppView;

export type TimelinePageProps = {
  branchColor: (bid: string | null | undefined) => string;
  branchLabelOf: (bid: string | null | undefined) => string;
  branchScopedNodes: TimelineNodeDto[];
  dayNodesSorted: () => { node: TimelineNodeDto; dayNo: number }[];
  nodeDateDisplay: (n: { node_type: string; date_label: string }) => string;
  removeNode: (nodeId: number, label: string) => Promise<void>;
  renderTimelineTrack: (opts?: { compact?: boolean }) => ReactNode;
  switchNode: (nodeId: number) => Promise<void>;
  timeGaps: () => { from: number; to: number }[];
  treeCollapsedRef: { current: Set<number> };
  /// **折叠感知**的行（`treeRows`）：收起某节点后，它的后代不会出现在这里
  treeRows: { n: TimelineNodeDto; depth: number; hasKids: boolean }[];
  forceTreeRedraw: Dispatch<SetStateAction<number>>;
  setCalOffset: Dispatch<SetStateAction<number>>;
  setGapPrompt: Dispatch<SetStateAction<{ from: number; to: number; day: string; title: string } | null>>;
  setTimelineView: Dispatch<SetStateAction<"list" | "cal" | "tree">>;
  setView: Dispatch<SetStateAction<ViewName>>;
  setMsg: Dispatch<SetStateAction<string>>;
  branches: BranchDto[];
  calOffset: number;
  currentBranchId: string | null;
  currentStory: Story | null;
  node: { id: number; date_label: string } | null;
  timelineNodes: TimelineNodeDto[];
  timelineView: "list" | "cal" | "tree";
};

export function TimelinePage(props: TimelinePageProps) {
  const { setMsg, branchScopedNodes, dayNodesSorted, nodeDateDisplay, removeNode, renderTimelineTrack, switchNode, timeGaps, setCalOffset, setGapPrompt, setTimelineView, setView, branches, calOffset, currentBranchId, currentStory, node, timelineNodes, timelineView } = props;
  // ⑥ 阶段摘要：攒够 x 天才压缩（x 可调；先手动，之后接自动）
  const [phaseMinDays, setPhaseMinDays] = useState(10);
  const [phaseBusy, setPhaseBusy] = useState(false);
  const [phaseInfo, setPhaseInfo] = useState("");

  // 进页面时读一次当前配置的 x
  useEffect(() => {
    void invoke<number>("get_phase_min_days").then((n) => setPhaseMinDays(n)).catch(() => {});
  }, []);

  const runPhaseSummary = async () => {
    if (!currentStory) { setMsg("⚠ 请先进入一个故事"); return; }
    setPhaseBusy(true);
    setPhaseInfo("");
    try {
      const made = await invoke<{ day_from: number; day_to: number; source_days: number; content: string } | null>(
        "run_phase_summary",
        { storyId: String(currentStory.id), minDays: phaseMinDays, saveAsDefault: true },
      );
      if (!made) {
        setPhaseInfo(`攒不够 ${phaseMinDays} 天有日摘要的天 —— 先补齐这些天的日摘要再压`);
      } else {
        setPhaseInfo(`✅ 已把第 ${made.day_from}~${made.day_to} 天（${made.source_days} 天）压成 ${made.content.length} 字` +
          `（原文约 ${made.source_days * 130} 字）`);
        setMsg(`🧾 阶段摘要已更新：第 ${made.day_from}~${made.day_to} 天`);
      }
    } catch (e) {
      setPhaseInfo("❌ 压缩失败：" + String(e));
    } finally {
      setPhaseBusy(false);
    }
  };

  return (
        <div>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
            <h3 style={{ margin: 0, fontSize: 15 }}>📅 时间线</h3>
            <span style={{ fontSize: 12, color: "#8a91a1" }}>点击节点可进入对应日期剧情</span>
            <div style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
              {/* ⑥ 阶段摘要：攒够 x 天再压缩（x 可调） */}
              <span style={{ display: "inline-flex", alignItems: "center", gap: 4, fontSize: 11.5, color: T.text3 }}
                title="每段阶段摘要覆盖多少天（按“有日摘要的天”从最老开始数，跨过时间流逝的空档）；压到哪儿，下次从那儿之后接着算">
                🧾 攒够
                <input type="number" min={2} max={200} value={phaseMinDays}
                  onChange={(e) => setPhaseMinDays(Math.max(2, Math.min(200, Number(e.target.value) || 10)))}
                  style={{ width: 46, padding: "3px 5px", borderRadius: 6, border: `1px solid ${T.line}`,
                    background: "#191c22", color: T.text1, fontSize: 11.5, fontFamily: "monospace", textAlign: "center" }} />
                天
                <button onClick={() => void runPhaseSummary()} disabled={phaseBusy}
                  title="手动压一段（会重建：忽略已有阶段摘要、按新的 x 重新规划）；同一个 x 会被记住，后续自动压缩也用这个值"
                  style={{ fontSize: 11.5, padding: "3px 10px", borderRadius: 6, border: `1px solid ${T.line}`,
                    background: phaseBusy ? T.panel3 : "#191c22", color: phaseBusy ? T.text3 : T.text1,
                    cursor: phaseBusy ? "wait" : "pointer", fontFamily: "inherit" }}>
                  {phaseBusy ? "压缩中…" : "压一段摘要"}
                </button>
              </span>
              <button onClick={() => setTimelineView("list")} style={{ fontSize: 12, padding: "4px 12px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: timelineView === "list" ? T.accentSoft : "#191c22", color: timelineView === "list" ? T.accent : T.text2, fontFamily: "inherit" }}>列表</button>
              <button onClick={() => setTimelineView("cal")} style={{ fontSize: 12, padding: "4px 12px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: timelineView === "cal" ? T.accentSoft : "#191c22", color: timelineView === "cal" ? T.accent : T.text2, fontFamily: "inherit" }}>日历</button>
              {/* 第五批 ③：**调试用**的时间线树状图 —— 从上往下按父子关系打印整棵树，
                  用来直观看清"节点到底挂在哪条线上、父是谁、分叉在哪" */}
              <button onClick={() => setTimelineView("tree")} title="按父子关系从上往下显示整棵树（排查分支/父链问题用）"
                style={{ fontSize: 12, padding: "4px 12px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: timelineView === "tree" ? T.accentSoft : "#191c22", color: timelineView === "tree" ? T.accent : T.text2, fontFamily: "inherit" }}>🌲 时间线树状图</button>
            </div>
          </div>
          {phaseInfo && (
            <div style={{ fontSize: 11.5, color: phaseInfo.startsWith("❌") ? T.danger : T.text2, marginBottom: 8 }}>
              {phaseInfo}
            </div>
          )}
          {timelineNodes.length === 0 && <p style={{ color: "#6f7686" }}>暂无节点，先在剧情页创建。</p>}
          {/* 第五批 ③：时间线树状图（调试视图）——
              每行一个节点，前缀用缩进 + └─ 表示层级；带上分支名与颜色、父节点 id。
              目的：一眼看出"节点挂在哪条线上"（节点带锁 / 天数错乱这类问题靠它定位最快）。 */}
          {timelineView === "tree" && (
            // v3.5.1：段框树（替换原来的列表式树状图）
            // 依据《时间线可视化前端实现建议》：框尺寸渲染前算好、展开集合内聚在组件内、
            // 不订阅流式事件；出问题有错误边界 +「列表」视图兜底。
            <TimelineTree
              nodes={timelineNodes}
              branches={branches}
              currentBranchId={currentBranchId}
              onJump={(id) => { switchNode(id); setView("story"); }}
            />
          )}
          {timelineView === "list" && (() => {
            // 按时间线顺序，在相邻天数节点之间内联标出"时间流逝"空档（不建节点，只是标出来）
            // 只列当前分支能看到的节点（共享历史 + 本分支）；别的分支去树状图看
            const ordered = [...branchScopedNodes].sort((a, b) => (a.sort_order - b.sort_order) || (a.id - b.id));
            const rows: ReactElement[] = [];
            const dayNoOf = (n: TimelineNodeDto) => {
              const v = parseInt(n.date_label.replace(/\D/g, ""), 10);
              return Number.isFinite(v) && v > 0 ? v : null;
            };
            ordered.forEach((n, i) => {
              rows.push(
                <div
                  key={n.id}
                  onClick={() => { switchNode(n.id); setView("story"); }}
                  title="点击进入该日期剧情"
                  style={{
                    border: `1px solid ${n.id === node?.id ? T.accent : T.line}`,
                    borderRadius: 10,
                    padding: "13px 16px",
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                    cursor: "pointer",
                    background: n.id === node?.id ? T.accentSoft : T.panel,
                    transition: "border-color .15s, background .15s",
                  }}
                  onMouseEnter={(e) => { e.currentTarget.style.borderColor = T.accent; e.currentTarget.style.background = "#20242d"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.borderColor = n.id === node?.id ? T.accent : T.line; e.currentTarget.style.background = n.id === node?.id ? T.accentSoft : T.panel; }}
                >
                  <div>
                    <div style={{ fontWeight: 600, fontSize: 14, display: "flex", alignItems: "center", gap: 8 }}>
                      {n.node_type === "interlude" ? "🎭 " : ""}{n.date_label}
                      {nodeDateDisplay(n) && <span style={{ fontSize: 11, color: T.text3, fontWeight: 400 }}>{nodeDateDisplay(n)}</span>}
                      {n.day_summary ? <span title={"已生成日摘要：" + n.day_summary} style={{ fontSize: 10, color: T.ok }}>📝</span> : null}
                      <span style={{ fontSize: 10, color: T.text3, background: "#191c22", border: `1px solid ${T.lineSoft}`, padding: "1px 7px", borderRadius: 8 }}>进入 →</span>
                    </div>
                    <div style={{ fontSize: 11, color: "#8a91a1", marginTop: 4 }}>{n.node_type === "interlude" ? "插曲 / 回忆" : "常规节点"}</div>
                    {n.tags ? (
                      <div style={{ display: "flex", gap: 4, marginTop: 4, flexWrap: "wrap" }}>
                        {n.tags.split(",").filter(Boolean).map((tg) => (
                          <span key={tg} style={{ fontSize: 10, color: T.accent, background: T.accentSoft, borderRadius: 6, padding: "1px 7px" }}>#{tg}</span>
                        ))}
                      </div>
                    ) : null}
                  </div>
                  <button
                    onClick={(e) => { e.stopPropagation(); removeNode(n.id, n.date_label); }}
                    style={{ color: T.danger, background: "transparent", border: "none", cursor: "pointer", fontSize: 12.5, padding: "4px 10px" }}
                  >
                    删除
                  </button>
                </div>
              );
              // 与下一个"天数节点"之间若跳号 → 内联一条"时间流逝"分隔（点它可插入事件）
              const nxt = ordered.slice(i + 1).find((x) => x.node_type !== "interlude" && x.node_type !== "flashback");
              const a = dayNoOf(n), b = nxt ? dayNoOf(nxt) : null;
              if (n.node_type !== "interlude" && n.node_type !== "flashback" && a && b && b - a > 1) {
                rows.push(
                  <button key={`gap-${n.id}`}
                    onClick={() => setGapPrompt({ from: a + 1, to: b - 1, day: String(a + 1), title: "" })}
                    title={`第 ${a + 1}~${b - 1} 天没有剧情（时间流逝）—— 点击可在这段里插入事件`}
                    style={{ border: `1px dashed ${T.line}`, borderRadius: 10, padding: "9px 16px", background: "transparent", color: T.text3, cursor: "pointer", fontSize: 11.5, textAlign: "left", fontFamily: "inherit" }}>
                    ⏳ 时间流逝 · 第 {a + 1}~{b - 1} 天（无事件）　<span style={{ color: T.accent }}>+ 在此插入事件</span>
                  </button>
                );
              }
            });
            return <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>{rows}</div>;
          })()}
          {timelineView === "list" && (() => {
            const gaps = timeGaps();
            const blank = gaps.reduce((s, g) => s + (g.to - g.from - 1), 0);
            return (
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginTop: 10, fontSize: 11, color: T.text3 }}>
                <span>时间维：共 {dayNodesSorted().length} 天有剧情</span>
                {blank > 0
                  ? <span style={{ color: T.warn }}>· 其中 {gaps.length} 段空白期，合计 {blank} 天无事件</span>
                  : <span>· 天数连续，没有空白期</span>}
              </div>
            );
          })()}
          {timelineView === "list" && renderTimelineTrack({ compact: true })}
          {timelineView === "cal" && (
            (() => {
              // 基准月：时间起点所在月（或今天），加上翻页偏移
              let baseY = new Date().getFullYear(), baseM = new Date().getMonth();
              if (currentStory) {
                const mm = currentStory.time_origin.trim().match(/^(\d{4})[-./年](\d{1,2})[-./月](\d{1,2})日?$/);
                if (mm) { baseY = +mm[1]; baseM = +mm[2] - 1; }
              }
              baseM += calOffset;
              const y = baseY + Math.floor(baseM / 12);
              const m = ((baseM % 12) + 12) % 12;
              // 节点 → 剧情日期映射（day 节点；插曲不占日期）
              const nodeDates = new Map<number, { y: number; m: number; d: number }>();
              if (currentStory) {
                const mm = currentStory.time_origin.trim().match(/^(\d{4})[-./年](\d{1,2})[-./月](\d{1,2})日?$/);
                if (mm) {
                  const base = new Date(+mm[1], +mm[2] - 1, +mm[3]);
                  for (const nn of timelineNodes) {
                    if (nn.node_type === "interlude" || nn.node_type === "flashback") continue;
                    const dayNo = parseInt(nn.date_label.replace(/\D/g, ""), 10);
                    if (!dayNo || dayNo <= 0) continue;
                    const d = new Date(base);
                    d.setDate(d.getDate() + (dayNo - 1));
                    nodeDates.set(nn.id, { y: d.getFullYear(), m: d.getMonth(), d: d.getDate() });
                  }
                }
              }
              const firstDay = new Date(y, m, 1).getDay();
              const daysInMonth = new Date(y, m + 1, 0).getDate();
              const cells: (number | null)[] = [];
              for (let i = 0; i < firstDay; i++) cells.push(null);
              for (let d = 1; d <= daysInMonth; d++) cells.push(d);
              while (cells.length % 7 !== 0) cells.push(null);
              const nodeForDate = (dd: number) => {
                for (const [nid, nd] of nodeDates) {
                  if (nd.y === y && nd.m === m && nd.d === dd) {
                    return timelineNodes.find((x) => x.id === nid) || null;
                  }
                }
                return null;
              };
              const interludeNodes = branchScopedNodes.filter((nn) => nn.node_type === "interlude" || nn.node_type === "flashback");
              return (
                <div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                    <button onClick={() => setCalOffset(calOffset - 1)} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, fontFamily: "inherit" }}>◀ 上月</button>
                    <span style={{ fontSize: 14, fontWeight: 600, color: T.text1 }}>{y} 年 {m + 1} 月</span>
                    <button onClick={() => setCalOffset(calOffset + 1)} style={{ fontSize: 12, padding: "3px 10px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, fontFamily: "inherit" }}>下月 ▶</button>
                    <button onClick={() => setCalOffset(0)} style={{ fontSize: 11, padding: "3px 8px", cursor: "pointer", borderRadius: 6, border: "1px dashed " + T.line, background: "transparent", color: T.text3, fontFamily: "inherit" }}>回到起点月</button>
                  </div>
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(7, 1fr)", gap: 4 }}>
                    {["日", "一", "二", "三", "四", "五", "六"].map((w) => (
                      <div key={w} style={{ textAlign: "center", fontSize: 11, color: T.text3, padding: "4px 0" }}>{w}</div>
                    ))}
                    {cells.map((dd, i) => {
                      if (dd === null) return <div key={i} />;
                      const nn = nodeForDate(dd);
                      const cur = node?.id && nodeDates.has(node.id) && nodeDates.get(node.id)!.y === y && nodeDates.get(node.id)!.m === m && nodeDates.get(node.id)!.d === dd;
                      return (
                        <div key={i} onClick={nn ? () => { switchNode(nn.id); setView("story"); } : undefined} style={{
                          minHeight: 54, borderRadius: 8, padding: 5, cursor: nn ? "pointer" : "default",
                          border: `1px solid ${cur ? T.accent : T.lineSoft}`,
                          background: nn ? T.accentSoft : T.panel2,
                          display: "flex", flexDirection: "column", gap: 2, boxSizing: "border-box",
                        }}>
                          <span style={{ fontSize: 11, color: nn ? T.accent : T.text3, fontWeight: nn ? 700 : 400 }}>{dd}</span>
                          {nn && (
                            <>
                              <span style={{ fontSize: 10, color: T.text2, fontWeight: 600 }}>{nn.date_label}</span>
                              {nn.tags && <span style={{ fontSize: 9, color: T.accent, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{nn.tags.split(",").filter(Boolean).slice(0, 2).map((t) => `#${t}`).join(" ")}</span>}
                            </>
                          )}
                        </div>
                      );
                    })}
                  </div>
                  {renderTimelineTrack()}
                  {interludeNodes.length > 0 && (
                    <div style={{ marginTop: 10, display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                      <span style={{ fontSize: 11, color: T.text3 }}>🎭 插曲 / 回忆（不占日期）：</span>
                      {interludeNodes.map((nn) => (
                        <button key={nn.id} onClick={() => { switchNode(nn.id); setView("story"); }} style={{ fontSize: 11, padding: "3px 10px", borderRadius: 12, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: "pointer", fontStyle: "italic", fontFamily: "inherit" }}>
                          {nn.date_label}{nn.tags ? ` · #${nn.tags.split(",")[0]}` : ""}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              );
            })()
          )}
        {/* v3.4：关键记忆（表白/承诺/牺牲/背叛，每种只记第一次） */}
        {currentStory && (
          <div style={{ marginTop: 14 }}>
            <KeyMemories
              storyId={String(currentStory.id)}
              storyTitle={currentStory.title}
              setMsg={setMsg}
            />
          </div>
        )}
        </div>
  );
}
