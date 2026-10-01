//! browse 命令层（commands/browse_api_cmds.rs）离线冒烟测试。
//!
//! 不经 GUI / Tauri 运行时，直接调用 `*_impl`（temp_state 模式复制自
//! tests/smoke_commands.rs，避免跨测试文件共享辅助函数）：
//! 1. 未登录（隔离空 cookie store）时全部 13 个命令以合法参数调用，
//!    统一被登录守卫拦截，文案逐字等于 `NOT_LOGGED_IN`（含「登录」关键字，
//!    前端据此弹登录窗）——同时即「合法参数 → 登录守卫」验证；
//! 2. 非法参数在登录守卫**之前**被粗校验拒绝，返回参数专属中文错误
//!    （在未登录状态下拿到参数错误，本身就是顺序的证明）；
//! 3. 非数字 id 说明：id 形参为 i64（契约 id:number，前端 F0 传 number），
//!    非数字值在 Tauri IPC 反序列化层即被拒、到不了 *_impl，离线不可测；
//!    此处验证数字域边界（0/负数）同样给出可读中文错误。
//!
//! 未登录态隔离：`AppState.cookies` 换成 `CookieStore::with_account(uuid)`
//! 独立条目——首次 load 落一次 keyring 读取（NoEntry → None → 未登录），
//! 绝不读写真实 `default` 凭据条目（Windows 无授权弹窗）。

use pixiv_tool_lib::commands::browse_api_cmds::{
    NOT_LOGGED_IN, browse_bookmark_add_impl, browse_bookmark_list_impl, browse_bookmark_remove_impl,
    browse_bookmark_tags_impl, browse_channel_impl, browse_comment_replies_impl,
    browse_discover_impl, browse_follow_latest_impl, browse_home_feed_impl,
    browse_novel_series_impl, browse_ranking_impl, browse_related_impl, browse_search_impl,
    browse_user_profile_impl, browse_user_works_impl, browse_watchlist_impl, browse_work_comments_impl,
    browse_work_detail_impl, build_browse_api,
};
use pixiv_tool_lib::cookies::CookieStore;
use pixiv_tool_lib::db::Db;
use pixiv_tool_lib::paths::AppPaths;
use pixiv_tool_lib::settings::Settings;
use pixiv_tool_lib::state::AppState;

/// 临时目录里的完整 AppState（与 smoke_commands::temp_state 同构），
/// 额外把 cookies 换成 uuid 隔离条目，保证「未登录」且不碰真实凭据。
fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "pixiv-tool-browse-smoke-{tag}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Db::open(&dir.join("app.db")).unwrap();
    let paths = AppPaths {
        data_dir: dir.clone(),
        config_dir: dir.join("config"),
        logs_dir: dir.join("logs"),
    };
    let mut state = AppState::new(paths, Settings::default(), db);
    state.cookies = CookieStore::with_account(&format!("smoke-browse-{}", uuid::Uuid::new_v4()));
    (state, dir)
}

