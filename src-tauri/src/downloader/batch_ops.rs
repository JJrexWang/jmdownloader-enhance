//! 批量下载 / 更新库存 这类「跑全收藏夹 / 跑全本地」的长任务。
//!
//! 从 `commands.rs` 抽出来：原来它们是 `#[tauri::command]` 包装，
//! 跟 `AppHandle` 紧耦合；现在变成 `pub async fn xxx(ctx: &dyn AppContext)`，
//! HTTP 服务端也能复用同一份业务逻辑。

use std::sync::Arc;
use std::time::Duration;

use eyre::WrapErr;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::Instrument;

use crate::downloader::download_manager::DownloadManager;
use crate::events::{
    dispatch_event, DownloadAllFavoritesEvent, UpdateDownloadedComicsEvent,
};
use crate::extensions::EyreReportToMessage;
use crate::service::AppContext;
use crate::types::{ChapterInfo, FavoriteSort};
use crate::utils;

/// 拉取整个收藏夹（分页并发），并为每本未下载的漫画创建下载任务。
///
/// 跟桌面端 `download_all_favorites` 命令等价，但接收 `&dyn AppContext`
/// 而不是 `AppHandle`，方便 HTTP 服务端复用。
#[allow(clippy::cast_possible_wrap)]
#[tracing::instrument(level = "error", skip_all)]
pub async fn download_all_favorites(ctx: &dyn AppContext) -> eyre::Result<()> {
    let config = ctx.config();
    let jm_client = ctx.jm_client().clone();
    let download_manager = ctx.download_manager();

    let mut favorite_comics = Vec::new();
    let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::GetFavoritesStart);

    let first_page = jm_client
        .get_favorite_folder(0, 1, FavoriteSort::FavoriteTime)
        .await
        .wrap_err("获取收藏夹第一页失败")?;
    favorite_comics.extend(first_page.list);

    let count = first_page.count;
    let total = first_page
        .total
        .parse::<i64>()
        .wrap_err("将收藏夹总数解析为 i64 失败")?;
    let page_count = (total / count) + 1;

    let sem = Arc::new(Semaphore::new(5));
    let mut join_set = JoinSet::new();
    for page in 2..=page_count {
        let jm_client = jm_client.clone();
        let sem = sem.clone();
        let get_favorite_task = async move {
            let _permit = sem.acquire().await?;
            let page = jm_client
                .get_favorite_folder(0, page, FavoriteSort::FavoriteTime)
                .await?;
            Ok::<_, eyre::Report>(page)
        };
        join_set.spawn(get_favorite_task.in_current_span());
    }

    while let Some(get_favorite_result) = join_set.join_next().await {
        let page = get_favorite_result
            .wrap_err("获取收藏夹页面的 join 失败")?
            .wrap_err("获取收藏夹失败")?;
        favorite_comics.extend(page.list);
    }

    let total = favorite_comics.len() as i64;
    let interval_sec = config.download_all_favorites_interval_sec;

    for (i, favorite_comic) in favorite_comics.into_iter().enumerate() {
        let comic_title = &favorite_comic.name;
        let comic_id = match favorite_comic.id.parse::<i64>() {
            Ok(id) => id,
            Err(err) => {
                let err_title = format!("下载收藏夹过程中，获取漫画`{comic_title}`失败，已跳过");
                tracing::error!(err_title, message = eyre::Report::from(err).to_message());
                let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::FailedComic {
                    comic_id: None,
                    comic_title: comic_title.clone(),
                });
                sleep(Duration::from_secs(interval_sec)).await;
                continue;
            }
        };

        let comic = match utils::get_comic(ctx, comic_id).await {
            Ok(comic) => comic,
            Err(err) => {
                let err_title = format!("下载收藏夹过程中，获取漫画`{comic_title}`失败，已跳过");
                tracing::error!(err_title, message = eyre::Report::from(err).to_message());
                let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::FailedComic {
                    comic_id: Some(comic_id),
                    comic_title: comic_title.clone(),
                });
                sleep(Duration::from_secs(interval_sec)).await;
                continue;
            }
        };

        let current = (i + 1) as i64;
        let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::GetComicsProgress {
            current,
            total,
            current_comic_title: comic.name.clone(),
        });

        let chapter_infos: Vec<&ChapterInfo> = comic
            .chapter_infos
            .iter()
            .filter(|chapter_info| chapter_info.is_downloaded != Some(true))
            .collect();

        if chapter_infos.is_empty() {
            sleep(Duration::from_secs(interval_sec)).await;
            continue;
        }

        let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::StartCreateDownloadTasks {
            comic_id: comic.id,
            comic_title: comic.name.clone(),
            current: 0,
            total: chapter_infos.len() as i64,
        });

        for (idx, chapter_info) in chapter_infos.into_iter().enumerate() {
            let current = idx as i64 + 1;
            let _ = download_manager.create_download_task(comic.clone(), chapter_info.chapter_id);

            let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::CreatingDownloadTask {
                comic_id: comic.id,
                current,
            });

            sleep(Duration::from_millis(100)).await;
        }

        let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::EndCreateDownloadTasks {
            comic_id: comic.id,
        });

        sleep(Duration::from_secs(interval_sec)).await;
    }

    let _ = dispatch_event(ctx, DownloadAllFavoritesEvent::GetComicsEnd);
    Ok(())
}

