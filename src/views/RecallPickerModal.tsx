// 「🔍 回顾哪一天」弹窗（2026-11-08 从 App.tsx 抽出，第四刀）。
//
// 为什么单独一个文件：它是**一整块自洽的 UI**（列表 + 多选 + 标签 + 置灰规则），
// 和 App 的业务状态只通过几个回调相连；塞在 App 里会一直占着 60 多行、又不好改样式。
//
// 交互规则（用户 2026-11-02 定）：
//   · **可多选**：连点几天 = 范围，按 node_id 升序保存；
//   · 最近三天 / 还没发生的天**置灰**（点了只提示，不选中）；
//   · 回顾注入的是**当天原始对话**，不是日摘要 —— 所以「没有日摘要」只是提示，不影响可回顾。

import { T } from "../theme";

export type RecallDay = {
  node_id: number;
  date_label: string;
  day: number | null;
  msg_count: number;
  preview: string;
  has_summary: boolean;
  eligible: boolean;
  tags?: string[];
};

export type RecallPickerModalProps = {
  days: RecallDay[];
  picked: number[];
  /** 点某天：选中 / 取消（置灰的天不会进这里，见 onBlocked） */
  onToggle: (nodeId: number) => void;
  /** 点了置灰的天（最近三天 / 还没发生）→ 由调用方给提示 */
  onBlocked: (label: string) => void;
  onClose: () => void;
};

export function RecallPickerModal({ days, picked, onToggle, onBlocked, onClose }: RecallPickerModalProps) {
  return (
    <div onClick={onClose}
      style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center",
        justifyContent: "center", zIndex: 80 }}>
      <div onClick={(e) => e.stopPropagation()}
        style={{ width: 560, maxHeight: "72vh", display: "flex", flexDirection: "column", border: `1px solid ${T.line}`,
          borderRadius: 12, background: T.panel2, overflow: "hidden" }}>
        <div style={{ padding: "14px 16px", borderBottom: `1px solid ${T.line}`, display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 14, fontWeight: 650 }}>🔍 回顾哪一天</span>
          <span style={{ fontSize: 11.5, color: T.text3 }}>会把那天的原始对话拼进本次提示词</span>
          <button onClick={onClose}
            style={{ marginLeft: "auto", background: "none", border: "none", color: T.text3, cursor: "pointer", fontSize: 16 }}>
            ✕
          </button>
        </div>
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "10px 12px" }}>
          {days.length === 0 ? (
            <p style={{ fontSize: 12.5, color: T.text3 }}>
              还没有可回顾的天 —— 需要**那天有聊过内容**（回顾注入的是原始对话，不依赖摘要）。
            </p>
          ) : days.map((d) => (
            <button key={d.node_id}
              onClick={() => (d.eligible ? onToggle(d.node_id) : onBlocked(d.date_label))}
              style={{ display: "block", width: "100%", textAlign: "left", marginBottom: 6, padding: "9px 11px",
                borderRadius: 9, cursor: d.eligible ? "pointer" : "not-allowed", fontFamily: "inherit",
                border: `1px solid ${picked.includes(d.node_id) ? T.accent : T.line}`,
                background: picked.includes(d.node_id) ? T.accentSoft : "transparent",
                opacity: d.eligible ? 1 : 0.5 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span style={{ fontSize: 12.5, fontWeight: 600, color: picked.includes(d.node_id) ? T.accent : T.text1 }}>
                  {picked.includes(d.node_id) ? "✓ " : ""}{d.date_label}
                </span>
                <span style={{ fontSize: 10.5, color: T.text3 }}>{d.msg_count} 条对话</span>
                {!d.has_summary && (
                  <span title="这天还没有日摘要 —— 手动回顾照样可用，只是自动检索找不到它"
                    style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8, border: `1px dashed ${T.line}`,
                      color: T.text3 }}>
                    无摘要
                  </span>
                )}
              </div>
              {d.tags && d.tags.length > 0 && (
                <div style={{ display: "flex", gap: 4, flexWrap: "wrap", marginTop: 5 }}>
                  {d.tags.slice(0, 6).map((tg) => (
                    <span key={tg} title="这一天的检索标签：手打标签 + 从日摘要派生"
                      style={{ fontSize: 9.5, padding: "1px 6px", borderRadius: 8,
                        border: `1px solid ${T.accent}44`, background: `${T.accent}14`, color: T.accent }}>
                      #{tg}
                    </span>
                  ))}
                  {d.tags.length > 6 && (
                    <span style={{ fontSize: 9.5, padding: "1px 5px", color: T.text3 }}>+{d.tags.length - 6}</span>
                  )}
                </div>
              )}
              <div style={{ fontSize: 11.5, color: T.text3, marginTop: 3, lineHeight: 1.6 }}>{d.preview}…</div>
            </button>
          ))}
        </div>
        <div style={{ padding: "10px 16px", borderTop: `1px solid ${T.line}`, fontSize: 11, color: T.text3, lineHeight: 1.7 }}>
          **可以连点几天 = 范围**（最近三天和还没发生的天置灰，不需要回顾）。回顾带进去的是**当天聊过的原始对话**（不是压缩后的摘要）。
          选了之后只在**下一次发送**生效。不选的话，软件会自己按你输入里的「第一次 / 那次 / 第 N 天」等词判断要不要回顾。
        </div>
      </div>
    </div>
  );
}
