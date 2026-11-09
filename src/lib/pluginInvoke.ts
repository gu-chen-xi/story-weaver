// 把「白名单 + 能力域」接到真正的 `invoke` 上（2026-11-10）。
//
// 为什么单独一个文件：`lib/pluginCommands.ts` 只放数据与纯判断（能被 `node:test` 直接跑），
// 这一层才 import `@tauri-apps/api/core`。分开之后，测试不用碰 Tauri 的模块解析。
//
// 用法（只有宿主机该用）：
//   const call = makePluginCall(descriptor.id, pluginCaps(descriptor).commandCaps ?? []);
//   <PluginCard host={{ ...hostData, call }} />
// 插件自己拿不到 `invoke`，只能拿到这个 bind 过的 `call`。

import { invoke } from "@tauri-apps/api/core";
import { checkPluginCall, type PluginCapDomain } from "./pluginCommands";

/** 宿主机交给插件的后端通道签名（`PluginHostData.call` 就是它） */
export type PluginCaller = <T>(name: string, args?: Record<string, unknown>) => Promise<T>;

/**
 * 给**某一个插件**造一个 `call`。
 *
 * 关键点：把"这个插件声明了哪些能力域"**烘进闭包**，调用时逐条校验 ——
 * 这样插件之间互相调不动（立绘插件拿不到 `ollama_*`），没声明域的插件一条也调不到。
 */
export function makePluginCall(pluginId: string, declared: readonly PluginCapDomain[]): PluginCaller {
  return async function call<T>(name: string, args?: Record<string, unknown>): Promise<T> {
    const verdict = checkPluginCall(pluginId, declared, name);
    if (!verdict.ok) throw new Error(verdict.message);
    return invoke<T>(name, args);
  };
}
