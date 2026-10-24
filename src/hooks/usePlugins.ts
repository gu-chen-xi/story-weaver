// 插件（立绘生成通道那一块）的运行期状态与动作（从 App.tsx 抽出，v3.4）。
//
// ## 为什么现在抽
//
// 「插件」是接下来要**长大**的板块：加一个内置插件，按现在的结构就意味着往 App.tsx 里
// 再塞一组 state + 几个动作（现在已经占了约 200 行）。把"一个插件的状态与动作"收成一个 hook，
// 以后每加一个插件就多一个 `useXxxPlugin()`，App.tsx 只需要 hook 调用 + 渲染。
//
// ## 这里是模板，不是框架
//
// 没有做"插件声明式 API"（那要先定插件能做什么、需要什么能力，属于以后的事）。
// 现在只把**已经存在的一坨状态**搬出 App，让出口清晰：
//
//     输入：会话侧的 currentStory / setMsg / setView / pluginState / genList / genSel / cloudForm
//     输出：状态 + 动作（openGenPanel / cloudOverrides / runGenerate / setPluginFlag / pluginOn）
//
// ## 逻辑与拆分前逐行一致
//
// 特别保留的一条业务约定：生成结果**一律烧录「AI」标识**（法规要求，用户 2026-10-12 定，
// 界面不提供开关）。它就在 runGenerate 里，别为了"更灵活"把它变成可选。

import { useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CloudForm, GenDescriptor, GenProbe } from "../plugins";
import { pluginKeyOf } from "../plugins/config";
import { PORTRAIT_GEN_ID } from "../plugins/builtin/portrait-gen/manifest";

// 插件总开关的配置键规则在 `plugins/config.ts`（含 `evaluate.auto` 这类历史键的兼容）

export type PluginRunState = { enabled: boolean; panelOpen: boolean };

export type UsePluginsDeps = {
  currentStory: { id: number } | null;
  setMsg: Dispatch<SetStateAction<string>>;
  /// 只用来跳转到插件页（App 的 setView 传进来即可）
  setView: (v: "plugins") => void;
  pluginState: Record<string, PluginRunState | undefined>;
  setPluginState: Dispatch<SetStateAction<Record<string, PluginRunState>>>;
  genList: GenDescriptor[];
  genSel: string;
  cloudForm: CloudForm;
  loadGenerators: () => void;
  loadCloudConfig: () => void;
  loadPortraits: () => Promise<void>;
};

