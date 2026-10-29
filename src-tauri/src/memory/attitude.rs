// 「AI 这一轮是什么态度」的打分器（2026-10-26，用户定的方案）。
//
// 为什么不用关键词布尔判断：`REJECTION` 只有 4 条（"我不喜欢你"这类），
// 而真实的拒绝长这样 ——「别。」「这话不该跟我说。」「我不回答这种问题。」，
// 一条都命中不了，于是"表白被拒"被记成了普通的"表白"。
//
// 规则（用户 2026-10-26 拍板）：
//   ① **只给 AI 的回复打分**（用户那句只进正文留档，不参与判定）；
//   ② 按强度分档，负向优先 —— 否则「我不喜欢你」会被「喜欢你」判成接受；
//   ③ **标点算短语的一部分**：「别。」是一个整体，「别忘了带伞」里的"别"不算；
//   ④ |总分| ≥ 2 才算有判定，否则「未定」；理由取排名前 3（权重 × 次数）。

/// 态度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stance {
    Refused,
    Accepted,
    Unclear,
}

impl Stance {
    /// front matter 里的值
    pub fn key(self) -> &'static str {
        match self {
            Stance::Refused => "refused",
            Stance::Accepted => "accepted",
            Stance::Unclear => "unclear",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Stance::Refused => "被拒绝",
            Stance::Accepted => "被接受",
            Stance::Unclear => "未定",
        }
    }
    pub fn parse(s: &str) -> Stance {
        match s.trim() {
            "refused" => Stance::Refused,
            "accepted" => Stance::Accepted,
            _ => Stance::Unclear,
        }
    }
}

/// 一个命中理由：短语 + 权重 + 出现次数
#[derive(Debug, Clone)]
pub struct Signal {
    pub phrase: String,
    pub weight: i32,
    pub count: usize,
}

impl Signal {
    pub fn strength(&self) -> i32 { self.weight * self.count as i32 }
}

#[derive(Debug, Clone)]
pub struct Attitude {
    pub score: i32,
    pub stance: Stance,
    /// 按 `权重 × 次数` 从高到低；同分时权重高的在前
    pub signals: Vec<Signal>,
}

// 强拒绝（+3）：明确否定这段关系
const REFUSE_STRONG: &[(&str, i32)] = &[
    ("我拒绝你", 3), ("我不喜欢你", 3), ("我们不可能", 3),
    ("别碰我", 3), ("走开", 3), ("离我远点", 3), ("我讨厌你", 3),
];

// 中拒绝（+2）：带理由的回避
const REFUSE_MID: &[(&str, i32)] = &[
    ("不该跟我说", 2), ("我不回答", 2), ("我还没准备好", 2),
    ("别过来", 2), ("别这样", 2), ("你冷静", 2), ("太快了", 2),
    ("我们还是朋友", 2), ("换个话题", 2),
    // 2026-10-26 第二轮：用户实测的三段拒绝一条都没命中，按真实说法补进来。
    // 原则：**只收"只能是拒绝"的说法**，别把「嗯」「好」这类含糊的收进来。
    ("别说了", 2), ("别讲了", 2), ("别再说", 2), ("打住", 2),
    ("我不接", 2), ("接不了", 2), ("不算数", 2), ("不同意", 2),
    ("你一个人", 2), ("一厢情愿", 2), ("自作多情", 2), ("自说自话", 2),
    ("没得商量", 3), ("我哪句话是同意", 3), ("我什么时候同意", 3),
];

// 弱拒绝（+1）：要带句末标点才算（用户明确要求：「别。」是一个整体）
const REFUSE_WEAK: &[(&str, i32)] = &[
    ("别。", 1), ("别！", 1), ("别…", 1), ("别？", 1), ("别，", 1),
    ("不要。", 1), ("不行。", 1), ("不可以。", 1),
];

// 否认前提（+2）：不否认"你喜欢我"，而是否认"这事成立"。
// 「你认错人了」「这种话，别在我这儿放」——用户实测（测试0）第一版判成未定。
const REFUSE_DENY: &[(&str, i32)] = &[
    ("认错人", 2), ("认错了", 2), ("搞错", 2), ("误会", 2),
    ("想多了", 2), ("想多", 1), ("开玩笑", 1), ("说笑", 1),
];
// 强接受（−3）
const ACCEPT_STRONG: &[(&str, i32)] = &[
    ("我也喜欢你", -3), ("我早就喜欢你", -3), ("我愿意", -3),
    ("我答应你", -3), ("我爱你", -3),
];

