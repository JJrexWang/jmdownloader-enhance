use std::sync::Arc;
use std::time::Duration;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

// TODO: 用`#![allow(clippy::used_underscore_binding)]`来消除警告
use eyre::{eyre, WrapErr};
use indexmap::IndexMap;
use tauri::AppHandle;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{instrument, Instrument};
use walkdir::WalkDir;

use crate::config::Config;
use crate::errors::{CommandError, CommandResult};
use crate::events::{
    dispatch_event, DownloadAllFavoritesEvent, UpdateDownloadedComicsEvent,
};
use crate::extensions::{EyreReportToMessage, WalkDirEntryExt};
use crate::service::AppContext;
use crate::responses::{GetUserProfileRespData, GetWeeklyInfoRespData};
use crate::types::{
    ChapterInfo, Comic, ComicInFavorite, ComicInSearch, ComicInWeekly, FavoriteSort,
    GetFavoriteResult, GetWeeklyResult, LogMetadata, SearchResultVariant, SearchSort,
};
use crate::{export, logger, utils};

#[tauri::command]
#[specta::specta]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
#[instrument(level = "error", skip_all)]
pub fn get_config(app: AppHandle) -> Config {
    let ctx: &dyn AppContext = &app;
    ctx.config().clone()

}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
#[instrument(level = "error", skip_all)]
pub fn save_config(app: AppHandle, config: Config) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();

    // ctx.config() 返回 guard，先读完再用，用完 drop，然后才能再 config_mut()
    let proxy_changed;
    let file_logger_changed;
    let enable_file_logger = config.enable_file_logger;
    {
        let current = ctx.config();
        proxy_changed = current.proxy_mode != config.proxy_mode
            || current.proxy_host != config.proxy_host
            || current.proxy_port != config.proxy_port;
        file_logger_changed = current.enable_file_logger != enable_file_logger;
    }

    {
        let mut config_state = ctx.config_mut();
        *config_state = config;
        config_state
            .save(ctx.paths().data_dir.as_path())
            .map_err(|err| CommandError::from("保存配置失败", err))?;
        tracing::debug!("保存配置成功");
    }

    if proxy_changed {
        jm_client.reload_client();
    }

    if file_logger_changed {
        if enable_file_logger {
            logger::reload_file_logger()
                .map_err(|err| CommandError::from("重新加载文件日志失败", err))?;
        } else {
            logger::disable_file_logger()
                .map_err(|err| CommandError::from("禁用文件日志失败", err))?;
        }
    }

    Ok(())

}

#[tauri::command]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub async fn login(
    app: AppHandle,
    username: String,
    password: String,
) -> CommandResult<GetUserProfileRespData> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();

    let user_profile = jm_client
        .login(&username, &password)
        .await
        .map_err(|err| CommandError::from("登录失败", err))?;

    Ok(user_profile)

}

#[tauri::command]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub async fn get_user_profile(app: AppHandle) -> CommandResult<GetUserProfileRespData> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();

    let user_profile = jm_client
        .get_user_profile()
        .await
        .map_err(|err| CommandError::from("获取用户信息失败", err))?;

    Ok(user_profile)

}

#[tauri::command]
#[specta::specta]
#[instrument(
    level = "error",
    skip_all,
    fields(keyword = keyword, page = page, sort = ?sort)
)]
pub async fn search(
    app: AppHandle,
    keyword: String,
    page: i64,
    sort: SearchSort,
) -> CommandResult<SearchResultVariant> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();

    let search_resp = jm_client
        .search(&keyword, page, sort)
        .await
        .map_err(|err| CommandError::from("搜索失败", err))?;

    let search_result = SearchResultVariant::from_search_resp(ctx, search_resp)
        .map_err(|err| CommandError::from("搜索失败", err))?;

    Ok(search_result)

}

