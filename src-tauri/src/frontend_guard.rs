// 前端结构守卫（`cargo test` 会跑）。
//
// 为什么需要它：`src/App.tsx` 曾经长到 **6788 行 / 159 个 useState**，
// 所有页面、所有逻辑挤在一个组件里，改一处要在六千行里翻。
// 2026-10-17 做了第一轮拆分（类型/主题/常量/纯函数/两个页面）。
//
// 这些断言防两件事：
//   ① **回涨**：以后往 App.tsx 里继续堆，行数超上限就红。
//   ② **回流**：把已经抽出去的模块又抄回 App.tsx（复制粘贴式的"重构"）。
//
// 上限的维护纪律：**只许往下调**。拆分有进展就把上限改小；需要往上调时必须
// 在 ROADMAP 里写明理由 —— 否则这个守卫会慢慢失去意义。

#[cfg(test)]
mod tests {
    /// App.tsx 的源码（编译期嵌入，测试不需要读文件系统）。
    const APP_TSX: &str = include_str!("../../src/App.tsx");
    /// 拆分后的模块，确保它们真的存在且不是空壳。
    const TYPES_TS: &str = include_str!("../../src/types.ts");
    const THEME_TS: &str = include_str!("../../src/theme.ts");
    const CONSTANTS_TS: &str = include_str!("../../src/constants.ts");
    const PARSE_TS: &str = include_str!("../../src/lib/parse.ts");
    const RICH_TSX: &str = include_str!("../../src/lib/rich.tsx");
    const TEMP_PAGE: &str = include_str!("../../src/views/TempPage.tsx");
    const COMPLIANCE_PAGE: &str = include_str!("../../src/views/CompliancePage.tsx");
    const TIMELINE_PAGE: &str = include_str!("../../src/views/TimelinePage.tsx");
    const PORTRAIT_PAGE: &str = include_str!("../../src/views/PortraitPage.tsx");
    const CHARS_PAGE: &str = include_str!("../../src/views/CharsPage.tsx");
    const WORLD_PAGE: &str = include_str!("../../src/views/WorldPage.tsx");
    const PARAMS_PAGE: &str = include_str!("../../src/views/ParamsPage.tsx");
    const PROPOSALS_PAGE: &str = include_str!("../../src/views/ProposalsPage.tsx");
    const PLUGINS_PAGE: &str = include_str!("../../src/views/PluginsPage.tsx");
    const STORY_PANEL: &str = include_str!("../../src/views/StoryPanel.tsx");
    const STORY_INPUT: &str = include_str!("../../src/views/StoryInput.tsx");
    const STORIES_PAGE: &str = include_str!("../../src/views/StoriesPage.tsx");
    const SIDEBAR: &str = include_str!("../../src/views/Sidebar.tsx");
    const STORY_SESSION: &str = include_str!("../../src/hooks/useStorySession.ts");
    // v3.4：从 App.tsx 抽出的两块业务逻辑 + 分支视图纯函数
    const USE_CHECK_PATCH: &str = include_str!("../../src/hooks/useCheckPatch.ts");
    const USE_COMPLIANCE: &str = include_str!("../../src/hooks/useCompliance.ts");
    const BRANCH_VIEW: &str = include_str!("../../src/lib/branchView.ts");
    const USE_PLUGINS: &str = include_str!("../../src/hooks/usePlugins.ts");
    // v3.4：关键记忆面板（表白/承诺/牺牲/背叛，每种只记第一次）
    const KEY_MEMORIES: &str = include_str!("../../src/views/KeyMemories.tsx");
    // v3.5.2：时间线视图状态与计算 / 纯日期计算
    const USE_TIMELINE_VIEW: &str = include_str!("../../src/hooks/useTimelineView.ts");
    // v3.5.6：A 步（state 按页面归拢）
    const USE_TEMP_FILES: &str = include_str!("../../src/hooks/useTempFiles.ts");
    const USE_APP_SETTINGS: &str = include_str!("../../src/hooks/useAppSettings.ts");
    const USE_PORTAITS: &str = include_str!("../../src/hooks/usePortraits.ts");
    const USE_STORY_INPUT_PREFS: &str = include_str!("../../src/hooks/useStoryInputPrefs.ts");
    const TIMELINE_DATES: &str = include_str!("../../src/lib/timelineDates.ts");
    // v3.5.1：时间线段框树（纯布局 + 渲染 + 错误边界）
    const TIMELINE_TREE: &str = include_str!("../../src/views/TimelineTree.tsx");
    // v3.5.4：对话区固定头部的"天数药丸横条"
    const DAY_STRIP: &str = include_str!("../../src/views/DayStrip.tsx");
    const TIMELINE_SEGMENTS: &str = include_str!("../../src/lib/timelineSegments.ts");
    // v3.5.2：逐日节点树（每个节点一个药丸，与段框共用通用布局）
    const TIMELINE_NODES: &str = include_str!("../../src/lib/timelineNodes.ts");
    const USE_GENERATION: &str = include_str!("../../src/hooks/useGeneration.ts");
    const CHECK_REPORT_MODAL: &str = include_str!("../../src/views/CheckReportModal.tsx");
    const INSERT_EVENT_MODAL: &str = include_str!("../../src/views/InsertEventModal.tsx");
    const DIRECTOR_PANEL: &str = include_str!("../../src/views/DirectorPanel.tsx");
    const SETTINGS_MODAL: &str = include_str!("../../src/views/SettingsModal.tsx");
    const EXPORT_MODAL: &str = include_str!("../../src/views/ExportModal.tsx");
    const LIB_CHARS: &str = include_str!("../../src/lib/chars.ts");