// 中接受（−2）
const ACCEPT_MID: &[(&str, i32)] = &[("我喜欢你", -2), ("我们在一起", -2), ("我也是", -2)];

/// 判定阈值：|score| ≥ 这个数才算"有态度"
pub const DECIDE_MIN: i32 = 2;


/// 把「」/“”/"" 里的**台词**抽出来（旁白不算态度）。
///
/// 为什么要分：旁白里的「她没应声」「没看他」可能是害羞，也可能是拒绝；
/// 而台词里的「别说了」「我不接」是明确态度。先看台词，没有台词才退回整段。
fn quoted_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let open = chars[i];
        let close = match open {
            '「' => '」',
            '“' => '”',
            '"' => '"',
            _ => { i += 1; continue; }
        };
        let mut j = i + 1;
        while j < chars.len() && chars[j] != close {
            j += 1;
        }
        if j >= chars.len() {
            break;
        }
        let inner: String = chars[i + 1..j].iter().collect();
        if !inner.trim().is_empty() {
            out.push(inner);
        }
        i = j + 1;
    }
    out
}

/// 一句短台词里带「不 / 没 / 别」—— 算一条弱拒绝。
///
/// 词表永远补不全（用户实测两轮都没命中），这条兜底：**台词很短又带否定词**，
/// 基本只能是拒绝或回避（「别说了。」「我不接。」「说多了，就不算数了。」）。
/// 旁白不参与（只扫引号内），避免「她没说话，脸红了」被判成拒绝。
fn has_short_negation(line: &str) -> bool {
    let n = line.chars().count();
    (1..=24).contains(&n) && line.chars().any(|c| c == '不' || c == '没' || c == '别')
}

/// 一条短语在文本里出现几次（允许重叠，但短语都是多字的，重叠很少）
fn count_of(text: &str, phrase: &str) -> usize {
    if phrase.is_empty() {
        return 0;
    }
    text.matches(phrase).count()
}

/// 给 AI 的回复打分。
pub fn score_reply(reply: &str) -> Attitude {
    let text = reply.trim();
    if text.is_empty() {
        return Attitude { score: 0, stance: Stance::Unclear, signals: Vec::new() };
    }
    // 有台词就只扫台词；没台词（纯旁白）才退回整段
    let lines = quoted_lines(text);
    let scopes: Vec<String> = if lines.is_empty() { vec![text.to_string()] } else { lines };

    let mut signals: Vec<Signal> = Vec::new();
    for scope in &scopes {
        for table in [REFUSE_STRONG, REFUSE_MID, REFUSE_DENY, REFUSE_WEAK, ACCEPT_STRONG, ACCEPT_MID] {
            for (phrase, weight) in table {
                let n = count_of(scope, phrase);
                if n == 0 {
                    continue;
                }
                match signals.iter_mut().find(|s| s.phrase == *phrase) {
                    Some(s) => s.count += n,
                    None => signals.push(Signal { phrase: phrase.to_string(), weight: *weight, count: n }),
                }
            }
        }
        if has_short_negation(scope) {
            match signals.iter_mut().find(|s| s.phrase == "短句否定") {
                Some(s) => s.count += 1,
                None => signals.push(Signal { phrase: "短句否定".to_string(), weight: 1, count: 1 }),
            }
        }
    }
    // 拒绝为正、接受为负（方便判断方向；排序按 |strength| 大在前）
    let score: i32 = signals.iter().map(|s| s.strength()).sum();
    signals.sort_by(|a, b| {
        b.strength()
            .abs()
            .cmp(&a.strength().abs())
            .then(b.weight.abs().cmp(&a.weight.abs()))
            .then(b.count.cmp(&a.count))
            .then(a.phrase.cmp(&b.phrase))
    });
    let stance = if score >= DECIDE_MIN {
        Stance::Refused
    } else if score <= -DECIDE_MIN {
        Stance::Accepted
    } else {
        Stance::Unclear
    };
    Attitude { score, stance, signals }
}

/// 给注入用的"理由"字符串：取前 N 条，写成「别。（3 次）」。
pub fn reasons_text(signals: &[Signal], max: usize) -> String {
    signals
        .iter()
        // 「短句否定」是兜底打分用的内部标记，不拿去给模型当"理由"（它不含信息量）
        .filter(|s| s.phrase != "短句否定")
        .take(max)
        .map(|s| if s.count > 1 { format!("{}（{} 次）", s.phrase, s.count) } else { s.phrase.clone() })
        .collect::<Vec<_>>()
        .join(" / ")
}