#[tauri::command]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(aid = aid))]
pub async fn get_comic(app: AppHandle, aid: i64) -> CommandResult<Comic> {
    let ctx: &dyn AppContext = &app;
    let comic = utils::get_comic(ctx, aid)
        .await
        .map_err(|err| CommandError::from("获取漫画信息失败", err))?;

    Ok(comic)

}

#[tauri::command(async)]
#[specta::specta]
#[instrument(
    level = "error",
    skip_all,
    fields(folder_id = folder_id, page = page, sort = ?sort)
)]
pub async fn get_favorite_folder(
    app: AppHandle,
    folder_id: i64,
    page: i64,
    sort: FavoriteSort,
) -> CommandResult<GetFavoriteResult> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();

    let get_favorite_resp_data = jm_client
        .get_favorite_folder(folder_id, page, sort)
        .await
        .map_err(|err| CommandError::from("获取收藏夹失败", err))?;

    let get_favorite_result = GetFavoriteResult::from_resp_data(ctx, get_favorite_resp_data)
        .map_err(|err| CommandError::from("获取收藏夹失败", err))?;

    Ok(get_favorite_result)

}

#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub async fn get_weekly_info(app: AppHandle) -> CommandResult<GetWeeklyInfoRespData> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();

    let weekly_info = jm_client
        .get_weekly_info()
        .await
        .map_err(|err| CommandError::from("获取每周必看信息失败", err))?;

    Ok(weekly_info)

}

#[tauri::command(async)]
#[specta::specta]
#[instrument(
    level = "error",
    skip_all,
    fields(category_id = category_id, type_id = type_id)
)]
pub async fn get_weekly(
    app: AppHandle,
    category_id: String,
    type_id: String,
) -> CommandResult<GetWeeklyResult> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();

    let get_weekly_resp_data = jm_client
        .get_weekly(&category_id, &type_id)
        .await
        .map_err(|err| CommandError::from("获取每周必看失败", err))?;

    let get_weekly_result = GetWeeklyResult::from_resp_data(ctx, get_weekly_resp_data)
        .map_err(|err| CommandError::from("获取每周必看失败", err))?;

    Ok(get_weekly_result)

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(
    level = "error",
    skip_all,
    fields(comic_id = comic.id, comic_title = comic.name, chapter_id = chapter_id)
)]
pub fn create_download_task(app: AppHandle, comic: Comic, chapter_id: i64) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    let download_manager = ctx.download_manager();

    download_manager
        .create_download_task(comic, chapter_id)
        .map_err(|err| CommandError::from("创建下载任务失败", err))?;
    Ok(())

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(comic_id = comic.id, comic_title = comic.name))]
pub fn create_download_tasks(app: AppHandle, comic: Comic, chapter_ids: Vec<i64>) {
    let ctx: &dyn AppContext = &app;
    let download_manager = ctx.download_manager();

    download_manager.create_download_tasks(comic, &chapter_ids);

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(chapter_id = chapter_id))]
pub fn pause_download_task(app: AppHandle, chapter_id: i64) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    let download_manager = ctx.download_manager();

    download_manager
        .pause_download_task(chapter_id)
        .map_err(|err| CommandError::from("暂停下载任务失败", err))?;
    Ok(())

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(chapter_id = chapter_id))]
pub fn resume_download_task(app: AppHandle, chapter_id: i64) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    let download_manager = ctx.download_manager();

    download_manager
        .resume_download_task(chapter_id)
        .map_err(|err| CommandError::from("恢复下载任务失败", err))?;
    Ok(())

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(chapter_id = chapter_id))]
pub fn delete_download_task(app: AppHandle, chapter_id: i64) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    let download_manager = ctx.download_manager();

    download_manager
        .delete_download_task(chapter_id)
        .map_err(|err| CommandError::from("删除下载任务失败", err))?;
    Ok(())

}

