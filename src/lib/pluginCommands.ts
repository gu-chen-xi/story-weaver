// 插件能调的后端命令白名单 + **能力域**（2026-11-07 建立，2026-11-10 按能力域收窄）。
//
// 两件事，别混：
//   ① **白名单**：插件是在同一个 webview 里跑的 JS，不能直接拿 `invoke`（否则能调后端任何一条
//      已注册命令：删故事、回滚快照、清调试记录…）。宿主只交 `host.call(name, args)`，
//      name 必须落在下面这张表里，表外一律拒绝。
//   ② **能力域**：每条命令归属一个域；插件必须在自己的 manifest 里用
//      `caps.commandCaps` **显式声明**要用哪个域 —— 于是立绘插件调不到 Ollama 的配置、
//      三维数值插件一条命令都调不到。白名单从"全局一张表"变成"**每个插件各自的子集**"。
//
// 本文件**只放数据与纯判断**（不 import `nice invoke`），这样 `node:test` 能直接测；
// 真正调后端的那层在 `lib/pluginInvoke.ts`（`makePluginCall`）。
//
// 维护规则：
//   · 只加**插件自己需要的**；宁可让插件报错，也不要图省事把 `*_delete` / `rollback_*` 加进来；
//   · 新增命令先问：这是插件该做的事吗？加完记得在对应 manifest 的 `commandCaps` 里声明；
//   · `why` 一栏必须写 —— review 时看的就是它。

/** 能力域。加一个域 = 这里加一个值 + 下面给它分配命令 + 在 manifest 里声明 */
export type PluginCapDomain = "ollama" | "portrait-gen";

type CommandSpec = {
  name: string;
  cap: PluginCapDomain;
  /** 为什么插件需要它（一句话，review 时看这个） */
  why: string;
};

/** 插件可调命令的**唯一**清单。顺序按域分组，README/报错信息都按它列。 */
export const PLUGIN_COMMAND_SPECS = [
  { name: "ollama_get", cap: "ollama", why: "读面板配置：地址 / 模型名 / 上下文档位 / 伏笔扫描开关" },
  { name: "ollama_set", cap: "ollama", why: "保存上面这些配置（含插件自己那几个键）" },
  { name: "ollama_probe", cap: "ollama", why: "「检查连接」：只读 /api/version，不启动也不下载" },
  { name: "ollama_test_chat", cap: "ollama", why: "「测试对话」：发一句验证模型能回话" },

  { name: "generator_list", cap: "portrait-gen", why: "列出这台机器上可用的出图通道" },
  { name: "generator_probe", cap: "portrait-gen", why: "探测某个通道通不通" },
  { name: "generator_generate", cap: "portrait-gen", why: "真的出一张图" },
  { name: "generator_cleanup_temp", cap: "portrait-gen", why: "清掉出图临时文件" },
  { name: "generator_list_temp", cap: "portrait-gen", why: "列出出图临时文件" },
  { name: "generator_ingest", cap: "portrait-gen", why: "把生成的图入库（与手工导入同一入口）" },
  { name: "generator_get_cloud_config", cap: "portrait-gen", why: "读云端通道配置（地址 / 模板 / 取图路径 / 图床白名单）" },
  { name: "generator_save_cloud_config", cap: "portrait-gen", why: "保存云端通道配置（Key 仍只进凭据库）" },
  { name: "generator_clear_cloud_key", cap: "portrait-gen", why: "只清「绘图通道」那一条 Key" },
] as const satisfies readonly CommandSpec[];

/** 命令名的字面量联合（给 `host.call` 的类型用） */
export type PluginCommand = (typeof PLUGIN_COMMAND_SPECS)[number]["name"];

/** 兼容旧引用：只要命令名 */
export const PLUGIN_COMMANDS: readonly PluginCommand[] = PLUGIN_COMMAND_SPECS.map((s) => s.name);

/** 全部能力域（报错信息里给"插件可以申请哪些"用） */
export const PLUGIN_CAP_DOMAINS: readonly PluginCapDomain[] = ["ollama", "portrait-gen"];

/** 这条命令属于哪个域；不在白名单里返回 `undefined` */
export function commandCapOf(name: string): PluginCapDomain | undefined {
  return PLUGIN_COMMAND_SPECS.find((s) => s.name === name)?.cap;
}

/** 某个域下有哪些命令（写文档 / 拼报错信息用） */
export function commandsOfCap(cap: PluginCapDomain): readonly string[] {
  return PLUGIN_COMMAND_SPECS.filter((s) => s.cap === cap).map((s) => s.name);
}

export type CallCheck = { ok: true } | { ok: false; message: string };

/**
 * 插件 `pluginId`（声明了 `declared` 这几个能力域）能不能调 `name`？
 *
 * **纯函数**：不抛异常、不碰后端，方便单测覆盖每一条拒绝理由。
 * 真正的调用在 `lib/pluginInvoke.ts`：先问这里，`ok` 才 `invoke`。
 */
export function checkPluginCall(
  pluginId: string,
  declared: readonly PluginCapDomain[],
  name: string,
): CallCheck {
  const cap = commandCapOf(name);
  if (!cap) {
    return {
      ok: false,
      message:
        `插件不能调用「${name}」：它不在插件命令白名单里。` +
        `插件可用的命令按能力域分组（${PLUGIN_CAP_DOMAINS.join(" / ")}），共 ${PLUGIN_COMMAND_SPECS.length} 条。`,
    };
  }
  if (!declared.includes(cap)) {
    return {
      ok: false,
      message:
        `插件「${pluginId}」没有声明能力域「${cap}」，因此不能调用「${name}」。` +
        `要它调得动，请在该插件 manifest 的 caps.commandCaps 里加上「${cap}」（该域下有 ${commandsOfCap(cap).length} 条命令）。`,
    };
  }
  return { ok: true };
}
