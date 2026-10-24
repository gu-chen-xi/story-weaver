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

type Props = {
  nodes: TimelineNodeDto[];
  branches: BranchDto[];
  currentBranchId: string | null;
  currentNodeId: number | null;
  /** 点某天 → 切过去（只切节点，不切分支） */
  onPick: (nodeId: number) => void;
};

export function DayStrip(props: Props) {
  const { nodes, branches, currentBranchId, currentNodeId, onPick } = props;
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const didAutoScroll = useRef(false);

  /**
   * 这条线要显示的天：树状图的**父链顺序**，只留"共享历史 + 本分支"。
   *
   * 为什么不直接用 `dayNodesSorted`（按天数排序）：那会把插曲和"倒挂"的天排乱，
   * 而用户明确要求"显示逻辑按照已有的时间线树状图已有的数据"。
   */
  const days = useMemo(() => {
    const root = buildDayTree(nodes, branches, currentBranchId, currentNodeId);
    if (!root) return [];
    const all = flattenDays(root);
    return all.filter((n) => n.branchId === null || n.branchId === currentBranchId);
  }, [nodes, branches, currentBranchId, currentNodeId]);

  // 进入对话区时滚到最右（最后一天）。只做一次：之后用户自己滑到哪儿就是哪儿。
  useEffect(() => {
    if (didAutoScroll.current) return;
    const el = scrollerRef.current;
    if (!el || days.length === 0) return;
    el.scrollLeft = el.scrollWidth;
    didAutoScroll.current = true;
  }, [days.length]);

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
            title={
              `${d.label}${d.isFork ? "（分叉点）" : ""}` +
              `${d.isSide ? " · 插曲/倒叙" : ""}` +
              `${d.branchId == null ? " · 主线" : " · 分支"}`
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
            {d.isSide && <span style={{ color: T.text3, fontSize: 10 }}>◇</span>}
            <span style={{ width: 6, height: 6, borderRadius: 3, background: d.color, flexShrink: 0 }} />
            {d.label}
          </button>
        );
      })}
    </div>
  );
}
