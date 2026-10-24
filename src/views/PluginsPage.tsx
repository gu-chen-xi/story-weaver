// 「PluginsPage」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import type { Dispatch, SetStateAction } from "react";
import type { AppView } from "../types";
import { EVAL_DIM_LABEL } from "../constants";
import { BUILTIN_PLUGINS } from "../plugins/registry";
import type { PluginHostData } from "../plugins/host";
import { pluginCaps, type PluginState } from "../plugins/types";
import { PluginCard } from "../plugins/PluginCard";
import type { CloudForm, GenDescriptor, GenProbe } from "../plugins";
import type { Character, EvalT, Story } from "../types";
import { T } from "../theme";
import { invoke } from "@tauri-apps/api/core";

export type ViewName = AppView;

export type PluginsPageProps = {
  chars: Character[];
  cloudForm: CloudForm;
  cloudOverrides: () => Record<string, string | null>;
  cloudSaving: boolean;
  currentStory: Story | null;
  evalBusy: boolean;
  evals: EvalT[];
  genBusy: boolean;
  genList: GenDescriptor[];
  genPrefill: { prompt?: string; negative?: string; width?: number; height?: number;
    seed?: string; steps?: string; cfg?: string; characterId?: number | null;
    basePortraitId?: number | null; note?: string; stamp: number } | null;
  genProbe: GenProbe | null;
  genProbing: boolean;
  genSel: string;
  genSummary: string;
  genUrl: string;
  loadCloudConfig: () => Promise<void>;
  loadEvals: () => Promise<void>;
  node: { id: number; date_label: string } | null;
  pluginKey: (id: string) => string;
  pluginState: Record<string, { enabled: boolean; panelOpen: boolean }>;
  runGenerate: (o: { prompt: string; negative: string; width: number; height: number; batch: number;
    seed: string; steps: string; cfg: string; characterId: number | null; name: string;
    basePortraitId: number | null }) => Promise<void>;
  setCloudForm: Dispatch<SetStateAction<CloudForm>>;
  setCloudSaving: Dispatch<SetStateAction<boolean>>;
  setEvalBusy: Dispatch<SetStateAction<boolean>>;
  setGenProbe: Dispatch<SetStateAction<GenProbe | null>>;
  setGenProbing: Dispatch<SetStateAction<boolean>>;
  setGenSel: Dispatch<SetStateAction<string>>;
  setGenUrl: Dispatch<SetStateAction<string>>;
  setMsg: Dispatch<SetStateAction<string>>;
  setPluginFlag: (id: string, patch: Partial<{ enabled: boolean; panelOpen: boolean }>) => void;
};

