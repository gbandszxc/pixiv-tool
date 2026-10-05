//! 写端点在线实测：add → 校验 → delete 往返（默认跳过）。
//!
//! 覆盖：收藏 add/delete（插画 / 小说）、关注 / 取关、评论 add（根评论 + 回复）
//! → delete 往返。
//!
//! 运行门槛（双重保险，防止误改真实数据）：
//! 1. 用例 `#[ignore]`，只在 `./dev.ps1 test-live` 下被收集；
//! 2. 还必须显式设置环境变量 `PIXIV_LIVE_WRITE=1`，否则打印跳过说明后返回。
//!
//! 安全流程（每个用例）：从日榜动态取样「非本人作品且当前未收藏」的候选 →
//! `restrict=1`（私密）add → 详情校验 bookmarkState 出现 → **无论断言成败都先
//! 尝试 delete 还原** → 校验 bookmarkState 消失 → 最后才集中 assert 全部步骤。
//! 断言消息不打印 cookie / csrf token 值。

use serde_json::Value;

use crate::common;

/// 写用例开关：必须显式 `PIXIV_LIVE_WRITE=1`。
fn write_enabled() -> bool {
    matches!(std::env::var("PIXIV_LIVE_WRITE").as_deref(), Ok("1"))
}

/// 仅在双重开关授权后私密关注未关注作者，并无条件尝试恢复。
#[tokio::test]
#[ignore = "需要真实登录态与 PIXIV_LIVE_WRITE=1"]
async fn live_user_follow_roundtrip() {
    if !write_enabled() {
        skip_note("live_user_follow_roundtrip");
        return;
    }
    let api = common::live_api();
    let uid = common::live_uid().await;
    let ranking = api
        .get_ranking("illust", "daily", 1, None)
        .await
        .expect("日榜取样");
    let mut target = None;
    for item in ranking["items"]
        .as_array()
        .expect("榜单条目")
        .iter()
        .take(20)
    {
        let id = item["author_id"].as_i64().unwrap_or(0);
        if id <= 0 || id == uid {
            continue;
        }
        let profile = api.get_user_profile(id).await.expect("作者状态");
        if profile["is_followed"].as_bool() == Some(false) {
            target = Some(id);
            break;
        }
    }
    let id = target.expect("需要未关注且非本人的候选作者");
    let added = api.set_user_follow(id, true, 1).await;
    let after_add = api.get_user_profile(id).await;
    // 断言前清理，避免中途失败留下关注关系。
    let removed = api.set_user_follow(id, false, 0).await;
    let after_remove = api.get_user_profile(id).await;
    assert!(added.is_ok(), "私密关注请求失败");
    assert_eq!(
        after_add.expect("关注后资料")["is_followed"].as_bool(),
        Some(true)
    );
    assert!(removed.is_ok(), "取消关注请求失败，请人工检查关注关系");
    assert_eq!(
        after_remove.expect("取消后资料")["is_followed"].as_bool(),
        Some(false)
    );
}

fn skip_note(name: &str) {
    eprintln!(
        "跳过 {name}：写操作会改动真实收藏；设置 PIXIV_LIVE_WRITE=1 后重跑 ./dev.ps1 test-live"
    );
}

/// 日榜前 20 件里取「非本人作品且未收藏」的候选 id。
async fn pick_unbookmarked(uid: i64, kind: &str) -> i64 {
    let api = common::live_api();
    let ranking = api
        .get_ranking(kind, "daily", 1, None)
        .await
        .unwrap_or_else(|e| panic!("{kind} 日榜请求失败（写用例取样）: {e}"));
    let candidates: Vec<(i64, i64)> =
        common::assert_list_envelope(&ranking, &format!("{kind} 日榜"))
            .iter()
            .take(20)
            .map(|item| (common::id_of(item), common::author_of(item)))
            .collect();
    for (id, author_id) in candidates {
        if author_id == uid {
            continue; // 不能收藏自己的作品
        }
        let detail = match kind {
            "novel" => api.get_work_detail_novel(id).await,
            _ => api.get_work_detail_illust(id).await,
        }
        .unwrap_or_else(|e| panic!("候选详情失败（{kind} {id}）: {e}"));
        // 未收藏 = bookmarkState 字段省略（书签 null/缺失）
        if detail.get("bookmarkState").is_none() {
            return id;
        }
    }
    panic!("{kind} 日榜前 20 件均为本人作品或已收藏，取不到可写候选");
}