    /// 行数上限。当前 **1249** 行；只许往下调。
    ///
    /// 历史：6788 → 1650（2026-10-17 首轮拆分）→ **1300**（2026-10-24 A/B 步重构：
    /// state 按页面收进 4 个 hook、props 收成带类型的对象）。
    /// 留 ~50 行余量，够小改动，但不给"再堆一个页面"留空间。
    const APP_MAX_LINES: usize = 1300;

    fn lines_of(s: &str) -> usize {
        s.lines().count()
    }

    #[test]
    fn app_tsx_must_not_grow_beyond_the_budget() {
        let n = lines_of(APP_TSX);
        assert!(
            n <= APP_MAX_LINES,
            "src/App.tsx 已经 {n} 行，超过上限 {APP_MAX_LINES} 行。\n\
             App.tsx 是单文件单组件（159 个 useState），继续堆会让改动越来越危险。\n\
             请把新东西放进 src/views/ 或 src/lib/，不要在 App.tsx 里加。\n\
             如果你确实需要提高上限，请在 ROADMAP 里写明理由后再改 APP_MAX_LINES。"
        );
    }

    /// 已经抽出去的顶层定义，不许再出现在 App.tsx 里（防止复制粘贴式回流）。
    #[test]
    fn extracted_modules_must_not_be_reinlined() {
        let forbidden: &[(&str, &str)] = &[
            ("const T = {", "主题色板已移到 src/theme.ts"),
            ("const PLUGIN_REGISTRY", "插件注册表已移到 src/constants.ts"),
            ("const SENSITIVE_WORDS", "敏感词表已移到 src/constants.ts"),
            ("function parseScriptBlocks", "剧本解析已移到 src/lib/parse.ts"),
            ("function splitPromptSections", "提示词分段已移到 src/lib/parse.ts"),
            ("function computeBudget", "预算计算已移到 src/lib/parse.ts"),
            ("const renderBold =", "加粗渲染已移到 src/lib/rich.tsx"),
            ("type Character = {", "数据类型已移到 src/types.ts"),
            ("<span>🖼 角色立绘</span>", "导演监视器面板已移到 views/DirectorPanel.tsx"),
            ("密钥保存在 Windows 凭据管理器（不写入数据库", "设置弹窗已移到 views/SettingsModal.tsx"),
            ("📥 导出剧情（TXT）", "导出弹窗已移到 views/ExportModal.tsx"),
            // v3.5.6：A 步抽出的四块 state，不许再写回 App
            ("const [tempFiles, setTempFiles]", "临时文件状态已移到 hooks/useTempFiles.ts"),
            ("const loadSettings = async", "设置读取已移到 hooks/useAppSettings.ts"),
            ("const [portraits, setPortraits]", "立绘素材状态已移到 hooks/usePortraits.ts"),
            ("const [directors, setDirectors]", "剧情输入台偏好已移到 hooks/useStoryInputPrefs.ts"),
            ("localStorage.setItem(\"ai.constraints\"", "剧情约束的落盘归 hooks/useStoryInputPrefs.ts（键名只许出现一次）"),
        ];
        for (needle, why) in forbidden {
            assert!(
                !APP_TSX.contains(needle),
                "App.tsx 里又出现了 `{needle}` —— {why}，请改成 import，不要抄回来。"
            );
        }
    }

