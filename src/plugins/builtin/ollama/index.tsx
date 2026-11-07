// 「Ollama 本地模型」插件（2026-10-31）。
//
// 边界（用户定）：
//   * 只支持 **Windows 版 Ollama**；不下载、不启动 —— 用户自己开服务、自己开这个开关；
//   * 我们只做：检查地址通不通（/api/version）+ 把**主模型**请求转到本地；
//   * **主模型只有一路**：开着它就用它，云端被替换（不混用）；
//   * 辅助项（伏笔扫描）是独立开关，可以跟云端一起用。

import { useEffect, useState } from "react";

import type { PluginDescriptor, PluginModule, PluginTheme } from "../../types";
import type { PluginHostData } from "../../host";

export const OLLAMA_ID = "ollama";

const DESCRIPTOR: PluginDescriptor = {
  id: OLLAMA_ID,
  icon: "🦙",
  name: "Ollama 本地模型（Windows）",
  status: "ready",
  desc: "把主角色的模型换成你本机的 Ollama。软件不下载、不启动 Ollama：先自己把 ollama 跑起来，再点这里的开关。开着时主模型只有这一路（云端被替换）；伏笔扫描这类辅助项可以单独留在云端。",
  caps: { canToggle: true, canViewPanel: true, needsStory: false },
  toggleLabel: "用 Ollama 替换云端主力模型",
  toggleHint: "开启前请先在面板里点一次「检查连接」；连不上时会拒绝生成，不会偷偷回落云端。",
};

type Cfg = {
  enabled: boolean;
  base_url: string;
  model: string;
  context_window: number;
  foreshadow_scan: boolean;
};

const CTX = [
  [32768, "32k"],
  [65536, "64k"],
  [131072, "128k"],
  [262144, "256k"],
  [1048576, "1M"],
] as [number, string][];

function Panel({ T, call }: { T: PluginTheme; call: PluginHostData["call"] }) {
  const [cfg, setCfg] = useState<Cfg | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");

  const load = async () => {
    try { setCfg(await call<Cfg>("ollama_get")); } catch (e) { setStatus("读取配置失败：" + String(e)); }
  };
  useEffect(() => { void load(); }, []);

  const save = async (patch: Partial<Cfg>) => {
    if (!cfg) return;
    const next = { ...cfg, ...patch };
    setCfg(next);
    try {
      await call("ollama_set", {
        enabled: next.enabled,
        baseUrl: next.base_url,
        model: next.model,
        contextWindow: next.context_window,
        foreshadowScan: next.foreshadow_scan,
      });
    } catch (e) { setStatus("保存失败：" + String(e)); }
  };

  const check = async () => {
    if (!cfg) return;
    setBusy(true); setStatus("检查中…");
    try {
      const v = await call<string>("ollama_probe", { baseUrl: cfg.base_url });
      setStatus("✅ 连上了，Ollama 版本 " + v);
    } catch (e) {
      setStatus("❌ " + String(e));
    } finally { setBusy(false); }
  };

  const testChat = async () => {
    if (!cfg) return;
    setBusy(true); setStatus("测试中…（发给本地模型一句「请回复：你好」）");
    try {
      const reply = await call<string>("ollama_test_chat");
      setStatus("✅ 本地模型回了：" + reply);
    } catch (e) {
      setStatus("❌ " + String(e));
    } finally { setBusy(false); }
  };

  if (!cfg) return <div style={{ fontSize: 11.5, color: T.text3 }}>读取配置…</div>;

  const row = { display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: T.text2 } as const;
  const input = {
    height: 26, padding: "0 8px", borderRadius: 7, border: "1px solid " + T.line,
    background: "#191c22", color: T.text1, fontFamily: "inherit", fontSize: 11.5,
  } as const;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={row}>
        <span style={{ width: 64, color: T.text3 }}>地址</span>
        <input value={cfg.base_url} onChange={(e) => setCfg({ ...cfg, base_url: e.target.value })}
          onBlur={() => void save({ base_url: cfg.base_url })} style={{ ...input, flex: 1 }} />
        <button onClick={() => void check()} disabled={busy}
          style={{ height: 26, padding: "0 10px", borderRadius: 7, border: "1px solid " + T.line, background: "transparent", color: T.text2, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>
          检查连接
        </button>
        <button onClick={() => void testChat()} disabled={busy}
          title="只用这里的地址和模型发一句「请回复：你好」，跟云端完全无关"
          style={{ height: 26, padding: "0 10px", borderRadius: 7, border: "none", background: T.accentSoft, color: T.accent, cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}>
          测试对话
        </button>
      </div>
      <div style={row}>
        <span style={{ width: 64, color: T.text3 }}>模型名</span>
        <input value={cfg.model} onChange={(e) => setCfg({ ...cfg, model: e.target.value })}
          onBlur={() => void save({ model: cfg.model })}
          placeholder="例如 qwen2.5:14b（ollama list 里看到的那个）" style={{ ...input, flex: 1 }} />
      </div>
      <div style={row}>
        <span style={{ width: 64, color: T.text3 }}>上下文</span>
        <select value={cfg.context_window} onChange={(e) => void save({ context_window: Number(e.target.value) })}
          style={{ ...input, padding: "0 6px" }}>
          {CTX.map(([v, lb]) => <option key={v} value={v}>{lb}</option>)}
        </select>
        <span style={{ color: T.text3, fontSize: 10.5 }}>这个档位会作为 options.num_ctx 发给 Ollama，主对话真正生效</span>
      </div>
      <label style={{ ...row, cursor: "pointer" }}>
        <input type="checkbox" checked={cfg.foreshadow_scan} onChange={(e) => void save({ foreshadow_scan: e.target.checked })} />
        伏笔扫描走本地（辅助项，可以和云端一起用）
      </label>
      <div style={{ fontSize: 10.5, color: T.text3, lineHeight: 1.7 }}>
        适配 Ollama <b>0.40.11</b>（Windows 版；本机实测同版本）。软件不负责下载/启动；没有服务时点「检查连接」会直接报错。
        <br />
        主模型同一时间只有一路：这个插件开着 → 走 Ollama（原生 /api/chat）；关掉 → 回到云端（参数页里的模型）。
        <br />
        主对话走的是 <b>原生接口</b>：上面选的上下文档位会作为 num_ctx 发过去（Ollama 自己的默认值 2048 只影响别的客户端），
        结束块里的真实用量也会记进「生成参数」页。想给全局兜底仍可设 <b>OLLAMA_CONTEXT_LENGTH=32768</b>。
      </div>
      {status && <div style={{ fontSize: 11, color: status.startsWith("❌") ? T.danger : T.text2 }}>{status}</div>}
    </div>
  );
}

export const OLLAMA_MODULE: PluginModule = {
  descriptor: DESCRIPTOR,
  renderPanel: ({ T, call }) => <Panel T={T} call={call} />,
};
