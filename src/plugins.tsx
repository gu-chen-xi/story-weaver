// 插件注册表与通用插件卡（2026-10-11 建立，为第四批 ⑥ 立绘生成等插件铺路）
//
// 设计目标：**加一个插件 = 往注册表里加一条描述 + 提供一个面板**，不用再写卡片布局、开关逻辑、状态徽标。
//
// 一个插件由三部分组成：
//   1. descriptor（元信息 + 能力声明）
//   2. state（当前状态：开关、徽标、面板是否展开）
//   3. panel（该插件自己的界面；由宿主负责渲染在卡片下方）
//
// 能力声明（capabilities）是这套东西的关键 —— 它让"插件能做什么"变成**数据**而不是散落在各个 if 里：
//   - canToggle      是否有总开关（如评估模型的「每轮自动评估」）
//   - canRunManually 是否能在界面上手动跑一次（如「评估本节点最后一条回复」）
//   - canViewPanel   是否有可展开的结果面板
//   - needsStory     是否必须先进入一个故事才能用
// 这些决定了卡片上该出现哪些控件、开关不可用时给什么提示。

import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";

/** 插件状态：规划中（不可用）/ 已实装（可用） */
export type PluginStatus = "planned" | "ready";

/** 插件能做什么 */
export type PluginCapabilities = {
  canToggle: boolean;      // 有总开关
  canRunManually: boolean; // 可手动执行
  canViewPanel: boolean;   // 有可展开面板
  needsStory: boolean;     // 需要先进入故事
};

/** 插件元信息 */
export type PluginDescriptor = {
  id: string;              // 稳定标识，同时用作配置键前缀：plugin.<id>.enabled
  icon: string;
  name: string;
  desc: string;
  status: PluginStatus;
  /** 能力声明（可部分给出，缺省按 false） */
  caps?: Partial<PluginCapabilities>;
  /** 开关文案（有总开关时用） */
  toggleLabel?: string;
  /** 开关旁的代价说明（如"会多消耗一次 API 调用"） */
  toggleHint?: string;
  /** 规划中插件的占位说明（status = planned 时显示） */
  plannedNote?: string;
};

/** 运行期状态（由宿主维护） */
export type PluginState = {
  /** 总开关是否打开 */
  enabled: boolean;
  /** 面板是否展开 */
  panelOpen: boolean;
  /** 状态徽标（如"已开启 · 每轮自动评估"） */
  badges: { text: string; tone: "ok" | "muted" | "warn" }[];
  /** 正在执行（禁用按钮并显示忙态文案） */
  busy?: boolean;
  /** 手动执行按钮的文案 */
  runLabel?: string;
  /** 手动执行按钮的提示（如"与上面的自动开关无关"） */
  runHint?: string;
  /** 面板内容 */
  panel?: ReactNode;
};

const CAPS_DEFAULT: PluginCapabilities = {
  canToggle: false,
  canRunManually: false,
  canViewPanel: false,
  needsStory: false,
};

export function pluginCaps(d: PluginDescriptor): PluginCapabilities {
  return { ...CAPS_DEFAULT, ...(d.caps ?? {}) };
}

/** 插件开关的配置键（统一命名，便于以后做"插件管理"页面或迁移） */
export const pluginConfigKey = (id: string) => `plugin.${id}.enabled`;

/** 最少依赖的插件卡；样式由宿主传入的 tokens 决定 */
export type PluginCardTokens = {
  line: string;
  lineSoft: string;
  panel: string;
  panel3: string;
  text1: string;
  text2: string;
  text3: string;
  accent: string;
  accentSoft: string;
  ok: string;
  warn: string;
  danger: string;
};

type Props = {
  descriptor: PluginDescriptor;
  state: PluginState;
  T: PluginCardTokens;
  onToggle?: (next: boolean) => void;
  onRun?: () => void;
  onTogglePanel?: () => void;
};

