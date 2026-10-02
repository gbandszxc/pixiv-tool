//! IPC 命令层集成冒烟测试。
//!
//! 不经 GUI / Tauri 运行时，直接以临时目录构造完整 AppState
//! （paths/settings/db 均指向 tempdir；CookieStore 仅构造、绝不读写
//! keyring——macOS 上真实读写会弹授权框），调用命令层抽出的 `*_impl`：
//! - settings get/apply round-trip（临时 config，落盘后重载一致）
//! - history_list 空库返回空 + 未知分类契约
//! - novel / illustration 批量删除：空列表错误契约与计数
//!
//! task 相关 impl（delete_tasks_impl 等）在 `commands::task_cmds` 内
//! 已有等价单测，此处不重复。

use pixiv_tool_lib::commands::history_cmds::history_list_impl;
use pixiv_tool_lib::commands::misc_cmds::{
    illustrations_batch_delete_impl, novels_batch_delete_impl,
};
use pixiv_tool_lib::commands::saucenao_cmds::saucenao_search_impl;
use pixiv_tool_lib::commands::settings_cmds::apply_settings_patch;
use pixiv_tool_lib::db::{Db, IllustrationInsert, NovelInsert, now_iso};
use pixiv_tool_lib::paths::AppPaths;
use pixiv_tool_lib::settings::Settings;
use pixiv_tool_lib::state::AppState;
use serde_json::json;

/// 临时目录里的完整 AppState（与 lib.rs setup 的装配方式一致）。
fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("pixiv-tool-smoke-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Db::open(&dir.join("app.db")).unwrap();
    let paths = AppPaths {
        data_dir: dir.clone(),
        config_dir: dir.join("config"),
        logs_dir: dir.join("logs"),
    };
    (AppState::new(paths, Settings::default(), db), dir)
}

