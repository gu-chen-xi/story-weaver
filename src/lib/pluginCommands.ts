// 插件能调的后端命令**能力域**（2026-11-07，安全审计 F-05 的一半）。
//
// 为什么要有它：插件是**在同一个 webview 里跑的 JS**，如果直接给它 `invoke`，
// 它就能调后端**任何**一个已注册命令（删故事、回滚快照、清调试记录…）。
// 现在宿主机只交一个 `call(name, args)`，name 必须落在下面这张表里 ——
// 表外的一律拒绝，而且错误信息里会写清「插件只能调这些」。
//
// 维护规则（很重要）：
//   · 只加**插件自己需要的**；宁可让插件报错，也不要图省事把 `*_delete` / `rollback_*` 加进来；
//   · 这张表是插件的**唯一**入口，所以不需要（也不应该）再给插件别的方式拿到 invoke；
//   · 新增命令时先问：这是插件该做的事吗？不是就别加。

import { invoke } from "@tauri-apps/api/core";

export const PLUGIN_COMMANDS = [
  // Ollama 本地模型插件
  "ollama_get",
  "ollama_set",
  "ollama_probe",
  "ollama_test_chat",
  // 立绘生成插件（生成 / 探测 / 临时文件 / 入库 / 云端通道配置）
  "generator_list",
  "generator_probe",
  "generator_generate",
  "generator_cleanup_temp",
  "generator_list_temp",
  "generator_ingest",
  "generator_get_cloud_config",
  "generator_save_cloud_config",
  "generator_clear_cloud_key",
] as const;

export type PluginCommand = (typeof PLUGIN_COMMANDS)[number];

export const isPluginCommand = (name: string): name is PluginCommand =>
  (PLUGIN_COMMANDS as readonly string[]).includes(name);

/// 插件调用后端的**唯一**通道。
export async function pluginCall<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isPluginCommand(name)) {
    throw new Error(`插件无权调用命令「${name}」（可用：${PLUGIN_COMMANDS.join("、")}）`);
  }
  return invoke<T>(name, args);
}
