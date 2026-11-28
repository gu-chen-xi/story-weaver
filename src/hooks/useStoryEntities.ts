import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { reportError } from "../lib/errors";
import type { Character, CharacterUsage, Relation, WorldSetting } from "../types";

export function useStoryEntities() {
  const [chars, setChars] = useState<Character[]>([]);
  const [charUsage, setCharUsage] = useState<CharacterUsage[]>([]);
  const [charsLoading, setCharsLoading] = useState(false);
  const [recallPick, setRecallPick] = useState<number[]>([]);
  const [recallDays, setRecallDays] = useState<{ node_id: number; date_label: string; day: number | null; msg_count: number; preview: string; has_summary: boolean; eligible: boolean; tags?: string[] }[]>([]);
  const [worlds, setWorlds] = useState<WorldSetting[]>([]);
  const [relations, setRelations] = useState<Relation[]>([]);

  const loadChars = async (storyId: number) => {
    setCharsLoading(true);
    try {
      const [list, usage] = await Promise.all([
        invoke<Character[]>("list_characters", { storyId: String(storyId) }),
        invoke<CharacterUsage[]>("character_usage", { storyId: String(storyId) }),
      ]);
      setChars(list);
      setCharUsage(usage);
    } catch (e) {
      reportError("加载角色", e);
    } finally {
      setCharsLoading(false);
    }
  };

  const loadWorlds = async (storyId: number) => {
    try {
      setWorlds(await invoke<WorldSetting[]>("list_world_settings", { storyId: String(storyId) }));
    } catch (e) {
      reportError("加载世界观", e);
    }
  };

  const loadRelations = async (storyId: number) => {
    try {
      setRelations(await invoke<Relation[]>("list_relations", { storyId: String(storyId) }));
    } catch (e) {
      reportError("加载关系", e);
    }
  };

  return {
    chars, setChars, charUsage, setCharUsage, charsLoading, setCharsLoading,
    recallPick, setRecallPick, recallDays, setRecallDays,
    worlds, setWorlds, relations, setRelations,
    loadChars, loadWorlds, loadRelations,
  };
}