fn cleanup(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// settings_get / settings_save 的核心路径：
/// snapshot → apply patch → 落盘 → 重新 load_or_init → 内存快照一致。
#[test]
fn settings_apply_round_trip_via_temp_config() {
    let (state, dir) = temp_state("settings");

    // 初始快照 = 默认值（等价 settings_get）
    let before = state.settings_snapshot();
    assert_eq!(before, Settings::default());

    // 等价 settings_save：patch → save → 更新内存锁
    let patch = json!({
        "language": "en-US",
        "max_wait_seconds": 3600,
        "novel_font_scale": 1.25
    });
    let updated =
        apply_settings_patch(&state.settings_snapshot(), &patch, &state.paths.data_dir).unwrap();
    updated.save(&state.paths.config_dir).unwrap();
    *state.settings.lock().unwrap() = updated;

    // 内存快照与落盘重载结果一致（跨“重启”round-trip）
    let snapshot = state.settings_snapshot();
    let reloaded = Settings::load_or_init(&state.paths.config_dir);
    assert_eq!(snapshot, reloaded);
    assert_eq!(reloaded.language, "en-US");
    assert_eq!(reloaded.max_wait_seconds, 3600);
    assert_eq!(reloaded.novel_font_scale, 1.25);
    // 未出现在 patch 里的键保持默认
    assert_eq!(reloaded.output_dir, Settings::default().output_dir);

    // 非法 patch 被中文校验拒绝且不落盘
    let err = apply_settings_patch(
        &reloaded,
        &json!({ "max_wait_seconds": 86401 }),
        &state.paths.data_dir,
    )
    .unwrap_err();
    assert_eq!(err, "最大等待时间必须是 30~86400 秒之间的整数");
    assert_eq!(
        Settings::load_or_init(&state.paths.config_dir).max_wait_seconds,
        3600,
        "被拒绝的 patch 不应影响已持久化的配置"
    );
    cleanup(&dir);
}

/// 空库 + 各分类（含未知分类契约）的 history_list 形状。
#[test]
fn history_list_empty_db_returns_empty() {
    let (state, dir) = temp_state("history");
    for category in ["all", "novel", "illustration"] {
        let body = history_list_impl(&state, category, 1, 20, None).unwrap();
        assert_eq!(body["items"], json!([]), "category={category}");
        assert_eq!(body["total"], json!(0), "category={category}");
        assert_eq!(body["page"], json!(1));
        assert_eq!(body["page_size"], json!(20));
    }
    // 关键词过滤在空库上同样返回空
    let body = history_list_impl(&state, "all", 1, 20, Some("不存在")).unwrap();
    assert_eq!(body["total"], json!(0));
    // 未知分类 → reject（旧 4xx detail 风格）
    assert_eq!(
        history_list_impl(&state, "manga", 1, 20, None).unwrap_err(),
        "未知历史分类: manga"
    );
    cleanup(&dir);
}

/// 历史（含插画）在写入后可见 —— 佐证 list_history 与 insert 的联合查询。
#[test]
fn history_list_sees_inserted_rows() {
    let (state, dir) = temp_state("history-rows");
    state
        .db
        .insert_novel(&NovelInsert {
            novel_id: 101,
            title: "小说A".into(),
            captured_at: now_iso(),
            ..Default::default()
        })
        .unwrap();
    state
        .db
        .insert_illustration(&IllustrationInsert {
            artwork_id: 202,
            title: "插画B".into(),
            captured_at: now_iso(),
            ..Default::default()
        })
        .unwrap();
    let body = history_list_impl(&state, "all", 1, 20, None).unwrap();
    assert_eq!(body["total"], json!(2));
    assert_eq!(body["items"].as_array().map(Vec::len), Some(2));
    // 分类过滤各自命中一条
    assert_eq!(
        history_list_impl(&state, "novel", 1, 20, None).unwrap()["total"],
        json!(1)
    );
    assert_eq!(
        history_list_impl(&state, "illustration", 1, 20, None).unwrap()["total"],
        json!(1)
    );
    // 关键词命中
    assert_eq!(
        history_list_impl(&state, "all", 1, 20, Some("插画")).unwrap()["total"],
        json!(1)
    );
    cleanup(&dir);
}

/// 批量删除：空列表契约（200 + error，不 reject）。
#[test]
fn batch_delete_empty_lists_return_error_contracts() {
    let (state, dir) = temp_state("batch-empty");
    assert_eq!(
        novels_batch_delete_impl(&state, Vec::new(), false).unwrap()["error"],
        json!("novel_ids 不能为空")
    );
    assert_eq!(
        illustrations_batch_delete_impl(&state, Vec::new(), false).unwrap()["error"],
        json!("illustration_ids 不能为空")
    );
    cleanup(&dir);
}

/// 批量删除计数与“未指定记录保留”。
#[test]
fn batch_delete_counts_and_keeps_unspecified() {
    let (state, dir) = temp_state("batch-count");
    for id in [1, 2, 3] {
        state
            .db
            .insert_novel(&NovelInsert {
                novel_id: id,
                title: format!("n{id}"),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
    }
    let body = novels_batch_delete_impl(&state, vec![1, 2], false).unwrap();
    assert_eq!(body["status"], json!("success"));
    assert_eq!(body["deleted"], json!(2));
    assert!(state.db.get_novel(3).is_some(), "未指定的小说保留");

    for id in [11, 12] {
        state
            .db
            .insert_illustration(&IllustrationInsert {
                artwork_id: id,
                title: format!("i{id}"),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
    }
    let body = illustrations_batch_delete_impl(&state, vec![11, 12, 99], false).unwrap();
    assert_eq!(body["deleted"], json!(2), "不存在的 id 不计入 deleted");
    assert!(state.db.get_illustration(11).is_none());
    assert!(state.db.get_illustration(12).is_none());
    cleanup(&dir);
}

/// saucenao_search 离线冒烟：参数粗校验 → key 检查 → 文件预检都发生在真实
/// 网络请求之前，以下场景全部不触网（解析与错误分类的单测在 saucenao.rs 内）。
#[tokio::test]
async fn saucenao_search_guards_are_offline() {
    let (state, dir) = temp_state("saucenao");

    // key 未配置 → reject 文案含「设置」（前端据此引导用户去设置页）
    let err = saucenao_search_impl(&state, "url", "https://example.com/a.jpg", None)
        .await
        .unwrap_err();
    assert!(err.contains("设置"), "got {err}");

    // 参数粗校验在 key 检查之前：非法来源类型 / 空 source / numres 越界
    for (source_type, source, numres) in [
        ("web", "https://example.com/a.jpg", None),
        ("file", "   ", None),
        ("url", "https://example.com/a.jpg", Some(0)),
        ("url", "https://example.com/a.jpg", Some(41)),
    ] {
        let err = saucenao_search_impl(&state, source_type, source, numres)
            .await
            .unwrap_err();
        assert!(
            err.contains("来源类型") || err.contains("不能为空") || err.contains("numres"),
            "source_type={source_type} source={source:?} numres={numres:?} got {err}"
        );
    }

    // 配置 key 后：不存在的本地文件在请求前被预检拦截（仍不触网）
    state.settings.lock().unwrap().saucenao_api_key = "test-key".into();
    let err = saucenao_search_impl(&state, "file", "Z:/definitely-missing.png", None)
        .await
        .unwrap_err();
    assert!(err.contains("不存在"), "got {err}");
    cleanup(&dir);
}
