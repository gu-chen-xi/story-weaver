// 「双池对比」预览（2026-11-13）。
//
// 参数页与开发者工具共用这一个组件：同一个故事、同一份数据，分别按**全局视角**和
// **角色视角**拼一遍 system 提示词，并排看差了什么、省了多少字。
//
// 边界（下面这句也写在界面上，别让人误会）：
//   只对比 **system 段**（排版规则 / 数据字典 / 世界 / 前史 / 角色 / 关系）；
//   尾部块（当前模式、数值与当下、关键记忆、示例对话、导演指令…）每轮现拼，不在这里比。
//
// 只读：不调模型、不写库（后端 `preview_prompt_pair`）。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";

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
  // 故事 / 扮演者变了就重算（只读、很便宜）
  useEffect(() => { void load(); }, [storyId, aiRole, myRole]);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { void load(); }, []);

  const box = (title: string, side: Side | undefined, tint: string) => (
    <div style={{ flex: 1, minWidth: 0, border: `1px solid ${T.line}`, borderRadius: 8, background: "#14171d" }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8, padding: "8px 10px", borderBottom: `1px solid ${T.line}`, color: tint, fontSize: 12, fontWeight: 650 }}>
        {title}
        <span style={{ marginLeft: "auto", color: T.text3, fontWeight: 400, fontSize: 11 }}>
          {side ? `${side.chars} 字 · ~${side.tokens} tok` : "—"}
        </span>
      </div>
      <pre style={{ margin: 0, padding: "10px 12px", maxHeight: 420, overflow: "auto", whiteSpace: "pre-wrap", fontSize: 11.5, lineHeight: 1.6, color: T.text2 }}>
        {side?.text || (busy ? "拼装中…" : "（没有内容）")}
      </pre>
    </div>
  );

  const saved = data ? data.global.chars - data.role.chars : 0;

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8, flexWrap: "wrap" }}>
        <span style={{ fontSize: 13, fontWeight: 650 }}>🔀 双池对比（全局视角 vs 角色视角）</span>
        <button onClick={() => void load()} disabled={busy || !storyId} style={{ height: 26, padding: "0 12px", borderRadius: 7, border: `1px solid ${T.line}`, background: T.panel2, color: T.text2, cursor: storyId ? "pointer" : "not-allowed", fontSize: 11.5, fontFamily: "inherit" }}>
          {busy ? "拼装中…" : "重新拼装"}
        </button>
        {data ? (
          <span style={{ fontSize: 11.5, color: saved >= 0 ? "#7ddf9c" : "#ffb37a" }}>
            角色视角比全局{saved >= 0 ? "少" : "多"} {Math.abs(saved)} 字
            {data.ai_role ? `（${data.ai_role}${data.my_role ? ` × ${data.my_role}` : ""}）` : ""}
          </span>
        ) : null}
        {!storyId ? <span style={{ fontSize: 11.5, color: T.text3 }}>先选一个故事</span> : null}
      </div>
      {err ? <div style={{ color: T.danger, fontSize: 12, marginBottom: 8 }}>❌ {err}</div> : null}
      <div style={{ display: "flex", gap: 10 }}>
        {box("全局视角（旁白 / 回退）", data?.global, "#9fb4d8")}
        {box("角色视角（对话模式）", data?.role, "#e3c07a")}
      </div>
      <p style={{ fontSize: 11, color: T.text3, marginTop: 8, lineHeight: 1.7 }}>
        只对比 system 段（排版规则 / 数据字典 / 世界 / 前史 / 角色 / 关系）；尾部块（当前模式、数值与当下、关键记忆、
        示例对话、导演指令）每轮现拼，不在这里比。只读：不调模型、不写库。
      </p>
    </div>
  );
}
