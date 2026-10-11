// 插件注册表与通用插件卡（2026-10-11 建立，为第四批 ⑥ 立绘生成等插件铺路）
//
// 设计目标：**加一个插件 = 往注册表里加一条描述 + 提供一个面板**，不用再写卡片布局、开关逻辑、状态徽标。
//
// 一个插件由三部分组成：
//   1. descriptor（元信息 + 能力声明）
//   2. state（当前状态：开关、徽标、面板是否展开）
//   3. panel（该插件自己的界面；由宿主负责渲染在卡片下方）
//
// 能力声明（capabilities）是这套东西的关键 —— 它让"插件能做什么"变成**数据**而不是散落在各个 if 里：
//   - canToggle      是否有总开关（如评估模型的「每轮自动评估」）
//   - canRunManually 是否能在界面上手动跑一次（如「评估本节点最后一条回复」）
//   - canViewPanel   是否有可展开的结果面板
//   - needsStory     是否必须先进入一个故事才能用
// 这些决定了卡片上该出现哪些控件、开关不可用时给什么提示。

import type { CSSProperties, ReactNode } from "react";

/** 插件状态：规划中（不可用）/ 已实装（可用） */
export type PluginStatus = "planned" | "ready";

/** 插件能做什么 */
export type PluginCapabilities = {
  canToggle: boolean;      // 有总开关
  canRunManually: boolean; // 可手动执行
  canViewPanel: boolean;   // 有可展开面板
  needsStory: boolean;     // 需要先进入故事
};

/** 插件元信息 */
export type PluginDescriptor = {
  id: string;              // 稳定标识，同时用作配置键前缀：plugin.<id>.enabled
  icon: string;
  name: string;
  desc: string;
  status: PluginStatus;
  /** 能力声明（可部分给出，缺省按 false） */
  caps?: Partial<PluginCapabilities>;
  /** 开关文案（有总开关时用） */
  toggleLabel?: string;
  /** 开关旁的代价说明（如"会多消耗一次 API 调用"） */
  toggleHint?: string;
  /** 规划中插件的占位说明（status = planned 时显示） */
  plannedNote?: string;
};

/** 运行期状态（由宿主维护） */
export type PluginState = {
  /** 总开关是否打开 */
  enabled: boolean;
  /** 面板是否展开 */
  panelOpen: boolean;
  /** 状态徽标（如"已开启 · 每轮自动评估"） */
  badges: { text: string; tone: "ok" | "muted" | "warn" }[];
  /** 正在执行（禁用按钮并显示忙态文案） */
  busy?: boolean;
  /** 手动执行按钮的文案 */
  runLabel?: string;
  /** 手动执行按钮的提示（如"与上面的自动开关无关"） */
  runHint?: string;
  /** 面板内容 */
  panel?: ReactNode;
};

const CAPS_DEFAULT: PluginCapabilities = {
  canToggle: false,
  canRunManually: false,
  canViewPanel: false,
  needsStory: false,
};

export function pluginCaps(d: PluginDescriptor): PluginCapabilities {
  return { ...CAPS_DEFAULT, ...(d.caps ?? {}) };
}

/** 插件开关的配置键（统一命名，便于以后做"插件管理"页面或迁移） */
export const pluginConfigKey = (id: string) => `plugin.${id}.enabled`;

/** 最少依赖的插件卡；样式由宿主传入的 tokens 决定 */
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
// 各插件自己的界面，由宿主在 PluginCard 的 panel prop 里渲染。
// 第一个落地的是评估模型（ROADMAP 第三批 ④）。

export type EvalDimScore = { key: string; score: number; reason: string; quote: string };
export type EvalRecord = {
  message_id: number; node_id: number; model: string;
  dims: EvalDimScore[]; total: number; summary: string; advice: string; created_at: string;
};

