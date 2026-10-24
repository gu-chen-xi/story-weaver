// 时间线视图相关的状态与计算（v3.5.2，从 App.tsx 抽出）。
//
// ## 为什么抽
//
// App.tsx 里散着一堆"时间线专属"的东西：视图切换（列表/日历/树状图）、日历翻页偏移、
// 时间流逝弹窗、"按天的排序/空档/日期换算"四个小工具，以及"在空档里插入事件"的动作。
// 它们彼此内聚、和 App 的其它状态几乎不交叉 —— 留在 App 里只是让那个文件更难读。
//
// ## 抽的时候刻意保留的两个行为
//
// 1. **一律按当前分支过滤**：日历、时间流逝、导出区间只看"这一条线"的剧情；
//    别的分支的天数不该混进来（否则日历上会出现不属于这条线的日期）。
// 2. **插曲/倒叙不算天数**：`node_type` 是 interlude / flashback 的节点不参与
//    "第 N 天"的排序与空档计算（它们本来就不占日期）。

import { useCallback, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Story, TimelineNodeDto } from "../types";
import { visibleForBranch } from "../lib/branchView";
// 纯日期计算放在 lib 里（那里能被前端测试编译，能单测闰年/跨月等边界）
import { dayNoOf, isDayNode, dateDisplayOf } from "../lib/timelineDates";

export type TimeGap = { from: number; to: number };
export type DayNodeRow = { node: TimelineNodeDto; dayNo: number };
export type GapPrompt = { from: number; to: number; day: string; title: string };

export type UseTimelineViewDeps = {
  timelineNodes: TimelineNodeDto[];
  currentBranchId: string | null;
  currentStory: Story | null;
  setMsg: (s: string) => void;
  loadTimelineAndMessages: (storyId: number, nodeId?: number, branchOverride?: string | null) => Promise<void>;
};

export { dayNoOf, isDayNode, dateDisplayOf };

export function useTimelineView(deps: UseTimelineViewDeps) {
  const { timelineNodes, currentBranchId, currentStory, setMsg, loadTimelineAndMessages } = deps;

  /** 时间线视图：日历 / 时间线树状图（「列表」已删，与树状图重复）*/
  const [timelineView, setTimelineView] = useState<"cal" | "tree">("tree");
  /** 日历翻页偏移（月） */
  const [calOffset, setCalOffset] = useState(0);
  /** 「在时间流逝空档里插入事件」的弹窗状态 */
  const [gapPrompt, setGapPrompt] = useState<GapPrompt | null>(null);

  /** 当前分支能看到的节点（共享历史 + 本分支） */
  const branchScopedNodes = useMemo(
    () => timelineNodes.filter((n) => visibleForBranch(n, currentBranchId)),
    [timelineNodes, currentBranchId],
  );

  /**
   * 当前分支的"天数节点"按天排序。
   *
   * 相邻跳号（如 5 → 8）之间就是一段"无事件的时间" —— 时间流逝提示与阶段摘要都靠它。
   */
  const dayNodesSorted = useCallback((): DayNodeRow[] => {
    return timelineNodes
      .filter((n) => (n.branch_id ?? null) === (currentBranchId ?? null))
      .filter(isDayNode)
      .map((n) => ({ node: n, dayNo: dayNoOf(n) }))
      .filter((x) => Number.isFinite(x.dayNo) && x.dayNo > 0)
      .sort((a, b) => a.dayNo - b.dayNo);
  }, [timelineNodes, currentBranchId]);

  /** 空档：相邻两个天数节点之间跳过的天数 */
  const timeGaps = useCallback((): TimeGap[] => {
    const arr = dayNodesSorted();
    const out: TimeGap[] = [];
    for (let i = 0; i + 1 < arr.length; i++) {
      if (arr[i + 1].dayNo - arr[i].dayNo > 1) {
        out.push({ from: arr[i].dayNo, to: arr[i + 1].dayNo });
      }
    }
    return out;
  }, [dayNodesSorted]);

  /** 剧情日期显示（"第 N 天" → "MM-DD"） */
  const nodeDateDisplay = useCallback(
    (n: { node_type: string; date_label: string }) => dateDisplayOf(n, currentStory?.time_origin),
    [currentStory?.time_origin],
  );

  /** 可导出区间选项：当前分支的天数节点 */
  const dayNodes = useMemo(
    () => timelineNodes.filter((n) => (n.branch_id ?? null) === (currentBranchId ?? null)).filter(isDayNode),
    [timelineNodes, currentBranchId],
  );

  /**
   * 在指定天数插入一个 day 节点（时间流逝区间里"插入事件"）。
   *
   * 校验两道：天数必须 > 0，且必须落在弹窗给的区间内 ——
   * 弹窗按钮已经禁用，但别的入口（键盘回车等）也可能绕过，所以这里再兜一道。
   */
  const insertEventAtDay = useCallback(async () => {
    if (!currentStory || !gapPrompt) return;
    const dayNo = Number(gapPrompt.day);
    if (!dayNo || dayNo <= 0) {
      setMsg("⚠ 请填写大于 0 的天数");
      return;
    }
    if (dayNo < gapPrompt.from || dayNo > gapPrompt.to) {
      setMsg(`⚠ 只能填区间内的天数（第 ${gapPrompt.from} ~ ${gapPrompt.to} 天）`);
      return;
    }
    if (dayNodesSorted().some((x) => x.dayNo === dayNo)) {
      setMsg(`⚠ 第 ${dayNo} 天已经存在，不能重复创建`);
      return;
    }
    try {
      const n = await invoke<{ id: number; date_label: string }>("create_node_at_day", {
        storyId: String(currentStory.id),
        dayNo,
        branchId: currentBranchId,
      });
      setGapPrompt(null);
      setMsg(`✅ 已在${n.date_label}插入事件节点`);
      await loadTimelineAndMessages(currentStory.id, n.id);
    } catch (e) {
      setMsg("❌ 插入失败: " + String(e));
    }
  }, [currentStory, gapPrompt, currentBranchId, dayNodesSorted, setMsg, loadTimelineAndMessages]);

  return {
    // 视图状态
    timelineView, setTimelineView, calOffset, setCalOffset,
    gapPrompt, setGapPrompt,
    // 数据
    branchScopedNodes, dayNodesSorted, timeGaps, nodeDateDisplay, dayNodes,
    // 动作
    insertEventAtDay,
    // 纯函数（给调用方直接用）
    dayNoOf,
  };
}
