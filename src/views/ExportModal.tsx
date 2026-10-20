// 导出粒度弹窗（从 App.tsx 拆分，2026-10-20）。
//
// 拆分方式：**JSX 原样搬**（仅 3 处必要替换：关窗回调、导出回调、节点列表改走 props），
// 连带把原来散在 App 里的 5 个导出草稿 state 一并搬进来 —— 这样"加一个导出选项"
// 不用再改主界面。
//
// 导出动作本身仍在 App（`onExport`）：它要用 currentStory / node / invoke，
// 那属于应用级上下文，不该塞进弹窗。

import { useState } from "react";
import { T } from "../theme";
import type { TimelineNodeDto } from "../types";

export type ExportScope = "all" | "current" | "range";
export type ExportMode = "all" | "narr" | "chat";
export type ExportFmt = "txt" | "md" | "html";

export type ExportOptions = {
  scope: ExportScope;
  from: number;
  to: number;
  mode: ExportMode;
  fmt: ExportFmt;
};

export type ExportModalProps = {
  /** 可选节点（用于"起止区间"的两个下拉） */
  days: TimelineNodeDto[];
  /** "第 3 天" → 3 */
  dayNoOf: (n: TimelineNodeDto) => number;
  onClose: () => void;
  onExport: (o: ExportOptions) => void | Promise<void>;
};

export function ExportModal({ days, dayNoOf, onClose, onExport }: ExportModalProps) {
  // 导出草稿（原在 App.tsx，初值与行为不变）
  const [expScope, setExpScope] = useState<ExportScope>("all");
  const [expFrom, setExpFrom] = useState(1);
  const [expTo, setExpTo] = useState(1);
  const [expMode, setExpMode] = useState<ExportMode>("all");
  const [expFmt, setExpFmt] = useState<ExportFmt>("txt");

  const exportNow = () => void onExport({ scope: expScope, from: expFrom, to: expTo, mode: expMode, fmt: expFmt });

  return (
    <div style={{ position: "fixed", inset: 0, background: "rgba(8,9,12,.62)", backdropFilter: "blur(3px)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
      <div style={{ width: 460, maxWidth: "90vw", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 14, padding: 20, boxSizing: "border-box" }}>
        <div style={{ display: "flex", alignItems: "center", marginBottom: 16 }}>
          <h3 style={{ margin: 0, fontSize: 15 }}>📥 导出剧情（TXT）</h3>
          <button onClick={onClose} style={{ marginLeft: "auto", fontSize: 14, padding: "4px 10px", cursor: "pointer", color: T.text2 }}>✕</button>
        </div>
        <div style={{ marginBottom: 14 }}>
          <div style={{ fontSize: 12, color: T.text3, marginBottom: 6 }}>节点范围</div>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            {([["all", "全部节点"], ["current", "当前节点"], ["range", "起止区间"]] as const).map(([v, l]) => (
              <button key={v} onClick={() => setExpScope(v)} style={{ padding: "6px 14px", borderRadius: 7, cursor: "pointer", fontSize: 12.5, border: `1px solid ${T.line}`, background: expScope === v ? T.accentSoft : "#191c22", color: expScope === v ? T.accent : T.text2, fontFamily: "inherit" }}>{l}</button>
            ))}
          </div>
          {expScope === "range" && (
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 8, flexWrap: "wrap" }}>
              <select value={expFrom} onChange={(e) => setExpFrom(Number(e.target.value))} style={{ padding: 6, fontSize: 12.5 }}>{days.map((n) => <option key={n.id} value={dayNoOf(n)}>{n.date_label}</option>)}</select>
              <span style={{ fontSize: 12, color: T.text3 }}>~</span>
              <select value={expTo} onChange={(e) => setExpTo(Number(e.target.value))} style={{ padding: 6, fontSize: 12.5 }}>{days.map((n) => <option key={n.id} value={dayNoOf(n)}>{n.date_label}</option>)}</select>
              <span style={{ fontSize: 11, color: T.text3 }}>（不含插曲 / 回忆）</span>
            </div>
          )}
        </div>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 12, color: T.text3, marginBottom: 6 }}>内容模式（用户输入始终保留）</div>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            {([["all", "全部"], ["narr", "仅旁白（AI）"], ["chat", "仅对话（AI）"]] as const).map(([v, l]) => (
              <button key={v} onClick={() => setExpMode(v)} style={{ padding: "6px 14px", borderRadius: 7, cursor: "pointer", fontSize: 12.5, border: `1px solid ${T.line}`, background: expMode === v ? T.accentSoft : "#191c22", color: expMode === v ? T.accent : T.text2, fontFamily: "inherit" }}>{l}</button>
            ))}
          </div>
        </div>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 12, color: T.text3, marginBottom: 6 }}>导出格式</div>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            {([["txt", "TXT 存档"], ["md", "MD 可编辑"], ["html", "HTML 阅读/打印"]] as const).map(([v, l]) => (
              <button key={v} onClick={() => setExpFmt(v)} style={{ padding: "6px 14px", borderRadius: 7, cursor: "pointer", fontSize: 12.5, border: `1px solid ${T.line}`, background: expFmt === v ? T.accentSoft : "#191c22", color: expFmt === v ? T.accent : T.text2, fontFamily: "inherit" }}>{l}</button>
            ))}
          </div>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 10, justifyContent: "flex-end", borderTop: `1px solid ${T.lineSoft}`, paddingTop: 14 }}>
          <span style={{ fontSize: 11, color: T.text3 }}>输出位置：exports\story · 文件名带时间戳</span>
          <button onClick={exportNow} style={{ padding: "8px 22px", border: "none", borderRadius: 8, background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontSize: 13, fontWeight: 650, cursor: "pointer", fontFamily: "inherit" }}>导出</button>
        </div>
      </div>
    </div>
  );
}
