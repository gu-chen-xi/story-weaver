# 插件开发（PLUGINS.md）

> 本文件最后同步 **2026-10-29**（3.10：注册表三张卡 —— 新增 `stat-growth`（三维数值，默认关））。
> 面向"要给织梦机加一个插件的人"——包括以后的我们自己。
>
> 读完这份应该能做到：**不改 `PluginsPage`、不改 `PluginCard`、不改 `usePlugins`**，
> 只加一个目录 + 注册表一行，插件就能出现在界面上并被开关。

---

## 一、插件是什么

一个插件 = **一条元信息** + **一个可选的界面** + **可选的卡片装饰**（徽标 / 手动运行 / 开关提示）。

它**不是**动态加载的外部代码。内置插件是主包的一部分（静态 `import`），
所以插件不能"绕过"类型检查，也不能做核心代码做不到的事（见 §六 安全边界）。

五件东西的职责划分（四份宿主代码 + 插件自己的目录）：

| 文件 | 职责 | 加插件时要改吗 |
|---|---|---|
| `src/plugins/types.ts` | 契约：元信息 / 能力 / 状态 / 模块的形状 | ❌ |
| `src/plugins/PluginCard.tsx` | 通用卡片：只排版，**不认任何 id** | ❌ |
| `src/plugins/registry.tsx` | 注册表：把各插件的模块列出来 | ✅ **加一行** |
| `src/views/PluginsPage.tsx` | 遍历注册表 + 把宿主数据交给插件 | ❌ |
| `src/plugins/builtin/<id>/` | 插件自己的元信息、界面、徽标 | ✅ **新建目录** |

---

## 二、三步加一个插件

### 第 1 步：建目录

以 `hello` 为例：

```
src/plugins/builtin/hello/
  manifest.ts     元信息（id / 名称 / 状态 / 能力声明）
  panel.tsx       界面（可选，没有就不写）
  index.tsx       导出 PluginModule（把 hostData 适配成自己的 props）
```

直接抄 `src/plugins/_template/` 的三个文件（`manifest.ts` / `panel.tsx` / `index.tsx`）改名即可 —— 那个目录是**示例模板**，
`tsc` 会一直检查它，所以它不会腐烂。

### 第 2 步：写 manifest

```ts
import type { PluginDescriptor } from "../../types";

export const HELLO_ID = "hello";          // id 会被引用的地方用常量，不要写死字符串

export const HELLO_MANIFEST: PluginDescriptor = {
  id: HELLO_ID,
  icon: "👋",
  name: "示例插件",
  status: "ready",                        // "planned" = 占位，界面只展示"为什么还没做"
  desc: "一句话说明它给用户带来什么（写清边界：没配置时也不会坏事）。",
  caps: {
    canToggle: true,                      // 有总开关
    canRunManually: false,                // 界面上有没有「运行一次」按钮
    canViewPanel: true,                   // 有没有可展开的面板
    needsStory: false,                    // 是否必须先进入一个故事
  },
  toggleLabel: "开启后做什么",            // 开关右侧的文案（canToggle 时用）
  toggleHint: "开启会多消耗一次 API 调用", // 开关的代价，必须诚实写出来
};
```

**能力声明是数据，不是装饰**：卡片上出现哪些控件、开关能不能点、要不要提示
"先进入故事"，都由它决定。以前这些散在 `PluginsPage` 的 `if` 里。

### 第 3 步：写模块（`index.tsx`）

```tsx
import type { PluginModule } from "../../types";
import { HELLO_MANIFEST } from "./manifest";
import { HelloPanel } from "./panel";

export const HELLO_MODULE: PluginModule = {
  descriptor: HELLO_MANIFEST,

  // 界面：宿主机给一份统一的 hostData，你自己挑需要的
  renderPanel: ({ T }) => <HelloPanel T={T} />,

  // 卡片徽标（可选）：只有你知道该显示什么
  badges: ({ state }) => state.enabled
    ? [{ text: "已开启", tone: "ok" }]
    : [{ text: "已关闭", tone: "muted" }],

  // 手动运行按钮（可选，配合 caps.canRunManually）
  // run: ({ host }) => ({ label: "跑一次", busy: false, canRun: host.hasStory, onClick: () => {} }),

  // 开关切换后给用户的一句提示（可选）
  toggleMessage: (on) => (on ? "✅ 示例插件：已开启" : "✅ 示例插件：已关闭"),
};
```