/// 走一遍本地已下载漫画，给每本「还有未下载章节」的漫画创建下载任务。
///
/// 跟桌面端 `update_downloaded_comics` 命令等价。
#[allow(clippy::cast_possible_wrap)]
#[tracing::instrument(level = "error", skip_all)]
pub async fn update_downloaded_comics(ctx: &dyn AppContext) -> eyre::Result<()> {
    let config = ctx.config();
    let download_manager = ctx.download_manager();

    let downloaded_comics = get_downloaded_comics(ctx);
    let total = downloaded_comics.len() as i64;
    let interval_sec = config.update_downloaded_comics_interval_sec;
    let _ = dispatch_event(ctx, UpdateDownloadedComicsEvent::GetComicStart { total });

    let id_to_dir_map = ctx
        .downloaded_comics_index()
        .get_or_build(ctx)
        .wrap_err("构建已下载漫画索引失败")?;

    for (i, downloaded_comic) in downloaded_comics.into_iter().enumerate() {
        let comic_title = downloaded_comic.name.clone();
        let comic_id = downloaded_comic.id;
        let current = (i + 1) as i64;
        let _ = dispatch_event(ctx, UpdateDownloadedComicsEvent::GetComicProgress {
            current,
            total,
            current_comic_title: comic_title.clone(),
        });

        let comic = match utils::get_comic_with_map(ctx, comic_id, Arc::clone(&id_to_dir_map)).await {
            Ok(comic) => comic,
            Err(err) => {
                let err_title = format!("更新库存过程中，获取漫画`{comic_title}`失败，已跳过");
                tracing::error!(err_title, message = eyre::Report::from(err).to_message());
                let _ = dispatch_event(ctx, UpdateDownloadedComicsEvent::FailedComic {
                    comic_id,
                    comic_title: comic_title.clone(),
                });
                sleep(Duration::from_secs(interval_sec)).await;
                continue;
            }
        };

        let has_downloaded_chapter = comic
            .chapter_infos
            .iter()
            .any(|chapter_info| chapter_info.is_downloaded == Some(true));
        if !has_downloaded_chapter {
            sleep(Duration::from_secs(interval_sec)).await;
            continue;
        }

        let chapter_infos: Vec<&ChapterInfo> = comic
            .chapter_infos
            .iter()
            .filter(|chapter| chapter.is_downloaded != Some(true))
            .collect();

        if chapter_infos.is_empty() {
            sleep(Duration::from_secs(interval_sec)).await;
            continue;
        }

        let _ = dispatch_event(ctx, UpdateDownloadedComicsEvent::CreateDownloadTasksStart {
            comic_id: comic.id,
            comic_title: comic.name.clone(),
            current: 0,
            total: chapter_infos.len() as i64,
        });

        for (idx, chapter_info) in chapter_infos.into_iter().enumerate() {
            let chapter_id = chapter_info.chapter_id;
            let current = idx as i64 + 1;

            let _ = download_manager.create_download_task(comic.clone(), chapter_id);

            let _ = dispatch_event(ctx, UpdateDownloadedComicsEvent::CreateDownloadTaskProgress {
                comic_id: comic.id,
                current,
            });

            sleep(Duration::from_millis(100)).await;
        }

        let _ = dispatch_event(ctx, UpdateDownloadedComicsEvent::CreateDownloadTasksEnd {
            comic_id: comic.id,
        });

        sleep(Duration::from_secs(interval_sec)).await;
    }

    let _ = dispatch_event(ctx, UpdateDownloadedComicsEvent::GetComicEnd);
    Ok(())
}

/// 从本地下载目录里 walk 出所有已下载的漫画。
fn get_downloaded_comics(ctx: &dyn AppContext) -> Vec<crate::types::Comic> {
    let config = ctx.config();
    let download_dir = config.download_dir.clone();
    let dir_fmt = config.dir_fmt.clone();
    let mode = config.chinese_normalization;
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&download_dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // 期望每个目录里有一个「漫画元数据.json」；没有就跳过
        let metadata_path = path.join("漫画元数据.json");
        if !metadata_path.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&metadata_path) else {
            continue;
        };
        let Ok(comic) = serde_json::from_str::<crate::types::Comic>(&content) else {
            continue;
        };
        let mut comic = comic;
        if let Ok(map) = ctx.downloaded_comics_index().get_or_build(ctx) {
            let _ = comic.update_fields(&map, &dir_fmt, mode);
        }
        out.push(comic);
    }
    out
}

// 让 DownloadManager 类型被引用（防止 lint 报警）；同时未来可以从这里继续
// 给 DownloadManager 加 batch 辅助方法。
#[allow(dead_code)]
fn _type_marker(_: &DownloadManager) {}
