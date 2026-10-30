// 「三维数值自动变化」插件（2026-10-29）。
//
// 为什么做成插件而不是默认功能：这是「AI 内容自动改库」——塘主定：默认关，
// 开关写 `plugin.stat-growth.enabled`（前端键名规范），后端按同一个键决定
// 注不注【状态变化】段、结不结算、写不写 stat_changes。关着时三个数值仍可手动改。

import type { PluginDescriptor, PluginModule } from "../../types";
import type { PluginTheme } from "../../types";

export const STAT_GROWTH_ID = "stat-growth";

const DESCRIPTOR: PluginDescriptor = {
  id: STAT_GROWTH_ID,
  icon: "🌱",
  name: "三维数值自动变化（体力 / 理智 / 存在感）",
  status: "ready",
  desc: "开启后，AI 会按这一轮发生的事自动调整三个数值（熬夜扣体力、被羞辱掉存在感、睡好恢复…）。"
    + "幅度由软件按固定表算（模型只报「发生了什么」），每次变化都写进账本、可在监视器/角色卡里单条撤销。",
  caps: { canToggle: true, canViewPanel: true, needsStory: true },
  toggleLabel: "自动变化",
  toggleHint: "默认关。开启后每轮多一行协议（不进正文），不会额外调用 API。",
};

const ROWS: [string, string, string][] = [
  ["体力", "受伤 / 生病 / 力竭 −15 · 睡好 +15 · 熬夜 −10（作息只在日结里判）", "单条上限 15"],
  ["理智", "被欺骗 / 撞见无法解释的事 / 崩溃 −10 · 被安抚 / 想通 +10 · 睡好 +4", "单条上限 10"],
  ["存在感", "被无视 −3 · 被看见 +3 · 被当众羞辱 −5 · 被认可 +5", "单条上限 5，当天累计只算 ±5"],
];

function RulesPanel({ T }: { T: PluginTheme }) {
  return (
    <div style={{ fontSize: 11.5, color: T.text2, lineHeight: 1.8 }}>
      <div style={{ color: T.text3, marginBottom: 4 }}>
        模型只写「发生了什么」，数字由软件按这张表算；表外类型不记账。同一故事日同一类型只算一次（「睡好」一天只恢复一次）。
      </div>
      {ROWS.map(([dim, rule, cap]) => (
        <div key={dim} style={{ display: "flex", gap: 8 }}>
          <span style={{ width: 52, flexShrink: 0, color: T.text1 }}>{dim}</span>
          <span style={{ flex: 1 }}>{rule}</span>
          <span style={{ flexShrink: 0, color: T.text3 }}>{cap}</span>
        </div>
      ))}
      <div style={{ color: T.text3, marginTop: 6 }}>
        撤销是单条的（反向补一次，历史保留）；换天不会自动恢复 —— 只有剧情里真的「睡好」才回体力。
      </div>
    </div>
  );
}

export const STAT_GROWTH_MODULE: PluginModule = {
  descriptor: DESCRIPTOR,
  renderPanel: ({ T }) => <RulesPanel T={T} />,
};