然后在 `src/plugins/registry.tsx` 里加一行：

```ts
import { HELLO_MODULE } from "./builtin/hello";

export const BUILTIN_PLUGINS: PluginModule[] = [
  OLLAMA_PLACEHOLDER,
  STAT_GROWTH_MODULE,
  PORTRAIT_GEN_MODULE,
  HELLO_MODULE,          // ← 就这一行
];
```

> 2026-10-29 起是三张卡：`stat-growth`（三维数值，默认关）、`portrait-gen`（立绘生成）、Ollama（`planned` 占位）。「评估模型」插件已在 2026-10-27 连后端一起删掉。

**结束。** 卡片、开关、面板、徽标、配置持久化都是现成的。

---

## 三、宿主机给你什么（`PluginHostData`）

插件不 import App、不读全局状态；宿主只交这一份数据（见 `src/plugins/host.ts`）：

```ts
type PluginHostData = {
  T: PluginCardTokens;            // 主题（面板要用的颜色/边框都在里面）
  hasStory: boolean;              // 是否已进入故事
  hasNode: boolean;               // 是否选中了剧情节点
  notify: (msg: string) => void;  // 给用户一条提示
  setFlag: (id, patch) => void;   // 改自己的开关/面板展开状态
  portraitGen: { … };             // 「立绘生成」用的数据（别的插件用不到就忽略）
};
```

> 具体数据（`portraitGen`）暂时是"所有插件的并集"。
> 插件多了以后如果嫌臃肿，可以改成按需注入 —— 但**不要**改回"宿主按 id 拼 props"，
> 那正是 v3.5.7 消灭的东西（守卫会红）。
>
> **面板可以直接用的全局设施（2026-10-25）**：`useConfirm()`（`src/confirm.tsx`）与 `useToast()`（`src/toast.tsx`）
> 都是 Provider 级的，插件面板 `import` 就能用 —— **不用**往 `PluginHostData` 里加东西。
> 立绘生成面板就是这么用的：清除 Key 走 `confirm({ danger: true })`，结果用小字留在控件旁边（T2 就地反馈）。
> 分层规矩见 [SPEC §4.13](SPEC.md)：**操作结果**（保存/删除/导入）用 toast，**某个控件的结果**留在原地。

---

## 四、配置与状态

| 你需要存的 | 怎么存 |
|---|---|
| 开关（`enabled`）| 自动：键名 `plugin.<id>.enabled`，宿主通过 `set_config` 落盘 |
| 面板展开（`panelOpen`）| 自动：同上，存在 `pluginState` |
| 其他配置（URL、模型名…）| 自己用 `get_config` / `set_config`，键名 `plugin.<id>.<字段>` |
| 云端绘图通道（立绘生成用）| 历史键名 `portrait.cloud_*`（不是 `plugin.<id>.*`）：`cloud_url` / `cloud_model` / `cloud_request_url` / `cloud_body_template` / `cloud_response_path`（自定义通道）/ `cloud_openai_response_path`（OpenAI 通道，**两份分开存**）/ `cloud_auth_style` / `cloud_extra_headers` / `cloud_ok_status` / `cloud_response_raw_b64`。新插件一律用 `plugin.<id>.*`，别再沿用 `portrait.cloud_*` |
| **API Key** | ❌ 不许进 configs 表 —— 见 §六 |

历史遗留的键名（插件化之前用的专属键名）在
`src/plugins/config.ts` 的 `LEGACY_PLUGIN_KEYS` 里映射；新增插件不需要动它（目前表是空的）。

---

## 五、测试要求

* **能把逻辑抽成纯函数就抽**（放 `src/lib/`，前端测试能覆盖到）。
  例：`pickGap()`（选哪段空档）就是从不适合测试的组件里抽出来的。