export type EvalPanelProps = {
  T: PluginCardTokens;
  /** 维度 key → 中文名 */
  dimLabel: Record<string, string>;
  records: EvalRecord[];
  busy: boolean;
  /** 是否可手动评估（有故事且选中了节点） */
  canRun: boolean;
  noStoryHint: string;
  onRun: () => void;
  onRefresh: () => void;
};

/** 评估结果面板：趋势条 + 逐条明细（四维 + 证据引文 + 总评 + 建议） */
export function EvalPanel({ T, dimLabel, records, busy, canRun, noStoryHint, onRun, onRefresh }: EvalPanelProps) {
  const scoreColor = (v: number) => (v >= 4.5 ? T.ok : v >= 3.5 ? T.accent : v >= 2.5 ? T.warn : T.danger);
  return (
    <div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 10, flexWrap: "wrap" }}>
        <button onClick={onRun} disabled={busy || !canRun}>{busy ? "评估中…" : "评估本节点最后一条回复"}</button>
        <button onClick={onRefresh}>刷新</button>
        <span style={{ fontSize: 11, color: T.text3 }}>{canRun ? "手动评估随时可用，与上面的自动开关无关" : noStoryHint}</span>
      </div>

      {records.length === 0 ? (
        <p style={{ color: T.text3, fontSize: 12.5 }}>还没有评估记录。点「评估本节点最后一条回复」评一条（会调一次 API）。</p>
      ) : (
        <>
          {/* 趋势条：左旧右新 */}
          <div style={{ display: "flex", alignItems: "flex-end", gap: 4, height: 46, marginBottom: 10 }}>
            {[...records].reverse().map((e) => (
              <div key={`${e.message_id}-${e.created_at}`} title={`消息 #${e.message_id} · 总分 ${e.total}`}
                style={{ width: 16, height: Math.max(6, (e.total / 5) * 44), background: scoreColor(e.total), borderRadius: 3, opacity: 0.85 }} />
            ))}
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {records.map((e) => (
              <div key={`${e.message_id}-${e.created_at}`} style={{ border: `1px solid ${T.line}`, borderRadius: 8, padding: "10px 12px" }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6, flexWrap: "wrap", gap: 6 }}>
                  <div style={{ fontSize: 12.5, color: T.text2 }}>
                    消息 #{e.message_id} · 节点 {e.node_id} · <span style={{ color: T.text3 }}>{e.created_at}</span>
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <span style={{ fontSize: 10.5, color: T.text3 }}>{e.model}</span>
                    <span style={{ fontSize: 14, fontWeight: 700, color: scoreColor(e.total) }}>{e.total} / 5</span>
                  </div>
                </div>

                <div style={{ display: "flex", gap: 6, marginBottom: 7, flexWrap: "wrap" }}>
                  {e.dims.map((d) => (
                    <span key={d.key} title={d.reason}
                      style={{
                        fontSize: 10.5, padding: "2px 7px", borderRadius: 6, border: `1px solid ${T.lineSoft}`,
                        color: d.score >= 4 ? T.ok : d.score === 3 ? T.warn : T.danger,
                      }}>
                      {dimLabel[d.key] ?? d.key} {d.score}
                    </span>
                  ))}
                </div>

                <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.75 }}>
                  {e.dims.map((d) => (
                    <div key={d.key} style={{ marginBottom: 2 }}>
                      <b style={{ color: T.text1 }}>{dimLabel[d.key] ?? d.key}</b>：{d.reason}
                      {d.quote && (
                        <span style={{ color: T.text3 }}> —— 证据：<i>{d.quote.length > 60 ? d.quote.slice(0, 60) + "…" : d.quote}</i></span>
                      )}
                    </div>
                  ))}
                </div>
                {e.summary && <div style={{ fontSize: 12, color: T.text2, marginTop: 6 }}>总评：{e.summary}</div>}
                {e.advice && <div style={{ fontSize: 12, color: T.accent, marginTop: 3 }}>建议：{e.advice}</div>}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