    /// 抽出的模块必须真的承载内容（不是只留个空壳骗过上面的检查）。
    #[test]
    fn extracted_modules_carry_real_content() {
        let checks: &[(&str, &str, usize)] = &[
            ("types.ts", TYPES_TS, 100),
            ("theme.ts", THEME_TS, 20),
            ("constants.ts", CONSTANTS_TS, 80),
            ("lib/parse.ts", PARSE_TS, 150),
            ("lib/rich.tsx", RICH_TSX, 10),
            ("views/TempPage.tsx", TEMP_PAGE, 40),
            ("views/CompliancePage.tsx", COMPLIANCE_PAGE, 120),
            ("views/TimelinePage.tsx", TIMELINE_PAGE, 120),
            ("views/PortraitPage.tsx", PORTRAIT_PAGE, 600),
            ("views/CharsPage.tsx", CHARS_PAGE, 450),
            ("views/WorldPage.tsx", WORLD_PAGE, 380),
            ("views/ParamsPage.tsx", PARAMS_PAGE, 380),
            ("views/ProposalsPage.tsx", PROPOSALS_PAGE, 350),
            ("views/PluginsPage.tsx", PLUGINS_PAGE, 150),
            ("views/StoryPanel.tsx", STORY_PANEL, 280),
            // C1（2026-10-18）：生成流抽到 hooks/useGeneration.ts 后，StoryInput 只剩渲染
            ("views/StoryInput.tsx", STORY_INPUT, 200),
            ("views/StoriesPage.tsx", STORIES_PAGE, 80),
            ("views/Sidebar.tsx", SIDEBAR, 130),
            ("hooks/useStorySession.ts", STORY_SESSION, 300),
            // 生成流（发送 / 停止 / 4 个流式事件）—— 逐字从 StoryInput 搬来，行为不变
            ("hooks/useGeneration.ts", USE_GENERATION, 240),
            // C2（2026-10-18）：两个弹窗从 App.tsx 抽出（检查报告 / 插入事件）
            ("views/CheckReportModal.tsx", CHECK_REPORT_MODAL, 120),
            ("views/InsertEventModal.tsx", INSERT_EVENT_MODAL, 40),
            ("lib/chars.ts", LIB_CHARS, 20),
            // 2026-10-20：导演监视器面板从 App.tsx 抽出（物理搬家，JSX 一字未改）
            ("views/DirectorPanel.tsx", DIRECTOR_PANEL, 200),
            // 2026-10-20：设置弹窗 / 导出弹窗抽出（JSX 原样搬 + 草稿 state 一并搬入）
            ("views/SettingsModal.tsx", SETTINGS_MODAL, 150),
            ("views/ExportModal.tsx", EXPORT_MODAL, 60),
            // v3.4：从 App.tsx 抽出的两块业务逻辑 + 分支视图纯函数
            ("hooks/useCheckPatch.ts", USE_CHECK_PATCH, 150),
            ("hooks/useCompliance.ts", USE_COMPLIANCE, 40),
            ("lib/branchView.ts", BRANCH_VIEW, 60),
            ("hooks/usePlugins.ts", USE_PLUGINS, 120),
            ("views/KeyMemories.tsx", KEY_MEMORIES, 120),
            ("hooks/useTimelineView.ts", USE_TIMELINE_VIEW, 80),
            // v3.5.6：A 步抽出的四块
            ("hooks/useTempFiles.ts", USE_TEMP_FILES, 30),
            ("hooks/useAppSettings.ts", USE_APP_SETTINGS, 50),
            ("hooks/usePortraits.ts", USE_PORTAITS, 35),
            ("hooks/useStoryInputPrefs.ts", USE_STORY_INPUT_PREFS, 45),
            ("lib/timelineDates.ts", TIMELINE_DATES, 25),
            ("views/TimelineTree.tsx", TIMELINE_TREE, 180),
            ("views/DayStrip.tsx", DAY_STRIP, 60),
            ("lib/timelineSegments.ts", TIMELINE_SEGMENTS, 200),
            ("lib/timelineNodes.ts", TIMELINE_NODES, 120),
        ];
        for (name, src, min) in checks {
            let n = lines_of(src);
            assert!(n >= *min, "src/{name} 只有 {n} 行（应至少 {min} 行）—— 拆分是不是被回退了？");
        }
    }

