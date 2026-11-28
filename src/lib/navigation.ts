import type { IconName } from "../components/icons";
import type { AppView } from "../types";

export type NavItem = {
  icon: IconName;
  label: string;
  view: AppView | null;
  action: (() => void) | null;
  sub?: boolean;
};

export function buildStoryNavItems(onOpenStories: () => void): NavItem[] {
  return [
    { icon: "folder", label: "故事列表", view: "stories", action: onOpenStories },
    { icon: "calendar", label: "时间线", view: "timeline", action: null, sub: true },
    { icon: "users", label: "角色管理", view: "chars", action: null, sub: true },
    { icon: "globe", label: "世界观设定", view: "world", action: null, sub: true },
    { icon: "book", label: "历史章节", view: "chapters", action: null, sub: true },
    { icon: "clipboard", label: "演进提案", view: "proposals", action: null, sub: true },
  ];
}