/// 解析 AI 回复**最后一行**的协议标记：`〔态度:接受〕` / `〔态度:拒绝〕`。
///
/// 为什么用标记而不是继续猜自然语言：模型怎么措辞都行（「你这算什么」「刚还念数字呢」…），
/// 词表永远补不完。标记是**协议**：只在最后一行、长度很短、形态固定，正文里出现同款字样不算。
///
/// 容错：全角/半角冒号与空格、外面多余标点都认；值里含「接受/同意」算接受，含「拒绝/不同意/回避」算拒绝。
/// 这一整行如果是态度标记，返回它的值（形状不对 / 值不认识 → None）
pub fn stance_line(line: &str) -> Option<Stance> {
    let t = line.trim().trim_matches(|c: char| c.is_whitespace() || "。.,，！!".contains(c));
    if t.chars().count() > 24 || !t.starts_with("〔态度") || !t.ends_with("〕") {
        return None;
    }
    let inner = t.trim_start_matches("〔态度").trim_end_matches("〕");
    let inner = inner.trim_start_matches(['：', ':', ' ', '　']).trim();
    if inner.contains("不接受") || inner.contains("不同意") || inner.contains("拒绝") || inner.contains("回避") {
        return Some(Stance::Refused);
    }
    if inner.contains("接受") || inner.contains("同意") {
        return Some(Stance::Accepted);
    }
    None
}

/// 这一行是不是「协议行」（任意 〔…〕 标记，长度 ≤ 24）。
///
/// 不按具体标记名判断：态度（〔态度:…〕）、导演代价（〔导演代价〕…）、
/// 赦免（〔赦免:…〕）都要算协议行 —— 否则末尾协议区扫到不认识的标记就停，
/// 那条标记会被当成正文（2026-10-27 赦免请求踩过）。
fn is_protocol_line(line: &str) -> bool {
    let t = line
        .trim()
        .trim_end_matches(|c: char| c.is_whitespace() || "。.,，！!".contains(c));
    // 导演代价是『〔导演代价〕类型：理由』—— 以标记开头、但不以〕结尾；
    // 长度要放宽（理由上限 32 字）。
    if crate::director::is_cost_line(t) {
        return t.chars().count() <= 60;
    }
    // 状态变化同理：『〔状态变化〕熬夜：理由』，理由上限 32 字
    if crate::stat_change::is_stat_line(t) {
        return t.chars().count() <= 60;
    }
    t.chars().count() <= 24 && t.starts_with('〔') && t.ends_with('〕')
}

/// 末尾协议区的起始行号：从最后一行往上，跳过空行与协议行，遇到正文就停。
///
/// 没有协议行的行号语义：返回 `lines().count()`（= 整个文本都在"协议区之外"）。
pub fn protocol_block_start(text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let mut start = lines.len();
    for (i, line) in lines.iter().enumerate().rev() {
        if line.trim().is_empty() || is_protocol_line(line) {
            start = i;
            continue;
        }
        break;
    }
    start
}

pub fn parse_marker(reply: &str) -> Option<Stance> {
    // 只在**末尾协议区**里找（2026-10-27）。
    // 为什么不是"只认最后一行"：同一轮里导演代价也是协议行，两条都要写，
    // 模型可能把 `〔导演代价〕…` 放在最后 —— 只认最后一行会把态度标记丢掉（记忆退化成分词打分）。
    // 为什么不是"最后 3 行里随便找"：标记后面还接着正文的，属于写歪了，不认。
    let start = protocol_block_start(reply);
    reply
        .lines()
        .skip(start)
        .find_map(stance_line)
}

/// 这一行**长得像**态度标记吗（只判形状，不判值）。
///
/// 为什么要单独一个形状判断：模型会自创表外值（2026-10-27 实测写了「〔态度:冷淡〕」）。
/// 解析时它算「没写」（退回台词打分），但**展示 / 历史里必须剥掉** ——
/// 否则气泡里会露出一个「〔态度 冷淡〕」的橙色小块，而且它会留在下一轮的历史里被模型继续模仿。
pub fn is_stance_shape(line: &str) -> bool {
    let t = line.trim().trim_matches(|c: char| c.is_whitespace() || "。.,，！!".contains(c));
    t.chars().count() <= 24 && t.starts_with("〔态度") && t.ends_with("〕")
}

/// 把协议标记从文本里剥掉（存库保留，展示 / 导出 / 记忆正文 / **历史**里都不要）。
///
/// 按**形状**剥：表外值（「冷淡」「敷衍」）也一起删 —— 它不是给用户看的内容。
pub fn strip_marker(text: &str) -> String {
    text.lines()
        .filter(|l| !is_stance_shape(l))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}


