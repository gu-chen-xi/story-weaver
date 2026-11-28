import { useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DebugReq, Story } from "../types";

type Args = { setMsg: Dispatch<SetStateAction<string>> };

export function useStoryCatalog({ setMsg }: Args) {
  const [stories, setStories] = useState<Story[]>([]);
  const [currentStory, setCurrentStory] = useState<Story | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [newOrigin, setNewOrigin] = useState("2026-01-01");
  const [newPrologue, setNewPrologue] = useState("");
  const [debugList, setDebugList] = useState<DebugReq[]>([]);
  const [selectedDebug, setSelectedDebug] = useState<DebugReq | null>(null);

  const loadStories = async () => {
    try {
      setStories(await invoke<Story[]>("list_stories"));
    } catch (e) {
      setMsg("❌ 加载故事失败: " + String(e));
    }
  };

  const loadDebug = async () => {
    if (!currentStory) return;
    try {
      setDebugList(await invoke<DebugReq[]>("list_debug_requests", { storyId: String(currentStory.id) }));
    } catch (e) {
      setMsg("❌ 加载设定版本失败: " + String(e));
    }
  };

  return {
    stories, setStories,
    currentStory, setCurrentStory,
    showNew, setShowNew,
    newTitle, setNewTitle,
    newOrigin, setNewOrigin,
    newPrologue, setNewPrologue,
    debugList, setDebugList,
    selectedDebug, setSelectedDebug,
    loadStories, loadDebug,
  };
}