/** 通用插件卡：徽标 + 开关 + 手动执行 + 可展开面板 */
export function PluginCard({ descriptor: d, state: s, T, onToggle, onRun, onTogglePanel }: Props) {
  const caps = pluginCaps(d);
  const planned = d.status === "planned";
  const toneColor = (t: "ok" | "muted" | "warn") => (t === "ok" ? T.ok : t === "warn" ? T.warn : T.text3);
  const toneBorder = (t: "ok" | "muted" | "warn") => (t === "ok" ? T.ok : t === "warn" ? T.warn : T.line);

  const card: CSSProperties = {
    border: `1px ${planned ? "dashed" : "solid"} ${T.line}`,
    borderRadius: 12,
    padding: "14px 16px",
    background: T.panel,
  };

  return (
    <div style={card}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
        <span style={{ fontSize: 22, opacity: planned ? 0.6 : 1 }}>{d.icon}</span>

        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <b style={{ fontSize: 13.5 }}>{d.name}</b>
            {s.badges.map((b, i) => (
              <span key={i} style={{
                fontSize: 10, padding: "1px 8px", borderRadius: 9,
                border: `${planned ? "1px dashed" : "1px solid"} ${toneBorder(b.tone)}`,
                color: toneColor(b.tone),
              }}>{b.text}</span>
            ))}
          </div>

          <div style={{ fontSize: 12, color: T.text3, marginTop: 4, lineHeight: 1.6 }}>{d.desc}</div>

          {/* 总开关 */}
          {caps.canToggle && !planned && (
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10, flexWrap: "wrap" }}>
              <label style={{ display: "flex", alignItems: "center", gap: 7, cursor: onToggle ? "pointer" : "not-allowed", opacity: onToggle ? 1 : 0.55 }}>
                <input type="checkbox" checked={s.enabled} disabled={!onToggle}
                  onChange={(e) => onToggle?.(e.target.checked)}
                  style={{ cursor: onToggle ? "pointer" : "not-allowed", accentColor: T.accent }} />
                <span style={{ fontSize: 12, color: T.text2 }}>{d.toggleLabel ?? "启用"}</span>
              </label>
              {d.toggleHint && <span style={{ fontSize: 10.5, color: T.text3 }}>{d.toggleHint}</span>}
            </div>
          )}

          {/* 手动执行 */}
          {caps.canRunManually && !planned && (
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 8, flexWrap: "wrap" }}>
              <button onClick={onRun} disabled={s.busy || !onRun}
                style={{ fontSize: 12, padding: "4px 12px", cursor: s.busy || !onRun ? "not-allowed" : "pointer", opacity: s.busy || !onRun ? 0.6 : 1 }}>
                {s.busy ? "执行中…" : (s.runLabel ?? "手动执行")}
              </button>
              {s.runHint && <span style={{ fontSize: 10.5, color: T.text3 }}>{s.runHint}</span>}
            </div>
          )}

        </div>

        {/* 面板开关 */}
        {caps.canViewPanel && !planned && (
          <button onClick={onTogglePanel} disabled={!onTogglePanel}
            style={{ fontSize: 12, padding: "5px 13px", cursor: onTogglePanel ? "pointer" : "not-allowed", opacity: onTogglePanel ? 1 : 0.5 }}>
            {s.panelOpen ? "收起" : (caps.canRunManually ? "查看结果" : "打开面板")}
          </button>
        )}

        {/* 规划中：占位按钮 */}
        {planned && (
          <button disabled style={{ fontSize: 12, padding: "4px 12px", cursor: "not-allowed", opacity: 0.5 }}>未开放</button>
        )}
      </div>

      {/* 面板 */}
      {caps.canViewPanel && !planned && s.panelOpen && (
        <div style={{ marginTop: 14, borderTop: `1px solid ${T.lineSoft}`, paddingTop: 12 }}>{s.panel}</div>
      )}

      {planned && d.plannedNote && (
        <div style={{ fontSize: 11, color: T.text3, marginTop: 8 }}>{d.plannedNote}</div>
      )}
    </div>
  );
}

// ===== 插件面板 =====
// 各插件自己的界面，由宿主在 PluginCard 的 panel prop 里渲染。
// 第一个落地的是评估模型（ROADMAP 第三批 ④）。