/// 从 AI 回复里抽「她的原话」当理由：前 1–2 句台词，拼起来截到 `max_chars`。
///
/// 为什么要它：靠协议标记判定时**没有词级理由**（标记只给结论），但人想看的是"她当时怎么说的"。
/// 比"理由：别说了"更可信 —— 那是原文。
pub fn quote_gist(reply: &str, max_chars: usize) -> String {
    let lines = quoted_lines(reply);
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for l in lines.iter().take(2) {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
        if out.chars().count() >= max_chars {
            break;
        }
    }
    let chars: Vec<char> = out.chars().collect();
    if chars.len() <= max_chars {
        out
    } else {
        format!("{}…", chars[..max_chars].iter().collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用户实测那一轮：三处拒绝信号，必须判成「被拒绝」，理由取前 3
    #[test]
    fn refuses_the_real_reply() {
        let reply = "（她没接，往后退了半步）「别。」（手抬了一下，像是要拦，停在半空又收回去）「这话不该跟我说。」*他是不是喝多了。*「你先站那儿，别过来。」";
        let a = score_reply(reply);
        assert_eq!(a.stance, Stance::Refused, "应判被拒：{a:?}");
        let reasons = reasons_text(&a.signals, 3);
        assert!(reasons.contains("别。"), "理由要含「别。」：{reasons}");
        assert!(reasons.contains("不该跟我说"), "{reasons}");
        assert!(reasons.contains("别过来"), "{reasons}");
    }

    /// 「我不喜欢你」不能被「喜欢你」判成接受（负向优先）
    #[test]
    fn explicit_refusal_beats_acceptance_substring() {
        let a = score_reply("「我不喜欢你。」");
        assert_eq!(a.stance, Stance::Refused, "{a:?}");
    }

    #[test]
    fn accepts_when_the_reply_accepts() {
        let a = score_reply("（她低头笑了一下）「我也喜欢你。」");
        assert_eq!(a.stance, Stance::Accepted, "{a:?}");
    }

    /// 用户明确要求：**标点算短语的一部分** ——「别。」是拒绝，「别忘了带伞」不是
    #[test]
    fn punctuation_is_part_of_the_phrase() {
        assert_eq!(score_reply("「别忘了带伞。」").stance, Stance::Unclear);
        assert_eq!(score_reply("「别。」").stance, Stance::Refused, "台词里的「别。」= 弱拒绝 + 短句否定，够阈值");
        assert_eq!(score_reply("「别。」「别。」").stance, Stance::Refused, "两次才够 2 分");
    }

    #[test]
    fn empty_or_neutral_is_unclear() {
        assert_eq!(score_reply("").stance, Stance::Unclear);
        assert_eq!(score_reply("（她把杯子搁回桌上）").stance, Stance::Unclear);
    }
    // ===== 2026-10-26 第二轮：用户实测的三段"被判成未定"的真实回复 =====
    // 它们的共同点：拒绝都在**台词**里，且措辞不在词表里（「别说了」「我不接」「不算数」…）。

    #[test]
    fn refuses_serial_one() {
        let reply = "（她没应声，点了一下头，又像是点给自己看的）[夏婉琴]「别说了。」（视线落回桌上的杯子，没看他）[夏婉琴]「这种话，我不接。」*他挑这个时候说，是觉得我该高兴吗。*";
        let a = score_reply(reply);
        assert_eq!(a.stance, Stance::Refused, "应判被拒：{a:?}");
        assert!(!reasons_text(&a.signals, 3).is_empty());
    }

    #[test]
    fn refuses_serial_two() {
        let reply = "（她笑了一下，很短，嘴角刚抬起来就压回去）[夏婉琴]「你今晚说第几次了。」（手指在杯沿上磨了半圈，没抬眼）[夏婉琴]「说多了，就不算数了。」*心里是松的。差一点就要应一声，可我不能。*";
        let a = score_reply(reply);
        assert_eq!(a.stance, Stance::Refused, "应判被拒：{a:?}");
        assert!(reasons_text(&a.signals, 3).contains("不算数"), "{:?}", a.signals);
    }

    #[test]
    fn refuses_serial_three() {
        let reply = "（她抬眼看他，笑意已经收了）[夏婉琴]「我哪句话是同意。」（手臂抱在一起，往后靠了靠）[夏婉琴]「说几遍，都是你一个人在说。」*心里其实不难受。越是…越不能让他看出来。*";
        let a = score_reply(reply);
        assert_eq!(a.stance, Stance::Refused, "应判被拒：{a:?}");
        let reasons = reasons_text(&a.signals, 3);
        assert!(reasons.contains("哪句话是同意") || reasons.contains("你一个人"), "{reasons}");
    }

    /// 反向：害羞的沉默不是拒绝（旁白里的"没"不算态度），含糊的「嗯。」也不算
    #[test]
    fn shy_silence_is_not_refusal() {
        assert_eq!(score_reply("（她没有说话，只是脸红了。）").stance, Stance::Unclear);
        assert_eq!(score_reply("[夏婉琴]「嗯。」").stance, Stance::Unclear);
    }
    /// 「你认错人了。」—— 否认的是"这事成立"，不是"你喜欢我"（测试0 实测判成了未定）
    #[test]
    fn refusal_by_denying_the_premise() {
        let reply = "（她没有抬头，指尖在杯沿上停了一下）[夏婉琴]「你认错人了。」（椅子往后挪了半寸，拉开那点距离）*他说这话之前，连我姓什么都没问过。*[夏婉琴]「这种话，别在我这儿放。」（说完就起身，把椅子推回原位）";
        let a = score_reply(reply);
        assert_eq!(a.stance, Stance::Refused, "应判被拒：{a:?}");
        assert!(reasons_text(&a.signals, 3).contains("认错人"), "{:?}", a.signals);
    }
    // ===== 协议标记（2026-10-26 用户定：不再猜措辞）=====

    #[test]
    fn parses_marker_on_the_last_line() {
        assert_eq!(parse_marker("（她别过脸）「别说了。」\n〔态度:拒绝〕"), Some(Stance::Refused));
        assert_eq!(parse_marker("（她笑了一下）\n\n〔态度:接受〕"), Some(Stance::Accepted));
        // 全角冒号 / 句末多一个句号 / 前后空白都认
        assert_eq!(parse_marker("正文\n〔态度：接受〕。"), Some(Stance::Accepted));
        assert_eq!(parse_marker("正文\n  〔态度:拒绝〕  "), Some(Stance::Refused));
    }

    #[test]
    fn ignores_marker_in_the_middle_or_wrong_shape() {
        assert_eq!(parse_marker("〔态度:拒绝〕\n后面还有正文"), None, "只认最后一行");
        assert_eq!(parse_marker("她说〔态度:拒绝〕这个词"), None, "不是整行");
        assert_eq!(parse_marker("· `[角色名]“台词”` 这种模板"), None, "无关文本");
        assert_eq!(parse_marker(""), None);
    }


    /// **两条协议共存**（2026-10-27）：收据在前、态度标记在后 → 态度仍能读出来；
    /// 反过来也行。只认最后一行会让其中一条静默丢失。
    #[test]
    fn marker_survives_a_cost_receipt_after_it() {
        assert_eq!(parse_marker("正文\n〔态度:拒绝〕\n〔导演代价〕条件：他交出钥匙"), Some(Stance::Refused));
        assert_eq!(parse_marker("正文\n〔导演代价〕白给：没有代价\n〔态度:接受〕"), Some(Stance::Accepted));
    }

    /// **表外值也要剥**（2026-10-27 实测）：模型写过「〔态度:冷淡〕」——
    /// 解析当它是没写（退回台词打分），但正文/历史里绝不能留着这块东西。
    #[test]
    fn strips_out_of_vocabulary_markers() {
        let got = strip_marker("（她把脸别开）「先把手拿开。」\n〔态度:冷淡〕");
        assert_eq!(got, "（她把脸别开）「先把手拿开。」");
        assert_eq!(parse_marker("正文\n〔态度:冷淡〕"), None, "表外值不判定");
        assert_eq!(strip_marker("正文\n〔态度 冷淡〕"), "正文", "没冒号、只有空格也剥");
    }
    #[test]
    fn strip_marker_removes_the_line_only() {
        let got = strip_marker("（她别过脸）「别说了。」\n〔态度:拒绝〕");
        assert_eq!(got, "（她别过脸）「别说了。」");
        assert_eq!(strip_marker("没有标记"), "没有标记");
    }
    #[test]
    fn quote_gist_takes_first_two_lines_and_clips() {
        let r = "（她别过脸）「别说了。」「这种话，我不接。」「你先站那儿。」";
        assert_eq!(quote_gist(r, 60), "别说了。 这种话，我不接。");
        assert_eq!(quote_gist("（没有台词）", 60), "", "没台词就空串");
        let long = quote_gist("「一二三四五六七八九十」", 4);
        assert_eq!(long, "一二三四…", "超长要截断加省略号");
    }
}