/// 插画收藏 add→delete 往返（restrict=1 私密）。
#[tokio::test]
#[ignore = "需要真实登录态与网络，且需 PIXIV_LIVE_WRITE=1：./dev.ps1 test-live"]
async fn live_bookmark_add_remove_illust_roundtrip() {
    if !write_enabled() {
        skip_note("live_bookmark_add_remove_illust_roundtrip");
        return;
    }
    let api = common::live_api();
    let uid = common::live_uid().await;
    let id = pick_unbookmarked(uid, "illust").await;

    // ① add（私密）
    let add = api.bookmark_add("illust", id, 1, &[]).await;
    let bookmark_id = match &add {
        Ok(value) => value
            .get("bookmarkId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        Err(_) => None,
    };

    // ② add 后校验（仅在拿到 bookmarkId 时）
    let after_add = match bookmark_id.as_deref() {
        Some(_) => Some(api.get_work_detail_illust(id).await),
        None => None,
    };

    // ③ 清理：无论上面成败，只要拿到 bookmarkId 就尝试删除还原
    let remove = match bookmark_id.as_deref() {
        Some(bid) => Some(api.bookmark_remove("illust", id, bid).await),
        None => None,
    };

    // ④ delete 后再校验（仅在删除请求成功时）
    let after_remove = match &remove {
        Some(Ok(_)) => Some(api.get_work_detail_illust(id).await),
        _ => None,
    };

    // ---- 全部断言（清理已完成，失败也不会留下脏数据）----
    let added = add.unwrap_or_else(|e| panic!("收藏 add 失败（illust {id}）: {e}"));
    let returned = added["bookmarkId"].as_str().unwrap_or("");
    assert!(
        !returned.is_empty(),
        "add 响应 bookmarkId 不应为空，实际: {added}"
    );
    assert_eq!(
        bookmark_id.as_deref(),
        Some(returned),
        "解析出的 bookmarkId 应与响应一致"
    );

    let detail = after_add
        .expect("未拿到 bookmarkId，未能校验 add 后的收藏态")
        .unwrap_or_else(|e| panic!("add 后详情请求失败（illust {id}）: {e}"));
    let state = detail
        .get("bookmarkState")
        .filter(|v| !v.is_null())
        .unwrap_or_else(|| panic!("add 后详情应出现 bookmarkState（illust {id}）"));
    assert_eq!(
        state["bookmarkId"].as_str(),
        Some(returned),
        "详情 bookmarkId 应与 add 返回一致"
    );
    assert_eq!(
        state["restrict"].as_i64(),
        Some(1),
        "restrict=1 应标记为非公开"
    );

    remove
        .expect("未尝试删除（bookmarkId 缺失）")
        .unwrap_or_else(|e| panic!("删除还原失败（illust {id} / bookmark {returned}）: {e}"));
    let restored = after_remove
        .expect("删除后详情请求失败")
        .unwrap_or_else(|e| panic!("删除后详情解析失败（illust {id}）: {e}"));
    assert!(
        restored.get("bookmarkState").is_none(),
        "删除后 bookmarkState 应消失（收藏已还原）"
    );
}

/// 小说收藏 add→delete 往返（restrict=1 私密；删除走旧式表单端点）。
#[tokio::test]
#[ignore = "需要真实登录态与网络，且需 PIXIV_LIVE_WRITE=1：./dev.ps1 test-live"]
async fn live_bookmark_add_remove_novel_roundtrip() {
    if !write_enabled() {
        skip_note("live_bookmark_add_remove_novel_roundtrip");
        return;
    }
    let api = common::live_api();
    let uid = common::live_uid().await;
    let id = pick_unbookmarked(uid, "novel").await;

    // ① add（私密）
    let add = api.bookmark_add("novel", id, 1, &[]).await;
    let bookmark_id = match &add {
        Ok(value) => value
            .get("bookmarkId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        Err(_) => None,
    };

    // ② add 后校验
    let after_add = match bookmark_id.as_deref() {
        Some(_) => Some(api.get_work_detail_novel(id).await),
        None => None,
    };

    // ③ 清理：小说删除走 /novel/bookmark_setting.php（旧式表单，tt=csrf token）
    let remove = match bookmark_id.as_deref() {
        Some(bid) => Some(api.bookmark_remove("novel", id, bid).await),
        None => None,
    };

    // ④ delete 后再校验
    let after_remove = match &remove {
        Some(Ok(_)) => Some(api.get_work_detail_novel(id).await),
        _ => None,
    };

    // ---- 全部断言（清理已完成）----
    let added = add.unwrap_or_else(|e| panic!("收藏 add 失败（novel {id}）: {e}"));
    let returned = added["bookmarkId"].as_str().unwrap_or("");
    assert!(
        !returned.is_empty(),
        "add 响应 bookmarkId 不应为空，实际: {added}"
    );
    assert_eq!(
        bookmark_id.as_deref(),
        Some(returned),
        "解析出的 bookmarkId 应与响应一致"
    );

    let detail = after_add
        .expect("未拿到 bookmarkId，未能校验 add 后的收藏态")
        .unwrap_or_else(|e| panic!("add 后详情请求失败（novel {id}）: {e}"));
    let state = detail
        .get("bookmarkState")
        .filter(|v| !v.is_null())
        .unwrap_or_else(|| panic!("add 后详情应出现 bookmarkState（novel {id}）"));
    assert_eq!(
        state["bookmarkId"].as_str(),
        Some(returned),
        "详情 bookmarkId 应与 add 返回一致（小说 id 可能为数字，解析已统一 String 化）"
    );
    assert_eq!(
        state["restrict"].as_i64(),
        Some(1),
        "restrict=1 应标记为非公开"
    );

    remove
        .expect("未尝试删除（bookmarkId 缺失）")
        .unwrap_or_else(|e| panic!("删除还原失败（novel {id} / bookmark {returned}）: {e}"));
    let restored = after_remove
        .expect("删除后详情请求失败")
        .unwrap_or_else(|e| panic!("删除后详情解析失败（novel {id}）: {e}"));
    assert!(
        restored.get("bookmarkState").is_none(),
        "删除后 bookmarkState 应消失（收藏已还原）"
    );
}

/// 本人作品列表首件插画的 id（评论写用例只在本人作品上留痕并清理）。
async fn pick_own_illust(uid: i64) -> i64 {
    let api = common::live_api();
    let works = api
        .get_user_works(uid, "illust", 1)
        .await
        .unwrap_or_else(|e| panic!("本人作品列表请求失败（评论写用例取样）: {e}"));
    common::assert_list_envelope(&works, "本人插画")
        .first()
        .map(common::id_of)
        .filter(|id| *id > 0)
        .unwrap_or_else(|| panic!("本人作品列表为空，无法执行评论写用例（uid {uid}）"))
}

/// 在线用例使用的官方表情贴图 id（网页面板可见目录中的一个，见前端
/// `PIXIV_COMMENT_STAMPS`）。
const STAMP_ID: &str = "301";

/// 评论列表信封里的 id 集合（契约 `{comments:[{id,…}], next?}`）。
fn comment_ids(listing: &Value) -> Vec<String> {
    listing["comments"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|c| c["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 评论写路径往返：在**本人作品**上发根评论 → 回复该评论 → 发官方表情贴图 →
/// 校验可见 → 无论成败都删除还原（先删贴图与回复再删根评论），最后校验列表已消失。
#[tokio::test]
#[ignore = "需要真实登录态与网络，且需 PIXIV_LIVE_WRITE=1：./dev.ps1 test-live"]
async fn live_comment_add_and_delete_roundtrip() {
    if !write_enabled() {
        skip_note("live_comment_add_and_delete_roundtrip");
        return;
    }
    let api = common::live_api();
    let uid = common::live_uid().await;
    let work_id = pick_own_illust(uid).await;

    // ① 根评论
    let root = api
        .post_comment(
            "illust",
            work_id,
            uid,
            "接口联调测试评论（随后自动删除）",
            None,
        )
        .await;
    let root_id = root
        .as_ref()
        .ok()
        .and_then(|v| v.get("comment_id").and_then(Value::as_str))
        .map(str::to_string);

    // ② 回复该根评论（parent_id 分支）
    let reply = match root_id.as_deref() {
        Some(rid) => Some(
            api.post_comment(
                "illust",
                work_id,
                uid,
                "接口联调测试回复（随后自动删除）",
                Some(rid),
            )
            .await,
        ),
        None => None,
    };
    let reply_id = reply
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|v| v.get("comment_id").and_then(Value::as_str))
        .map(str::to_string);

    // ②b 表情贴图（stamp 分支：type=stamp&stamp_id，不带 comment）
    let stamped = match root_id.as_deref() {
        Some(rid) => Some(
            api.post_stamp_comment("illust", work_id, uid, STAMP_ID, Some(rid))
                .await,
        ),
        None => None,
    };
    let stamped_id = stamped
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|v| v.get("comment_id").and_then(Value::as_str))
        .map(str::to_string);

    // ③ 删除前可见性取样（用于证明删除确有状态变化）
    let before = match root_id.as_deref() {
        Some(_) => Some(api.get_work_comments("illust", work_id, 0).await),
        None => None,
    };

    // ④ 清理：先删回复与贴图（挂根评论之下），再删根评论
    let del_stamp = match stamped_id.as_deref() {
        Some(sid) => Some(api.comment_delete("illust", work_id, sid).await),
        None => None,
    };
    let del_reply = match reply_id.as_deref() {
        Some(rid) => Some(api.comment_delete("illust", work_id, rid).await),
        None => None,
    };
    let del_root = match root_id.as_deref() {
        Some(rid) => Some(api.comment_delete("illust", work_id, rid).await),
        None => None,
    };

    // ⑤ 删除后再取样
    let after = match &del_root {
        Some(Ok(_)) => Some(api.get_work_comments("illust", work_id, 0).await),
        _ => None,
    };

    // ---- 全部断言（清理已完成，失败也不留下评论）----
    let root_value = root.unwrap_or_else(|e| panic!("根评论发布失败（illust {work_id}）: {e}"));
    let root_returned = root_value["comment_id"].as_str().unwrap_or("");
    assert!(
        !root_returned.is_empty(),
        "根评论响应 comment_id 不应为空，实际: {root_value}"
    );
    assert_eq!(
        root_value.get("parent_id"),
        None,
        "根评论响应不应带 parent_id（实测 null → 契约省略）"
    );
    assert!(
        !root_value["user_name"].as_str().unwrap_or("").is_empty(),
        "根评论响应应含 user_name，实际: {root_value}"
    );

    let reply_value = reply
        .expect("未拿到根评论 id，未能验证回复分支")
        .unwrap_or_else(|e| panic!("回复发布失败（illust {work_id}）: {e}"));
    assert_eq!(
        reply_value["parent_id"].as_str(),
        Some(root_returned),
        "回复响应 parent_id 应指向被回复的根评论"
    );

    let stamp_value = stamped
        .expect("未拿到根评论 id，未能验证贴图分支")
        .unwrap_or_else(|e| panic!("表情贴图发布失败（illust {work_id}）: {e}"));
    assert_eq!(
        stamp_value["stamp_id"].as_str(),
        Some(STAMP_ID),
        "贴图响应 stamp_id 应与请求一致"
    );
    assert_eq!(
        stamp_value["parent_id"].as_str(),
        Some(root_returned),
        "贴图回复的 parent_id 应指向被回复的根评论"
    );

    let visible = comment_ids(
        &before
            .expect("未取样删除前评论列表")
            .unwrap_or_else(|e| panic!("删除前评论列表请求失败（illust {work_id}）: {e}")),
    );
    assert!(
        visible.contains(&root_returned.to_string()),
        "删除前该评论应出现在列表首页（{visible:?}）"
    );

    del_stamp
        .expect("未尝试删除表情贴图")
        .unwrap_or_else(|e| panic!("表情贴图删除失败（评论 {stamped_id:?}）: {e}"));
    del_reply
        .expect("未尝试删除回复")
        .unwrap_or_else(|e| panic!("回复删除失败（评论 {reply_id:?}）: {e}"));
    del_root
        .expect("未尝试删除根评论")
        .unwrap_or_else(|e| panic!("根评论删除失败（评论 {root_returned}）: {e}"));

    let remaining = comment_ids(
        &after
            .expect("删除后评论列表请求失败")
            .unwrap_or_else(|e| panic!("删除后评论列表解析失败（illust {work_id}）: {e}")),
    );
    assert!(
        !remaining.contains(&root_returned.to_string()),
        "删除后该评论不应仍出现在列表（{remaining:?}）"
    );
}
