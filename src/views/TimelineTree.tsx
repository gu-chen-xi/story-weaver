// 时间线「段框树」（v3.5.1）—— 替换原来的列表式"树状图"。
//
// 设计依据：用户 2026-10-24 提供的《织梦机-时间线可视化前端实现建议》。
//
// ## 三条硬约定（都来自文档，别随手改）
//
// 1. **框宽高在渲染前由数据算出**（`lib/timelineSegments.ts` 负责），组件不测 DOM；
// 2. **不订阅流式事件** —— 树只在「切分支 / 新建分支 / 新节点 / 展开状态」变化时更新；
// 3. **展开集合与滚动位置内聚在组件内部** —— 提到上层就会出现
//    "切走再切回，展开状态和滚动位置错乱"（文档点名的最常见缺陷）。
//
// ## 错误边界 + 退路
//
// 复杂渲染一处未捕获异常就白屏。所以：组件自带错误边界兜底，
// 外面还留了「切回列表」的按钮（在时间线页的视图切换那里，本来就有）。

import { Component, memo, useCallback, useMemo, useState, type ReactNode } from "react";
import { T } from "../theme";
import type { BranchDto, TimelineNodeDto } from "../types";
import {
  buildSegments,
  dotsAria,
  layoutTree,
  type LayoutOpts,
  type Segment,
} from "../lib/timelineSegments";
import {
  buildDayTree,
  dayTreeToUnits,
  flattenDays,
  layoutUnits,
  type DayNode,
} from "../lib/timelineNodes";

// ── 常量（框尺寸集中一处；改这里就够，不要在 JSX 里散落数字）──
const ROW_H = 16;
const PAD_Y = 9;
const GAP_X = 18;
const GAP_Y = 30;
const BOX_W = 186;

const PALETTE = {
  boxBg: "#171a21",
  boxBgCurrent: "#1d2230",
  line: "#2a2f3a",
  text1: "#e6e9ef",
  text2: "#aab0bd",
  text3: "#7c8391",
  accent: "#FF7A18",
};

type Props = {
  nodes: TimelineNodeDto[];
  branches: BranchDto[];
  currentBranchId: string | null;
  /** 精简态：窄栏用（省掉提示行） */
  compact?: boolean;
  /** 当前节点（逐日模式下高亮） */
  currentNodeId?: number | null;
  /** 点某一天 → 跳到剧情页 */
  onJump?: (nodeId: number) => void;
};

