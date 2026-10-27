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
  /** 正在赦免的那一行（按钮置灰，防连点） */
  busyId: number | null;
  onExcuse: (id: number) => void;
};

export function DirectorLedgerView({ ledger, characterName, busyId, onExcuse }: Props) {
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
                <button
                  disabled={excused || busyId === r.id}
                  onClick={() => onExcuse(r.id)}
                  title="赦免：留个记录，但不再计入累计（她原谅了这次）"
                  style={{ flexShrink: 0, fontSize: 10.5, padding: "1px 7px", borderRadius: 6, cursor: excused ? "default" : "pointer", border: `1px solid ${T.lineSoft}`, background: "transparent", color: excused ? T.text3 : T.text2, fontFamily: "inherit" }}
                >赦免</button>
                <button
                  disabled
                  title="撤销暂时关闭（2026-10-27 用户定：默认不可点）—— 记错了先留着，等「赦免」流程做完再走"
                  style={{ flexShrink: 0, fontSize: 10.5, padding: "1px 7px", borderRadius: 6, cursor: "pointer", border: `1px solid ${T.lineSoft}`, background: "transparent", color: T.danger, fontFamily: "inherit" }}
                >撤销</button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
