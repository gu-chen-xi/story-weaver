// 「立绘生成」插件的面板与提示词工具（v3.5.7，从 src/plugins.tsx 抽出）。
//
// 为什么单独一个目录：这是插件系统里**最大的一块私货**（面板 ~150 行 + 提示词拼装 +
// 云端通道表单）。它不属于"插件框架"，只属于这个插件。

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useConfirm } from "../../../confirm";
import type { PluginCardTokens } from "../../PluginCard";

export type GenSupports = {
  negative: boolean; size: boolean; steps: boolean; cfg: boolean; seed: boolean; batch: boolean;
  /** 能否**每次生成**指定模型名（云端服务如硅基流动把 model 放在请求体里） */
  model: boolean;
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
  response_path: string;  // 自定义形态：响应字段路径，如 url:images.0.url（留空 = auto:data）
  openai_response_path: string; // OpenAI 形态：取图路径（单独一份，留空 = auto:data）
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
  onGenerate: (opts: { prompt: string; negative: string; width: number; height: number; batch: number; seed: string; steps: string; cfg: string; model: string; characterId: number | null; name: string; basePortraitId: number | null }) => void;
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

/** API 参数名（seed / steps / cfg / model）用等宽字体：一眼看出"这是照服务商文档填的字段名" */
const MONO = "Consolas, 'Courier New', monospace";

export function PortraitGenPanel(props: PortraitGenPanelProps) {
  const { T } = props;
  const confirm = useConfirm();
  const [prompt, setPrompt] = useState("");
  const [negative, setNegative] = useState(DEFAULT_NEGATIVE);
  const [sizeIdx, setSizeIdx] = useState(0);
  const [batch, setBatch] = useState("1");
  const [seed, setSeed] = useState("");
  const [steps, setSteps] = useState("");
  const [cfg, setCfg] = useState("");
  const [charId, setCharId] = useState<string>(props.defaultCharacterId != null ? String(props.defaultCharacterId) : "");
  const [prefillNote, setPrefillNote] = useState("");
  const [keyMsg, setKeyMsg] = useState("");
  const lastStamp = useRef(0);

  /** 清除绘图通道的 Key（凭据库里可能躺着一条很早以前填的、你自己都不认识的 Key） */
  const clearCloudKey = async () => {
    const ok = await confirm({
      title: "清除绘图 API Key",
      body:
        "只删 Windows 凭据管理器里「绘图通道」那一条（账号 portrait_api_key）。\n\n" +
        "· 对话模型的 Key 不受影响\n" +
        "· 已保存的地址 / 模板 / 素材也不受影响\n\n" +
        "清完之后这里会变成「还没设置」，可以重新粘贴一个。",
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke<string>("generator_clear_cloud_key");
      props.onCloud({ has_api_key: false, api_key: "" });
      setKeyMsg("已清除（对话模型的 Key 未受影响）");
    } catch (e) {
      setKeyMsg("清除失败：" + String(e));
    }
  };

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
  // 云端配置区的标签/输入框样式：字段名用等宽字体，和 seed / steps / cfg 那套语言一致
  const CLBL = { fontSize: 11.5, color: T.text2, fontFamily: MONO } as const;
  /** 标签里的英文参数名（小一号、比中文淡）—— 中文给人看，英文用来对文档 */
  const CMONO = { fontFamily: MONO, color: T.text3, fontSize: 10.5 } as const;
  const CINP = { fontSize: 12, padding: "4px 8px", width: "100%", boxSizing: "border-box" } as const;

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

      {/* ☁ 云端通道配置（只对云端通道显示）
          2026-10-25 补回：P1 拆插件时这块 JSX 丢了 —— props（cloud / onCloud / onSaveCloud /
          savingCloud）一直在传，却没有任何控件渲染它，于是云端通道连地址都填不了。
          设计依据：docs/design/立绘系统.md「选中云端通道时才出现云端配置区」。 */}
      {cur?.channel === "cloud" && (
        <div style={{ border: `1px solid ${T.line}`, borderRadius: 10, background: T.panel3, padding: 12, marginBottom: 12 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 9 }}>
            <b style={{ fontSize: 12.5, color: T.text1 }}>☁ 云端通道配置</b>
            <span style={{ fontSize: 10.5, color: T.text3 }}>保存后长期生效（模型也只在这里填，参数区不再重复）</span>
            <span style={{ marginLeft: "auto", fontSize: 10.5, color: props.cloud.has_api_key ? T.ok : T.text3 }}>
              {props.cloud.has_api_key
                ? "API Key 已存在系统凭据库（账号 portrait_api_key）✅"
                : "还没设置 API Key"}
            </span>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "7px 10px", alignItems: "center" }}>
            {cur.id === "cloud_openai_image" ? (
              <>
                {/* OpenAI Images API 只需要这三样：地址 / 模型（在下面共用行）/ Key。
                    鉴权固定 Bearer、响应固定 data[]，所以不显示 auth / 额外头 / 取图路径。
                    接非官方或代理服务请走「自定义 URL + 请求模板」通道（2026-10-25 用户定） */}
                <span style={CLBL}>基础地址 <span style={CMONO}>base_url</span></span>
                <input value={props.cloud.base_url} onChange={(e) => props.onCloud({ base_url: e.target.value })}
                  placeholder="留空 = 沿用已保存的地址" style={CINP} />
              </>
            ) : (
              <>
                <span style={CLBL}>请求地址 <span style={CMONO}>url</span></span>
                <input value={props.cloud.url} onChange={(e) => props.onCloud({ url: e.target.value })}
                  placeholder="留空 = 沿用已保存的地址" style={CINP} />
                <span style={CLBL}>请求体模板 <span style={CMONO}>body_template</span></span>
                <textarea rows={3} value={props.cloud.body_template} onChange={(e) => props.onCloud({ body_template: e.target.value })}
                  placeholder="留空 = 沿用已保存的模板"
                  style={{ ...CINP, resize: "vertical", lineHeight: 1.6, fontFamily: MONO }} />
                <span style={CLBL}>响应字段路径 <span style={CMONO}>response_path</span></span>
                <input value={props.cloud.response_path} onChange={(e) => props.onCloud({ response_path: e.target.value })}
                  placeholder="留空 = auto:data" style={CINP} />
              </>
            )}
            <span style={CLBL}>默认模型 <span style={CMONO}>model</span></span>
            <input value={props.cloud.model} onChange={(e) => props.onCloud({ model: e.target.value })}
              placeholder="留空 = 请求里不带 model 字段" style={CINP} />
            {/* 鉴权 / 额外请求头只对自定义通道显示：OpenAI 官方就一种鉴权（Bearer），
                需要 api-key 头或自定义头的都是第三方/代理，那种走自定义通道 */}
            {cur.id !== "cloud_openai_image" && (
              <>
                <span style={CLBL}>鉴权方式 <span style={CMONO}>auth</span></span>
                <select value={props.cloud.auth_style || "bearer"} onChange={(e) => props.onCloud({ auth_style: e.target.value })}
                  style={{ ...CINP, width: 160 }}>
                  <option value="bearer">Bearer（多数 OpenAI 兼容服务）</option>
                  <option value="api-key">api-key 头</option>
                  <option value="none">不带鉴权</option>
                </select>
                <span style={CLBL}>额外请求头</span>
                <input value={props.cloud.extra_headers} onChange={(e) => props.onCloud({ extra_headers: e.target.value })}
                  placeholder="留空 = 不带额外请求头" style={CINP} />
              </>
            )}
            <span style={CLBL}>API Key</span>
            <input type="password" value={props.cloud.api_key} onChange={(e) => props.onCloud({ api_key: e.target.value })}
              placeholder={props.cloud.has_api_key ? "留空 = 不改动已存的 Key" : "粘贴你的 API Key"} style={CINP} />
          </div>

          {/* OpenAI 通道的固定项说清楚，免得用户找不到"鉴权 / 取图路径"以为功能缺了 */}
          {cur.id === "cloud_openai_image" && (
            <div style={{ fontSize: 10.5, color: T.text3, marginTop: 8, lineHeight: 1.7 }}>
              OpenAI Images API 是固定的：鉴权只有 <code style={{ fontFamily: MONO }}>Bearer</code>、
              响应固定 <code style={{ fontFamily: MONO }}>data[]</code>，所以这里只有地址 / 模型 / Key。
              <br />接非官方或代理服务（响应形状不一样，例如返回 <code style={{ fontFamily: MONO }}>images[].url</code>）
              请用「云端 · 自定义 URL + 请求模板」通道。
            </div>
          )}

          <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10, flexWrap: "wrap" }}>
            <button onClick={props.onSaveCloud} disabled={props.savingCloud}>
              {props.savingCloud ? "保存中…" : "保存云端配置"}
            </button>
            {props.cloud.has_api_key && (
              <button onClick={() => void clearCloudKey()} style={{ fontSize: 11.5 }}>
                清除已存的 Key
              </button>
            )}
            <span style={{ fontSize: 10.5, color: T.text3 }}>
              API Key 只进 Windows 凭据管理器（不写数据库、不进日志）；填完可用上方「检查配置」验一次
            </span>
            {keyMsg && <span style={{ fontSize: 10.5, color: keyMsg.startsWith("清除失败") ? T.danger : T.ok }}>{keyMsg}</span>}
          </div>
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
        {/* 以下三项直接用 **API 参数名**（与请求模板里的占位符同名），中文解释见下方那三行 */}
        {sup?.seed && (
          <>
            <span style={{ fontSize: 12, color: T.text2, fontFamily: MONO }}>seed</span>
            <input value={seed} onChange={(e) => setSeed(e.target.value)} placeholder="留空 = 随机（-1）" style={{ fontSize: 12, padding: "3px 8px", width: 120 }} />
          </>
        )}
        {sup?.steps && (
          <>
            <span style={{ fontSize: 12, color: T.text2, fontFamily: MONO }}>steps</span>
            <input value={steps} onChange={(e) => setSteps(e.target.value)} placeholder="留空 = 服务默认" style={{ fontSize: 12, padding: "3px 8px", width: 120 }} />
          </>
        )}
        {sup?.cfg && (
          <>
            <span style={{ fontSize: 12, color: T.text2, fontFamily: MONO }}>cfg</span>
            <input value={cfg} onChange={(e) => setCfg(e.target.value)} placeholder="留空 = 服务默认" style={{ fontSize: 12, padding: "3px 8px", width: 120 }} />
          </>
        )}
      </div>

      {/* 这三个字段名照服务商文档写，但光看名字没人知道要填什么 —— 中文解释放这里 */}
      <div style={{ fontSize: 10.5, color: T.text3, marginTop: 7, lineHeight: 1.85, maxWidth: 620 }}>
        上面这三个名字就是请求模板里的占位符：{" "}
        <code style={{ fontFamily: MONO }}>{"{{seed}}"}</code>{" "}
        <code style={{ fontFamily: MONO }}>{"{{steps}}"}</code>{" "}
        <code style={{ fontFamily: MONO }}>{"{{cfg}}"}</code>。
        <br />
        <b style={{ color: T.text2, fontFamily: MONO }}>seed</b> = 随机种子：填同一个数字就能生成同一张图（微调用）；留空 = 每次随机。
        <br />
        <b style={{ color: T.text2, fontFamily: MONO }}>steps</b> = 采样步数（画几遍）：越大越细，也越慢；留空 = 用服务默认，常见 20~30。
        <br />
        <b style={{ color: T.text2, fontFamily: MONO }}>cfg</b> = 提示词听话程度：越大越严格照提示词、也更僵；留空 = 用服务默认，常见 7~7.5。
        <br />
        这两项留空时占位符会被替换成空串 —— 服务端不一定接受空值，这种情况直接在模板里删掉那两个字段。
      </div>

      {/* 固定行为说明（不提供开关）：AI 生成内容带标识是法规要求，做成软件既定行为 */}
      <div style={{ marginTop: 12, fontSize: 11.5, color: T.text2, lineHeight: 1.75, border: `1px solid ${T.lineSoft}`, borderRadius: 8, padding: "9px 11px" }}>
        🔖 每次生成的结果都会自动写入 <b>「AI」标识</b>（<b>左上角</b>角标 + PNG 元数据里的 AIGenerated 标识）。
        <span style={{ color: T.text3 }}>
          <br />放在左上角是为了和平台自己的水印（多在右下角）区分开：<b>服务方原有水印一律原样保留</b>，我们的只加不改，不覆盖、不裁掉。
          <br />这是**加**标识、不是去标识；本软件不提供关闭选项。
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
          // model 只有一处来源：云端配置区（2026-10-25 用户要求去掉参数区那个重复输入框）
          seed, steps, cfg, model: props.cloud.model,
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


