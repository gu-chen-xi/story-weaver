// 「这一天」药丸横条（v3.5.4）。
//
// 用户要求（2026-10-24）：
//   * 排在「分支条 + 下一天/插入插曲」下面，属于**固定头部**的一部分；
//   * 显示**当前这条线**的天数（数据与「时间线树状图」一致，按父链顺序）；
//   * **左右滑动**，不做切换分支（那是上面那排的事）；
//   * 进对话区时默认滚到**最后一天**（只判断一次 —— 判断标准是"进入对话区"）；
//   * 药丸样式（与树状图的逐日模式统一）。

import { useEffect, useMemo, useRef } from "react";
import { T } from "../theme";
import type { BranchDto, TimelineNodeDto } from "../types";
import { buildDayTree, flattenDays } from "../lib/timelineNodes";
import { branchPathIds } from "../lib/branchView";

type Props = {
  nodes: TimelineNodeDto[];
  branches: BranchDto[];
  currentBranchId: string | null;
  currentNodeId: number | null;
  /** 点某天 → 切过去（只切节点，不切分支） */
  onPick: (nodeId: number) => void;
  /** 右键某天 → 删除（旧「天数排」的 × 撤掉后，这是删任意一天的入口） */
  onDelete?: (nodeId: number, label: string) => void;
};

export function DayStrip(props: Props) {
  const { nodes, branches, currentBranchId, currentNodeId, onPick, onDelete } = props;
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  /// 上一次自动滚过的"线"（null = 还没滚过）。用它判断"要不要再滚一次"。
  const scrolledLineRef = useRef<string | null>(null);

  /**
   * 这条线要显示的天：树状图的**父链顺序**，只留"共享历史（到分叉点为止）+ 本分支"。
   *
   * 为什么不直接用 `dayNodesSorted`（按天数排序）：那会把插曲和"倒挂"的天排乱，
   * 而用户明确要求"显示逻辑按照已有的时间线树状图已有的数据"。
   *
   * **共享历史 ≠ 所有主线天**（2026-10-26 修）：原来这里写的是 `branchId === null || === 当前分支`，
   * 于是"从第 6 天分叉"的分支会把主线 7~17 天也画进来（用户实测：药丸条上是 16、17、7、8）。
   * 正确的口径与 `branchPathIds` 一致：本分支自己的节点 ∪ 从分叉点沿父链上溯的那串 ——
   * 分叉之后的主线天是**另一条时间线**，不该出现。
   */
  const days = useMemo(() => {
    const root = buildDayTree(nodes, branches, currentBranchId, currentNodeId);
    if (!root) return [];
    const visible = branchPathIds(nodes, branches, currentBranchId);
    return flattenDays(root).filter((n) => visible.has(n.id));
  }, [nodes, branches, currentBranchId, currentNodeId]);

  // 什么时候滚到最右（最后一天）——用户 2026-10-24 定：
  //   * **进入对话区**（本组件挂载 / 数据刚到位）→ 滚一次；
  //   * **每次点「分支切换」**（currentBranchId 变了）→ 再滚一次；
  //   * 同一条线内的其它变化（比如只是点了另一天）**不滚** ——
  //     否则用户正想看的左边几颗药丸会被弹走。
  useEffect(() => {
    const el = scrollerRef.current;
    if (!el || days.length === 0) return;
    const line = String(currentBranchId ?? "main");
    if (scrolledLineRef.current === line) return;
    el.scrollLeft = el.scrollWidth;
    scrolledLineRef.current = line;
  }, [currentBranchId, days.length]);

  if (days.length === 0) return null;

  return (
    <div
      ref={scrollerRef}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 6,
        overflowX: "auto",
        overflowY: "hidden",
        paddingBottom: 2,
        // 横向滚动的细滚动条（Firefox/Chrome 都能认）
        scrollbarWidth: "thin",
      }}
      title="左右滑动查看这条线的所有天；点某天切过去（不切换分支）"
    >
      {days.map((d) => {
        const active = currentNodeId != null && d.id === currentNodeId;
        return (
          <button
            key={d.id}
            onClick={() => onPick(d.id)}
            onContextMenu={(e) => {
              if (!onDelete) return;
              e.preventDefault();
              onDelete(d.id, d.label);
            }}
            title={
              `${d.label}${d.isFork ? "（分叉点）" : ""}` +
              `${d.isSide ? " · 插曲/倒叙" : ""}` +
              `${d.branchId == null ? " · 主线" : ` · ${d.lineName}${d.isLineStart ? " 起点" : ""}`}`
              + (onDelete ? "\n右键删除这一天" : "")
            }
            style={{
              flex: "0 0 auto",
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              fontSize: 11.5,
              padding: "3px 10px",
              borderRadius: 12,
              cursor: "pointer",
              fontFamily: "inherit",
              whiteSpace: "nowrap",
              color: active ? T.text1 : T.text2,
              fontWeight: active ? 650 : 400,
              background: active ? T.accentSoft : T.panel2,
              border: `1px solid ${active ? T.accent : d.color}`,
            }}
          >
            {d.isFork && <span style={{ color: T.accent, fontSize: 10 }}>★</span>}
            {/* 这条线的第一个节点：给个 ▸，一眼看出「从这儿开始是分支」 */}
            {d.isLineStart && d.branchId != null && (
              <span style={{ color: d.color, fontSize: 10, fontWeight: 700 }}>▸</span>
            )}
            {d.isSide && <span style={{ color: T.text3, fontSize: 10 }}>◇</span>}
            <span style={{ width: 6, height: 6, borderRadius: 3, background: d.color, flexShrink: 0 }} />
            {d.label}
          </button>
        );
      })}
    </div>
  );
}
