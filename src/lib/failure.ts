// 流式失败标记（2026-11-04）：后端在已生成正文末尾追加 `〔失败原因〕xxx`。
// 正文里要剥掉它，界面单独用红色小字显示。
export const FAIL_MARK = "〔失败原因〕";

/** 正文部分（去掉失败原因段） */
export function stripFailure(content: string): string {
  const i = content.indexOf(FAIL_MARK);
  return i < 0 ? content : content.slice(0, i);
}

/** 失败原因（没有则 null） */
export function failureOf(content: string): string | null {
  const i = content.indexOf(FAIL_MARK);
  if (i < 0) return null;
  return content.slice(i + FAIL_MARK.length).trim() || "未知原因";
}
