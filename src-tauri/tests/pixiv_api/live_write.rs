//! 写端点在线实测：add → 校验 → delete 往返（默认跳过）。
//!
//! 运行门槛（双重保险，防止误改真实收藏）：
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
    let candidates: Vec<(i64, i64)> = common::assert_list_envelope(&ranking, &format!("{kind} 日榜"))
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
