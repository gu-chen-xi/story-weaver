#[cfg(test)]
mod pair_prompt_tests {
    use crate::generate_stream::prompt::build_system_prompt_pair;
    use sqlx::SqlitePool;

    /// 造一个临时 story.db（含完整表结构），插两个角色 + 两条方向关系 + 一条世界观
    async fn fixture(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.tmp/pair-prompt-tests")
            .join(format!("{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::story_db::init_story_db_at(dir.join("story.db")).await.unwrap();

        // 两个角色：夏婉琴（AI 演）、顾宸熙（我演）。各带一个独有标记，方便断言"谁的卡在场"
        for (name, age, gender, one_liner) in [
            ("夏婉琴", "18", "女", "计算机系学生"),
            ("顾宸熙", "19", "男", "有钱人家的小少爷"),
        ] {
            sqlx::query(
                "INSERT INTO characters(name, age, gender, one_liner, profile) VALUES(?1, ?2, ?3, ?4, 'MARK_' || ?1)",
            )
            .bind(name).bind(age).bind(gender).bind(one_liner)
            .execute(&pool).await.unwrap();
        }
        let xia: i64 = sqlx::query_scalar("SELECT id FROM characters WHERE name = '夏婉琴'").fetch_one(&pool).await.unwrap();
        let gu: i64 = sqlx::query_scalar("SELECT id FROM characters WHERE name = '顾宸熙'").fetch_one(&pool).await.unwrap();

        // 一行一个方向：a→b 与 b→a 是两条独立记录
        for (from, to, rel, desc) in [
            (xia, gu, "老朋友", "认识四年了并且还是邻居"),
            (gu, xia, "旧识", "相识已久"),
        ] {
            sqlx::query(
                "INSERT INTO relations(from_character_id, to_character_id, relation_type, direction, description) VALUES(?1, ?2, ?3, '单向', ?4)",
            )
            .bind(from).bind(to).bind(rel).bind(desc)
            .execute(&pool).await.unwrap();
        }
        (pool, dir)
    }

    #[tokio::test]
    async fn pair_keeps_global_complete_and_role_scoped() {
        let (pool, dir) = fixture("scope").await;
        let (global, role) = build_system_prompt_pair(&pool, &None, "2022-01-01", "", "夏婉琴", "顾宸熙")
            .await
            .unwrap();

        // 全局：两张卡、两条关系都在
        assert!(global.contains("MARK_夏婉琴"), "全局应含她的卡");
        assert!(global.contains("MARK_顾宸熙"), "全局应含他的卡");
        assert!(global.contains("夏婉琴 是 顾宸熙 的「老朋友」"));
        assert!(global.contains("顾宸熙 是 夏婉琴 的「旧识」"));

        // 角色视角：只留她自己的卡 + 只留 from=她 的那一行关系
        assert!(role.contains("MARK_夏婉琴"), "角色视角要有她自己的卡");
        assert!(!role.contains("MARK_顾宸熙"), "别人的档案不该出现（她会知道不该知道的设定）");
        assert!(role.contains("夏婉琴 是 顾宸熙 的「老朋友」"));
        assert!(!role.contains("顾宸熙 是 夏婉琴 的「旧识」"), "反向关系行属于别人的隐私");

        // 但"对面那个人"的**最小认知**要给（名字 / 年龄 / 性别 / 一句话身份）
        assert!(role.contains("【你对面的他】"), "角色视角应有【你对面的他】");
        assert!(role.contains("顾宸熙（男，19 岁）"), "对面那行应带名字/性别/年龄");

        // 关系单独成段（2026-11-14）
        assert!(role.contains("【关系图谱】"), "关系应有独立段标题");
        assert!(global.contains("【关系图谱】"));

        // 两个视角都不是空壳
        assert!(role.len() > 500, "角色视角不该被裁成空提示词（实际 {} 字）", role.chars().count());
        assert!(role.chars().count() < global.chars().count(), "角色视角应该比全局短");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn pair_without_roles_keeps_both_identical() {
        // 没指定扮演者（「自由」/空）→ 两栏必须一模一样，不能误筛
        let (pool, dir) = fixture("norole").await;
        let (global, role) = build_system_prompt_pair(&pool, &None, "2022-01-01", "", "", "").await.unwrap();
        assert_eq!(global, role, "没有扮演者时角色视角必须与全局完全一致");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
