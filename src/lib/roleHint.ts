// 切换「我扮演 / AI 扮演」时的提示文案（2026-11-11）。
//
// 为什么要有：2026-11-11 起对话模式按**配对视角**取料（ROADMAP §七）。
// 换扮演者 = 换一段关系的视角 —— 上一段关系的对话不会消失，只是不在当前视角里。
// 这件事不提示会吓人（"我的历史没了？"），所以切一下就弹一条 T3 提示。
//
// 纯函数，好让 `node:test` 直接测文案与"没变就不弹"。

export type RolePair = { ai: string; my: string };

const disp = (s: string) => (s && s.trim() && s.trim() !== "自由" ? s.trim() : "自由");

/**
 * 返回要提示的文案；**配对没变就返回 `null`**（不弹）。
 *
 * 只在"真的换了人"时提示：`自由 → 自由`、`夏 → 夏` 都不算变化。
 */
export function roleSwitchHint(prev: RolePair, next: RolePair): string | null {
  const sameAi = disp(prev.ai) === disp(next.ai);
  const sameMy = disp(prev.my) === disp(next.my);
  if (sameAi && sameMy) return null;
  const who = sameAi ? "我扮演" : sameMy ? "AI 扮演" : "两边扮演者";
  return (
    `🔀 ${who}已切换 → 现在是「${disp(next.ai)}」×「${disp(next.my)}」的视角。` +
    `这段关系里的旧对话不会消失，只是不在这个视角里（旁白一直看得到）。`
  );
}
