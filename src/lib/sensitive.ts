// 敏感词过滤（纯逻辑 / 零依赖 / 可单测）。
//
// 定位（2026-10-12 用户定，2026-11-01 重申）：**只挡辱骂与脏话**，
// 不做内容审查 —— 剧情里的暴力、死亡、冲突、黑暗题材都是正常创作内容，不在此列。
//
// 三条硬要求（2026-11-01）：
//   ① 输入侧**硬拦**（不允许辱骂 AI 或角色），② 输出侧替换成 ***，
//   ③ 不许误伤正常中文（"妈妈的""狗日子""体操你们"必须放行）。
//
// 为什么不用一个扁平数组做 `includes`：
//   · 原实现区分大小写 → "傻b" 漏、"傻B" 才拦；
//   · 原实现不做归一化 → "傻 逼""傻·逼""ＣＮＭ" 全漏；
//   · 原实现是纯子串 → "妈的" 会把"她回到妈妈的房间"整条拦掉（实测误伤）。
// 所以这里做两件事：**先归一化再匹配**，以及**给容易误伤的短词加前置/后置护栏**。

/** 命中后用来替换的掩码 */
export const SENSITIVE_MASK = "***";

/** 一条规则：w 是**归一化后**的形态（小写、无分隔符） */
export type SensitiveRule = {
  w: string;
  /** 紧挨在词前面的字符命中这些就**不算**（用来躲"妈妈的"里的"妈的"） */
  notBefore?: string;
  /** 紧挨在词后面的字符命中这些就**不算**（用来躲"自我操作"里的"我操"） */
  notAfter?: string;
};

// 词表按"辱骂 / 脏话"收口，**不要往这里加题材词**（死亡、暴力、政治、宗教都不属于本表）。
// 加词只改数据，不改逻辑；但加了必须补 tests/sensitive.test.cjs 里的用例。
export const SENSITIVE_RULES: SensitiveRule[] = [
  // ===== 辱骂（原有 20 条，误伤的三条改成带护栏） =====
  { w: "傻逼" }, { w: "傻b" }, { w: "傻子吧" },
  { w: "草泥马" }, { w: "去你妈" }, { w: "操你妈" },
  { w: "你妈的" }, { w: "他妈的" }, { w: "滚你妈" },
  { w: "妈的", notBefore: "妈" },          // 放行「妈妈的」「你妈妈的身体」
  { w: "操你", notBefore: "体" },          // 放行「把体操你们练完」
  { w: "狗东西" }, { w: "混蛋" }, { w: "王八蛋" }, { w: "贱人" }, { w: "婊子" },
  { w: "死全家" }, { w: "杂种", notAfter: "优" }, { w: "废物点心" },   // 放行「杂种优势」
  // ===== 辱骂（2026-11-01 第一批扩充，均为低剧情价值） =====
  { w: "智障" }, { w: "脑残" }, { w: "弱智" }, { w: "白痴" }, { w: "低能儿" },
  { w: "蠢货" }, { w: "蠢猪" }, { w: "蠢驴" }, { w: "蠢蛋" },
  { w: "畜生" }, { w: "畜牲" }, { w: "禽兽" },
  { w: "杂碎", notBefore: "牛羊猪鸡鸭", notAfter: "汤锅" },   // 放行「牛杂碎汤」「羊杂碎」
  { w: "狗屎" }, { w: "狗屁" }, { w: "狗娘养的" }, { w: "狗日的" },   // 不再用裸「狗日」，放行「狗日子」
  { w: "贱货" }, { w: "骚货" }, { w: "下贱" }, { w: "贱骨头" },
  { w: "日你妈" }, { w: "干你娘" }, { w: "妈了个逼" }, { w: "你妈死了" },
  { w: "你全家死" }, { w: "龟儿子" }, { w: "鳖孙" }, { w: "王八羔子" },
  { w: "小杂种" }, { w: "滚犊子" },
  // ===== 脏话 / 粗口（含常见谐音变体） =====
  { w: "傻屄" }, { w: "傻比" }, { w: "沙比" }, { w: "煞笔" }, { w: "傻叉" },
  { w: "逼样" }, { w: "屄" }, { w: "鸡巴" },
  { w: "卧槽" }, { w: "我艹" }, { w: "操蛋" }, { w: "他娘的" },
  { w: "我操", notAfter: "作场演" },       // 放行「自我操作」
  // ===== 字母缩写（走词边界，避免误伤 USB / TMD 型号之类） =====
  { w: "sb" }, { w: "nmsl" }, { w: "tmd" }, { w: "cnm" }, { w: "wcnm" }, { w: "wdnmd" }, { w: "mdzz" },
];

