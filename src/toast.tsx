// 全局轻提示（toast，2026-10-25）。
//
// 为什么要有：原来全应用只有一个 `msg` 状态（App.tsx），而它**只在一个地方渲染** ——
// 剧情页输入台上方的浮层（StoryInput）。于是：
//   · 在「插件 / 素材 / 设定 / 参数」等页面点保存，提示写进 msg 却没人渲染 → 当时看不到；
//   · 等回到剧情页，那条「✅ 已保存」还飘在那儿 → 看着像"保存提示跑到对话区去了"。
//
// 分层约定（改代码前先想清楚这条提示属于哪一层）：
//   T1 全局 toast（本文件）：**任何页面的操作结果** —— 保存/删除/导入/导出/清理/失败。
//      判据：换个页面也应该看到的，就放这里。
//   T2 就地反馈（页面内小字）：**属于某个控件/区块**的结果 —— 例如插件的「检查配置」结果、
//      开发者工具的试运行结果、Key 清除结果。已经有本地 state 的就保持现状，别升级成全局。
//   T3 剧情区浮层（StoryInput 里的 msg）：**这段剧情刚发生了什么** —— 切分支、切天数、
//      删节点、日摘要、生成中断、导演指令被锁。判据：离开剧情上下文就没意义的，留在那儿。
//
// 语气从文案前缀推断（✅/🗑/📋 → 绿，❌ → 红，⚠ → 黄），所以调用方不需要改原有字符串。

import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import { T } from "./theme";

type Tone = "ok" | "err" | "warn" | "info";
type Item = { id: number; text: string; tone: Tone };

const MAX_ITEMS = 4;

/**
 * 「导演监视器」面板的**外框总宽**：`width: 300` + `borderLeft: 1px` = 301
 * （见 `views/DirectorPanel.tsx:51`；项目里没有全局 `box-sizing: border-box`，
 *  所以 300 是内容宽，边框得另算）。
 * toast 的右边距 = 这个宽度 + 10px —— 用户 2026-10-25 指定：**站在监视器左边 10px 处**，
 * 不压在监视器上，也不贴着主工作区的边。
 */
const DIRECTOR_PANEL_W = 301;

/** 按文案前缀判断语气（保持各页面原有 emoji 文案不变） */
function toneOf(text: string): Tone {
  if (text.startsWith("❌")) return "err";
  if (text.startsWith("⚠")) return "warn";
  if (/^(✅|🗑|📋|💾|🧹|📝|🛟|🟢|🟡|🧾|📂|📊|📁|🔍|📚)/.test(text)) return "ok";
  return "info";
}

const TONE_COLOR: Record<Tone, string> = {
  ok: T.ok,
  err: T.danger,
  warn: T.warn,
  info: T.accent,
};

const ToastCtx = createContext<((text: string) => void) | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Item[]>([]);
  const seq = useRef(0);

  const push = useCallback((text: string) => {
    const t = (text ?? "").trim();
    if (!t) return;
    const id = ++seq.current;
    const tone = toneOf(t);
    setItems((prev) => [...prev, { id, text: t, tone }].slice(-MAX_ITEMS));
    // 错误留久一点（用户要看清楚为什么失败），其余 3.2 秒
    window.setTimeout(() => {
      setItems((prev) => prev.filter((x) => x.id !== id));
    }, tone === "err" ? 6000 : 3200);
  }, []);

  const dismiss = useCallback((id: number) => {
    setItems((prev) => prev.filter((x) => x.id !== id));
  }, []);

  const value = useMemo(() => push, [push]);

  return (
    <ToastCtx.Provider value={value}>
      {children}
      {/* 右下角堆叠：右边距 = 导演监视器外框总宽 + 10px；距底 96px 不挡输入台的发送键 */}
      <div
        style={{
          position: "fixed", right: DIRECTOR_PANEL_W + 10, bottom: 96, zIndex: 8500,
          display: "flex", flexDirection: "column", gap: 8,
          alignItems: "flex-end", pointerEvents: "none",
        }}
      >
        {items.map((it) => (
          <div
            key={it.id}
            onClick={() => dismiss(it.id)}
            title="点击关闭"
            style={{
              pointerEvents: "auto",
              maxWidth: 460,
              padding: "9px 13px",
              borderRadius: 10,
              border: `1px solid ${T.line}`,
              borderLeft: `3px solid ${TONE_COLOR[it.tone]}`,
              background: T.panel2,
              boxShadow: "0 12px 32px rgba(0,0,0,.45)",
              color: it.tone === "err" ? "#ff9b95" : T.text1,
              fontSize: 12,
              lineHeight: 1.65,
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              cursor: "pointer",
            }}
          >{it.text}</div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

/** 推一条全局提示；没包 Provider 时退化成 no-op（不让界面因为提示炸掉） */
export function useToast() {
  const push = useContext(ToastCtx);
  return push ?? (() => {});
}
