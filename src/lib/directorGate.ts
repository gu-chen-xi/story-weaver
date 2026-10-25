// 「导演指令」的软件闸门（v3.6.1）。
//
// 为什么放在软件层而不是提示词里：
//   关系强度 = 0 是「彻底决裂」—— 这时候还让导演指令生效，等于逼 AI 演一出
//   「她连看都不愿看你，却还是答应了」，人设必崩；而「让 AI 自己判断能不能拒」
//   已经证明过不可靠（模型对「可以拒绝」的理解是「尽量拒绝、显得有骨气」）。
//   所以这里做成硬闸门：前端拿到关系数据直接判，判完只给「能 / 不能」两个结果。
//
// 只看一个方向：**AI 扮演的角色 → 用户扮演的角色**。
//   强度 0 是「那一方对另一方毫无投入」；用户对 AI 的 0 不代表 AI 会拒收指令，
//   （用户本来就还在写这一段戏，他的态度由他打字表达，不由数值决定）。
//
// 没有关系记录 = 不拦。数据稀疏（很多人根本没建关系）不该变成"功能不可用"，
// 只有**明确填了 0** 才锁 —— 宁可漏拦，也不要为了拦一条把所有人的按钮关掉。

export type GateChar = { id: number; name: string };

export type GateRelation = {
  from_character_id: number;
  to_character_id: number;
  direction: string;
  strength: number;
};

export type DirectorGate = {
  /** true = 本次不能用导演指令（按钮置灰、面板不展开、已排队的指令不发） */
  blocked: boolean;
  /** 给用户看的原因；不拦时为空串 */
  reason: string;
};

const FREE = "自由";

/**
 * 判断本次能不能用导演指令。
 *
 * 拦的条件只有一条：**AI 扮演的角色 → 用户扮演的角色** 这条关系的强度 = 0。
 * 其余情况（旁白模式 / 有人是「自由」/ 角色不存在 / 两人是同一个 / 没有关系记录）
 * 一律放行。
 */
export function directorGate(args: {
  mode: string;
  myRole: string;
  aiRole: string;
  chars: GateChar[];
  relations: GateRelation[];
}): DirectorGate {
  const open: DirectorGate = { blocked: false, reason: "" };

  // 导演指令本来就只在沉浸对话里出现（旁白＝上帝扩写，没有"谁演谁"）
  if (args.mode !== "对话") return open;
  if (!args.myRole || !args.aiRole) return open;
  if (args.myRole === FREE || args.aiRole === FREE) return open;

  const ai = args.chars.find((c) => c.name === args.aiRole);
  const my = args.chars.find((c) => c.name === args.myRole);
  if (!ai || !my || ai.id === my.id) return open;

  const rel = relationFromAiToMe(args.relations, ai.id, my.id);
  // 没有明确填 0 就不拦（>0 是"还有一点希望"，负数按 0 处理）
  if (!rel || rel.strength > 0) return open;

  return {
    blocked: true,
    reason: `当前「${ai.name}」对「${my.name}」的关系强度 = 0（彻底决裂），导演指令不能添加`,
  };
}

/** AI→我 这条关系；没有时，只有「双向」那条反向记录能顶上（双向两侧共用一个数值）。 */
function relationFromAiToMe(
  relations: GateRelation[],
  aiId: number,
  myId: number,
): GateRelation | null {
  const direct = relations.find(
    (r) => r.from_character_id === aiId && r.to_character_id === myId,
  );
  if (direct) return direct;
  // 「单向」的反向那条是**用户对 AI** 的投入，读法不同，不能拿来当 AI 的态度
  return (
    relations.find(
      (r) =>
        r.from_character_id === myId &&
        r.to_character_id === aiId &&
        r.direction.trim() === "双向",
    ) ?? null
  );
}