#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(aid = aid))]
pub async fn download_comic(app: AppHandle, aid: i64) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    let download_manager = ctx.download_manager();

    let comic = utils::get_comic(ctx, aid)
        .await
        .map_err(|err| CommandError::from("获取漫画信息失败", err))?;

    let comic_title = &comic.name;

    let chapter_ids: Vec<i64> = comic
        .chapter_infos
        .iter()
        .filter(|chapter_info| chapter_info.is_downloaded != Some(true))
        .map(|chapter_info| chapter_info.chapter_id)
        .collect();

    if chapter_ids.is_empty() {
        let err = eyre!("漫画`{comic_title}`的所有章节都已存在于下载目录，无需重复下载");
        return Err(CommandError::from("一键下载漫画失败", err));
    }

    for chapter_id in chapter_ids {
        download_manager
            .create_download_task(comic.clone(), chapter_id)
            .map_err(|err| CommandError::from("一键下载漫画失败", err))?;
    }

    tracing::debug!("一键下载漫画成功，已为所有需要下载的章节创建下载任务");
    Ok(())

}

#[allow(clippy::cast_possible_wrap)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub async fn download_all_favorites(app: AppHandle) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    crate::downloader::batch_ops::download_all_favorites(ctx)
        .await
        .map_err(|err| CommandError::from("一键下载收藏夹失败", err))?;
    Ok(())
}

#[allow(clippy::cast_possible_wrap)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub async fn update_downloaded_comics(app: AppHandle) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    crate::downloader::batch_ops::update_downloaded_comics(ctx)
        .await
        .map_err(|err| CommandError::from("更新已下载漫画失败", err))?;
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(path = path))]
pub fn show_path_in_file_manager(app: AppHandle, path: &str) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    ctx.reveal_item_in_dir(std::path::Path::new(path))
        .map_err(|err| CommandError::from("在文件管理器中打开失败", err))?;
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub async fn sync_favorite_folder(app: AppHandle) -> CommandResult<()> {
    let ctx: &dyn AppContext = &app;
    let jm_client = ctx.jm_client();
    // 同步收藏夹的方式是随便收藏一个漫画
    // 调用两次toggle是因为要把新收藏的漫画取消收藏
    let task1 = jm_client.toggle_favorite_comic(468_984);
    let task2 = jm_client.toggle_favorite_comic(468_984);
    let (resp1, resp2) =
        tokio::try_join!(task1, task2).map_err(|err| CommandError::from("同步收藏夹失败", err))?;
    if resp1.toggle_type == resp2.toggle_type {
        let toggle_type = resp1.toggle_type;
        let err_title = "同步收藏夹失败";
        let err = eyre!("两个请求都是`{toggle_type:?}`操作");
        return Err(CommandError::from(err_title, err));
    }

    Ok(())

}