/** 兼容旧引用：只要词面（归一化后的字符串） */
export const SENSITIVE_WORDS: string[] = SENSITIVE_RULES.map((r) => r.w);

/** 归一化时会**直接丢掉**的字符：零宽、空白、常见分隔符 */
const SKIP_CHAR =
  /[\u200B-\u200F\u2060\uFEFF\u00AD\s·・．.。，,、;；:：!！?？"'“”‘’`^*~\-_|/\\()（）\[\]【】<>《》「」『』…—+=#@$%&]/;

/** 按首字符建桶：10k 字的消息也只扫一遍，不跟词表条数相乘 */
const BY_FIRST = new Map<string, SensitiveRule[]>();
for (const r of SENSITIVE_RULES) {
  const list = BY_FIRST.get(r.w[0]);
  if (list) list.push(r);
  else BY_FIRST.set(r.w[0], [r]);
}

/**
 * 归一化：NFKC（全角→半角）+ 小写 + 丢掉零宽/空白/分隔符。
 * 返回的 map[i] = 归一化后第 i 个字符在**原文**里的下标，
 * 有了它才能把"归一化空间里找到的位置"落回原文去替换。
 */
/** 单字符归一化；常规汉字和 ASCII 走快路（NFKC 对它们是恒等变换），避免逐字调 API */
function foldChar(ch: string): string {
  const c = ch.charCodeAt(0);
  if (c >= 0x4e00 && c <= 0x9fff) return ch;              // 常用汉字
  if (c < 0x80) return c >= 65 && c <= 90 ? String.fromCharCode(c + 32) : ch;
  return ch.normalize("NFKC").toLowerCase();              // 全角 / 兼容字符 / 其它
}

export function normalizeForMatch(src: string): { text: string; map: number[] } {
  const chars: string[] = [];
  const map: number[] = [];
  for (let i = 0; i < src.length; i++) {
    const ch = src[i];
    if (SKIP_CHAR.test(ch)) continue;
    const n = foldChar(ch);
    // NFKC 可能把 1 个字符拆成多个（如 ㍿）→ 每个都记到同一个原文下标上
    for (let k = 0; k < n.length; k++) {
      chars.push(n[k]);
      map.push(i);
    }
  }
  return { text: chars.join(""), map };
}

type Hit = { start: number; end: number };

const isAsciiWord = (ch: string | undefined) => !!ch && /[a-z0-9]/.test(ch);

function allowed(r: SensitiveRule, s: string, i: number, j: number): boolean {
  if (r.notBefore && i > 0 && r.notBefore.includes(s[i - 1])) return false;
  if (r.notAfter && j < s.length && r.notAfter.includes(s[j])) return false;
  // 纯字母缩写：前后不能再接字母数字，否则 USB / WCNM2 之类会误伤
  if (/^[a-z]+$/.test(r.w)) {
    if (isAsciiWord(s[i - 1]) || isAsciiWord(s[j])) return false;
  }
  return true;
}

/** 在归一化后的文本里找出所有命中（按起点排序、重叠的只留最长那条） */
function findAll(norm: string): Hit[] {
  const raw: Hit[] = [];
  for (let i = 0; i < norm.length; i++) {
    const bucket = BY_FIRST.get(norm[i]);
    if (!bucket) continue;
    for (const r of bucket) {
      if (!norm.startsWith(r.w, i)) continue;
      if (allowed(r, norm, i, i + r.w.length)) raw.push({ start: i, end: i + r.w.length });
    }
  }
  raw.sort((a, b) => a.start - b.start || b.end - a.end);
  const out: Hit[] = [];
  let last = -1;
  for (const h of raw) {
    if (h.start < last) continue;
    out.push(h);
    last = h.end;
  }
  return out;
}

export type SensitiveHit = { word: string; start: number; end: number };

/**
 * 查第一条命中（给"输入侧硬拦"用）。
 * 返回的 word 是**原文里的那段字符**，方便提示里原样回显给用户。
 */
export function findSensitive(src: string): SensitiveHit | null {
  const { text, map } = normalizeForMatch(src);
  if (!text) return null;
  const hits = findAll(text);
  if (!hits.length) return null;
  const h = hits[0];
  const start = map[h.start];
  const end = map[h.end - 1] + 1;
  return { word: src.slice(start, end) || text.slice(h.start, h.end), start, end };
}

/** 有没有命中（不需要知道命中了什么时用） */
export function hasSensitive(src: string): boolean {
  const { text } = normalizeForMatch(src);
  return !!text && findAll(text).length > 0;
}

// 渲染层每条消息都会被问一次 → 命中缓存，避免流式期间反复扫同一段长文本
const CACHE = new Map<string, string>();
const CACHE_MAX = 240;

/** 把命中的词替换成 ***（**只用于显示**；库里存的仍然是原文） */
export function sanitizeText(src: string): string {
  if (!src) return src;
  const cached = CACHE.get(src);
  if (cached !== undefined) return cached;
  const out = sanitizeUncached(src);
  if (CACHE.size >= CACHE_MAX) CACHE.clear();
  CACHE.set(src, out);
  return out;
}

function sanitizeUncached(src: string): string {
  const { text, map } = normalizeForMatch(src);
  if (!text) return src;
  const hits = findAll(text);
  if (!hits.length) return src;
  let out = "";
  let pos = 0;
  for (const h of hits) {
    const s = map[h.start];
    const e = map[h.end - 1] + 1;
    if (s < pos) continue;
    out += src.slice(pos, s) + SENSITIVE_MASK;
    pos = e;
  }
  return out + src.slice(pos);
}

/** 流式过滤的留字长度：至少要比最长的词多，才能在跨 chunk 时不断词 */
export const STREAM_HOLDBACK = 8;

/**
 * 流式增量过滤：**先扣住尾巴再下发**。
 *
 * 原来的写法是对每个 delta 单独 `split/join`，于是"傻"在前一个 chunk、"逼"在后一个
 * chunk 时谁都匹配不上 —— 中文流式按字切分，这种漏是必现的。
 * 这里保留最后 STREAM_HOLDBACK 个字符不下发；如果某个命中恰好跨过切点，
 * 就把切点退回到该词的起点，整词留到下一轮（那次它一定完整）。
 *
 * @param flush 生成结束时传 true，把剩下的尾巴一次性吐出（含掩码）
 */
export function sanitizeStream(
  carry: string,
  delta: string,
  flush = false
): { out: string; carry: string } {
  const buf = carry + delta;
  if (flush) return { out: sanitizeText(buf), carry: "" };
  if (buf.length <= STREAM_HOLDBACK) return { out: "", carry: buf };

  let cut = buf.length - STREAM_HOLDBACK;
  const { text, map } = normalizeForMatch(buf);
  for (const h of findAll(text)) {
    const s = map[h.start];
    const e = map[h.end - 1] + 1;
    if (s < cut && e > cut) cut = s; // 跨过切点 → 整词留到下一轮
  }
  if (cut <= 0) return { out: "", carry: buf };
  return { out: sanitizeText(buf.slice(0, cut)), carry: buf.slice(cut) };
}
