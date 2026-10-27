// 插件配置键（v3.5.7）。
//
// 统一键名：`plugin.<id>.enabled`。
// 早期版本有插件在"插件化之前"就用了自己的键名，这些**必须继续认**，
// 否则老用户的开关状态会丢 —— 所以这里维护一张 legacy 表。
// （2026-10-27：评估模型整套删除，它那条 `evaluate.auto` 兼容项也一并删了。）
//
// 为什么把它从 hook 里挪出来：原来 `PLUGIN_KEY` 是 `usePlugins.ts` 里的一个字面量对象，
// 只有 hook 自己用；放到这里之后，"某个插件有专属键名"是**数据**，
// 加插件时只需在表里补一行，不用改逻辑。

/** 默认键名：插件化之后新增的插件都用这个 */
export const pluginConfigKey = (id: string) => `plugin.${id}.enabled`;

/** 历史键名（插件化之前就在用的），键 = 插件 id */
export const LEGACY_PLUGIN_KEYS: Record<string, string> = {};

/** 某个插件实际使用的配置键（有 legacy 就用 legacy） */
export const pluginKeyOf = (id: string) => LEGACY_PLUGIN_KEYS[id] ?? pluginConfigKey(id);
