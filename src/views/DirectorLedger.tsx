// 「导演账本」（2026-10-27）：导演指令的代价记录 —— 最近 N 条 + 累计，可逐条撤销/赦免。
//
// 为什么单独一个组件：StoryInput 已经 450 多行（JSX 为主），账本的表格与两个按钮
// 放进去会把那个文件推向 500 行以上；而账本本身没有业务逻辑，纯展示 + 两个回调。

import { T } from "../theme";
import type { DirectorLedger } from "../types";

type Props = {
  ledger: DirectorLedger | null;
  /** AI 扮演者（账按角色分，标题要写清是谁的账） */
  characterName: string;
  /** 正在请求/确认赦免的那一行（按钮置灰，防连点） */
  busyId: number | null;
  /** 发一次"请求她原谅"（弹窗确认 + 可补一句话） */
  onRequestPardon: (id: number) => void;
  /** 她同意之后才能真正赦免 */
  onApplyPardon: (id: number) => void;
  /** 撤销：只对已赦免的账开放（删掉这一行） */
  onUndo: (id: number) => void;
};

export function DirectorLedgerView({ ledger, characterName, busyId, onRequestPardon, onApplyPardon, onUndo }: Props) {
  const who = characterName && characterName !== "自由" ? characterName : "（自由身份）";
  const total = ledger?.total ?? 0;
  const limit = ledger?.limit ?? 0;
  const pct = limit > 0 ? Math.min(100, Math.round((total / limit) * 100)) : 0;
  const full = limit > 0 && total >= limit;
  const hintOf = (kind: string) => ledger?.kinds.find((k) => k.name === kind)?.hint ?? "";

  return (
    <div style={{ width: "100%", marginTop: 2, border: `1px solid ${full ? "rgba(255,107,107,.5)" : T.line}`, background: "#15181e", borderRadius: 9, padding: "8px 10px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <span style={{ fontSize: 11.5, color: full ? T.danger : T.accent, fontWeight: 600 }}>📒 导演账本 · {who}</span>
        <span style={{ fontSize: 11, color: full ? T.danger : T.text2 }}>累计 {total} / {limit}</span>
        <span style={{ width: 90, height: 5, borderRadius: 3, background: "#12141a", border: `1px solid ${T.lineSoft}`, overflow: "hidden", display: "inline-block" }}>
          <span style={{ display: "block", height: "100%", width: `${pct}%`, background: full ? T.danger : T.accent }} />
        </span>
        <span style={{ marginLeft: "auto", fontSize: 10.5, color: T.text3 }}>
          {full ? "已到上限：导演指令被锁，撤销或赦免几笔就解锁" : "分值由软件按类型算，AI 只写类型和理由"}
        </span>
      </div>

      {!ledger || ledger.rows.length === 0 ? (
        <div style={{ marginTop: 6, fontSize: 11, color: T.text3, lineHeight: 1.7 }}>
          还没记过账。用一次导演指令，AI 会在回复最后一行写「〔导演代价〕类型：理由」，软件按类型自己算分。
        </div>
      ) : (
        <div style={{ marginTop: 6, display: "flex", flexDirection: "column", gap: 4 }}>
          {ledger.rows.map((r) => {
            const excused = r.excused === 1;
            return (
              <div key={r.id} style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 11, opacity: excused ? 0.5 : 1 }}>
                <span style={{ width: 58, color: T.text3, flexShrink: 0 }}>{r.day_label || "—"}</span>
                <span title={hintOf(r.kind)} style={{ flexShrink: 0, padding: "0 6px", borderRadius: 8, border: `1px solid ${T.lineSoft}`, color: T.accent, background: T.accentSoft }}>{r.kind}</span>
                <span title={r.reason} style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: T.text2 }}>
                  {r.reason}
                  {r.source === "rule" && <span style={{ marginLeft: 5, fontSize: 10, color: T.text3 }}>（推断）</span>}
                </span>
                <span style={{ flexShrink: 0, color: excused ? T.text3 : T.warn, fontWeight: 600 }}>
                  {excused ? "已赦免" : `+${r.score}`}
                </span>
                {r.rel_delta > 0 && (
                  <span title="这笔账同时自动扣掉的关系强度（白给 −10 / 心软 −5）"
                    style={{ flexShrink: 0, fontSize: 10, color: T.danger, border: `1px solid ${T.lineSoft}`, borderRadius: 6, padding: "0 5px" }}>
                    关系 −{r.rel_delta}
                  </span>
                )}
                {(() => {
                  // 赦免是三段式：请求 → 她同意 → 确认。按钮文案跟着状态走（2026-10-27 用户定）。
                  const agreed = r.pardon_verdict === "agreed";
                  const waiting = r.pardon_verdict === "pending" || r.pardon_verdict === "unclear";
                  const usedUp = r.pardon_count >= 3 && !agreed;
                  const label = excused ? "已赦免"
                    : agreed ? "确认赦免"
                    : waiting ? "等她回答"
                    : usedUp ? "已问满 3 次"
                    : r.pardon_count > 0 ? `请求赦免 ${r.pardon_count}/3` : "请求赦免";
                  const canClick = !excused && !waiting && !usedUp && busyId !== r.id;
                  return (
                    <button
                      disabled={!canClick}
                      onClick={() => (agreed ? onApplyPardon(r.id) : onRequestPardon(r.id))}
                      title={excused ? "这笔账已经赦免过了"
                        : agreed ? "她同意了：点这里正式赦免（免分数 + 关系回补一半）"
                        : waiting ? "已经问过了，等她回答"
                        : usedUp ? "这条消息已经问满 3 次，不能再问"
                        : "请求赦免：先问过她，同意了才算"}
                      style={{ flexShrink: 0, fontSize: 10.5, padding: "1px 7px", borderRadius: 6,
                        cursor: canClick ? "pointer" : "default",
                        border: `1px solid ${agreed ? T.ok : T.lineSoft}`, background: "transparent",
                        color: !canClick ? T.text3 : agreed ? T.ok : T.text2, fontFamily: "inherit" }}
                    >{label}</button>
                  );
                })()}
                <button
                  disabled={!excused}
                  onClick={() => onUndo(r.id)}
                  title={excused ? "撤销：把这笔已赦免的账删掉（只允许删已赦免的账）" : "撤销只对「已赦免」的账开放"}
                  style={{ flexShrink: 0, fontSize: 10.5, padding: "1px 7px", borderRadius: 6, cursor: excused ? "pointer" : "default", border: `1px solid ${T.lineSoft}`, background: "transparent", color: excused ? T.danger : T.text3, fontFamily: "inherit" }}
                >撤销</button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
