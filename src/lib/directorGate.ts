// 「导演指令」的软件闸门（v3.7.0）。
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

/**
 * 导演代价（2026-10-27）：闸门的**第二个条件**。
 *
 * 强度闸门管"能不能开这个口子"（彻底决裂 = 不能）；代价闸门管"开了这么多次，她还买不买账"。
 * 累计只算**没被赦免**的那些（后端 ledger 已经算好，前端只拿来比）。
 */
export type DirectorCostState = { total: number; limit: number };

const FREE = "自由";

/**
 * 判断本次能不能用导演指令。
 *
 * 拦的条件**两条**：
 *   ① **AI 扮演的角色 → 用户扮演的角色** 的关系强度 = 0（彻底决裂，见 v3.7.0）；
 *   ② 这一对角色的**导演代价**累计 ≥ 上限（开挂太多，见 2026-10-27 的账本）。
 * 其余情况（旁白模式 / 有人是「自由」/ 角色不存在 / 两人是同一个 / 没有关系记录 / 拿不到账本）
 * 一律放行。
 */
export function directorGate(args: {
  mode: string;
  myRole: string;
  aiRole: string;
  chars: GateChar[];
  relations: GateRelation[];
  /** 当前 AI 扮演者的导演代价累计（拿不到账本时传 null = 不拦） */
  cost?: DirectorCostState | null;
}): DirectorGate {
  const open: DirectorGate = { blocked: false, reason: "" };

  // 导演指令本来就只在沉浸对话里出现（旁白＝上帝扩写，没有"谁演谁"）
  if (args.mode !== "对话") return open;
  if (!args.myRole || !args.aiRole) return open;

  // 先把能判的一条判了：强度 0 = 彻底决裂，什么都不用谈（比"代价满了"更根本 ——
  // 报了代价，用户去赦免也还是发不出去，等于骗他白点一遍）。
  const ai = args.chars.find((c) => c.name === args.aiRole);
  const my = args.chars.find((c) => c.name === args.myRole);
  const resolvable =
    args.myRole !== FREE && args.aiRole !== FREE && !!ai && !!my && ai.id !== my.id;
  if (resolvable && ai && my) {
    const rel = relationFromAiToMe(args.relations, ai.id, my.id);
    if (rel && rel.strength <= 0) {
      return {
        blocked: true,
        reason: `当前「${ai.name}」对「${my.name}」的关系强度 = 0（彻底决裂），导演指令不能添加`,
      };
    }
  }

  // 条件二（2026-10-27）：导演代价攒满了 —— 开挂太多，她不再买账。
  // **放在"自由 / 查无此人"这些放行判断之前**：否则把 AI 扮演者切成「自由」就能绕开上限，
  // 而「自由」那一本账（character_id = NULL）会一直涨、永远不锁。阈值来自后端 Ledger.limit。
  const cost = args.cost;
  if (cost && cost.limit > 0 && cost.total >= cost.limit) {
    const who = args.aiRole && args.aiRole !== FREE ? `「${args.aiRole}」的` : "这条线的";
    return {
      blocked: true,
      reason: `${who}导演代价已累计 ${cost.total}/${cost.limit} —— 开挂太多，她不再买账；先到「📒 导演账本」里撤销或赦免几笔`,
    };
  }

  // 其余情况一律放行：自由 / 查无此人 / 同一角色 / 没有关系记录 —— 数据稀疏不该锁功能
  return open;
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
