// 对话区「自动跟随最新内容」的行为（2026-11-04 从 App.tsx 抽出）。
//
// 规则（用户 2026-10-18 定）：
//  · 点发送 / AI 逐字变长时，视图底部跟随最新内容；
//  · 用户一旦自己滚动（滚轮 / 触摸 / 拖滚动条），立刻停止跟随，不打搅他回看历史。
//
// 为什么用 wheel / touchstart / mousedown 判断"用户滚"：
// 程序自己改 scrollTop **不会**触发这些事件 —— 能干净地区分"用户在滚"与"我们在滚"。

import { useCallback, useEffect, useRef, type RefObject } from "react";

export type UseFollowBottomArgs = {
  /** 当前视图；只有 story 页才跟随 */
  view: string;
  /** 生成请求 id：变了说明新一次生成开始 → 重新打开跟随并贴底 */
  genRequestId: string | null;
  /** 消息列表：流式增量会改它 → 跟随到底 */
  messages: unknown[];
  /** 对话区滚动容器 */
  scrollRef: RefObject<HTMLDivElement | null>;
};

export function useFollowBottom({ view, genRequestId, messages, scrollRef }: UseFollowBottomArgs) {
  const followRef = useRef(false);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [scrollRef]);

  // 开始一次生成 → 重新打开跟随 + 先贴到底（让新气泡可见）
  useEffect(() => {
    if (view !== "story") return;
    if (genRequestId) {
      followRef.current = true;
      scrollToBottom();
    }
  }, [genRequestId, view, scrollToBottom]);

  // 消息变化（流式增量 / 新气泡）→ 跟随到底
  useEffect(() => {
    if (view !== "story") return;
    if (followRef.current) scrollToBottom();
  }, [messages, view, scrollToBottom]);

  // 用户滚动 → 停止跟随（只挂一次）
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const stop = () => { followRef.current = false; };
    el.addEventListener("wheel", stop, { passive: true });
    el.addEventListener("touchstart", stop, { passive: true });
    el.addEventListener("mousedown", stop);
    return () => {
      el.removeEventListener("wheel", stop);
      el.removeEventListener("touchstart", stop);
      el.removeEventListener("mousedown", stop);
    };
  }, [scrollRef]);

  return { scrollToBottom };
}