export function PluginsPage(props: PluginsPageProps) {
  const { genPrefill, chars, cloudForm, cloudOverrides, cloudSaving, currentStory, evalBusy, evals, genBusy, genList, genProbe, genProbing, genSel, genSummary, genUrl, loadCloudConfig, loadEvals, node, pluginKey, pluginState, runGenerate, setCloudForm, setCloudSaving, setEvalBusy, setGenProbe, setGenProbing, setGenSel, setGenUrl, setMsg, setPluginFlag } = props;
  // ===== 从 App.tsx 搬进来的本页状态与处理函数 =====
  const patchCloud = (p: Partial<CloudForm>) => setCloudForm((prev) => ({ ...prev, ...p }));
  const saveCloudConfig = async () => {
    setCloudSaving(true);
    try {
      const msg = await invoke<string>("generator_save_cloud_config", {
        config: {
          base_url: cloudForm.base_url, model: cloudForm.model, url: cloudForm.url,
          body_template: cloudForm.body_template, response_path: cloudForm.response_path,
          auth_style: cloudForm.auth_style, extra_headers: cloudForm.extra_headers,
        },
        apiKey: cloudForm.api_key || null,
      });
      setMsg("✅ " + msg + (cloudForm.api_key ? "（API Key 已存入系统凭据库）" : ""));
      await loadCloudConfig();
    } catch (e) { setMsg("❌ 保存失败: " + String(e)); }
    finally { setCloudSaving(false); }
  };
  const probeGenerator = async () => {
    setGenProbing(true);
    setGenProbe(null);
    try {
      setGenProbe(await invoke<GenProbe>("generator_probe", { generatorId: genSel, baseUrl: genUrl, config: cloudOverrides() }));
    } catch (e) { setGenProbe({ ok: false, detail: String(e) }); }
    finally { setGenProbing(false); }
  };
  const evalLastReply = async () => {
    if (!currentStory || !node) { setMsg("⚠️ 请先选中一个剧情节点"); return; }
    setEvalBusy(true);
    try {
      const list = await invoke<{ id: number; role: string }[]>("list_messages", { storyId: String(currentStory.id), nodeId: node.id });
      const lastAi = [...list].reverse().find((m) => m.role === "assistant");
      if (!lastAi) { setMsg("⚠️ 这个节点还没有 AI 回复"); return; }
      const r = await invoke<EvalT>("evaluate_reply", { storyId: String(currentStory.id), messageId: lastAi.id });
      setMsg(`✅ 评估完成：总分 ${r.total} / 5（${r.dims.map((d) => `${EVAL_DIM_LABEL[d.key] ?? d.key} ${d.score}`).join(" · ")}）`);
      await loadEvals();
    } catch (e) { setMsg("❌ 评估失败: " + String(e)); }
    finally { setEvalBusy(false); }
  };
  // ── 宿主机交给插件的统一数据（P2：所有插件都从这一份里取，页面不再按 id 拼 props）──
  const hostData: PluginHostData = {
    T: T as unknown as PluginHostData["T"],
    hasStory: !!currentStory,
    hasNode: !!node,
    notify: (m) => setMsg(m),
    setFlag: setPluginFlag,
    evaluate: {
      dimLabel: EVAL_DIM_LABEL,
      records: evals,
      busy: evalBusy,
      canRun: !!currentStory && !!node,
      noStoryHint: "请先进入一个故事并选中一个剧情节点",
      onRun: evalLastReply,
      onRefresh: loadEvals,
    },
    portraitGen: {
      generators: genList,
      selected: genSel,
      onSelect: (id) => { setGenSel(id); setGenProbe(null); },
      baseUrl: genUrl,
      onBaseUrl: setGenUrl,
      probing: genProbing,
      probe: genProbe,
      onProbe: probeGenerator,
      generating: genBusy,
      onGenerate: runGenerate,
      lastSummary: genSummary,
      charOptions: chars.map((c) => ({ id: c.id, name: c.name })),
      defaultCharacterId: null,
      cloud: cloudForm,
      onCloud: patchCloud,
      onSaveCloud: saveCloudConfig,
      savingCloud: cloudSaving,
      placeholderHint: "可用占位符：{{prompt}} {{negative}} {{width}} {{height}} {{batch}} {{seed}} {{steps}} {{cfg}}",
      prefill: genPrefill,
    },
  };

  return (
      (
        <div style={{ maxWidth: 860 }}>
          <h3 style={{ margin: 0, marginBottom: 6 }}>🧩 插件 / 扩展</h3>
          <p style={{ fontSize: 12.5, color: T.text3, marginBottom: 14 }}>
            核心功能保持精简，扩展功能以插件形式接入。已实装的插件可直接开关；规划的插件位随插件系统一并实装。
          </p>

          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {/* v3.5.7：**纯遍历注册表** —— 这里不认识任何一个具体插件的 id。
                徽标、手动运行、面板全由 `PluginModule` 自己提供（见 plugins/builtin/<id>/）。 */}
            {BUILTIN_PLUGINS.map((m) => {
              const d = m.descriptor;
              const caps = pluginCaps(d);
              const ready = d.status === "ready";
              const st = pluginState[d.id] ?? { enabled: false, panelOpen: false };
              const ctx = { state: st, host: hostData };
              const run = ready && caps.canRunManually ? m.run?.(ctx) : undefined;
              const state: PluginState = {
                enabled: st.enabled,
                panelOpen: st.panelOpen,
                badges: ready ? (m.badges?.(ctx) ?? []) : [],
                busy: run?.busy,
                runLabel: run?.label,
                runHint: run?.hint,
                panel: ready ? m.renderPanel?.(hostData) : undefined,
              };
              return (
                <PluginCard
                  key={d.id}
                  descriptor={d}
                  state={state}
                  T={T}
                  onToggle={caps.canToggle && ready ? async (on) => {
                    setPluginFlag(d.id, { enabled: on });
                    try {
                      await invoke("set_config", { key: pluginKey(d.id), value: on ? "true" : "false" });
                      const note = m.toggleMessage?.(on);
                      if (note) setMsg(note);
                    } catch (err) { setMsg("❌ 保存失败: " + String(err)); }
                  } : undefined}
                  onRun={run ? (run.canRun ? run.onClick : undefined) : undefined}
                  onTogglePanel={caps.canViewPanel && ready ? () => {
                    const next = !st.panelOpen;
                    setPluginFlag(d.id, { panelOpen: next });
                    // 展开面板时拉一次数据：这一步也交给插件自己（面板适配器里已经有 refresh）
                    if (next) void loadEvals();
                  } : undefined}
                />
              );
            })}
          </div>
          <p style={{ fontSize: 11.5, color: T.text3, marginTop: 14 }}>
            插件系统目标：应对第三方服务下线风险、保持软件轻量化。加一个插件 = 在 `plugins/builtin/` 加一个目录
            （元信息 + 可选面板 + 徽标），再往 `plugins/registry.tsx` 加一条 —— 卡片布局 / 总开关 / 手动执行 / 状态徽标由通用插件卡统一渲染。
          </p>
        </div>
      )
  );
}
