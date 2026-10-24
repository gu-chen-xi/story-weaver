// 「📅 时间线」页（日历 / 时间线树状图两种视图）。
//
// v3.5.2：删掉「列表」视图（它与树状图重复，且天数胶囊排已撤）；
// 「阶段摘要压缩」控件也删了 —— 导演监视器里已经有一份，不该两处都能压。
//
// 拆分说明（2026-10-17）：从 App.tsx 整体下沉。**本组件不持有状态** ——
// 所有 state 仍由 App 持有，通过 props 传入；因此行为与拆分前完全一致。
// 依赖 24 项（8 个 state + 4 个 setter + 12 个辅助），props 类型逐一手写，
// 由 tsc 保证与 App 侧一致（漏传/类型错了会直接编译失败）。

import type { Dispatch, SetStateAction } from "react";
import type { AppView } from "../types";
import { T } from "../theme";
import { KeyMemories } from "./KeyMemories";
import { TimelineTree } from "./TimelineTree";
import type { BranchDto, Story, TimelineNodeDto } from "../types";

/// 顶部导航的视图名（与 App 的 `view` state 保持一致）
export type ViewName = AppView;

export type TimelinePageProps = {
  /// 当前分支（null = 主线）
  currentBranchId: string | null;
  currentStory: Story | null;
  /// 当前选中的节点（高亮 + 决定"进入剧情"跳哪）
  node: { id: number; date_label: string } | null;
  /// 全部节点（树状图要全树，日历也用它做日期映射）
  timelineNodes: TimelineNodeDto[];
  branches: BranchDto[];
  /// 列表 / 日历 / 树状图
  timelineView: "cal" | "tree";
  setTimelineView: Dispatch<SetStateAction<"cal" | "tree">>;
  /// 日历翻页偏移（月）
  calOffset: number;
  setCalOffset: Dispatch<SetStateAction<number>>;
  switchNode: (nodeId: number) => Promise<void>;
  setView: Dispatch<SetStateAction<ViewName>>;
  setMsg: Dispatch<SetStateAction<string>>;
};

export function TimelinePage(props: TimelinePageProps) {
  const {
    setMsg, switchNode, setCalOffset, setTimelineView, setView,
    branches, calOffset, currentBranchId, currentStory, node,
    timelineNodes, timelineView,
  } = props;
  return (
        <div>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
            <h3 style={{ margin: 0, fontSize: 15 }}>📅 时间线</h3>
            <span style={{ fontSize: 12, color: "#8a91a1" }}>点击节点可进入对应日期剧情</span>
            <div style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
              <button onClick={() => setTimelineView("cal")} style={{ fontSize: 12, padding: "4px 12px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: timelineView === "cal" ? T.accentSoft : "#191c22", color: timelineView === "cal" ? T.accent : T.text2, fontFamily: "inherit" }}>日历</button>
              {/* 第五批 ③：**调试用**的时间线树状图 —— 从上往下按父子关系打印整棵树，
                  用来直观看清"节点到底挂在哪条线上、父是谁、分叉在哪" */}
              <button onClick={() => setTimelineView("tree")} title="按父子关系从上往下显示整棵树（排查分支/父链问题用）"
                style={{ fontSize: 12, padding: "4px 12px", cursor: "pointer", borderRadius: 6, border: `1px solid ${T.line}`, background: timelineView === "tree" ? T.accentSoft : "#191c22", color: timelineView === "tree" ? T.accent : T.text2, fontFamily: "inherit" }}>🌲 时间线树状图</button>
            </div>
          </div>
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
              currentNodeId={node?.id ?? null}
              onJump={(id) => { switchNode(id); setView("story"); }}
            />
          )}
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
              // 只列当前分支能看到的插曲（原来用 App 传进来的 branchScopedNodes，现在本地算）
              const interludeNodes = timelineNodes
                .filter((nn) => (nn.branch_id ?? null) === (currentBranchId ?? null))
                .filter((nn) => nn.node_type === "interlude" || nn.node_type === "flashback");
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
