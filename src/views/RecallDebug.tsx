// 「🔍 回顾试算」：输入一句话，看**会带哪几天、为什么、各得多少分**（C3）。
//
// 为什么需要：回顾打分的权重是拍出来的（标签词 20 / 实体 12 / 手打标签 25 …），
// 合不合适只有实际看结果才知道。以前只能靠"发一条消息 → 翻生成参数 → 猜为什么带了这天"，
// 现在在这里输入同一句话就能直接看到分数与理由，还能就地调权重。
//
// 全部纯本地：只调后端的 `recall_explain`（只算不注入），不花 API。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";

type ExplainRow = {
  day_label: string;
  day: number | null;
  score: number;
  picked: boolean;
  why: string[];
};

export type RecallWeights = {
  event: number;
  day_exact: number;
  hand_tag: number;
  tag_word: number;
  entity: number;
  name_weak: number;
  min_score: number;
  recency_max: number;
};

const LABELS: [keyof RecallWeights, string][] = [
  ["event", "首次事件"],
  ["day_exact", "指定某天"],
  ["hand_tag", "手打标签"],
  ["tag_word", "标签词"],
  ["entity", "实体名"],
  ["name_weak", "角色名弱"],
  ["min_score", "门槛"],
  ["recency_max", "接近度"],
];

export function RecallDebug(props: { storyId: string | null; nodeId: number | null }) {
  const { storyId, nodeId } = props;
  const [text, setText] = useState("");
  const [rows, setRows] = useState<ExplainRow[] | null>(null);
  const [w, setW] = useState<RecallWeights | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [showW, setShowW] = useState(false);

  useEffect(() => {
    void invoke<RecallWeights>("recall_get_weights").then(setW).catch(() => {});
  }, []);

  const run = async () => {
    if (!storyId || !text.trim()) return;
    setBusy(true);
    setMsg("");
    try {
      setRows(await invoke<ExplainRow[]>("recall_explain", {
        storyId, text: text.trim(), nodeId: nodeId ?? null,
      }));
    } catch (e) {
      setMsg("❌ 试算失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };

  const saveW = async (next: RecallWeights) => {
    setW(next);
    try {
      await invoke("recall_set_weights", { weights: next });
      setMsg("✅ 权重已保存（下次生成生效）");
      if (rows) await run();
    } catch (e) {
      setMsg("❌ 保存失败：" + String(e));
    }
  };

  const picked = (rows ?? []).filter((r) => r.picked).slice(0, 3);

  return (
    <div>
      <div style={{ fontSize: 10.5, color: T.text3, letterSpacing: 1.1, marginBottom: 8,
        display: "flex", alignItems: "center", gap: 6 }}>
        <span>🔍 回顾试算</span>
        <span style={{ marginLeft: "auto", cursor: "pointer" }} onClick={() => setShowW((v) => !v)}
          title="调整打分权重">⚙ 权重</span>
      </div>

      <div style={{ display: "flex", gap: 4, marginBottom: 6 }}>
        <input value={text} onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") void run(); }}
          placeholder="例：还记得那次牵手吗"
          style={{ flex: 1, minWidth: 0, padding: "4px 7px", borderRadius: 6, border: `1px solid ${T.line}`,
            background: "#191c22", color: T.text1, fontSize: 11, fontFamily: "inherit" }} />
        <button onClick={() => void run()} disabled={busy || !storyId || !text.trim()}
          style={{ padding: "4px 9px", borderRadius: 6, border: `1px solid ${T.line}`,
            background: busy ? T.panel3 : T.panel2, color: busy ? T.text3 : T.text1,
            fontSize: 11, cursor: busy ? "wait" : "pointer", fontFamily: "inherit" }}>
          {busy ? "…" : "试算"}
        </button>
      </div>

      {showW && w && (
        <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, padding: "7px 8px", marginBottom: 8,
          display: "grid", gridTemplateColumns: "1fr 1fr", gap: "4px 8px" }}>
          {LABELS.map(([k, label]) => (
            <label key={k} style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 10.5, color: T.text3 }}>
              <span style={{ width: 52 }}>{label}</span>
              <input type="number" value={w[k]}
                onChange={(e) => setW({ ...w, [k]: Number(e.target.value) || 0 })}
                onBlur={() => void saveW(w)}
                style={{ width: 48, padding: "2px 4px", borderRadius: 5, border: `1px solid ${T.line}`,
                  background: "#191c22", color: T.text1, fontSize: 10.5, fontFamily: "monospace", textAlign: "center" }} />
            </label>
          ))}
        </div>
      )}

      {msg && <div style={{ fontSize: 10.5, marginBottom: 6, color: msg.startsWith("❌") ? T.danger : T.text2 }}>{msg}</div>}

      {rows && rows.length === 0 && (
        <div style={{ fontSize: 11, color: T.text3 }}>没有可回顾的天（或这天是当前节点）。</div>
      )}

      {rows && rows.length > 0 && (
        <div style={{ fontSize: 10.5, color: T.text3, marginBottom: 5, lineHeight: 1.6 }}>
          会带出 <b style={{ color: T.accent }}>{Math.min(picked.length, 3)}</b> 天
          {picked.length > 0 && `（${picked.map((r) => r.day_label).join(" / ")}）`}
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
        {(rows ?? []).slice(0, 6).map((r) => (
          <div key={r.day_label} style={{ fontSize: 10.5, padding: "4px 7px", borderRadius: 6,
            border: `1px solid ${r.picked ? `${T.accent}55` : T.line}`,
            background: r.picked ? `${T.accent}10` : "transparent" }}>
            <div style={{ display: "flex", gap: 6 }}>
              <span style={{ color: r.picked ? T.accent : T.text3, fontWeight: r.picked ? 600 : 400 }}>
                {r.day_label}
              </span>
              <span style={{ color: T.text3, fontFamily: "monospace" }}>{r.score} 分</span>
              {!r.picked && r.score > 0 && <span style={{ color: T.text3, opacity: 0.7 }}>· 未达门槛</span>}
            </div>
            {r.why.length > 0 && (
              <div style={{ color: T.text3, marginTop: 2, lineHeight: 1.5 }}>{r.why.slice(0, 3).join(" · ")}</div>
            )}
          </div>
        ))}
      </div>

      {rows && rows.length > 6 && (
        <div style={{ fontSize: 10, color: T.text3, marginTop: 4 }}>（只显示前 6 天，按分数降序）</div>
      )}
    </div>
  );
}