export function usePlugins(deps: UsePluginsDeps) {
  const {
    currentStory, setMsg, setView, pluginState, setPluginState,
    genList, genSel, cloudForm, loadGenerators, loadCloudConfig, loadPortraits,
  } = deps;

  // 插件运行期状态（注册表驱动：加插件只需在 PLUGIN_REGISTRY 里加一条）
  const setPluginFlag = (id: string, patch: Partial<{ enabled: boolean; panelOpen: boolean }>) =>
    setPluginState((prev) => ({ ...prev, [id]: { ...(prev[id] ?? { enabled: false, panelOpen: false }), ...patch } }));

  // 第四批 ⑥ P1：立绘生成通道（后端注册表下发，如实报告每个通道是否已实装）
  const [genUrl, setGenUrl] = useState("http://127.0.0.1:7860");
  const [genProbing, setGenProbing] = useState(false);
  const [genProbe, setGenProbe] = useState<GenProbe | null>(null);
  const [genBusy, setGenBusy] = useState(false);
  const [genSummary, setGenSummary] = useState("");
  const [cloudSaving, setCloudSaving] = useState(false);

  /** 面板预填（从角色卡 / 素材重出带过来）；stamp 变化才灌入，避免覆盖用户正在编辑的内容 */
  const [genPrefill, setGenPrefill] = useState<{
    stamp: number; prompt?: string; negative?: string; width?: number; height?: number;
    seed?: string; steps?: string; cfg?: string; characterId?: number | null;
    basePortraitId?: number | null; note?: string;
  } | null>(null);

  /** 打开立绘生成面板并（可选）预填 —— 所有「去生成」的入口都走这里 */
  const openGenPanel = (prefill?: { prompt?: string; negative?: string; width?: number; height?: number; seed?: string; steps?: string; cfg?: string; characterId?: number | null; basePortraitId?: number | null; note?: string }) => {
    if (prefill) setGenPrefill({ ...prefill, stamp: Date.now() });
    setView("plugins");
    setGenProbe(null);
    loadGenerators();
    loadCloudConfig();
    // 立绘生成面板的宿主是插件页；id 从注册表拿，避免写死字符串
    setPluginFlag(PORTRAIT_GEN_ID, { panelOpen: true });
  };

  /** 把界面当前值组装成"覆盖值"（留空字段不传，后端会用已保存值） */
  const cloudOverrides = () => ({
    base_url: cloudForm.base_url || null,
    model: cloudForm.model || null,
    url: cloudForm.url || null,
    body_template: cloudForm.body_template || null,
    response_path: cloudForm.response_path || null,
    auth_style: cloudForm.auth_style || null,
    extra_headers: cloudForm.extra_headers || null,
    api_key: cloudForm.api_key || null,
  });

  // 生成 → 逐张入库（入库走既有 portrait_import，严守 R1）
  const runGenerate = async (o: { prompt: string; negative: string; width: number; height: number; batch: number; seed: string; steps: string; cfg: string; characterId: number | null; name: string }) => {
    if (!currentStory) { setMsg("⚠️ 请先进入一个故事（生成立绘需要知道绑定给哪个角色）"); return; }
    setGenBusy(true);
    setGenSummary("");
    try {
      const res = await invoke<{ files: string[]; generator_id: string; seeds: number[]; meta: string }>("generator_generate", {
        generatorId: genSel,
        req: {
          prompt: o.prompt, negative: o.negative, width: o.width, height: o.height,
          steps: o.steps ? Number(o.steps) : null, cfg: o.cfg ? Number(o.cfg) : null,
          seed: o.seed ? Number(o.seed) : null, batch: o.batch, base_portrait_id: null,
        },
        config: cloudOverrides(),
      });
      if (!res.files || res.files.length === 0) { setMsg("⚠️ 通道没有返回图片"); return; }
      // 逐张入库（若开了烧录开关，先把 AI 标识烧进图里 —— 只加标识、不改内容）
      let ok = 0;
      let stamped = 0;
      const names: string[] = [];
      for (let i = 0; i < res.files.length; i++) {
        let filePath = res.files[i];
        // 固定烧录「AI」标识（显式角标 + 隐式元数据）。失败不阻断：按原图入库，内容不受影响。
        try {
          filePath = await invoke<string>("stamp_ai_label", { path: filePath });
          stamped++;
        } catch (e) {
          console.error("烧录 AI 标识失败，按原图入库：", e);
        }
        try {
          const item = await invoke<{ id: number; name: string; ver: string }>("generator_ingest", {
            req: {
              file: filePath,
              name: o.name + (res.files.length > 1 ? ` #${i + 1}` : ""),
              role: "",
              tags: "AI生成",
              group: "AI 生成",
              story_id: currentStory.id,
              character_id: o.characterId,
              generator_id: res.generator_id,
              channel: genList.find((g) => g.id === genSel)?.channel ?? "local",
              prompt: o.prompt, negative: o.negative,
              seed: String(res.seeds[i] ?? res.seeds[0] ?? ""),
              steps: o.steps, cfg: o.cfg, model: "",
              base_portrait_id: null,   // 入库请求字段（写入素材库 derived_from 列）
            },
          });
          ok++;
          names.push(`${item.name}（${item.ver}）`);
        } catch (e) {
          console.error("入库失败：", e);
        }
      }
      setGenSummary(`✅ 已入库 ${ok}/${res.files.length} 张${stamped > 0 ? `（其中 ${stamped} 张已烧录 AI 标识）` : ""}：${names.join("；")}`);
      await loadPortraits();
      await invoke("generator_cleanup_temp", { keep: [] }).catch(() => {});
    } catch (e) {
      setMsg("❌ 生成失败: " + String(e));
    } finally {
      setGenBusy(false);
    }
  };

  // 键名规则（含历史遗留键）统一由 plugins/config.ts 决定，这里不再写死任何一个插件 id
  const pluginKey = (id: string) => pluginKeyOf(id);
  const pluginOn = (id: string) => pluginState[id]?.enabled ?? false;

  return {
    // 运行期状态
    genUrl, setGenUrl, genProbing, setGenProbing, genProbe, setGenProbe,
    genBusy, genSummary, cloudSaving, setCloudSaving,
    genPrefill, setGenPrefill,
    // 动作
    openGenPanel, cloudOverrides, runGenerate, setPluginFlag, pluginKey, pluginOn,
  };
}