fn cleanup(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// ①+③：未登录下全部 13 个命令（合法参数）统一被登录守卫拦截，文案逐字一致。
#[tokio::test]
async fn not_logged_in_blocks_all_commands_with_login_error() {
    let (state, dir) = temp_state("nologin");
    assert_eq!(
        browse_home_feed_impl(&state).await.unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_channel_impl(&state, "illust").await.unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_watchlist_impl(&state, "manga").await.unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_discover_impl(&state).await.unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_follow_latest_impl(&state, "novel", "r18", 2)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_search_impl(&state, "novel", "オリジナル", None, None, None, None, 1)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_ranking_impl(&state, "ugoira", "weekly", 1, None)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_work_detail_impl(&state, "novel", 9000012)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_related_impl(&state, "manga", 9000021, Some(10))
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_user_profile_impl(&state, 28640).await.unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_user_works_impl(&state, 28640, "manga", 1)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_novel_series_impl(&state, 1093870, Some(30))
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_work_comments_impl(&state, "manga", 9000021, 10)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_comment_replies_impl(&state, "novel", "900000001", 1)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    // 收藏（契约 v3.1）：4 个命令合法参数 → 统一登录守卫拦截
    assert_eq!(
        browse_bookmark_list_impl(&state, "illust", "show", None, 0, None)
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_bookmark_list_impl(&state, "novel", "hide", Some("風景"), 48, Some(30))
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_bookmark_tags_impl(&state, "novel")
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_bookmark_add_impl(&state, "illust", 9000021, 1, &["風景".to_string()])
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_bookmark_add_impl(&state, "novel", 9000012, 0, &[])
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    assert_eq!(
        browse_bookmark_remove_impl(&state, "novel", 9000012, "3100000001")
            .await
            .unwrap_err(),
        NOT_LOGGED_IN
    );
    // 登录守卫本体同文案（err() 规避 PixivApi 无 Debug）
    assert_eq!(
        build_browse_api(&state)
            .err()
            .expect("未登录应被登录守卫拦截"),
        NOT_LOGGED_IN
    );
    cleanup(&dir);
}

/// ②：非法参数在登录守卫之前被拒（未登录状态下拿到的是参数错误而非登录错误）。
#[tokio::test]
async fn invalid_params_rejected_before_login_guard() {
    let (state, dir) = temp_state("badparams");

    // 空 word（含全空白）
    assert_eq!(
        browse_search_impl(&state, "illust", "", None, None, None, None, 1)
            .await
            .unwrap_err(),
        "搜索词不能为空"
    );
    assert_eq!(
        browse_search_impl(&state, "novel", "   ", None, None, None, None, 1)
            .await
            .unwrap_err(),
        "搜索词不能为空"
    );

    // 非法 kind
    assert_eq!(
        browse_channel_impl(&state, "video").await.unwrap_err(),
        "不支持的频道类型: video"
    );
    assert_eq!(
        browse_watchlist_impl(&state, "illust").await.unwrap_err(),
        "不支持的追更类型: illust"
    );
    assert_eq!(
        browse_search_impl(&state, "video", "w", None, None, None, None, 1)
            .await
            .unwrap_err(),
        "不支持的搜索类型: video"
    );
    assert_eq!(
        browse_work_detail_impl(&state, "video", 1)
            .await
            .unwrap_err(),
        "不支持的作品类型: video"
    );
    assert_eq!(
        browse_related_impl(&state, "video", 1, None)
            .await
            .unwrap_err(),
        "不支持的作品类型: video"
    );
    assert_eq!(
        browse_user_works_impl(&state, 1, "ugoira", 1)
            .await
            .unwrap_err(),
        "不支持的作品类型: ugoira"
    );
    assert_eq!(
        browse_follow_latest_impl(&state, "manga", "all", 1)
            .await
            .unwrap_err(),
        "不支持的类型: manga（仅 illust|novel）"
    );

    // 非法 mode
    assert_eq!(
        browse_follow_latest_impl(&state, "illust", "safe_x", 1)
            .await
            .unwrap_err(),
        "不支持的过滤模式: safe_x"
    );
    assert_eq!(
        browse_search_impl(&state, "illust", "w", None, Some("strict"), None, None, 1)
            .await
            .unwrap_err(),
        "不支持的过滤模式: strict"
    );
    // 排行榜 mode 按 kind 区分白名单（male 仅 novel，rookie 仅作品）
    assert_eq!(
        browse_ranking_impl(&state, "illust", "male", 1, None)
            .await
            .unwrap_err(),
        "不支持的排行榜模式: male"
    );
    assert_eq!(
        browse_ranking_impl(&state, "novel", "rookie", 1, None)
            .await
            .unwrap_err(),
        "不支持的排行榜模式: rookie"
    );
    assert_eq!(
        browse_ranking_impl(&state, "video", "daily", 1, None)
            .await
            .unwrap_err(),
        "不支持的排行榜类型: video"
    );

    // date 非 yyyymmdd
    for bad in ["2026-09-29", "2026093", "2026093a", "abcdefgh"] {
        assert_eq!(
            browse_ranking_impl(&state, "illust", "daily", 1, Some(bad))
                .await
                .unwrap_err(),
            format!("date 格式应为 yyyymmdd: {bad}"),
            "date={bad}"
        );
    }

    // page < 1
    assert_eq!(
        browse_follow_latest_impl(&state, "illust", "all", 0)
            .await
            .unwrap_err(),
        "页码必须从 1 开始"
    );
    assert_eq!(
        browse_search_impl(&state, "illust", "w", None, None, None, None, -1)
            .await
            .unwrap_err(),
        "页码必须从 1 开始"
    );
    assert_eq!(
        browse_ranking_impl(&state, "illust", "daily", 0, None)
            .await
            .unwrap_err(),
        "页码必须从 1 开始"
    );
    assert_eq!(
        browse_user_works_impl(&state, 1, "illust", 0)
            .await
            .unwrap_err(),
        "页码必须从 1 开始"
    );

    // id 数字域越界（非数字 id 在 IPC 反序列化层即被拒，见文件头说明）
    assert_eq!(
        browse_work_detail_impl(&state, "illust", 0)
            .await
            .unwrap_err(),
        "作品 ID 必须为正整数"
    );
    assert_eq!(
        browse_work_detail_impl(&state, "manga", -5)
            .await
            .unwrap_err(),
        "作品 ID 必须为正整数"
    );
    assert_eq!(
        browse_related_impl(&state, "illust", 0, None)
            .await
            .unwrap_err(),
        "作品 ID 必须为正整数"
    );
    assert_eq!(
        browse_user_profile_impl(&state, 0).await.unwrap_err(),
        "用户 ID 必须为正整数"
    );
    assert_eq!(
        browse_user_works_impl(&state, 0, "illust", 1)
            .await
            .unwrap_err(),
        "用户 ID 必须为正整数"
    );
    assert_eq!(
        browse_novel_series_impl(&state, 0, None).await.unwrap_err(),
        "系列 ID 必须为正整数"
    );

    // limit 越界（1~30）与 last_order 负数
    assert_eq!(
        browse_related_impl(&state, "illust", 1, Some(0))
            .await
            .unwrap_err(),
        "limit 必须在 1~30 之间"
    );
    assert_eq!(
        browse_related_impl(&state, "illust", 1, Some(31))
            .await
            .unwrap_err(),
        "limit 必须在 1~30 之间"
    );
    assert_eq!(
        browse_novel_series_impl(&state, 1, Some(-1))
            .await
            .unwrap_err(),
        "last_order 不能为负数"
    );

    // 评论：kind 白名单（manga 复用 illusts 端点，ugoira 不支持）
    assert_eq!(
        browse_work_comments_impl(&state, "ugoira", 1, 0)
            .await
            .unwrap_err(),
        "不支持的作品类型: ugoira"
    );
    assert_eq!(
        browse_comment_replies_impl(&state, "video", "1", 1)
            .await
            .unwrap_err(),
        "不支持的作品类型: video"
    );

    // 评论：offset 负数 / 作品 id 越界 / 回复 page<1
    assert_eq!(
        browse_work_comments_impl(&state, "illust", 1, -1)
            .await
            .unwrap_err(),
        "offset 不能为负数"
    );
    assert_eq!(
        browse_work_comments_impl(&state, "illust", 0, 0)
            .await
            .unwrap_err(),
        "作品 ID 必须为正整数"
    );
    assert_eq!(
        browse_comment_replies_impl(&state, "illust", "1", 0)
            .await
            .unwrap_err(),
        "页码必须从 1 开始"
    );

    // 评论：comment_id 非空（含全空白）
    assert_eq!(
        browse_comment_replies_impl(&state, "illust", "", 1)
            .await
            .unwrap_err(),
        "评论 ID 不能为空"
    );
    assert_eq!(
        browse_comment_replies_impl(&state, "novel", "   ", 1)
            .await
            .unwrap_err(),
        "评论 ID 不能为空"
    );

    // 收藏（契约 v3.1）：非法参数在登录守卫之前被拒
    // kind 白名单（契约只分 illust|novel；manga/ugoira 收藏由前端传 "illust"）
    assert_eq!(
        browse_bookmark_list_impl(&state, "manga", "show", None, 0, None)
            .await
            .unwrap_err(),
        "不支持的收藏类型: manga"
    );
    assert_eq!(
        browse_bookmark_tags_impl(&state, "ugoira")
            .await
            .unwrap_err(),
        "不支持的收藏类型: ugoira"
    );
    assert_eq!(
        browse_bookmark_add_impl(&state, "video", 1, 0, &[])
            .await
            .unwrap_err(),
        "不支持的收藏类型: video"
    );
    assert_eq!(
        browse_bookmark_remove_impl(&state, "", 1, "1")
            .await
            .unwrap_err(),
        "不支持的收藏类型: "
    );
    // rest 白名单
    assert_eq!(
        browse_bookmark_list_impl(&state, "illust", "all", None, 0, None)
            .await
            .unwrap_err(),
        "不支持的可见范围: all"
    );
    // offset 负数
    assert_eq!(
        browse_bookmark_list_impl(&state, "illust", "show", None, -1, None)
            .await
            .unwrap_err(),
        "offset 不能为负数"
    );
    // limit 区间（1~100）
    assert_eq!(
        browse_bookmark_list_impl(&state, "illust", "show", None, 0, Some(0))
            .await
            .unwrap_err(),
        "limit 必须在 1~100 之间"
    );
    assert_eq!(
        browse_bookmark_list_impl(&state, "novel", "show", None, 0, Some(101))
            .await
            .unwrap_err(),
        "limit 必须在 1~100 之间"
    );
    // restrict 0|1
    assert_eq!(
        browse_bookmark_add_impl(&state, "illust", 1, 2, &[])
            .await
            .unwrap_err(),
        "restrict 只能是 0（公开）或 1（非公开）: 2"
    );
    assert_eq!(
        browse_bookmark_add_impl(&state, "illust", 1, -1, &[])
            .await
            .unwrap_err(),
        "restrict 只能是 0（公开）或 1（非公开）: -1"
    );
    // id 数字域越界
    assert_eq!(
        browse_bookmark_add_impl(&state, "illust", 0, 0, &[])
            .await
            .unwrap_err(),
        "作品 ID 必须为正整数"
    );
    assert_eq!(
        browse_bookmark_remove_impl(&state, "novel", -1, "1")
            .await
            .unwrap_err(),
        "作品 ID 必须为正整数"
    );
    // bookmark_id 非空（含全空白）
    assert_eq!(
        browse_bookmark_remove_impl(&state, "illust", 1, "")
            .await
            .unwrap_err(),
        "收藏 ID 不能为空"
    );
    assert_eq!(
        browse_bookmark_remove_impl(&state, "novel", 1, "   ")
            .await
            .unwrap_err(),
        "收藏 ID 不能为空"
    );

    cleanup(&dir);
}
