// 「左侧导航」（从 App.tsx 拆分，2026-10-17）。
// 只是一棵按钮树：一级入口 + 故事打开后的二级入口（时间线/角色/世界观/提案）。

import type { Dispatch, SetStateAction } from "react";
import { T } from "../theme";
import type { GenProbe } from "../plugins";
import type { Story } from "../types";

/// 顶部导航的视图名（与 App 的 view state 保持一致）
export type ViewName =
  | "chars" | "world" | "relations" | "story" | "proposals" | "temp"
  | "timeline" | "stories" | "plugins" | "portrait" | "compliance" | "params" | "home";

export type SidebarProps = {
  complianceAck: boolean;
  currentStory: Story | null;
  loadCloudConfig: () => Promise<void>;
  loadComplianceAck: () => Promise<void>;
  loadDebug: () => Promise<void>;
  loadEvals: () => Promise<void>;
  loadGenerators: () => Promise<void>;
  navItems: { icon: string; label: string; view: ViewName | null; sub?: boolean; action?: (() => void) | null }[];
  pendingGroups: number;
  setGenProbe: Dispatch<SetStateAction<GenProbe | null>>;
  setShowSettings: Dispatch<SetStateAction<boolean>>;
  setView: Dispatch<SetStateAction<ViewName>>;
  view: ViewName;
};

export function Sidebar(props: SidebarProps) {
  const { complianceAck, currentStory, loadCloudConfig, loadComplianceAck, loadDebug, loadEvals, loadGenerators, navItems, pendingGroups, setGenProbe, setShowSettings, setView, view } = props;
  return (
      <div style={{ width: 200, borderRight: `1px solid ${T.line}`, display: "flex", flexDirection: "column", background: T.panel, flexShrink: 0 }}>
        <div style={{ padding: "14px 16px 12px", borderBottom: `1px solid ${T.lineSoft}` }}>
          <div style={{ fontWeight: 700, fontSize: 14, letterSpacing: 0.4 }}>Story-Weaver</div>
          <div style={{ fontSize: 11, color: T.text3, marginTop: 4 }}>剧情导演工作台</div>
        </div>
        <div style={{ flex: 1, padding: "8px", display: "flex", flexDirection: "column", gap: 2, overflowY: "auto" }}>
          {/* 剧情区：故事列表（一级） */}
          {navItems.filter((it) => !it.sub).map((it) => {
            const active = it.view && view === it.view;
            return (
              <button
                key={it.label}
                onClick={() => { if (it.action) { it.action(); return; } if (!currentStory) { setView("stories"); return; } setView(it.view as "chars" | "world" | "proposals" | "timeline" | "stories"); }}
                style={{
                  display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
                  borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                  background: active ? T.accentSoft : "transparent",
                  color: active ? T.accent : T.text2,
                  fontWeight: active ? 600 : 400,
                }}
              >
                {active && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
                <span>{it.icon}</span>
                <span>{it.label}</span>
              </button>
            );
          })}
          {/* 故事列表展开后的二级入口（时间线 / 角色管理 / 世界观设定 / 演进提案）：整体右移，形成层级 */}
          {currentStory && (
            <div style={{ marginLeft: 6, paddingLeft: 8, borderLeft: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 2 }}>
              {navItems.filter((it) => it.sub).map((it) => {
                const active = it.view && view === it.view;
                return (
                  <button
                    key={it.label}
                    onClick={() => { setView(it.view as "chars" | "world" | "proposals" | "timeline"); }}
                    style={{
                      display: "flex", alignItems: "center", gap: 10, padding: "9px 8px",
                      borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                      background: active ? T.accentSoft : "transparent",
                      color: active ? T.accent : T.text2,
                      fontWeight: active ? 600 : 400,
                    }}
                  >
                    {active && <span style={{ position: "absolute", left: -7, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
                    <span>{it.icon}</span>
                    <span>{it.label}</span>
                    {it.view === "proposals" && pendingGroups > 0 && (
                      <span style={{ marginLeft: "auto", background: T.danger, color: "#fff", fontSize: 10, padding: "1px 6px", borderRadius: 9 }}>{pendingGroups}</span>
                    )}
                  </button>
                );
              })}
            </div>
          )}
          {/* 立绘素材：开始界面在故事列表下；进入故事后自动落到提案下 */}
          <button
            onClick={() => { setView("portrait"); }}
            style={{
              display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
              borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
              background: view === "portrait" ? T.accentSoft : "transparent",
              color: view === "portrait" ? T.accent : T.text2,
              fontWeight: view === "portrait" ? 600 : 400,
            }}
          >
            {view === "portrait" && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
            <span>🖼</span>
            <span>立绘素材</span>
          </button>
          {/* 生成参数：位于立绘素材下方 */}
          <button
            onClick={() => { setView("params"); if (currentStory) loadDebug(); }}
            style={{
              display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
              borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
              background: view === "params" ? T.accentSoft : "transparent",
              color: view === "params" ? T.accent : T.text2,
              fontWeight: view === "params" ? 600 : 400,
            }}
          >
            {view === "params" && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
            <span>🎛</span>
            <span>生成参数</span>
          </button>
          {/* 扩展 / 工程区 */}
          <div style={{ marginTop: 10, paddingTop: 10, borderTop: `1px solid ${T.lineSoft}`, display: "flex", flexDirection: "column", gap: 2 }}>
            <div style={{ fontSize: 10, color: T.text3, padding: "2px 10px 4px", letterSpacing: 0.8 }}>扩展 / 工程</div>
            <button
              onClick={() => { setView("plugins"); setGenProbe(null); loadEvals(); loadGenerators(); loadCloudConfig(); }}
              style={{
                display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
                borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                background: view === "plugins" ? T.accentSoft : "transparent",
                color: view === "plugins" ? T.accent : T.text2,
                fontWeight: view === "plugins" ? 600 : 400,
              }}
            >
              {view === "plugins" && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
              <span>🧩</span>
              <span>插件 / 扩展</span>
            </button>
            <button
              onClick={() => { setView("compliance"); loadComplianceAck(); }}
              style={{
                display: "flex", alignItems: "center", gap: 10, padding: "9px 10px",
                borderRadius: 8, border: "none", cursor: "pointer", fontSize: 13, textAlign: "left", position: "relative",
                background: view === "compliance" ? T.accentSoft : "transparent",
                color: view === "compliance" ? T.accent : T.text2,
                fontWeight: view === "compliance" ? 600 : 400,
              }}
            >
              {view === "compliance" && <span style={{ position: "absolute", left: -8, top: 8, bottom: 8, width: 3, background: T.accent, borderRadius: "0 3px 3px 0" }} />}
              <span>⚖</span>
              <span>合规与责任</span>
              {!complianceAck && <span title="尚未同意当前条款版本" style={{ marginLeft: "auto", fontSize: 9.5, padding: "1px 6px", borderRadius: 8, background: T.warn, color: "#231505", fontWeight: 700 }}>待读</span>}
            </button>
          </div>
        </div>
        <div style={{ padding: 8, borderTop: `1px solid ${T.lineSoft}` }}>
          <button
            onClick={() => setShowSettings(true)}
            style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 10px", borderRadius: 8, border: "none", background: "transparent", cursor: "pointer", fontSize: 13, color: T.text2, width: "100%" }}
          >
            ⚙️ 设置
          </button>
        </div>
      </div>
  );
}
