#[cfg(test)]
mod tail_view_tests {
    use crate::generate_stream::prompt::PromptScope;
    use crate::generate_stream::tail_view::build_tail_scope_view;
    use sqlx::SqlitePool;

    /// 两个角色（各有属性）+ 两条方向关系 + 只有"他"写了示例对话
    async fn fixture(tag: &str) -> (SqlitePool, String, std::path::PathBuf) {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.tmp/tail-view-tests")
            .join(format!("{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::story_db::init_story_db_at(dir.join("story.db")).await.unwrap();

        sqlx::query(
            "INSERT INTO characters(name, age, gender, stat_stamina, stat_sanity, stat_screen, current_status) VALUES('夏婉琴','18','女',30,25,25,'MARK_XIA_STAT')",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO characters(name, age, gender, stat_stamina, stat_sanity, stat_screen, current_status, dialogue_examples) VALUES('顾宸熙','19','男',80,80,80,'MARK_GU_STAT','MARK_GU_EXAMPLE 她说：你来了。')",
        ).execute(&pool).await.unwrap();
        let xia: i64 = sqlx::query_scalar("SELECT id FROM characters WHERE name='夏婉琴'").fetch_one(&pool).await.unwrap();
        let gu: i64 = sqlx::query_scalar("SELECT id FROM characters WHERE name='顾宸熙'").fetch_one(&pool).await.unwrap();
        for (f, t, rel, desc, strength) in [
            (xia, gu, "老朋友", "认识四年", 81_i64),
            (gu, xia, "旧识", "相识已久", 55_i64),
        ] {
            sqlx::query(
                "INSERT INTO relations(from_character_id,to_character_id,relation_type,direction,description,strength) VALUES(?1,?2,?3,'单向',?4,?5)",
            ).bind(f).bind(t).bind(rel).bind(desc).bind(strength).execute(&pool).await.unwrap();
        }
        (pool, String::new(), dir)
    }

    #[tokio::test]
    async fn global_tail_lists_everyone() {
        let (pool, _sid, dir) = fixture("global").await;
        // 全局视角下"用谁的示例"仍按三级回落（这里让 AI 扮演他 → 就取他的示例）
        let text = build_tail_scope_view(&pool, &dir.join("mem"), "test-story", &None, PromptScope::default(), "顾宸熙")
            .await
            .unwrap();
        assert!(text.contains("MARK_XIA_STAT"), "全局应有她的属性行");
        assert!(text.contains("MARK_GU_STAT"), "全局应有他的属性行");
        assert!(text.contains("夏婉琴 → 顾宸熙"), "全局应有 a→b 强度行");
        assert!(text.contains("顾宸熙 → 夏婉琴"), "全局应有 b→a 强度行");
        assert!(text.contains("MARK_GU_EXAMPLE"), "全局能取到示例对话（三级回落）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn role_tail_only_shows_her_own() {
        let (pool, _sid, dir) = fixture("role").await;
        let text = build_tail_scope_view(
            &pool,
            &dir.join("mem"),
            "test-story",
            &None,
            PromptScope { role: "夏婉琴", my_role: "顾宸熙" },
            "夏婉琴",
        )
        .await
        .unwrap();
        assert!(text.contains("MARK_XIA_STAT"), "她自己属性要在");
        assert!(!text.contains("MARK_GU_STAT"), "别人的属性不该出现（漏风点 ②）");
        assert!(text.contains("夏婉琴 → 顾宸熙"), "她出发的那条要留");
        assert!(!text.contains("顾宸熙 → 夏婉琴"), "别人对她的那条是隐私（漏风点 ①）");
        assert!(!text.contains("MARK_GU_EXAMPLE"), "示例只认她，她没有示例就不给（漏风点 ③）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn no_role_means_same_as_global() {
        let (pool, _sid, dir) = fixture("norole").await;
        let g = build_tail_scope_view(&pool, &dir.join("mem"), "test-story", &None, PromptScope::default(), "")
            .await
            .unwrap();
        let r = build_tail_scope_view(&pool, &dir.join("mem"), "test-story", &None, PromptScope { role: "", my_role: "" }, "")
            .await
            .unwrap();
        assert_eq!(g, r, "没指定扮演者时尾部两块必须一致");
        let _ = std::fs::remove_dir_all(&dir);
    }
}


#[cfg(test)]
mod memory_out_of_tail_tests {
    use crate::generate_stream::prompt::PromptScope;
    use crate::generate_stream::tail_view::build_tail_scope_view;

    /// 2026-11-15 用户定：关键记忆**不在尾部第②段**里，按关系组单独成第③段。
    #[tokio::test]
    async fn tail_view_does_not_contain_memory() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.tmp/tail-no-mem-tests")
            .join(format!("{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::story_db::init_story_db_at(dir.join("story.db")).await.unwrap();
        sqlx::query("INSERT INTO characters(name, age, gender) VALUES('夏婉琴','18','女')")
            .execute(&pool)
            .await
            .unwrap();
        // 记忆目录里有卡，但 tail_view 不该读它
        let mem = dir.join("mem");
        std::fs::create_dir_all(&mem).unwrap();
        std::fs::write(
            mem.join("001-表白.md"),
            "---
kind: confession
label: 表白
day: 第 3 天
node_id: 1
branch: main
characters: 夏婉琴,顾宸熙
importance: 90
---

# 第 3 天 · 表白

他说喜欢她。
",
        )
        .unwrap();
        let text = build_tail_scope_view(
            &pool,
            &mem,
            "test-story",
            &None,
            PromptScope { role: "夏婉琴", my_role: "顾宸熙" },
            "夏婉琴",
        )
        .await
        .unwrap();
        assert!(!text.contains("关键记忆"), "尾部不该再出现关键记忆（已挪到第③段）");
        assert!(!text.contains("表白"), "卡片内容也不该出现");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
