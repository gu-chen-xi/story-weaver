// 设置弹窗（从 App.tsx 拆分，2026-10-20）。
//
// 拆分方式：**JSX 原样搬**，并把"只有本弹窗用得到"的东西全部搬进来：
//   草稿 state：baseUrl / apiKeyInput / hasApiKey / testBusy / testResult /
//               settingsTab / ctxWindowInput / reserveRatio / maxTokensInput
//   handler   ：保存（写 config）/ 测试连接 / 清除已保存 Key / 打开时读取这些配置
//
// 留在 App 的是**已生效值**（modelName / nRecentInput / autoAnalyze / ctxWindow）——
// 顶栏显示模型名、生成参数页要用它们。弹窗保存后通过 `onApplied` 把这些值回传给 App。
//
// 注：本组件只在 `showSettings` 为真时挂载，所以"打开即读取"用 useEffect([]) 即可，
// 不需要父子间再做刷新协议。

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtNum } from "../lib/parse";
import { T } from "../theme";
import { useConfirm } from "../confirm";

export type SettingsApplied = {
  baseUrl: string;
  modelName: string;
  nRecent: string;
  ctxWindow: number;
  autoAnalyze: boolean;
};

export type SettingsModalProps = {
  /** 打开时就该显示的已生效值（App 里维护，顶栏/参数页也在用） */
  initial: { modelName: string; nRecent: string; autoAnalyze: boolean };
  onClose: () => void;
  /** 「💾 数据管理」：回 App 里切页 */
  onOpenData: () => void;
  /** 保存成功后回传"已生效值"，让 App 更新顶栏 / 参数页 */
  onApplied: (v: SettingsApplied) => void;
  onMsg: (s: string) => void;
};

