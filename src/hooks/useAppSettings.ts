// 应用设置页 / 顶栏的状态（v3.5.6，A 步第二块）。
//
// 归属：`views/SettingsModal`（设置弹窗）、`views/ParamsPage`（生成参数页）、
// 顶栏「模型：xxx」徽标、以及生成流程里的 `autoAnalyze`。
//
// `loadSettings` 还负责读**插件开关**（有 `canToggle` 能力的插件才有配置键），
// 所以它需要 `setPluginState` —— 由 `usePlugins` 提供，调用方传进来即可，
// 这样这个 hook 自己不依赖 plugins 的内部结构。

import { useCallback, useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PluginRunState } from "./usePlugins";
import { pluginCaps } from "../plugins";
import { PLUGIN_REGISTRY } from "../constants";

// 用 `usePlugins` 导出的真实类型（原来这里自己写了一份 { enabled, panelOpen }，
// 会随插件状态结构变化而悄悄漂移 —— 现在类型不匹配会直接编译失败）
/// 与 `usePlugins` 里的 state 同一形状（App 直接把这个 setter 传进来，不需要断言）
export type PluginStateMap = Record<string, PluginRunState>;

export type UseAppSettingsDeps = {
  /// `usePlugins()` 返回的 setter（只用来把读到的插件开关写回去）
  setPluginState: Dispatch<SetStateAction<PluginStateMap>>;
  /// 读取当前插件面板展开状态时用（loadSettings 里要保留它）
  getPluginState: () => Record<string, PluginRunState | undefined>;
  /// 插件配置键（`usePlugins` 里维护，含个别插件的特例键）
  pluginKey: (id: string) => string;
};

export function useAppSettings({ setPluginState, getPluginState, pluginKey }: UseAppSettingsDeps) {
  const [showSettings, setShowSettings] = useState(false);
  const [modelName, setModelName] = useState("");
  const [ctxWindow, setCtxWindow] = useState(32768);
  const [nRecentInput, setNRecentInput] = useState("10");
  const [autoAnalyze, setAutoAnalyze] = useState(true);

  const loadSettings = useCallback(async () => {
    try {
      // App 只读"已生效值"（顶栏模型名 / 参数页 N / 生成流程的 autoAnalyze）；
      // 其余草稿与 Key 状态由「设置」弹窗自己读取（见 views/SettingsModal.tsx）。
      const mn = await invoke<string | null>("get_config", { key: "model.name" });
      setModelName(mn ?? "");
      const cw = await invoke<string | null>("get_config", { key: "model.context_window" });
      if (cw) {
        const n = Number(cw);
        if (n > 0) setCtxWindow(n);
      }
      const nr = await invoke<string | null>("get_config", { key: "context.n_recent" });
      if (nr) setNRecentInput(nr);
      const aa = await invoke<string | null>("get_config", { key: "evolution.auto_analyze" });
      setAutoAnalyze(aa !== "false");
      // 插件开关：逐一读取（有能力开关的插件才有配置键）
      const loaded: PluginStateMap = {};
      for (const d of PLUGIN_REGISTRY) {
        if (!pluginCaps(d).canToggle) continue;
        const v = await invoke<string | null>("get_config", { key: pluginKey(d.id) });
        loaded[d.id] = { enabled: v === "true", panelOpen: getPluginState()[d.id]?.panelOpen ?? false };
      }
      setPluginState((prev) => ({ ...prev, ...loaded }));
    } catch (e) {
      console.error(e);
    }
  }, [setPluginState, getPluginState, pluginKey]);

  return {
    showSettings, setShowSettings,
    modelName, setModelName,
    ctxWindow, setCtxWindow,
    nRecentInput, setNRecentInput,
    autoAnalyze, setAutoAnalyze,
    loadSettings,
  };
}