* 面板本身不写单测（测试只编译 `src/lib`，见 `tsconfig.split.json`）。
* 逻辑之外还有**静态守卫**（`tests/*.test.cjs` 直接读源码字符串，不跑 DOM）：
  `noNativeConfirm` 扫 `src/**` 不许出现原生 `window.confirm` / 未 `await` 的 `confirm(` —— 插件面板同样受约束。
* 动过插件系统后跑：`npx tsc --noEmit`、`npm run test`、`cargo test --lib`。

---

## 六、安全边界（硬规则，违反直接红）

插件是**内置代码**，但仍受全套红线约束（完整版见仓库根 `AGENTS.md` §二 与 `SECURITY.md`）：

1. **不执行外部程序**：不许 `Command::new` / `tauri-plugin-shell`；
2. **AI 输出只当文本**：不解析 `tool_call` / `function_call`，不做"AI 说什么就执行什么"；
3. **前端无注入面**：不许 `dangerouslySetInnerHTML` / `innerHTML` / `eval`；
4. **CSP 不放松**：`connect-src` 只有 `'self' ipc: http://ipc.localhost`；
5. **API Key 只进凭据库**：不得写进 configs、日志、错误串、前端存储；
6. **权限只加必要的**：`src-tauri/capabilities/` 增权限必须在提交信息里说明理由。

插件**不该**做的事（设计约束）：

* 不直接读写数据库（走后端命令）；
* 不接管核心链路（剧情生成、时间线、记忆）—— 那些不是插件点；
* 不在 `PluginsPage` / `PluginCard` 里加 `if (id === …)`（守卫 `plugins_page_must_not_branch_by_id` 会失败）。

---

## 七、守卫会检查什么

`src-tauri/src/frontend_guard.rs`（`cargo test --lib` 会跑）：

| 测试 | 管什么 |
|---|---|
| `plugins_page_must_not_branch_by_id` | `PluginsPage.tsx` 不许出现 `d.id === "portrait-gen"` 这类分支、不许用旧 `PLUGIN_REGISTRY` |
| `plugin_template_must_not_be_registered` | `registry.tsx` 里不许引用 `_template`（模板只当例子，不许变成第四张卡）；模板必须仍用契约类型 |
| `plugin_docs_must_explain_the_three_steps` | 本文件必须还在，且含"三步加插件"的关键小节（文档被删会红）|
| `extracted_modules_carry_real_content` | 插件各文件的行数下限（防止"留个空壳"）|
| `extracted_modules_must_not_be_reinlined` | 已抽出的东西不许抄回 `App.tsx` |

> `frontend_guard.rs` 共 11 项测试；其余几项分别管：`App.tsx` 行数上限、测试命令真的接在 `npm test` 上、
> 测试文件不许被掏空、导演监视器面板顺序、构建脚本 `sync:run` 的 dev 标志判据。清单见 [测试用例.md](测试用例.md)。

---

## 八、常见问题

**Q：我的插件需要新的宿主数据（比如"当前分支"）怎么办？**
A：加在 `PluginHostData` 里（`src/plugins/host.ts`），宿主在 `PluginsPage` 里填。
不要改成"插件直接读 App 状态"。

**Q：我的插件要一个独立页面（不是卡片面板）？**
A：那是"页面级功能"，不属于插件系统 —— 走 `src/views/` + `App` 的 view 路由。
插件系统的边界是"卡片 + 面板"。

**Q：面板要不要懒加载？**
A：现在不用（内置插件，静态打包）。若将来做"外部安装的插件"，
`PluginModule.renderPanel` 这一层就是改造点（换成 `lazy()` + 动态 import）。

---

## 九、相关代码

| 文件 | 看什么 |
|---|---|
| `src/plugins/types.ts` | 契约全文（最权威）|
| `src/plugins/registry.tsx` | 现有两个插件怎么注册的 |
| `src/plugins/_template/` | 可抄的骨架 |
| `src/plugins/builtin/portrait-gen/` | 带大面板 + 云端表单的例子 |