export function SettingsModal({ initial, onClose, onOpenData, onApplied, onMsg }: SettingsModalProps) {
  const confirm = useConfirm();
  // ── 本弹窗自己的草稿（原在 App.tsx，初值/行为不变）──
  const [baseUrl, setBaseUrl] = useState("");
  const [modelName, setModelName] = useState(initial.modelName);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [hasApiKey, setHasApiKey] = useState(false);
  const [testBusy, setTestBusy] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; detail: string; reply?: string } | null>(null);
  const [settingsTab, setSettingsTab] = useState<"api" | "debug" | "data">("api");
  const [ctxWindowInput, setCtxWindowInput] = useState("32768");
  const [reserveRatio, setReserveRatio] = useState("0.8");
  const [maxTokensInput, setMaxTokensInput] = useState("4000");
  const [nRecentInput, setNRecentInput] = useState(initial.nRecent);
  const [autoAnalyze, setAutoAnalyze] = useState(initial.autoAnalyze);

  // 上下文窗口的本地即时校验（原在 App 里算，只被本弹窗使用）
  const cwNumber = Number(ctxWindowInput) || 0;
  const cwWarn = cwNumber > 0 && cwNumber < 32768;
  const cwInvalid = ctxWindowInput.trim() !== "" && (!cwNumber || cwNumber <= 0);

  // 打开即读取（原 App.loadSettings 的一部分，逐字搬来）
  useEffect(() => {
    void loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const loadAll = async () => {
    try {
      // 主模型 Key 存在系统凭据库里，界面查不到 —— 向后端问一个布尔值（只回"有没有"，绝不回明文）
      invoke<{ has_key: boolean }>("main_model_key_status")
        .then((r) => setHasApiKey(!!r.has_key))
        .catch(() => setHasApiKey(false));
      const bu = await invoke<string | null>("get_config", { key: "model.base_url" });
      const mn = await invoke<string | null>("get_config", { key: "model.name" });
      setBaseUrl(bu ?? "");
      setModelName(mn ?? initial.modelName);
      const cw = await invoke<string | null>("get_config", { key: "model.context_window" });
      if (cw) setCtxWindowInput(cw);
      const rr = await invoke<string | null>("get_config", { key: "model.context_reserve_ratio" });
      if (rr) setReserveRatio(rr);
      const mt = await invoke<string | null>("get_config", { key: "model.max_tokens" });
      if (mt) setMaxTokensInput(mt);
      const nr = await invoke<string | null>("get_config", { key: "context.n_recent" });
      if (nr) setNRecentInput(nr);
      const aa = await invoke<string | null>("get_config", { key: "evolution.auto_analyze" });
      setAutoAnalyze(aa !== "false");
    } catch (e) {
      console.error(e);
    }
  };

  const saveAll = async () => {
    try {
      await invoke("set_config", { key: "model.base_url", value: baseUrl });
      await invoke("set_config", { key: "model.name", value: modelName });
      // 上下文预算三项：窗口 / 系数 / 单次最大输出（安全余量保持默认 800，一般不用改）
      const cw = cwNumber;
      if (cw > 0) await invoke("set_config", { key: "model.context_window", value: String(cw) });
      const rr = Number(reserveRatio);
      if (rr > 0 && rr <= 1) await invoke("set_config", { key: "model.context_reserve_ratio", value: String(rr) });
      const mt = Number(maxTokensInput);
      if (mt > 0) await invoke("set_config", { key: "model.max_tokens", value: String(mt) });
      const nr = Number(nRecentInput);
      if (nr > 0) await invoke("set_config", { key: "context.n_recent", value: String(nr) });
      await invoke("set_config", { key: "evolution.auto_analyze", value: autoAnalyze ? "true" : "false" });

      if (apiKeyInput) {
        await invoke("set_api_key", { account: "openai_api_key", key: apiKeyInput });
        setHasApiKey(true);
        setApiKeyInput("");
      }
      onApplied({ baseUrl, modelName, nRecent: nRecentInput, ctxWindow: cw, autoAnalyze });
      onMsg("✅ 设置已保存");
    } catch (e) {
      onMsg("❌ 保存失败: " + String(e));
    }
  };

  const runTest = async () => {
    setTestBusy(true);
    setTestResult(null);
    try {
      const r = await invoke<{ ok: boolean; detail: string; reply?: string }>("test_api_connection");
      setTestResult(r);
    } catch (e) {
      setTestResult({ ok: false, detail: "调用失败: " + String(e) });
    } finally {
      setTestBusy(false);
    }
  };

  /// 清除已保存的主模型 Key（`delete_api_key` 之前没有 UI 入口，命令等于白注册）。
  const clearKey = async () => {
    if (!(await confirm({
      title: "清除 API Key",
      body: "清除已保存的 API Key？\n\n只删 Windows 凭据管理器里的那一条，不影响其它数据（要恢复得重新填一次）。",
      danger: true,
    }))) return;
    try {
      await invoke("delete_api_key", { account: "openai_api_key" });
      setHasApiKey(false);
      setApiKeyInput("");
      onMsg("🗑 已清除保存的 API Key");
    } catch (e) {
      onMsg("❌ 清除失败：" + String(e));
    }
  };

  return (
    <div style={{ position: "fixed", inset: 0, background: "rgba(8,9,12,.62)", backdropFilter: "blur(3px)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
      <div style={{ width: 560, maxHeight: "80vh", overflowY: "auto", background: T.panel, border: `1px solid ${T.line}`, borderRadius: 16, boxShadow: "0 24px 70px rgba(0,0,0,.6)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "16px 20px", borderBottom: `1px solid ${T.line}` }}>
          <div style={{ width: 30, height: 30, borderRadius: 9, background: T.accentSoft, display: "flex", alignItems: "center", justifyContent: "center", color: T.accent, fontSize: 15 }}>⚙️</div>
          <div>
            <h3 style={{ margin: 0, fontSize: 14.5, fontWeight: 650 }}>设置</h3>
            <p style={{ margin: "2px 0 0", fontSize: 11.5, color: T.text3 }}>系统设置 · API / 数据管理</p>
          </div>
          <button onClick={onClose} style={{ marginLeft: "auto", color: T.text3, cursor: "pointer", padding: 4, borderRadius: 6, border: "none", background: "transparent", fontSize: 14 }}>✕</button>
        </div>
        <div style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button onClick={() => setSettingsTab("api")} style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${T.line}`, cursor: "pointer", background: settingsTab === "api" ? T.accentSoft : "#191c22", color: settingsTab === "api" ? T.accent : T.text2, fontSize: 12.5, fontWeight: settingsTab === "api" ? 600 : 400 }}>🔑 API 设置</button>
            <button onClick={onOpenData} style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${T.line}`, cursor: "pointer", background: "#191c22", color: T.text2, fontSize: 12.5 }}>💾 数据管理</button>
          </div>
          {settingsTab === "api" && (
            <div>
              <div style={{ display: "grid", gridTemplateColumns: "110px 1fr", gap: 8, alignItems: "center" }}>
                <label style={{ fontSize: 12, color: T.text2 }}>Base URL</label>
                <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.deepseek.com/v1" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                <label style={{ fontSize: 12, color: T.text2 }}>模型名</label>
                <input value={modelName} onChange={(e) => setModelName(e.target.value)} placeholder="deepseek-chat" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                <label style={{ fontSize: 12, color: T.text2 }}>
                  API Key
                  <span style={{ marginLeft: 8, fontSize: 11, color: hasApiKey ? T.ok : T.warn }}>
                    {hasApiKey ? "● 已保存到系统凭据库" : "○ 尚未设置"}
                  </span>
                </label>
                <input type="password" value={apiKeyInput} onChange={(e) => setApiKeyInput(e.target.value)} placeholder="（留空则不修改已有 Key）" style={{ padding: "8px 11px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <button onClick={() => void clearKey()} disabled={!hasApiKey} title="只删除系统凭据库里的这一条 Key" style={{ height: 28, padding: "0 10px", borderRadius: 7, border: `1px solid ${hasApiKey ? T.danger : T.line}`, background: "transparent", color: hasApiKey ? T.danger : T.text3, fontSize: 11.5, cursor: hasApiKey ? "pointer" : "not-allowed", fontFamily: "inherit" }}>🗑 清除已保存的 Key</button>
                </div>
                {/* 合规/安全说明：用户有权知道自己的密钥存在哪里 */}
                <div style={{ fontSize: 10.5, color: T.text3 }}>密钥保存在 Windows 凭据管理器（不写入数据库、不进备份、不随作品导出）</div>
                <label style={{ fontSize: 12, color: T.text2 }}>上下文窗口</label>
                <div>
                  <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginBottom: 6 }}>
                    {[
                      { label: "DeepSeek flash / v4-pro · 1M", v: "1000000" },
                      { label: "GPT-4o 系 · 128k", v: "128000" },
                      { label: "保守档 · 32k", v: "32768" },
                    ].map((p) => (
                      <button key={p.v} onClick={() => setCtxWindowInput(p.v)} style={{ height: 24, padding: "0 8px", borderRadius: 6, border: `1px solid ${ctxWindowInput === p.v ? T.accent : T.line}`, background: ctxWindowInput === p.v ? "rgba(255,122,24,.12)" : "transparent", color: ctxWindowInput === p.v ? T.accent : T.text3, cursor: "pointer", fontSize: 10.5 }}>{p.label}</button>
                    ))}
                  </div>
                  <input value={ctxWindowInput} onChange={(e) => setCtxWindowInput(e.target.value.replace(/[^0-9]/g, ""))} placeholder="模型官方上下文长度，如 1000000" style={{ width: "100%", boxSizing: "border-box", padding: "8px 11px", borderRadius: 8, border: `1px solid ${cwInvalid || cwWarn ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                  <div style={{ fontSize: 10.5, color: cwInvalid || cwWarn ? "#ff8c85" : T.text3, marginTop: 4, lineHeight: 1.6 }}>
                    {cwInvalid ? "⚠ 请填一个大于 0 的整数" : cwWarn ? "⚠ 建议 ≥ 32,768（低于此值历史几乎放不下；填模型官方值即可）" : "填模型官方上下文长度；软件只吃其中一部分（见下方系数）"}
                  </div>
                </div>
                <label style={{ fontSize: 12, color: T.text2 }}>预算系数</label>
                <div>
                  <input value={reserveRatio} onChange={(e) => setReserveRatio(e.target.value.replace(/[^0-9.]/g, ""))} placeholder="0.8" style={{ width: 90, padding: "8px 11px", borderRadius: 8, border: `1px solid ${!(Number(reserveRatio) > 0 && Number(reserveRatio) <= 1) ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                  <span style={{ fontSize: 10.5, color: T.text3, marginLeft: 8 }}>0.8 = 只吃窗口的 80%，留 20% 保底防突发（0~1）</span>
                </div>
                <label style={{ fontSize: 12, color: T.text2 }}>每日上限</label>
                <div>
                  <input value={nRecentInput} onChange={(e) => setNRecentInput(e.target.value.replace(/[^0-9]/g, ""))} placeholder="10" style={{ width: 90, padding: "8px 11px", borderRadius: 8, border: `1px solid ${!(Number(nRecentInput) > 0) ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                  <span style={{ fontSize: 10.5, color: T.text3, marginLeft: 8 }}>条/天；由预算倒推的 N 会被它封顶（跨天时"近三天"各自的上限）</span>
                </div>
                <label style={{ fontSize: 12, color: T.text2 }}>单次最大输出</label>
                <div>
                  <input value={maxTokensInput} onChange={(e) => setMaxTokensInput(e.target.value.replace(/[^0-9]/g, ""))} placeholder="4000" style={{ width: 90, padding: "8px 11px", borderRadius: 8, border: `1px solid ${!(Number(maxTokensInput) > 0) ? "rgba(240,85,77,.55)" : T.line}`, background: "#191c22", color: T.text1, fontFamily: "monospace", fontSize: 12, outline: "none" }} />
                  <span style={{ fontSize: 10.5, color: T.text3, marginLeft: 8 }}>token；会连同安全余量（800）一起从预算里预留出去</span>
                </div>
              </div>
              <div style={{ marginTop: 10, padding: "8px 10px", borderRadius: 8, background: "rgba(255,122,24,.07)", border: "1px solid rgba(255,122,24,.25)", fontSize: 11, color: T.text2, lineHeight: 1.7 }}>
                按当前设置：可用输入预算 = {fmtNum(Math.max(Math.round(cwNumber * (Number(reserveRatio) || 0.8)) - (Number(maxTokensInput) || 4000) - 800, 0))} token
                <span style={{ color: T.text3 }}>（= 窗口 {fmtNum(cwNumber)} × {Number(reserveRatio) || 0.8} − 输出 {fmtNum(Number(maxTokensInput) || 4000)} − 安全余量 800）</span>
              </div>
              <div style={{ marginTop: 12, display: "flex", gap: 8 }}>
                <button onClick={() => void saveAll()} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: `1px solid ${T.line}`, background: "#191c22", color: T.text2, cursor: "pointer", fontSize: 12.5 }}>保存设置</button>
                <button onClick={() => void runTest()} disabled={testBusy} style={{ height: 32, padding: "0 16px", borderRadius: 8, border: "none", background: "linear-gradient(135deg,#FF7A18,#ff9a45)", color: "#231505", cursor: "pointer", fontSize: 12.5, fontWeight: 650 }}>{testBusy ? "测试中..." : "测试连接"}</button>
              </div>
              {testResult && (
                <div style={{ marginTop: 10, padding: 8, background: testResult.ok ? "rgba(55,201,120,.1)" : "rgba(240,85,77,.1)", border: `1px solid ${testResult.ok ? "rgba(55,201,120,.4)" : "rgba(240,85,77,.4)"}`, borderRadius: 6, fontSize: 13, color: testResult.ok ? T.ok : "#ff8c85" }}>
                  <div>{testResult.ok ? "✅" : "❌"} {testResult.detail}</div>
                  {testResult.reply && <div style={{ marginTop: 4, color: T.text1 }}>AI 回复：<b>{testResult.reply}</b></div>}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
