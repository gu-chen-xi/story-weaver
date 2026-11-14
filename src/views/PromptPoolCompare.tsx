// 「双池对比」预览（2026-11-13 建立，2026-11-14 改成 tab + diff）。
//
// 参数页的「🔀 双池对比」tab 与开发者工具共用这一个组件：
//   ① 上半部分：这块**是什么 / 比什么 / 不比什么**（边界写在最前面，免得误会）；
//   ② 下半部分：**到底改了啥** —— 全局视角 → 角色视角的行级 diff（红删绿加）。
//
// 只读：不调模型、不写库（后端 `preview_prompt_pair`）。

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";
import { diffLines, diffStats } from "../lib/textDiff";

type Side = { text: string; chars: number; tokens: number };
type Pair = { ai_role: string; my_role: string; global: Side; role: Side };

export function PromptPoolCompare(props: { storyId?: number | null }) {
  const { storyId } = props;
  const [data, setData] = useState<Pair | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [showRaw, setShowRaw] = useState(false);

  const load = async () => {
    if (!storyId) return;
    setBusy(true);
    setErr("");
    try {
      setData(await invoke<Pair>("preview_prompt_pair", { storyId: String(storyId), aiRole: null, myRole: null }));
    } catch (e) {
      setErr(String(e));
      setData(null);
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => { void load(); }, [storyId]);

  const lines = useMemo(
    () => (data ? diffLines(data.global.text, data.role.text) : []),
    [data],
  );
  const stat = useMemo(() => diffStats(lines), [lines]);
  const saved = data ? data.global.chars - data.role.chars : 0;

  return (
    <div>
      {/* ① 说明（放最上面） */}
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
        <button onClick={() => setShowRaw((v) => !v)} disabled={!data} style={{ height: 26, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: "transparent", color: showRaw ? T.accent : T.text3, cursor: data ? "pointer" : "not-allowed", fontSize: 11.5, fontFamily: "inherit" }}>
          {showRaw ? "只看差异" : "看两栏原文"}
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

      {/* ② 改了啥 */}
      {showRaw && data ? (
        <div style={{ display: "flex", gap: 10 }}>
          {[
            ["全局视角", data.global, "#9fb4d8"],
            ["角色视角", data.role, "#e3c07a"],
          ].map(([title, side, tint]) => (
            <div key={title as string} style={{ flex: 1, minWidth: 0, border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d" }}>
              <div style={{ padding: "8px 10px", borderBottom: `1px solid ${T.line}`, color: tint as string, fontSize: 12, fontWeight: 650 }}>
                {title as string}
                <span style={{ marginLeft: 10, color: T.text3, fontWeight: 400, fontSize: 11 }}>
                  {(side as Side).chars} 字 · ~{(side as Side).tokens} tok
                </span>
              </div>
              <pre style={{ margin: 0, padding: "10px 12px", maxHeight: 460, overflow: "auto", whiteSpace: "pre-wrap", fontSize: 11.5, lineHeight: 1.6, color: T.text2 }}>
                {(side as Side).text}
              </pre>
            </div>
          ))}
        </div>
      ) : (
        <div style={{ border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d", maxHeight: 520, overflow: "auto" }}>
          {busy ? (
            <div style={{ padding: 12, fontSize: 12, color: T.text3 }}>拼装中…</div>
          ) : !data ? (
            <div style={{ padding: 12, fontSize: 12, color: T.text3 }}>（没有内容）</div>
          ) : (
            <pre style={{ margin: 0, padding: "8px 0", fontSize: 11.5, lineHeight: 1.6, whiteSpace: "pre-wrap", wordBreak: "break-all" }}>
              {lines.map((l, i) => (
                <div
                  key={i}
                  style={{
                    padding: "0 12px",
                    color: l.kind === "add" ? "#8fe0a8" : l.kind === "del" ? "#ff9d96" : T.text3,
                    background: l.kind === "add" ? "rgba(55,201,120,.10)" : l.kind === "del" ? "rgba(240,85,77,.10)" : "transparent",
                  }}
                >
                  {(l.kind === "add" ? "+ " : l.kind === "del" ? "− " : "  ") + l.text}
                </div>
              ))}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