// ── 单个段框（memo：文档要求"框组件用 memo 包裹"）──
const SegBox = memo(function SegBox(p: {
  x: number; y: number; w: number; h: number;
  seg: Segment;
  dots: string[];
  expanded: boolean;
  compact: boolean;
  onToggle: (key: string) => void;
  onJump?: (nodeId: number) => void;
}) {
  const { x, y, w, h, seg, dots, expanded, compact, onToggle, onJump } = p;
  const isCurrent = seg.isCurrent;
  const borderColor = seg.empty ? PALETTE.line : isCurrent ? PALETTE.accent : `${seg.color}88`;
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: w,
        minHeight: h,
        boxSizing: "border-box",
        border: `1px ${seg.empty ? "dashed" : "solid"} ${borderColor}`,
        borderLeft: `3px solid ${seg.empty ? PALETTE.line : seg.color}`,
        borderRadius: 10,
        background: isCurrent ? PALETTE.boxBgCurrent : PALETTE.boxBg,
        padding: `${PAD_Y}px 10px`,
        cursor: seg.empty ? "default" : "pointer",
        userSelect: "none",
      }}
      onClick={() => { if (!seg.empty) onToggle(seg.key); }}
      title={dotsAria(seg)}
      role={seg.empty ? undefined : "button"}
      aria-expanded={seg.empty ? undefined : expanded}
    >
      {/* ① 线名（+ 当前线标记） */}
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11.5, fontWeight: 650, color: PALETTE.text1 }}>
        <span style={{ width: 7, height: 7, borderRadius: 4, background: seg.color, flex: "0 0 7px" }} />
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{seg.lineName}</span>
        {seg.empty && <span style={{ fontSize: 10, color: PALETTE.text3 }}>（尚未开始）</span>}
        {isCurrent && !seg.empty && (
          <span style={{ fontSize: 9.5, color: PALETTE.accent, border: `1px solid ${PALETTE.accent}66`, borderRadius: 4, padding: "0 4px" }}>
            当前
          </span>
        )}
        <span style={{ marginLeft: "auto", fontSize: 10, color: PALETTE.text3 }}>{seg.empty ? "" : expanded ? "▾" : "▸"}</span>
      </div>

      {/* ② 范围 + 节点数 */}
      {!seg.empty && (
        <div style={{ fontSize: 10.5, color: PALETTE.text2, marginTop: 2 }}>
          {seg.rangeLabel} · {seg.nodes.length} 个节点
        </div>
      )}

      {/* ③ 点阵（● 节点 ★ 分叉点 ⋯ 省略） */}
      {!seg.empty && (
        <div
          style={{ fontSize: 10, letterSpacing: 1.2, marginTop: 3, color: PALETTE.text2, lineHeight: 1.2 }}
          aria-hidden="true"
        >
          {dots.map((d: string, i: number) => (
            <span key={i} style={{ color: d === "★" ? PALETTE.accent : d === "⋯" ? PALETTE.text3 : PALETTE.text2 }}>{d}</span>
          ))}
        </div>
      )}

      {/* ④ 分叉提示（仅分叉段） */}
      {seg.hasFork && !compact && (
        <div style={{ fontSize: 9.5, color: PALETTE.text3, marginTop: 3 }}>
          ★ {seg.nodes[seg.nodes.length - 1]?.label ?? "段末"} = 分叉点
        </div>
      )}
      {/* 当前所在：参考图里是框底单独一行，比标题旁的徽章更醒目 */}
      {isCurrent && !seg.empty && !compact && (
        <div style={{ fontSize: 9.5, color: PALETTE.accent, marginTop: 2 }}>● 当前所在</div>
      )}

      {/* ⑤ 展开态：段内逐日节点（点一下跳到那天） */}
      {expanded && !seg.empty && (
        <div style={{ marginTop: 6, borderTop: `1px dashed ${PALETTE.line}`, paddingTop: 5, display: "flex", flexDirection: "column", gap: 2 }}>
          {seg.nodes.slice(0, 30).map((n) => (
            <div
              key={n.id}
              onClick={(e) => { e.stopPropagation(); onJump?.(n.id); }}
              style={{
                fontSize: 10.5, color: n.isFork ? PALETTE.accent : PALETTE.text2,
                cursor: onJump ? "pointer" : "default", padding: "1px 0",
                whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis",
              }}
              title={n.isFork ? `${n.label}（分叉点）` : n.label}
            >
              {n.isFork ? "★ " : n.isSide ? "◇ " : "· "}{n.label}
            </div>
          ))}
          {seg.nodes.length > 30 && (
            <div style={{ fontSize: 10, color: PALETTE.text3 }}>（还有 {seg.nodes.length - 30} 个节点未展开）</div>
          )}
        </div>
      )}
    </div>
  );
});

