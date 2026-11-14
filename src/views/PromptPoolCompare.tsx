// 「双池对比」预览（2026-11-13 建立，2026-11-14 改为「两栏 + 差异」）。
//
// 布局（用户 2026-11-14 定）：
//   ① 说明卡：比什么、不比什么、只读；
//   ② **上半：两栏原文并排**（左＝全局视角，右＝角色视角）；
//   ③ **下半：只放"会变的那部分"** —— 行级 diff（红删绿加），一眼看改了什么。
//
// 两栏一模一样时说明**这个故事还没记录扮演者**（`play.ai_role` 为空），
// 这时会提示一句：先用对话模式生成一轮，或在输入台把「我扮演 / AI 扮演」选好。
//
// 只读：不调模型、不写库（后端 `preview_prompt_pair`）。

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { diffLines, diffStats } from "../lib/textDiff";

type Side = { text: string; chars: number; tokens: number };
type Pair = { ai_role: string; my_role: string; global: Side; role: Side };

export function PromptPoolCompare(props: { storyId?: number | null; aiRole?: string; myRole?: string }) {
  const { storyId, aiRole = "", myRole = "" } = props;
  const [data, setData] = useState<Pair | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const load = async () => {
    if (!storyId) return;
    setBusy(true);
    setErr("");
    try {
      setData(await invoke<Pair>("preview_prompt_pair", {
        storyId: String(storyId),
        aiRole: aiRole || null,
        myRole: myRole || null,
      }));
    } catch (e) {
      setErr(String(e));
      setData(null);
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => { void load(); }, [storyId, aiRole, myRole]);

  const lines = useMemo(() => (data ? diffLines(data.global.text, data.role.text) : []), [data]);
  const stat = useMemo(() => diffStats(lines), [lines]);
  // 只放"会变的部分"：连续 same 压成一条省略标记
  const changed = useMemo(() => {
    const out: { kind: string; text: string; skip?: number }[] = [];
    let skip = 0;
    for (const l of lines) {
      if (l.kind === "same") { skip++; continue; }
      if (skip > 0) { out.push({ kind: "skip", text: "", skip }); skip = 0; }
      out.push(l);
    }
    if (skip > 0) out.push({ kind: "skip", text: "", skip });
    return out;
  }, [lines]);

  const saved = data ? data.global.chars - data.role.chars : 0;

  return (
    <div>
      {/* ① 说明 */}
      <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: T.panel2, padding: "10px 12px", marginBottom: 12 }}>
        <div style={{ fontSize: 13, fontWeight: 650, marginBottom: 6 }}>🔀 双池对比（全局视角 vs 角色视角）</div>
        <p style={{ margin: 0, fontSize: 11.5, color: T.text3, lineHeight: 1.8 }}>
          只对比 <b>system 段</b>（排版规则 / 数据字典 / 世界 / 前史 / 角色 / 关系）；尾部块（当前模式、数值与当下、关键记忆、
          示例对话、导演指令）每轮现拼，不在这里比。<b>只读</b>：不调模型、不写库。
        </p>
      </div>

      {/* 操作行 */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8, flexWrap: "wrap" }}>
        <button onClick={() => void load()} disabled={busy || !storyId} style={{ height: 26, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: storyId ? "pointer" : "not-allowed", fontSize: 11.5, fontFamily: "inherit" }}>
          {busy ? "拼装中…" : "重新拼装"}
        </button>
        {data ? (
          <span style={{ fontSize: 11.5, color: T.text2 }}>
            {data.ai_role ? `${data.ai_role}${data.my_role ? ` × ${data.my_role}` : ""} ｜ ` : ""}
            全局 {data.global.chars} 字 → 角色 {data.role.chars} 字（{saved >= 0 ? "少" : "多"} {Math.abs(saved)} 字）
            <span style={{ color: "#7ddf9c" }}> ｜ +{stat.added}</span>
            <span style={{ color: "#ff8c85" }}> −{stat.removed}</span>
          </span>
        ) : !storyId ? (
          <span style={{ fontSize: 11.5, color: T.text3 }}>先选一个故事</span>
        ) : null}
      </div>

      {err ? <div style={{ color: T.danger, fontSize: 12, marginBottom: 8 }}>❌ {err}</div> : null}

      {data && !data.ai_role ? (
        <div style={{ border: `1px solid ${T.warn ?? T.line}`, background: "rgba(255,179,122,.08)", borderRadius: 8, padding: "8px 10px", marginBottom: 8, fontSize: 11.5, color: "#ffb37a", lineHeight: 1.7 }}>
          ⚠ 这个故事还没记录扮演者（`play.ai_role` 为空），所以两栏一模一样、差异为空。
          先在输入台的「对话」模式下发一轮（或在「我扮演 / AI 扮演」里选好人），再点「重新拼装」。
        </div>
      ) : null}

      {/* ② 上半：两栏原文 */}
      <div style={{ display: "flex", gap: 10 }}>
        {[
          ["全局视角", data?.global, "#9fb4d8"],
          ["角色视角", data?.role, "#e3c07a"],
        ].map(([title, side, tint]) => (
          <div key={title as string} style={{ flex: 1, minWidth: 0, border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d" }}>
            <div style={{ padding: "8px 10px", borderBottom: `1px solid ${T.line}`, color: tint as string, fontSize: 12, fontWeight: 650 }}>
              {title as string}
              <span style={{ marginLeft: 10, color: T.text3, fontWeight: 400, fontSize: 11 }}>
                {side ? `${(side as Side).chars} 字 · ~${(side as Side).tokens} tok` : "—"}
              </span>
            </div>
            <pre style={{ margin: 0, padding: "10px 12px", maxHeight: 320, overflow: "auto", whiteSpace: "pre-wrap", fontSize: 11.5, lineHeight: 1.6, color: T.text2 }}>
              {side ? (side as Side).text : busy ? "拼装中…" : "（没有内容）"}
            </pre>
          </div>
        ))}
      </div>

      {/* ③ 下半：只有会变的部分 */}
      <div style={{ marginTop: 12, border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d" }}>
        <div style={{ padding: "8px 10px", borderBottom: `1px solid ${T.line}`, fontSize: 12, fontWeight: 650, color: T.text2 }}>
          差异（只列会变的部分）
          <span style={{ marginLeft: 10, fontWeight: 400, fontSize: 11, color: T.text3 }}>
            +{stat.added} 行 / −{stat.removed} 行{saved !== 0 ? ` · 少 ${Math.abs(saved)} 字` : ""}
          </span>
        </div>
        <pre style={{ margin: 0, padding: "8px 0", maxHeight: 360, overflow: "auto", fontSize: 11.5, lineHeight: 1.6, whiteSpace: "pre-wrap", wordBreak: "break-all" }}>
          {busy ? (
            <div style={{ padding: "0 12px", color: T.text3 }}>拼装中…</div>
          ) : !data ? (
            <div style={{ padding: "0 12px", color: T.text3 }}>（没有内容）</div>
          ) : stat.added + stat.removed === 0 ? (
            <div style={{ padding: "0 12px", color: T.text3 }}>两栏完全一样 —— 这一轮没有"会变的部分"。</div>
          ) : (
            changed.map((l, i) =>
              l.kind === "skip" ? (
                <div key={i} style={{ padding: "0 12px", color: T.text3, opacity: 0.6 }}>⋯ 省略 {l.skip} 行相同的内容</div>
              ) : (
                <div
                  key={i}
                  style={{
                    padding: "0 12px",
                    color: l.kind === "add" ? "#8fe0a8" : "#ff9d96",
                    background: l.kind === "add" ? "rgba(55,201,120,.10)" : "rgba(240,85,77,.10)",
                  }}
                >
                  {(l.kind === "add" ? "+ " : "− ") + l.text}
                </div>
              ),
            )
          )}
        </pre>
      </div>
    </div>
  );
}
