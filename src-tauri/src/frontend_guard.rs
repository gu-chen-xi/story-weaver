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

    /// 行数上限。当前 6209 行；只许往下调。
    const APP_MAX_LINES: usize = 6050;

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
            ("views/TimelinePage.tsx", TIMELINE_PAGE, 250),
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
}
