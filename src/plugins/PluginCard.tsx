// 通用插件卡（v3.5.7，从 src/plugins.tsx 抽出）。
//
// 它**只负责排版**：图标 / 名称 / 描述 / 徽标 / 开关 / 「手动运行」/ 「展开面板」。
// 它不认识任何一个具体插件的 id —— 这是"注册表与页面独立"的关键：
// 加插件只需要在 registry 里加一条，这里一个字都不用改。

import type { CSSProperties } from "react";
import { pluginCaps, type PluginDescriptor, type PluginState } from "./types";

export type PluginCardTokens = {
  line: string;
  lineSoft: string;
  panel: string;
  panel3: string;
  text1: string;
  text2: string;
  text3: string;
  accent: string;
  accentSoft: string;
  ok: string;
  warn: string;
  danger: string;
};

type Props = {
  descriptor: PluginDescriptor;
  state: PluginState;
  T: PluginCardTokens;
  onToggle?: (next: boolean) => void;
  onRun?: () => void;
  onTogglePanel?: () => void;
};

/** 通用插件卡：徽标 + 开关 + 手动执行 + 可展开面板 */
export function PluginCard({ descriptor: d, state: s, T, onToggle, onRun, onTogglePanel }: Props) {
  const caps = pluginCaps(d);
  const planned = d.status === "planned";
  const toneColor = (t: "ok" | "muted" | "warn") => (t === "ok" ? T.ok : t === "warn" ? T.warn : T.text3);
  const toneBorder = (t: "ok" | "muted" | "warn") => (t === "ok" ? T.ok : t === "warn" ? T.warn : T.line);

  const card: CSSProperties = {
    border: `1px ${planned ? "dashed" : "solid"} ${T.line}`,
    borderRadius: 12,
    padding: "14px 16px",
    background: T.panel,
  };

  return (
    <div style={card}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
        <span style={{ fontSize: 22, opacity: planned ? 0.6 : 1 }}>{d.icon}</span>

        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <b style={{ fontSize: 13.5 }}>{d.name}</b>
            {s.badges.map((b, i) => (
              <span key={i} style={{
                fontSize: 10, padding: "1px 8px", borderRadius: 9,
                border: `${planned ? "1px dashed" : "1px solid"} ${toneBorder(b.tone)}`,
                color: toneColor(b.tone),
              }}>{b.text}</span>
            ))}
          </div>

          <div style={{ fontSize: 12, color: T.text3, marginTop: 4, lineHeight: 1.6 }}>{d.desc}</div>

          {/* 总开关 */}
          {caps.canToggle && !planned && (
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10, flexWrap: "wrap" }}>
              <label style={{ display: "flex", alignItems: "center", gap: 7, cursor: onToggle ? "pointer" : "not-allowed", opacity: onToggle ? 1 : 0.55 }}>
                <input type="checkbox" checked={s.enabled} disabled={!onToggle}
                  onChange={(e) => onToggle?.(e.target.checked)}
                  style={{ cursor: onToggle ? "pointer" : "not-allowed", accentColor: T.accent }} />
                <span style={{ fontSize: 12, color: T.text2 }}>{d.toggleLabel ?? "启用"}</span>
              </label>
              {d.toggleHint && <span style={{ fontSize: 10.5, color: T.text3 }}>{d.toggleHint}</span>}
            </div>
          )}

          {/* 手动执行 */}
          {caps.canRunManually && !planned && (
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 8, flexWrap: "wrap" }}>
              <button onClick={onRun} disabled={s.busy || !onRun}
                style={{ fontSize: 12, padding: "4px 12px", cursor: s.busy || !onRun ? "not-allowed" : "pointer", opacity: s.busy || !onRun ? 0.6 : 1 }}>
                {s.busy ? "执行中…" : (s.runLabel ?? "手动执行")}
              </button>
              {s.runHint && <span style={{ fontSize: 10.5, color: T.text3 }}>{s.runHint}</span>}
            </div>
          )}

        </div>

        {/* 面板开关 */}
        {caps.canViewPanel && !planned && (
          <button onClick={onTogglePanel} disabled={!onTogglePanel}
            style={{ fontSize: 12, padding: "5px 13px", cursor: onTogglePanel ? "pointer" : "not-allowed", opacity: onTogglePanel ? 1 : 0.5 }}>
            {s.panelOpen ? "收起" : (caps.canRunManually ? "查看结果" : "打开面板")}
          </button>
        )}

        {/* 规划中：占位按钮 */}
        {planned && (
          <button disabled style={{ fontSize: 12, padding: "4px 12px", cursor: "not-allowed", opacity: 0.5 }}>未开放</button>
        )}
      </div>

      {/* 面板 */}
      {caps.canViewPanel && !planned && s.panelOpen && (
        <div style={{ marginTop: 14, borderTop: `1px solid ${T.lineSoft}`, paddingTop: 12 }}>{s.panel}</div>
      )}

      {planned && d.plannedNote && (
        <div style={{ fontSize: 11, color: T.text3, marginTop: 8 }}>{d.plannedNote}</div>
      )}
    </div>
  );
}

// ===== 插件面板 =====
