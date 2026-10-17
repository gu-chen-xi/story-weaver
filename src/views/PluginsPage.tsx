// 「PluginsPage」页（从 App.tsx 拆分，2026-10-17）。
//
// 拆分方式：视图 + 本页专属的 state/处理函数一起下沉（见 ROADMAP §10.66）。
// 只保留跨页共享项作为 props。

import type { Dispatch, SetStateAction } from "react";
import { EVAL_DIM_LABEL, PLUGIN_REGISTRY } from "../constants";
import { EvalPanel, PluginCard, PortraitGenPanel, pluginCaps } from "../plugins";
import type { CloudForm, GenDescriptor, GenProbe, PluginState } from "../plugins";
import type { Character, EvalT, Story } from "../types";
import { T } from "../theme";
import { invoke } from "@tauri-apps/api/core";

export type ViewName =
  | "chars" | "world" | "relations" | "story" | "proposals" | "temp" | "timeline" | "stories" | "plugins" | "portrait" | "compliance" | "params" | "home";

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
  return (
      (
        <div style={{ maxWidth: 860 }}>
          <h3 style={{ margin: 0, marginBottom: 6 }}>🧩 插件 / 扩展</h3>
          <p style={{ fontSize: 12.5, color: T.text3, marginBottom: 14 }}>
            核心功能保持精简，扩展功能以插件形式接入。已实装的插件可直接开关；规划的插件位随插件系统一并实装。
          </p>

          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {PLUGIN_REGISTRY.map((d) => {
              const caps = pluginCaps(d);
              const st = pluginState[d.id] ?? { enabled: false, panelOpen: false };
              const isEval = d.id === "evaluate";
              const ready = d.status === "ready";
              // 状态徽标（每个插件自己决定显示什么；卡片只负责排版）
              const badges: { text: string; tone: "ok" | "muted" | "warn" }[] = [];
              if (ready) {
                if (caps.canToggle) badges.push({ text: st.enabled ? (isEval ? "已开启 · 每轮自动评估" : "已开启") : "已关闭", tone: st.enabled ? "ok" : "muted" });
                if (isEval && evals.length > 0) badges.push({ text: `已评 ${evals.length} 条`, tone: "muted" });
                if (d.id === "portrait-gen") {
                  const g = genList.find((x) => x.id === genSel);
                  if (g) badges.push({ text: g.implemented ? g.name : `${g.name}·未实装`, tone: g.implemented ? "ok" : "warn" });
                }
                if (caps.needsStory && !currentStory) badges.push({ text: "需进入故事", tone: "warn" });
              }
              const state: PluginState = {
                enabled: st.enabled,
                panelOpen: st.panelOpen,
                badges,
                busy: isEval ? evalBusy : false,
                runLabel: isEval ? "评估本节点最后一条回复" : undefined,
                runHint: caps.needsStory && !currentStory ? "请先进入一个故事" : undefined,
                panel: isEval ? (
                  <EvalPanel
                    T={T}
                    dimLabel={EVAL_DIM_LABEL}
                    records={evals}
                    busy={evalBusy}
                    canRun={!!currentStory && !!node}
                    noStoryHint="请先进入一个故事并选中一个剧情节点"
                    onRun={evalLastReply}
                    onRefresh={loadEvals}
                  />
                ) : d.id === "portrait-gen" ? (
                  <PortraitGenPanel
                    T={T}
                    generators={genList}
                    selected={genSel}
                    onSelect={(id) => { setGenSel(id); setGenProbe(null); }}
                    baseUrl={genUrl}
                    onBaseUrl={setGenUrl}
                    probing={genProbing}
                    probe={genProbe}
                    onProbe={probeGenerator}
                    generating={genBusy}
                    onGenerate={runGenerate}
                    lastSummary={genSummary}
                    charOptions={chars.map((c) => ({ id: c.id, name: c.name }))}
                    defaultCharacterId={null}
                    cloud={cloudForm}
                    onCloud={patchCloud}
                    onSaveCloud={saveCloudConfig}
                    savingCloud={cloudSaving}
                    placeholderHint="可用占位符：{{prompt}} {{negative}} {{width}} {{height}} {{batch}} {{seed}} {{steps}} {{cfg}} {{model}} {{api_key}}"
                    prefill={genPrefill}
                  />
                ) : undefined,
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
                      if (isEval) setMsg(on ? "✅ 评估模型：已开启每轮自动评估（会多调一次 API）" : "✅ 评估模型：已关闭自动评估（仍可手动评估与查看）");
                    } catch (err) { setMsg("❌ 保存失败: " + String(err)); }
                  } : undefined}
                  onRun={isEval && ready ? evalLastReply : undefined}
                  onTogglePanel={caps.canViewPanel && ready ? () => {
                    const next = !st.panelOpen;
                    setPluginFlag(d.id, { panelOpen: next });
                    if (next && isEval) loadEvals();
                  } : undefined}
                />
              );
            })}
          </div>
          <p style={{ fontSize: 11.5, color: T.text3, marginTop: 14 }}>
            插件系统目标：应对第三方服务下线风险、保持软件轻量化。加一个插件 = 在注册表里加一条描述 +（可选）提供一个面板，
            卡片布局 / 总开关 / 手动执行 / 状态徽标由通用插件卡统一渲染。
          </p>
        </div>
      )
  );
}
