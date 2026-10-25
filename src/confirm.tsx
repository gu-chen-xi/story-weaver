// 自绘确认框（2026-10-25）。
//
// 为什么要自己画：原来的 `window.confirm` 是 WebView 的原生框 ——
//   标题写着「tauri.localhost」、位置贴窗口顶部居中、颜色跟系统走（浅色）、
//   文本一长就被挤成窄条。在深色界面里非常出戏，用户直接点名要改：位置 / 颜色 / 大小和内容。
//
// 用法（替换原生 confirm，语义一一对应）：
//   const confirm = useConfirm();
//   if (!(await confirm("删除这条关系？"))) return;
//   if (!(await confirm({ title: "删除角色", body: `删除「${name}」？`, danger: true }))) return;
//
// 交互约定：居中弹窗、Esc / 点遮罩 = 取消、焦点默认给「取消」（删除类误按回车不会出事）。
//
// 为什么不排队：确认框是同步阻塞式交互，同一时刻弹两个必然是 bug。
// 已有弹窗时后来的请求直接返回 false（当成"用户没同意"），宁可少做也不挂住。
//
// 组件挂在 `main.tsx` 的 `<ConfirmProvider>`，用 portal 落到 document.body，
// 不受任何页面的 transform / overflow 影响。

import {
  createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { T } from "./theme";

export type ConfirmOptions = {
  /** 标题，默认「确认操作」/ 危险操作为「确认删除」 */
  title?: string;
  /** 正文，支持 \n 换行原样显示 */
  body: string;
  okText?: string;
  cancelText?: string;
  /** 危险操作：⚠ 图标 + 红色确定按钮（删除类都用它） */
  danger?: boolean;
};

type Pending = { opts: ConfirmOptions; resolve: (ok: boolean) => void };

const ConfirmCtx = createContext<((opts: string | ConfirmOptions) => Promise<boolean>) | null>(null);

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<Pending | null>(null);
  const pendingRef = useRef<Pending | null>(null);

  const ask = useCallback((opts: string | ConfirmOptions) => {
    const o: ConfirmOptions = typeof opts === "string" ? { body: opts } : opts;
    if (pendingRef.current) return Promise.resolve(false);
    return new Promise<boolean>((resolve) => {
      const p: Pending = { opts: o, resolve };
      pendingRef.current = p;
      setPending(p);
    });
  }, []);

  const close = useCallback((ok: boolean) => {
    const p = pendingRef.current;
    pendingRef.current = null;
    setPending(null);
    p?.resolve(ok);
  }, []);

  return (
    <ConfirmCtx.Provider value={ask}>
      {children}
      {pending && <ConfirmDialog opts={pending.opts} onClose={close} />}
    </ConfirmCtx.Provider>
  );
}

export function useConfirm() {
  const ask = useContext(ConfirmCtx);
  if (!ask) throw new Error("useConfirm 必须在 <ConfirmProvider> 里使用");
  return ask;
}

function ConfirmDialog({ opts, onClose }: { opts: ConfirmOptions; onClose: (ok: boolean) => void }) {
  const cancelRef = useRef<HTMLButtonElement | null>(null);
  const danger = opts.danger === true;

  // 默认焦点给「取消」—— 删除类操作里最安全的那颗
  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  // Esc = 取消。Enter **不**做成"全局确定"：焦点默认在「取消」上，
  // 这样误按一次回车是安全的（删除类操作里这一下很值钱）；
  // 想确定就 Tab 到确定按钮或直接点 —— 按钮本来就有回车语义。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); onClose(false); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return createPortal(
    <div
      // 遮罩：点空白处 = 取消（跟原生框的"点外面不关"不同，这里更顺手）
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(false); }}
      style={{
        position: "fixed", inset: 0, zIndex: 9000,
        background: "rgba(6,8,12,.66)",
        display: "flex", alignItems: "center", justifyContent: "center",
        padding: 24,
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="dsh-confirm-title"
        style={{
          width: 460, maxWidth: "100%",
          background: T.panel,
          border: `1px solid ${danger ? "rgba(240,85,77,.28)" : T.line}`,
          borderRadius: 14,
          boxShadow: "0 28px 70px rgba(0,0,0,.6), 0 0 0 1px rgba(255,255,255,.02) inset",
          overflow: "hidden",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "16px 18px 0" }}>
          <span
            style={{
              width: 26, height: 26, borderRadius: 8, flexShrink: 0,
              display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 14,
              background: danger ? "rgba(240,85,77,.14)" : T.accentSoft,
            }}
          >{danger ? "🗑" : "❓"}</span>
          <b id="dsh-confirm-title" style={{ fontSize: 13.5, color: T.text1, fontWeight: 600 }}>
            {opts.title ?? (danger ? "确认删除" : "确认操作")}
          </b>
        </div>

        <div
          style={{
            padding: "10px 18px 4px",
            fontSize: 12.5, lineHeight: 1.8, color: T.text2,
            whiteSpace: "pre-wrap", wordBreak: "break-word",
            maxHeight: "46vh", overflowY: "auto",
          }}
        >{opts.body}</div>

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 10, padding: "14px 18px 16px" }}>
          <button
            ref={cancelRef}
            onClick={() => onClose(false)}
            style={{
              height: 32, padding: "0 16px", borderRadius: 8,
              border: `1px solid ${T.line}`, background: T.panel2, color: T.text2,
              fontSize: 12.5, cursor: "pointer", fontFamily: "inherit",
            }}
          >{opts.cancelText ?? "取消"}</button>
          <button
            onClick={() => onClose(true)}
            style={{
              height: 32, padding: "0 16px", borderRadius: 8, border: "none",
              background: danger ? T.danger : "linear-gradient(135deg,#FF7A18,#ff9a45)",
              color: danger ? "#fff" : "#231505",
              fontSize: 12.5, fontWeight: 650, cursor: "pointer", fontFamily: "inherit",
            }}
          >{opts.okText ?? "确定"}</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
