import { useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Character, CharacterUsage, Relation, WorldSetting } from "../types";

type Args = { setMsg: Dispatch<SetStateAction<string>> };

export function useStoryEntities({ setMsg }: Args) {
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
      setMsg("❌ 加载角色失败: " + String(e));
    } finally {
      setCharsLoading(false);
    }
  };

  const loadWorlds = async (storyId: number) => {
    try {
      setWorlds(await invoke<WorldSetting[]>("list_world_settings", { storyId: String(storyId) }));
    } catch (e) {
      setMsg("❌ 加载世界观失败: " + String(e));
    }
  };

  const loadRelations = async (storyId: number) => {
    try {
      setRelations(await invoke<Relation[]>("list_relations", { storyId: String(storyId) }));
    } catch (e) {
      setMsg("❌ 加载关系失败: " + String(e));
    }
  };

  return {
    chars, setChars, charUsage, setCharUsage, charsLoading, setCharsLoading,
    recallPick, setRecallPick, recallDays, setRecallDays,
    worlds, setWorlds, relations, setRelations,
    loadChars, loadWorlds, loadRelations,
  };
}
