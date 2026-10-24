// 「评估模型」插件的面板（v3.5.7，从 src/plugins.tsx 抽出）。
//
// 这个文件只属于 evaluate 插件 —— 通用契约在 `plugins/types.ts`，
// 通用卡片在 `plugins/PluginCard.tsx`。

import type { PluginCardTokens } from "../../PluginCard";

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


// ===== 立绘生成面板（ROADMAP 第四批 ⑥ · P1 骨架） =====
//
// 关键设计（见 docs/design/立绘系统.md）：
//   · 通道注册表由后端下发（generator_list），**如实报告**每个通道是否已实装
//   · 未实装的通道在界面上**标灰并写清原因**，不假装可用
//   · 未配置/连不上时只显示黄字提示 + 手工导入指引，**绝不阻塞**素材库与角色管理
//   · 生成结果一律经 portrait_import 入库（本面板不直接写任何文件/数据库）