    /// 拆分后的模块应该用 `export` 暴露，而不是靠全局。
    #[test]
    fn extracted_modules_export_their_symbols() {
        for (name, src) in [
            ("types.ts", TYPES_TS),
            ("theme.ts", THEME_TS),
            ("constants.ts", CONSTANTS_TS),
            ("lib/parse.ts", PARSE_TS),
            ("hooks/useCheckPatch.ts", USE_CHECK_PATCH),
            ("hooks/useCompliance.ts", USE_COMPLIANCE),
            ("lib/branchView.ts", BRANCH_VIEW),
            ("hooks/usePlugins.ts", USE_PLUGINS),
            ("views/KeyMemories.tsx", KEY_MEMORIES),
            ("hooks/useTimelineView.ts", USE_TIMELINE_VIEW),
            ("lib/timelineDates.ts", TIMELINE_DATES),
            ("views/TimelineTree.tsx", TIMELINE_TREE),
            ("views/DayStrip.tsx", DAY_STRIP),
            ("lib/timelineSegments.ts", TIMELINE_SEGMENTS),
            ("lib/timelineNodes.ts", TIMELINE_NODES),
        ] {
            assert!(
                src.contains("export "),
                "src/{name} 里没有任何 export —— 拆出来的东西必须能被 import"
            );
        }
    }

    // ───────────── 前端单元测试的守卫 ─────────────
    //
    // 前端测试是 `npm test` 跑的（Node 自带的 node:test，零依赖）。
    // 这里只守两件"被悄悄删掉"的事：
    //   ① package.json 里必须还有 test 脚本；
    //   ② tests/ 里必须有足够多的用例（防止被整体清空后没人发现）。

    const PACKAGE_JSON: &str = include_str!("../../package.json");
    const TEST_PARSE: &str = include_str!("../../tests/parse.test.cjs");
    const TEST_RUNNER: &str = include_str!("../../scripts/test-frontend.mjs");

    #[test]
    fn frontend_test_command_is_wired() {
        assert!(
            PACKAGE_JSON.contains("\"test\"") && PACKAGE_JSON.contains("test-frontend.mjs"),
            "package.json 里缺少 test 脚本（应为 `node scripts/test-frontend.mjs`）—— \
             前端单元测试会悄悄消失。"
        );
        assert!(
            TEST_RUNNER.contains("node:test") || TEST_RUNNER.contains("--test"),
            "scripts/test-frontend.mjs 看起来已经调不动 node 的测试运行器了。"
        );
    }

    /// 用例数量下限：只许加，不许减到这个数以下。
    #[test]
    fn frontend_tests_are_not_hollowed_out() {
        let cases = TEST_PARSE.matches("test(\"").count();
        assert!(
            cases >= 30,
            "tests/parse.test.cjs 只剩 {cases} 个用例（应 ≥30）—— 测试是不是被删了？"
        );
        // 覆盖到的关键函数，缺一个都说明测试缩水了
        for func in [
            "parseScriptBlocks",
            "splitPromptSections",
            "computeBudget",
            "parseDebugHistory",
            "parseDirector",
            "stripDirector",
            "estimateTokens",
        ] {
            assert!(
                TEST_PARSE.contains(func),
                "tests/parse.test.cjs 里已经没有针对 `{func}` 的测试了"
            );
        }
    }

    /// 导演监视器面板的**区块顺序**是用户明确指定的，不许随手调换：
    /// 角色立绘 → 当前角色卡 → 关系图谱 → 🧩 上下文概览 → 🧾 摘要管理 → 🔍 回顾试算。
    ///
    /// 为什么要守卫：这几块在历史上被搬过一次（`摘要管理` 曾插错位置到面板外面，
    /// 用户反馈"看不见"）。现在它是独立文件了，用位置比较就能低成本锁住顺序。
    #[test]
    fn director_panel_keeps_user_specified_order() {
        let needles = [
            ("<span>🖼 角色立绘</span>", "角色立绘"),
            ("<span>🪪 当前角色卡</span>", "当前角色卡"),
            ("<span>🕸 关系图谱</span>", "关系图谱"),
            ("<ContextPanel storyId", "🧩 上下文概览"),
            ("<PhaseManager", "🧾 摘要管理"),
            ("<RecallDebug storyId", "🔍 回顾试算"),
        ];
        let mut last = 0usize;
        for (needle, name) in needles {
            let pos = DIRECTOR_PANEL
                .find(needle)
                .unwrap_or_else(|| panic!("DirectorPanel.tsx 里找不到「{name}」（{needle}）—— 是不是被删了？"));
            assert!(
                pos >= last,
                "DirectorPanel.tsx 的区块顺序被改乱了：「{name}」出现在上一块之前。\n\
                 正确顺序：角色立绘 → 当前角色卡 → 关系图谱 → 🧩 上下文概览 → 🧾 摘要管理 → 🔍 回顾试算"
            );
            last = pos;
        }
    }
}