export type EvalDimScore = { key: string; score: number; reason: string; quote: string };
export type EvalRecord = {
  message_id: number; node_id: number; model: string;
  dims: EvalDimScore[]; total: number; summary: string; advice: string; created_at: string;
};

export type EvalPanelProps = {
  T: PluginCardTokens;
  /** 维度 key → 中文名 */
  dimLabel: Record<string, string>;
  records: EvalRecord[];
  busy: boolean;
  /** 是否可手动评估（有故事且选中了节点） */
  canRun: boolean;
  noStoryHint: string;
  onRun: () => void;
  onRefresh: () => void;
};

/** 评估结果面板：趋势条 + 逐条明细（四维 + 证据引文 + 总评 + 建议） */
export function EvalPanel({ T, dimLabel, records, busy, canRun, noStoryHint, onRun, onRefresh }: EvalPanelProps) {
  const scoreColor = (v: number) => (v >= 4.5 ? T.ok : v >= 3.5 ? T.accent : v >= 2.5 ? T.warn : T.danger);
  return (
    <div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 10, flexWrap: "wrap" }}>
        <button onClick={onRun} disabled={busy || !canRun}>{busy ? "评估中…" : "评估本节点最后一条回复"}</button>
        <button onClick={onRefresh}>刷新</button>
        <span style={{ fontSize: 11, color: T.text3 }}>{canRun ? "手动评估随时可用，与上面的自动开关无关" : noStoryHint}</span>
      </div>

      {records.length === 0 ? (
        <p style={{ color: T.text3, fontSize: 12.5 }}>还没有评估记录。点「评估本节点最后一条回复」评一条（会调一次 API）。</p>
      ) : (
        <>
          {/* 趋势条：左旧右新 */}
          <div style={{ display: "flex", alignItems: "flex-end", gap: 4, height: 46, marginBottom: 10 }}>
            {[...records].reverse().map((e) => (
              <div key={`${e.message_id}-${e.created_at}`} title={`消息 #${e.message_id} · 总分 ${e.total}`}
                style={{ width: 16, height: Math.max(6, (e.total / 5) * 44), background: scoreColor(e.total), borderRadius: 3, opacity: 0.85 }} />
            ))}
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {records.map((e) => (
              <div key={`${e.message_id}-${e.created_at}`} style={{ border: `1px solid ${T.line}`, borderRadius: 8, padding: "10px 12px" }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6, flexWrap: "wrap", gap: 6 }}>
                  <div style={{ fontSize: 12.5, color: T.text2 }}>
                    消息 #{e.message_id} · 节点 {e.node_id} · <span style={{ color: T.text3 }}>{e.created_at}</span>
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <span style={{ fontSize: 10.5, color: T.text3 }}>{e.model}</span>
                    <span style={{ fontSize: 14, fontWeight: 700, color: scoreColor(e.total) }}>{e.total} / 5</span>
                  </div>
                </div>

                <div style={{ display: "flex", gap: 6, marginBottom: 7, flexWrap: "wrap" }}>
                  {e.dims.map((d) => (
                    <span key={d.key} title={d.reason}
                      style={{
                        fontSize: 10.5, padding: "2px 7px", borderRadius: 6, border: `1px solid ${T.lineSoft}`,
                        color: d.score >= 4 ? T.ok : d.score === 3 ? T.warn : T.danger,
                      }}>
                      {dimLabel[d.key] ?? d.key} {d.score}
                    </span>
                  ))}
                </div>

                <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.75 }}>
                  {e.dims.map((d) => (
                    <div key={d.key} style={{ marginBottom: 2 }}>
                      <b style={{ color: T.text1 }}>{dimLabel[d.key] ?? d.key}</b>：{d.reason}
                      {d.quote && (
                        <span style={{ color: T.text3 }}> —— 证据：<i>{d.quote.length > 60 ? d.quote.slice(0, 60) + "…" : d.quote}</i></span>
                      )}
                    </div>
                  ))}
                </div>
                {e.summary && <div style={{ fontSize: 12, color: T.text2, marginTop: 6 }}>总评：{e.summary}</div>}
                {e.advice && <div style={{ fontSize: 12, color: T.accent, marginTop: 3 }}>建议：{e.advice}</div>}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}


// ===== 立绘生成面板（ROADMAP 第四批 ⑥ · P1 骨架） =====
//
// 关键设计（见 docs/design/立绘系统.md）：
//   · 通道注册表由后端下发（generator_list），**如实报告**每个通道是否已实装
//   · 未实装的通道在界面上**标灰并写清原因**，不假装可用
//   · 未配置/连不上时只显示黄字提示 + 手工导入指引，**绝不阻塞**素材库与角色管理
//   · 生成结果一律经 portrait_import 入库（本面板不直接写任何文件/数据库）

export type GenSupports = {
  negative: boolean; size: boolean; steps: boolean; cfg: boolean; seed: boolean; batch: boolean;
};
export type GenDescriptor = {
  id: string; name: string; channel: string;
  needs_base_url: boolean; needs_api_key: boolean;
  supports: GenSupports;
  implemented: boolean; note: string;
};
export type GenProbe = { ok: boolean; detail: string };

/** 云端通道配置（P3）。api_key 不在这份结构里回传明文，只用 has_api_key 表示"是否已设置"。 */
export type CloudForm = {
  base_url: string;       // OpenAI 形态：填到 /v1 为止
  model: string;          // OpenAI 形态的模型名
  url: string;            // 自定义形态：完整请求地址
  body_template: string;  // 自定义形态：请求体模板（JSON 文本，可含占位符）
  response_path: string;  // 自定义形态：响应字段路径，如 b64:data.0.b64_json
  auth_style: string;     // bearer / api-key / none
  extra_headers: string;  // 额外请求头，形如 X-Org=abc;X-Foo=bar
  api_key: string;        // 仅用于"本次要保存/本次探测"的新值（留空 = 不动已存的）
  has_api_key: boolean;   // 后端已存的 Key（回显用）
};
export type GenResult = { files: string[]; generator_id: string; seeds: number[]; meta: string };

export type PortraitGenPanelProps = {
  T: PluginCardTokens;
  generators: GenDescriptor[];
  /** 当前选中的通道 id */
  selected: string;
  onSelect: (id: string) => void;
  baseUrl: string;
  onBaseUrl: (v: string) => void;
  probing: boolean;
  probe: GenProbe | null;
  onProbe: () => void;
  generating: boolean;
  /** 生成并入库（入库走既有 portrait_import） */
  onGenerate: (opts: { prompt: string; negative: string; width: number; height: number; batch: number; seed: string; steps: string; cfg: string; characterId: number | null; name: string; basePortraitId: number | null }) => void;
  /** 上次结果（已入库的素材名列表，用于反馈） */
  lastSummary: string;
  /** 角色下拉（可选：从角色卡带入时预填） */
  charOptions: { id: number; name: string }[];
  defaultCharacterId: number | null;
  /** 云端配置（P3）：未选云端通道时传 null 即可 */
  cloud: CloudForm;
  onCloud: (patch: Partial<CloudForm>) => void;
  onSaveCloud: () => void;
  savingCloud: boolean;
  /** 占位符说明里用到 */
  placeholderHint: string;
  /**
   * 外部预填（从角色卡「AI 生成立绘」或素材「重出这张」带过来）。
   * `stamp` 每次预填都变，面板据此把值灌进受控输入框（同一次预填不会反复覆盖你的编辑）。
   */
  prefill: {
    stamp: number;
    prompt?: string;
    negative?: string;
    width?: number;
    height?: number;
    seed?: string;
    steps?: string;
    cfg?: string;
    characterId?: number | null;
    /** 重出链：入库时写入 derived_from */
    basePortraitId?: number | null;
    /** 给用户看的一句话（说明这次预填从哪来） */
    note?: string;
  } | null;
};

/**
 * 角色 → 文生图提示词（角色卡「一键预填」用）。
 *
 * 原则：**只放有信息量的东西**，宁可少也不要给模型垃圾关键词 ——
 * 无意义的标签（如"其他"这种定位）会稀释提示词、还可能误导出图。
 * 性别有值就直接给模型认得的 `1girl` / `1boy`，没有才留一个待用户选择的占位。
 */
export function characterToPrompt(c: {
  name?: string; role?: string; gender?: string; age?: string;
  one_liner?: string; traits?: string; speech_style?: string; profile?: string; alias?: string;
}): string {
  const bits: string[] = [];

  // 1) 主体：性别有值就用模型认得的标签
  const gender = (c.gender ?? "").trim();
  if (gender.includes("女") || gender.includes("female") || gender.includes("girl")) bits.push("1girl");
  else if (gender.includes("男") || gender.includes("male") || gender.includes("boy")) bits.push("1boy");
  else bits.push("1girl or 1boy — 请按角色改这一项");

  // 2) 身份描述：定位与一句话身份去重后合并（"女主" + "女主，身材瘦小…" 只留后者）
  const role = (c.role ?? "").trim();
  const one = (c.one_liner ?? "").trim();
  const parts: string[] = [];
  // 定位里没信息量的值不参与
  const MEANINGLESS_ROLE = ["其他", "其他角色", "配角", "无名", "路人"];
  if (role && !MEANINGLESS_ROLE.includes(role) && !one.includes(role)) parts.push(role);
  if (one) parts.push(one);
  if (parts.length) bits.push(parts.join("，"));

  // 3) 年龄 / 性格（影响气质与表情）
  const age = (c.age ?? "").trim();
  if (age) bits.push(`age ${age}`);
  const traits = (c.traits ?? "").trim();
  if (traits) bits.push(`personality: ${traits}`);

  // 4) 立绘常用构图与画质
  bits.push("upper body", "portrait", "looking at viewer", "detailed face", "soft lighting", "simple background");
  return bits.join(", ");
}

/** 角色 → 负向提示词默认值 */
export const DEFAULT_NEGATIVE = "lowres, bad anatomy, bad hands, extra fingers, watermark, text, signature";



const SIZE_PRESETS: { label: string; w: number; h: number }[] = [
  { label: "立绘 3:4（768×1024）", w: 768, h: 1024 },
  { label: "立绘 2:3（832×1216）", w: 832, h: 1216 },
  { label: "半身 1:1（1024×1024）", w: 1024, h: 1024 },
  { label: "横向 4:3（1024×768）", w: 1024, h: 768 },
];

export function PortraitGenPanel(props: PortraitGenPanelProps) {
  const { T } = props;
  const [prompt, setPrompt] = useState("");
  const [negative, setNegative] = useState(DEFAULT_NEGATIVE);
  const [sizeIdx, setSizeIdx] = useState(0);
  const [batch, setBatch] = useState("1");
  const [seed, setSeed] = useState("");
  const [steps, setSteps] = useState("");
  const [cfg, setCfg] = useState("");
  const [charId, setCharId] = useState<string>(props.defaultCharacterId != null ? String(props.defaultCharacterId) : "");
  const [prefillNote, setPrefillNote] = useState("");
  const lastStamp = useRef(0);

  // 外部预填：只在 stamp 变化时灌入（之后用户的手动编辑不会被覆盖）
  useEffect(() => {
    const pf = props.prefill;
    if (!pf || pf.stamp === lastStamp.current) return;
    lastStamp.current = pf.stamp;
    if (pf.prompt !== undefined) setPrompt(pf.prompt);
    if (pf.negative !== undefined) setNegative(pf.negative);
    if (pf.seed !== undefined) setSeed(pf.seed);
    if (pf.steps !== undefined) setSteps(pf.steps);
    if (pf.cfg !== undefined) setCfg(pf.cfg);
    if (pf.characterId !== undefined) setCharId(pf.characterId == null ? "" : String(pf.characterId));
    if (pf.width && pf.height) {
      const i = SIZE_PRESETS.findIndex((s) => s.w === pf.width && s.h === pf.height);
      if (i >= 0) setSizeIdx(i);
    }
    setPrefillNote(pf.note ?? "");
  }, [props.prefill]);

  const cur = props.generators.find((g) => g.id === props.selected) ?? props.generators[0];
  const sz = SIZE_PRESETS[sizeIdx] ?? SIZE_PRESETS[0];
  const sup = cur?.supports;

  return (
    <div>
      {/* 预填来源提示（从角色卡 / 重出带过来时显示） */}
      {prefillNote && (
        <div style={{ fontSize: 11, color: T.accent, background: T.accentSoft, border: `1px solid ${T.accent}`, borderRadius: 7, padding: "6px 9px", marginBottom: 10, lineHeight: 1.6 }}>
          ✨ {prefillNote}
        </div>
      )}

      {/* 通道 */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap", marginBottom: 10 }}>
        <span style={{ fontSize: 12, color: T.text2 }}>通道</span>
        <select value={props.selected} onChange={(e) => props.onSelect(e.target.value)} style={{ fontSize: 12, padding: "3px 8px" }}>
          {props.generators.map((g) => (
            <option key={g.id} value={g.id} disabled={!g.implemented}>
              {g.name}{g.implemented ? "" : "（未实装）"}
            </option>
          ))}
        </select>
        {/* 服务地址只对**本地**通道显示；云端通道的地址在下方「云端配置」里，避免两个地址框互相误导 */}
        {cur && cur.channel === "local" && (
          <input value={props.baseUrl} onChange={(e) => props.onBaseUrl(e.target.value)}
            placeholder="http://127.0.0.1:7860"
            style={{ fontSize: 12, padding: "3px 8px", minWidth: 220 }} />
        )}
        {cur && (
          <button onClick={props.onProbe} disabled={props.probing || !cur.implemented}>
            {props.probing ? "测试中…" : cur.channel === "local" ? "测试连接" : "检查配置"}
          </button>
        )}
        {cur && cur.channel === "local" && (
          <span style={{ fontSize: 10.5, color: T.text3 }}>需要本机已启动相应服务</span>
        )}
      </div>

      {/* 通道状态：成功绿字 / 失败黄字（不阻塞其它功能） */}
      {cur && !cur.implemented && (
        <div style={{ fontSize: 11.5, color: T.warn, marginBottom: 10, lineHeight: 1.7 }}>
          ⚠ {cur.note}
          <br />素材库的手工导入、绑定、分组、版本、导出与角色卡展示都不受影响 —— 没有绘图服务也能正常用。
        </div>
      )}
      {props.probe && (
        <div style={{ fontSize: 11.5, color: props.probe.ok ? T.ok : T.warn, marginBottom: 10, lineHeight: 1.7 }}>
          {props.probe.ok ? "✅ " : "⚠ "}{props.probe.detail}
          {!props.probe.ok && cur?.channel === "local" && (
            <>
              <br />没有本地绘图服务？把上面的「通道」切到云端两种之一，用自己的云端服务即可（只需填地址与 API Key）。
            </>
          )}
        </div>
      )}

      {/* 参数 */}
      <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "8px 10px", alignItems: "center", maxWidth: 620 }}>
        <span style={{ fontSize: 12, color: T.text2 }}>提示词</span>
        <textarea value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={3}
          placeholder="例如：1girl, nun habit, silver hair, standing, upper body, soft light"
          style={{ fontSize: 12, padding: "5px 8px", width: "100%", boxSizing: "border-box", resize: "vertical" }} />

        {sup?.negative && (
          <>
            <span style={{ fontSize: 12, color: T.text2 }}>负向</span>
            <textarea value={negative} onChange={(e) => setNegative(e.target.value)} rows={2}
              style={{ fontSize: 12, padding: "5px 8px", width: "100%", boxSizing: "border-box", resize: "vertical" }} />
          </>
        )}

        {sup?.size && (
          <>
            <span style={{ fontSize: 12, color: T.text2 }}>尺寸</span>
            <select value={sizeIdx} onChange={(e) => setSizeIdx(Number(e.target.value))} style={{ fontSize: 12, padding: "3px 8px" }}>
              {SIZE_PRESETS.map((s, i) => <option key={s.label} value={i}>{s.label}</option>)}
            </select>
          </>
        )}

        <span style={{ fontSize: 12, color: T.text2 }}>入库为角色</span>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          <select value={charId} onChange={(e) => setCharId(e.target.value)} style={{ fontSize: 12, padding: "3px 8px", minWidth: 140 }}>
            <option value="">（不绑定）</option>
            {props.charOptions.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
          <span style={{ fontSize: 10.5, color: T.text3 }}>绑定后角色管理的立绘展示会直接显示（与手工导入的图完全等价）</span>
        </div>

        {sup?.batch && (
          <>
            <span style={{ fontSize: 12, color: T.text2 }}>张数</span>
            <input value={batch} onChange={(e) => setBatch(e.target.value)} style={{ fontSize: 12, padding: "3px 8px", width: 70 }} />
          </>
        )}
        {sup?.seed && (
          <>
            <span style={{ fontSize: 12, color: T.text2 }}>种子</span>
            <input value={seed} onChange={(e) => setSeed(e.target.value)} placeholder="留空 = 随机" style={{ fontSize: 12, padding: "3px 8px", width: 120 }} />
          </>
        )}
        {sup?.steps && (
          <>
            <span style={{ fontSize: 12, color: T.text2 }}>步数</span>
            <input value={steps} onChange={(e) => setSteps(e.target.value)} placeholder="留空 = 服务默认" style={{ fontSize: 12, padding: "3px 8px", width: 120 }} />
          </>
        )}
        {sup?.cfg && (
          <>
            <span style={{ fontSize: 12, color: T.text2 }}>CFG</span>
            <input value={cfg} onChange={(e) => setCfg(e.target.value)} placeholder="留空 = 服务默认" style={{ fontSize: 12, padding: "3px 8px", width: 120 }} />
          </>
        )}
      </div>

      {/* 固定行为说明（不提供开关）：AI 生成内容带标识是法规要求，做成软件既定行为 */}
      <div style={{ marginTop: 12, fontSize: 11.5, color: T.text2, lineHeight: 1.75, border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "9px 11px" }}>
        🔖 每次生成的结果都会自动写入 <b>「AI」标识</b>（右下角角标 + PNG 元数据里的 AIGenerated 标识）。
        <span style={{ color: T.text3 }}>
          <br />这是**加**标识、不是去标识；服务商原有的水印与元数据一律保留，本软件不提供关闭选项。
          <br />只对 PNG 生效；其它格式或处理失败会按原图入库（不影响生成，也不会改坏图片）。
        </span>
      </div>

      {/* 合规提示（AI 生成内容标识义务）：不做弹窗阻断，但每次生成前都能看到 */}
      <div style={{ fontSize: 10.5, color: T.text3, marginTop: 12, lineHeight: 1.7, borderTop: `1px solid ${T.lineSoft}`, paddingTop: 8 }}>
        ⚖ 合规提示：按《人工智能生成合成内容标识办法》（2025-09-01 施行），AI 生成内容须带
        「显式标识」（可见水印）与「隐式标识」（文件元数据），不得删除、篡改、隐匿。
        本软件全程字节直通、不剥离标识；请勿用请求头关掉服务方水印，对外发布时请自行标注“AI 生成”。
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 12, flexWrap: "wrap" }}>
        <button onClick={() => props.onGenerate({
          prompt, negative, width: sz.w, height: sz.h,
          batch: Math.max(1, Number(batch) || 1),
          seed, steps, cfg,
          characterId: charId ? Number(charId) : null,
          name: (charId ? (props.charOptions.find((c) => String(c.id) === charId)?.name ?? "") : "") || "AI 立绘",
          basePortraitId: props.prefill?.basePortraitId ?? null,
        })} disabled={props.generating || !cur?.implemented}>
          {props.generating ? "生成中…（本地出图可能几十秒）" : "✨ 开始生成并入库"}
        </button>
        <span style={{ fontSize: 10.5, color: T.text3 }}>
          结果直接进素材库（走与手工导入相同的入口，自动记录版本），可在「立绘素材」里查看与管理
        </span>
      </div>
      {props.lastSummary && (
        <div style={{ fontSize: 11.5, color: T.ok, marginTop: 8 }}>{props.lastSummary}</div>
      )}
    </div>
  );
}

