// 「开发者工具」页（v3.4，**只给作者用**）
//
// 三道门禁，缺一不可：
//   1. 入口在 App 里被 `import.meta.env.DEV` 挡住 —— release 包的前端里没有这个按钮；
//   2. 后端每条 dev 命令在 `debug_assertions = false` 时直接返回错误；
//   3. 后端每条命令先校验"目标故事标题以「测试」开头" —— 真实故事改不到。
//
// 它的用途不是"调模型"，而是**用伪造的 AI 输出撞真实的接收管线**：
// 摘要怎么落、评估 JSON 畸形了怎么兜、静默提案过不过白名单闸门。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { T } from "../theme";

type DevStory = { id: number; title: string; created_at: string };
type DevStatus = { dev_build: boolean; test_stories: DevStory[]; next_title: string; data_root: string };
type InjectResult = {
  dry_run: boolean;
  target: string;
  outcome: string;
  changes: string[];
  snapshot: number | null;
};

type Pipeline = "summary" | "eval" | "proposal";

const boxS = { border: `1px solid ${T.line}`, borderRadius: 12, background: T.panel, padding: 14 } as const;
const labelS = { display: "flex", flexDirection: "column", gap: 5, fontSize: 11.5, color: T.text3 } as const;
const inputS = { height: 32, padding: "0 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12.5, outline: "none" } as const;
const accentS = { height: 32, padding: "0 16px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", fontSize: 12.5, fontWeight: 650, cursor: "pointer" } as const;
const ghostS = { height: 32, padding: "0 14px", borderRadius: 8, border: `1px solid ${T.line}`, background: "transparent", color: T.text2, fontSize: 12.5, cursor: "pointer" } as const;
const taS = { padding: "9px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontSize: 12, fontFamily: "ui-monospace, Consolas, monospace", lineHeight: 1.6, resize: "vertical" as const, outline: "none" };

/// 各管线的"伪造响应"模板 —— 点一下就能载入，改几个字就能测边界
const TEMPLATES: Record<Pipeline, string> = {
  summary: "用户把显卡拿出来给大肥鱼看，理由是工作需要；对方盯着那块卡看了很久，嘴上还在嫌弃，最后伸手接过又停住。",
  eval: JSON.stringify(
    {
      dims: [
        { key: "in_char", score: 4, reason: "语气保持住了傲娇", quote: "都说了不许叫大肥鱼" },
        { key: "plot", score: 3, reason: "推进较慢", quote: "" },
      ],
      summary: "整体合格，节奏偏慢",
      advice: "可以多给一点动作细节",
    },
    null,
    2,
  ),
  proposal: JSON.stringify(
    {
      board: "characters",
      target_kind: "character",
      target_name: "大肥鱼",
      field: "profile",
      before: "",
      value: "（注入的伪造人设修改）",
    },
    null,
    2,
  ),
};

export function DevToolsPage() {
  const [tab, setTab] = useState<"overview" | "inject">("overview");
  const [status, setStatus] = useState<DevStatus | null>(null);
  const [storyId, setStoryId] = useState<number | null>(null);
  const [pipeline, setPipeline] = useState<Pipeline>("summary");
  const [nodeId, setNodeId] = useState("1");
  const [messageId, setMessageId] = useState("1");
  const [payload, setPayload] = useState(TEMPLATES.summary);
  const [result, setResult] = useState<InjectResult | null>(null);
  const [snaps, setSnaps] = useState<[number, string, number][]>([]);
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    try {
      const st = await invoke<DevStatus>("dev_status");
      setStatus(st);
      if (storyId === null && st.test_stories.length) setStoryId(st.test_stories[0].id);
    } catch (e) {
      setMsg("❌ " + String(e));
    }
  };

  useEffect(() => { void refresh(); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, []);

  useEffect(() => {
    if (storyId === null) return;
    invoke<[number, string, number][]>("dev_snapshots", { storyId })
      .then(setSnaps)
      .catch(() => setSnaps([]));
  }, [storyId, result]);

  const createStory = async () => {
    setBusy(true);
    try {
      const st = await invoke<DevStatus>("dev_create_test_story");
      setStatus(st);
      const created = st.test_stories[st.test_stories.length - 1];
      if (created) setStoryId(created.id);
      setMsg(`✅ 已新建「${created?.title ?? "测试故事"}」`);
    } catch (e) {
      setMsg("❌ " + String(e));
    } finally {
      setBusy(false);
    }
  };

  const run = async (dryRun: boolean) => {
    if (storyId === null) { setMsg("❌ 先选一个测试故事"); return; }
    setBusy(true);
    setResult(null);
    try {
      let r: InjectResult;
      if (pipeline === "summary") {
        r = await invoke<InjectResult>("dev_inject_summary", {
          storyId, nodeId: Number(nodeId) || 0, text: payload, dryRun,
        });
      } else if (pipeline === "eval") {
        r = await invoke<InjectResult>("dev_inject_eval", {
          storyId, messageId: Number(messageId) || 0, raw: payload, dryRun,
        });
      } else {
        r = await invoke<InjectResult>("dev_inject_proposal", { storyId, patchJson: payload, dryRun });
      }
      setResult(r);
      setMsg(dryRun ? "· 试运行完成（未写库）" : "✅ 已落地");
    } catch (e) {
      setMsg("❌ " + String(e));
    } finally {
      setBusy(false);
    }
  };

  const rollback = async (version: number) => {
    if (storyId === null) return;
    if (!confirm(`回滚到快照 v${version}？这会覆盖当前设定。`)) return;
    try {
      setMsg("✅ " + (await invoke<string>("dev_rollback", { storyId, version })));
    } catch (e) {
      setMsg("❌ " + String(e));
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 1080 }}>
      <div style={{ border: `1px solid ${T.danger}55`, background: "rgba(240,85,77,.08)", borderRadius: 10, padding: "9px 13px", fontSize: 11.5, color: "#ff8c85", lineHeight: 1.75 }}>
        <b>开发者工具</b>（仅开发构建可见）· 只允许操作标题以「测试」开头的故事 ·
        只注入<b>伪造的 AI 输出</b>去撞真实管线，<b>不调用模型</b>。
      </div>

      <div style={{ display: "flex", gap: 4, padding: 3, border: `1px solid ${T.line}`, borderRadius: 10, background: "#12141a", alignSelf: "flex-start" }}>
        {([["overview", "🔧 概览"], ["inject", "💉 数据注入"]] as [typeof tab, string][]).map(([k, label]) => (
          <button key={k} onClick={() => setTab(k)} style={{ border: "none", borderRadius: 7, padding: "7px 16px", fontSize: 12.5, fontWeight: 600, cursor: "pointer", background: tab === k ? T.accent : "transparent", color: tab === k ? "#231505" : T.text2 }}>
            {label}
          </button>
        ))}
      </div>

      {msg && <div style={{ fontSize: 12, color: msg.startsWith("❌") ? T.danger : T.text2 }}>{msg}</div>}

      {tab === "overview" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={boxS}>
            <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 10 }}>环境</div>
            <div style={{ display: "grid", gridTemplateColumns: "110px 1fr", gap: "6px 12px", fontSize: 12 }}>
              <span style={{ color: T.text3 }}>构建类型</span>
              <span style={{ color: status?.dev_build ? T.accent : T.danger }}>{status?.dev_build ? "开发构建（debug_assertions）" : "发布构建 —— 开发者工具不可用"}</span>
              <span style={{ color: T.text3 }}>数据根</span>
              <span style={{ color: T.text2, wordBreak: "break-all" }}>{status?.data_root ?? "…"}</span>
              <span style={{ color: T.text3 }}>下一个测试故事</span>
              <span style={{ color: T.text1, fontWeight: 600 }}>{status?.next_title ?? "…"}</span>
            </div>
          </div>

          <div style={boxS}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 10 }}>
              <div style={{ fontSize: 12.5, fontWeight: 650 }}>测试故事（{status?.test_stories.length ?? 0}）</div>
              <button onClick={createStory} disabled={busy} style={accentS}>＋ 新建测试故事</button>
              <button onClick={() => void refresh()} style={ghostS}>刷新</button>
            </div>
            {(!status || status.test_stories.length === 0) ? (
              <div style={{ fontSize: 12, color: T.text3, padding: "18px 0" }}>
                还没有测试故事。点「＋ 新建测试故事」会创建 <b>{status?.next_title ?? "测试0"}</b>。
              </div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {status.test_stories.map((s) => (
                  <div key={s.id} style={{ display: "flex", alignItems: "center", gap: 10, border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "8px 11px", fontSize: 12 }}>
                    <span style={{ fontWeight: 600 }}>{s.title}</span>
                    <span style={{ color: T.text3, fontSize: 11.5 }}>#{s.id} · {s.created_at}</span>
                    <button onClick={() => { setStoryId(s.id); setTab("inject"); }} style={{ ...ghostS, marginLeft: "auto", height: 26 }}>用它注入 →</button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {tab === "inject" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ ...boxS, display: "flex", gap: 10, alignItems: "flex-end", flexWrap: "wrap" }}>
            <label style={{ ...labelS, flex: "1 1 220px" }}>
              测试故事（只列「测试」开头的）
              <select value={storyId ?? ""} onChange={(e) => setStoryId(Number(e.target.value))} style={inputS}>
                {(!status || status.test_stories.length === 0) && <option value="">（先去概览新建一个）</option>}
                {status?.test_stories.map((s) => <option key={s.id} value={s.id}>{s.title}</option>)}
              </select>
            </label>
            <label style={{ ...labelS, flex: "0 0 220px" }}>
              管线
              <select
                value={pipeline}
                onChange={(e) => {
                  const p = e.target.value as Pipeline;
                  setPipeline(p);
                  setPayload(TEMPLATES[p]);
                  setResult(null);
                }}
                style={inputS}
              >
                <option value="summary">日摘要（timeline_nodes.day_summary）</option>
                <option value="eval">回复评估（reply_evaluations）</option>
                <option value="proposal">静默演进提案（change_proposals, pending）</option>
              </select>
            </label>
            {pipeline === "summary" && (
              <label style={{ ...labelS, flex: "0 0 120px" }}>
                节点 id
                <input value={nodeId} onChange={(e) => setNodeId(e.target.value)} style={inputS} />
              </label>
            )}
            {pipeline === "eval" && (
              <label style={{ ...labelS, flex: "0 0 120px" }}>
                消息 id
                <input value={messageId} onChange={(e) => setMessageId(e.target.value)} style={inputS} />
              </label>
            )}
          </div>

          <div style={boxS}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
              <div style={{ fontSize: 12.5, fontWeight: 650 }}>伪造成 AI 输出</div>
              <button onClick={() => setPayload(TEMPLATES[pipeline])} style={{ ...ghostS, height: 26 }}>载入模板</button>
              <span style={{ fontSize: 11.5, color: T.text3 }}>
                {pipeline === "summary" ? "纯文本" : "JSON"}
              </span>
            </div>
            <textarea value={payload} onChange={(e) => setPayload(e.target.value)} rows={10} style={{ ...taS, width: "100%", boxSizing: "border-box" }} />
            <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
              <button onClick={() => void run(true)} disabled={busy} style={ghostS}>① 试运行（不落地）</button>
              <button onClick={() => void run(false)} disabled={busy} style={accentS}>② 真落地（先打快照）</button>
            </div>
          </div>

          {result && (
            <div style={{ ...boxS, borderColor: result.outcome.startsWith("拒绝") ? T.danger : T.line }}>
              <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>
                {result.dry_run ? "试运行结果" : "落地结果"} · {result.target}
              </div>
              <div style={{ fontSize: 12.5, color: result.outcome.startsWith("拒绝") ? T.danger : T.accent, marginBottom: 8 }}>
                {result.outcome}
              </div>
              {result.changes.length > 0 && (
                <div style={{ fontSize: 12, color: T.text2, lineHeight: 1.8 }}>
                  {result.changes.map((c, i) => <div key={i}>· {c}</div>)}
                </div>
              )}
              {result.snapshot !== null && (
                <div style={{ fontSize: 11.5, color: T.text3, marginTop: 8 }}>
                  已打快照 v{result.snapshot} —— 可在下面回滚
                </div>
              )}
            </div>
          )}

          <div style={boxS}>
            <div style={{ fontSize: 12.5, fontWeight: 650, marginBottom: 8 }}>快照（本故事）</div>
            {snaps.length === 0 ? (
              <div style={{ fontSize: 12, color: T.text3 }}>还没有快照。真落地时会自动打。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                {snaps.slice(0, 12).map(([v, at, temp]) => (
                  <div key={v} style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 12, borderBottom: `1px solid ${T.lineSoft}`, paddingBottom: 5 }}>
                    <span style={{ fontWeight: 600 }}>v{v}</span>
                    <span style={{ color: T.text3 }}>{at}{temp === 1 ? " · 临时" : ""}</span>
                    <button onClick={() => void rollback(v)} style={{ ...ghostS, marginLeft: "auto", height: 24, color: T.danger }}>回滚到 v{v}</button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