function TreeBody(props: Props) {
  const { nodes, branches, currentBranchId, currentNodeId, compact = false, onJump } = props;
  // 展开集合内聚在这里（文档 §四.3）
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set<string>());
  // 两种呈现（用户参考图）：段框压缩 / 逐日全展开。共用同一套布局算法，只是"一个单位"不同。
  const [mode, setMode] = useState<"seg" | "day">("seg");

  const opts: LayoutOpts = useMemo(
    () => ({ boxW: compact ? 150 : BOX_W, rowH: ROW_H, padY: PAD_Y, gapX: GAP_X, gapY: GAP_Y, expanded }),
    [compact, expanded],
  );

  const seg = useMemo(
    () => buildSegments(nodes, branches, currentBranchId),
    [nodes, branches, currentBranchId],
  );
  const repairedCount = useMemo(() => {
    if (!seg) return 0;
    let n = 0;
    const walk = (s: Segment) => { if (s.repaired) n += 1; s.children.forEach(walk); };
    walk(seg);
    return n;
  }, [seg]);  const layout = useMemo(() => (seg ? layoutTree(seg, opts) : null), [seg, opts]);

  // 回调引用稳定（否则 memo 直接失效 —— 文档 §四.1）
  const toggle = useCallback((key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  }, []);

  const expandAll = useCallback(() => {
    if (!seg) return;
    const all = new Set<string>();
    const walk = (s: Segment) => { if (!s.empty) all.add(s.key); s.children.forEach(walk); };
    walk(seg);
    setExpanded(all);
  }, [seg]);

  const collapseAll = useCallback(() => setExpanded(new Set<string>()), []);

  // 逐日树（图二）：每个节点一个药丸，分叉点加粗
  const dayRoot = useMemo(
    () => (mode === "day" ? buildDayTree(nodes, branches, currentBranchId, currentNodeId ?? null) : null),
    [mode, nodes, branches, currentBranchId, currentNodeId],
  );
  // 逐日渲染用：key = `d<id>`（与 dayTreeToUnits 的键一致）
  const dayNodes = useMemo(() => {
    const m = new Map<string, DayNode>();
    if (dayRoot) flattenDays(dayRoot).forEach((n) => m.set(`d${n.id}`, n));
    return m;
  }, [dayRoot]);
  const dayLayout = useMemo(() => {
    if (!dayRoot) return null;
    const depthMap = new Map<number, number>();
    let cur = 0;
    const walkDepth = (n: DayNode, d: number) => {
      depthMap.set(n.id, d);
      cur = Math.max(cur, d);
      n.children.forEach((c) => walkDepth(c, d + 1));
    };
    walkDepth(dayRoot, 0);
    const units = dayTreeToUnits(dayRoot, DAY_PILL_H, (n) => depthMap.get(n.id) ?? 0);
    return { layout: layoutUnits(units, 14, 26), depthMap };
  }, [dayRoot]);

  if (!seg || !layout) {
    return <div style={{ color: T.text3, fontSize: 12, padding: 12 }}>还没有节点。</div>;
  }

  // 图例 + 模式切换
  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8, flexWrap: "wrap", fontSize: 11, color: T.text3 }}>
        <span>共 {nodes.length} 个节点 · {branches.length} 条分支</span>
        {repairedCount > 0 && (
          <span
            style={{ color: "#F59E0B" }}
            title="这些节点的 parent_id 没有连到根（历史数据或建分支的边角情况）。已按分支挂回它的分叉点，未丢失。"
          >
            ⚠ {repairedCount} 段父链断开，已按分支挂回
          </span>
        )}
        <span style={{ color: "#6B7280" }}>● 主线</span>
        {branches.map((b) => (
          <span key={b.id} style={{ color: b.color }} title={b.note ? `备注：${b.note}` : undefined}>
            ● {b.name}{b.note ? " ✎" : ""}
          </span>
        ))}
        <div style={{ flex: 1 }} />
        {/* 两种呈现（用户参考图一 / 图二） */}
        <button
          onClick={() => setMode("seg")}
          style={{ ...btnStyle, ...(mode === "seg" ? activeBtn : null) }}
          title="把一段连续无分叉的剧情压成一个框（信息密度低，适合分支多）"
        >
          段框
        </button>
        <button
          onClick={() => setMode("day")}
          style={{ ...btnStyle, ...(mode === "day" ? activeBtn : null) }}
          title="每个节点单独画成一个药丸（信息密度高，适合看具体哪一天）"
        >
          逐日
        </button>
        {mode === "seg" && (
          <>
            <button onClick={expandAll} style={btnStyle}>全部展开</button>
            <button onClick={collapseAll} style={btnStyle}>全部收起</button>
          </>
        )}
      </div>

      <div style={{ overflow: "auto", border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel2, padding: 12 }}>
        {mode === "seg" && (
          <div style={{ position: "relative", width: layout.width, height: layout.height, minWidth: "100%" }}>
            {/* 连线在框下面（不糊在文字上） */}
            <svg
              width={layout.width}
              height={layout.height}
              style={{ position: "absolute", left: 0, top: 0, zIndex: 0, overflow: "visible" }}
              aria-hidden="true"
            >
              {layout.edges.map((e) => {
                const midY = (e.y1 + e.y2) / 2;
                return (
                  <path
                    key={e.fromKey + "->" + e.toKey}
                    d={`M ${e.x1} ${e.y1} C ${e.x1} ${midY}, ${e.x2} ${midY}, ${e.x2} ${e.y2}`}
                    fill="none"
                    stroke={e.color}
                    strokeWidth={1.7}
                    opacity={0.8}
                  />
                );
              })}
            </svg>
            {layout.boxes.map((b) => (
              <SegBox
                key={b.key}
                x={b.x}
                y={b.y}
                w={b.w}
                h={b.h}
                seg={b.seg}
                dots={b.dots}
                expanded={expanded.has(b.key)}
                compact={compact}
                onToggle={toggle}
                onJump={onJump}
              />
            ))}
          </div>
        )}

        {mode === "day" && dayRoot && dayLayout && (
          <div style={{ position: "relative", width: dayLayout.layout.width, height: dayLayout.layout.height, minWidth: "100%" }}>
            <svg
              width={dayLayout.layout.width}
              height={dayLayout.layout.height}
              style={{ position: "absolute", left: 0, top: 0, zIndex: 0, overflow: "visible" }}
              aria-hidden="true"
            >
              {dayLayout.layout.edges.map((e) => {
                const midY = (e.y1 + e.y2) / 2;
                const child = dayNodes.get(e.toKey);
                return (
                  <path
                    key={e.fromKey + "->" + e.toKey}
                    d={`M ${e.x1} ${e.y1} C ${e.x1} ${midY}, ${e.x2} ${midY}, ${e.x2} ${e.y2}`}
                    fill="none"
                    stroke={child?.color ?? "#4b5563"}
                    strokeWidth={1.6}
                    opacity={0.85}
                  />
                );
              })}
            </svg>
            {dayLayout.layout.placed.map((pl) => {
              const n = dayNodes.get(pl.key);
              if (!n) return null;
              return (
                <DayPill
                  key={pl.key}
                  x={pl.x}
                  y={pl.y}
                  w={pl.w}
                  node={n}
                  onJump={onJump}
                />
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

// ── 逐日模式的药丸（参考图二）──
const DAY_PILL_H = 22;

const activeBtn: React.CSSProperties = {
  borderColor: PALETTE.accent,
  color: PALETTE.accent,
  background: "rgba(255,122,24,.10)",
};

const DayPill = memo(function DayPill(p: {
  x: number; y: number; w: number;
  node: DayNode;
  onJump?: (nodeId: number) => void;
}) {
  const { x, y, w, node, onJump } = p;
  const isFork = node.isFork;
  const isMain = node.branchId == null;
  const border = node.isCurrent ? PALETTE.accent : isFork ? PALETTE.text1 : `${node.color}aa`;
  const bg = isFork ? "#0f1218" : PALETTE.boxBg;
  return (
    <div style={{ position: "absolute", left: x, top: y, width: w }}>
      <div
        onClick={() => onJump?.(node.id)}
        title={
          `${node.label}${isFork ? "（分叉点）" : ""}` +
          `${node.isLineStart ? ` · 起点：${node.lineName}` : ""}` +
          `${node.isCurrent ? " · 当前" : ""}`
        }
        style={{
          height: DAY_PILL_H,
          boxSizing: "border-box",
          border: `1px ${isFork ? "2px" : "1px"} solid ${border}`,
          borderRadius: isFork ? 8 : 6,
          background: bg,
          color: isFork ? PALETTE.text1 : PALETTE.text2,
          fontSize: 10.5,
          fontWeight: isFork || node.isCurrent ? 700 : 400,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 3,
          cursor: onJump ? "pointer" : "default",
          whiteSpace: "nowrap",
          overflow: "hidden",
          padding: "0 6px",
        }}
      >
        {node.isSide && <span style={{ color: PALETTE.text3 }}>◇</span>}
        {isFork && <span style={{ color: PALETTE.accent }}>★</span>}
        <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{node.label}</span>
      </div>
      {/* 这条线的起点：标出线名（图二里"分支A · 活着"那种标签） */}
      {node.isLineStart && !isMain && (
        <div style={{ fontSize: 9.5, color: node.color, marginTop: 2, textAlign: "center", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {node.lineName}
        </div>
      )}
    </div>
  );
});

const btnStyle: React.CSSProperties = {
  fontSize: 11.5,
  padding: "3px 10px",
  borderRadius: 6,
  border: `1px solid ${T.line}`,
  background: T.panel3,
  color: T.text2,
  cursor: "pointer",
  fontFamily: "inherit",
};

// ── 错误边界：一处未捕获异常不该让整页白屏（文档 §七）──
class TreeErrorBoundary extends Component<{ children: ReactNode }, { err: Error | null }> {
  state: { err: Error | null } = { err: null };
  static getDerivedStateFromError(err: Error) {
    return { err };
  }
  render(): ReactNode {
    if (this.state.err) {
      return (
        <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, padding: 14, background: T.panel2 }}>
          <div style={{ fontSize: 12.5, color: "#EA6668", marginBottom: 6 }}>时间线渲染失败</div>
          <div style={{ fontSize: 11, color: T.text3, marginBottom: 8, wordBreak: "break-word" }}>
            {String(this.state.err?.message ?? this.state.err)}
          </div>
          <div style={{ fontSize: 11, color: T.text3 }}>可以点上面的「列表」继续用；或在这里重试。</div>
          <button style={{ ...btnStyle, marginTop: 8 }} onClick={() => this.setState({ err: null })}>重试</button>
        </div>
      );
    }
    return this.props.children;
  }
}

export function TimelineTree(props: Props) {
  return (
    <TreeErrorBoundary>
      <TreeBody {...props} />
    </TreeErrorBoundary>
  );
}