#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_lines)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub fn get_downloaded_comics(app: AppHandle) -> Vec<Comic> {
    let ctx: &dyn AppContext = &app;
    let download_dir = ctx.config().download_dir.clone();
    // 遍历下载目录，获取所有漫画元数据文件的路径和修改时间
    let mut metadata_path_with_modify_time = Vec::new();
    for entry in WalkDir::new(&download_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();

        if !entry.is_comic_metadata() {
            continue;
        }

        let metadata = match path
            .metadata()
            .map_err(eyre::Report::from)
            .wrap_err(format!("获取`{}`的metadata失败", path.display()))
        {
            Ok(metadata) => metadata,
            Err(err) => {
                let err_title = "获取已下载漫画的过程中遇到错误，已跳过";
                let message = err.to_message();
                tracing::error!(err_title, message);
                continue;
            }
        };

        let modify_time = match metadata
            .modified()
            .map_err(eyre::Report::from)
            .wrap_err(format!("获取`{}`的修改时间失败", path.display()))
        {
            Ok(modify_time) => modify_time,
            Err(err) => {
                let err_title = "获取已下载漫画的过程中遇到错误，已跳过";
                let message = err.to_message();
                tracing::error!(err_title, message);
                continue;
            }
        };

        metadata_path_with_modify_time.push((path.to_path_buf(), modify_time));
    }
    // 按照文件修改时间排序，最新的排在最前面
    metadata_path_with_modify_time.sort_by(|(_, a), (_, b)| b.cmp(a));

    let mut downloaded_comics = Vec::new();
    for (metadata_path, _) in metadata_path_with_modify_time {
        // 用当前配置的 dir_fmt 渲染章节目录名，便于精确匹配 zip 文件名
        let config = ctx.config();
        let dir_fmt = config.dir_fmt.clone();
        let mode = config.chinese_normalization;
        match Comic::from_metadata(&metadata_path, &dir_fmt, mode) {
            Ok(comic) => downloaded_comics.push(comic),
            Err(err) => {
                let err_title = "获取已下载漫画的过程中遇到错误，已跳过";
                let message = err.to_message();
                tracing::error!(err_title, message);
            }
        }
    }
    // 按照漫画ID分组，以方便去重
    let mut comics_by_id: IndexMap<i64, Vec<Comic>> = IndexMap::new();
    for comic in downloaded_comics {
        comics_by_id.entry(comic.id).or_default().push(comic);
    }

    let mut unique_comics = Vec::new();
    for (_comic_id, mut comics) in comics_by_id {
        // 该漫画ID对应的所有漫画下载目录，可能有多个版本，所以需要去重
        let comic_download_dirs: Vec<&PathBuf> = comics
            .iter()
            .filter_map(|comic| comic.comic_download_dir.as_ref())
            .collect();

        if comic_download_dirs.is_empty() {
            // 其实这种情况不应该发生，因为漫画元数据文件应该总是有下载目录的
            continue;
        }

        // 选第一个作为保留的漫画
        let chosen_download_dir = comic_download_dirs[0];

        if comics.len() > 1 {
            let dir_paths_string = comic_download_dirs
                .iter()
                .map(|path| format!("`{}`", path.display()))
                .collect::<Vec<String>>()
                .join(", ");
            // 如果有重复的漫画，打印错误信息
            let comic_title = &comics[0].name;
            let err_title = "获取已下载漫画的过程中遇到错误";
            let message = eyre!("所有版本路径: [{dir_paths_string}]")
                .wrap_err(format!(
                    "此次获取已下载漫画的结果中只保留版本`{}`",
                    chosen_download_dir.display()
                ))
                .wrap_err(format!(
                    "漫画`{comic_title}`在下载目录里有多个版本，请手动处理，只保留一个版本"
                ))
                .to_message();
            tracing::error!(err_title, message);
        }
        // 取第一个作为保留的漫画
        let chosen_comic = comics.remove(0);
        unique_comics.push(chosen_comic);
    }

    unique_comics

}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
#[instrument(level = "error", skip_all, fields(comic_id = comic.id, comic_title = comic.name))]
pub fn export_cbz(app: AppHandle, comic: Comic) -> CommandResult<()> {
    export::cbz(&app, &comic).map_err(|err| CommandError::from("导出cbz失败", err))?;
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
#[instrument(level = "error", skip_all, fields(comic_id = comic.id, comic_title = comic.name))]
pub fn export_pdf(app: AppHandle, comic: Comic) -> CommandResult<()> {
    export::pdf(&app, &comic).map_err(|err| CommandError::from("导出pdf失败", err))?;
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn export_cbz_chapters(
    app: AppHandle,
    comic: Comic,
    chapter_ids: Vec<i64>,
) -> CommandResult<()> {
    let comic_title = comic.name.clone();
    export::cbz_chapters(&app, &comic, chapter_ids)
        .wrap_err(format!("漫画`{comic_title}`导出指定章节cbz失败"))
        .map_err(|err| CommandError::from("导出指定章节cbz失败", err))?;
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn export_pdf_chapters(
    app: AppHandle,
    comic: Comic,
    chapter_ids: Vec<i64>,
) -> CommandResult<()> {
    let comic_title = comic.name.clone();
    export::pdf_chapters(&app, &comic, chapter_ids)
        .wrap_err(format!("漫画`{comic_title}`导出指定章节pdf失败"))
        .map_err(|err| CommandError::from("导出指定章节pdf失败", err))?;
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all)]
pub fn get_logs_dir_size(app: AppHandle) -> CommandResult<u64> {
    let logs_dir = logger::logs_dir(&app)
        .wrap_err("获取日志目录失败")
        .map_err(|err| CommandError::from("获取日志目录大小失败", err))?;
    let logs_dir_size = std::fs::read_dir(&logs_dir)
        .wrap_err(format!("读取日志目录`{}`失败", logs_dir.display()))
        .map_err(|err| CommandError::from("获取日志目录大小失败", err))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum::<u64>();
    tracing::debug!("获取日志目录大小成功");
    Ok(logs_dir_size)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(comic_id = comic.id, comic_title = comic.name))]
pub fn get_synced_comic(app: AppHandle, mut comic: Comic) -> CommandResult<Comic> {
    let ctx: &dyn AppContext = &app;
    let id_to_dir_map = ctx.downloaded_comics_index().get_or_build(ctx)
        .map_err(|err| CommandError::from("同步Comic字段失败", err))?;
    let config = ctx.config();
    let dir_fmt = config.dir_fmt.clone();
    let mode = config.chinese_normalization;

    comic
        .update_fields(&id_to_dir_map, &dir_fmt, mode)
        .map_err(|err| CommandError::from("同步Comic字段失败", err))?;

    Ok(comic)

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(comic_id = comic.id, comic_title = comic.name))]
pub fn get_synced_comic_in_favorite(
    app: AppHandle,
    mut comic: ComicInFavorite,
) -> CommandResult<ComicInFavorite> {
    let ctx: &dyn AppContext = &app;
    let id_to_dir_map = ctx.downloaded_comics_index().get_or_build(ctx)
        .map_err(|err| CommandError::from("同步ComicInFavorite字段失败", err))?;

    comic.update_fields(&id_to_dir_map);

    Ok(comic)

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(comic_id = comic.id, comic_title = comic.name))]
pub fn get_synced_comic_in_search(
    app: AppHandle,
    mut comic: ComicInSearch,
) -> CommandResult<ComicInSearch> {
    let ctx: &dyn AppContext = &app;
    let id_to_dir_map = ctx.downloaded_comics_index().get_or_build(ctx)
        .map_err(|err| CommandError::from("同步ComicInSearch字段失败", err))?;

    comic.update_fields(&id_to_dir_map);

    Ok(comic)

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(comic_id = comic.id, comic_title = comic.name))]
pub fn get_synced_comic_in_weekly(
    app: AppHandle,
    mut comic: ComicInWeekly,
) -> CommandResult<ComicInWeekly> {
    let ctx: &dyn AppContext = &app;
    let id_to_dir_map = ctx.downloaded_comics_index().get_or_build(ctx)
        .map_err(|err| CommandError::from("同步ComicInWeekly字段失败", err))?;

    comic.update_fields(&id_to_dir_map);

    Ok(comic)

}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command(async)]
#[specta::specta]
#[instrument(level = "error", skip_all, fields(path = path))]
pub fn open_log_file(path: &str) -> CommandResult<Vec<LogMetadata>> {
    let log_file = File::open(path).map_err(|err| CommandError::from("打开日志文件失败", err))?;
    let reader = BufReader::new(log_file);

    let mut logs = Vec::new();
    let mut line_num = 0;

    for line_result in reader.lines() {
        line_num += 1;

        let line = line_result
            .wrap_err(format!("读取日志文件的第`{line_num}`行失败"))
            .map_err(|err| CommandError::from("打开日志文件失败", err))?;

        if line.trim().is_empty() {
            continue;
        }

        let log = serde_json::from_str::<LogMetadata>(&line)
            .wrap_err(format!("将日志文件的第`{line_num}`行解析为LogMetadata失败"))
            .map_err(|err| CommandError::from("打开日志文件失败", err))?;

        logs.push(log);
    }

    Ok(logs)
}
