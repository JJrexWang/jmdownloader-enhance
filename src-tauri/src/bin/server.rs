//! `server` binary — 把 JMComic 下载器跑成一个 axum HTTP 服务。
//!
//! 启动流程：
//! 1. 读取 `JM_CONFIG_DIR`（默认 `/config`）作为 data_dir；
//!    读取 `JM_DOWNLOADS_DIR`（默认 `/downloads`）作为下载根目录；
//! 2. 构造 `HttpAppContext`（包含 Config、JmClient、DownloadManager 等）；
//! 3. 设置 tracing subscriber，把日志也写到 stdout（docker logs 能看到）；
//! 4. 设置 axum 路由 + SSE；
//! 5. 监听 `0.0.0.0:${JM_PORT:-8080}`。

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;

use jmcomic_downloader_lib::config::Config;
use jmcomic_downloader_lib::export;
use jmcomic_downloader_lib::logger;
use jmcomic_downloader_lib::service::{AppContext, HttpAppContext};
use jmcomic_downloader_lib::types::{Comic, FavoriteSort, SearchSort};
use jmcomic_downloader_lib::downloader::batch_ops;
use jmcomic_downloader_lib::types;
use jmcomic_downloader_lib::utils;

type SharedState = Arc<HttpAppContext>;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // tracing 由 logger::init 统一初始化（包括 stdout 输出和 RUST_LOG 过滤），
    // 避免重复 init global subscriber 失败。

    // 1. 加载/构造配置 + 路径。
    let paths = HttpAppContext::paths_from_env()?;
    let config = Config::new(&paths.data_dir)?;

    // 2. 构造 ctx。
    let ctx = HttpAppContext::build(paths, config);

    // 3. 设置文件日志（写到 ctx.paths().logs_dir）+ stdout + RUST_LOG。
    logger::init(ctx.clone())?;

    // 4. 触发自动登录（如果设置了 JM_USERNAME/JM_PASSWORD）。
    {
        let username = std::env::var("JM_USERNAME").unwrap_or_default();
        let password = std::env::var("JM_PASSWORD").unwrap_or_default();
        if !username.is_empty() {
            tracing::info!("检测到 JM_USERNAME，启动时尝试自动登录");
            {
                let mut cfg = ctx.config_mut();
                cfg.username = username;
                cfg.password = password;
            }
            if let Err(err) = ctx.auto_login().await {
                tracing::warn!(?err, "自动登录失败，将以未登录状态继续运行");
            }
        } else {
            tracing::info!("未设置 JM_USERNAME，跳过自动登录");
        }
    }

    // 5. 构造 axum router。
    let shared: SharedState = ctx.clone();
    let app = Router::new()
        .route("/health", get(health))
        .route("/events", get(sse_events))
        .route("/config", get(get_config).post(save_config))
        .route("/login", post(login))
        .route("/user-profile", get(get_user_profile))
        .route("/search", post(search))
        .route("/comic/:id", get(get_comic))
        .route("/favorites", post(get_favorite_folder))
        .route("/weekly-info", get(get_weekly_info))
        .route("/weekly", post(get_weekly))
        .route("/download/task", post(create_download_task))
        .route("/download/tasks", post(create_download_tasks))
        .route("/download/pause", post(pause_download_task))
        .route("/download/resume", post(resume_download_task))
        .route("/download/delete", post(delete_download_task))
        .route("/download/comic", post(download_comic))
        .route("/download/all-favorites", post(download_all_favorites))
        .route("/download/update-downloaded", post(update_downloaded_comics))
        .route("/export/cbz", post(export_cbz))
        .route("/export/pdf", post(export_pdf))
        .route("/export/cbz/chapters", post(export_cbz_chapters))
        .route("/export/pdf/chapters", post(export_pdf_chapters))
        .route("/logs/size", get(get_logs_dir_size))
        .with_state(shared);

    // 6. 监听。
    let port: u16 = std::env::var("JM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("jmcomic-downloader HTTP server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

// -----------------------------------------------------------------------
// 通用响应类型
// -----------------------------------------------------------------------

#[derive(Serialize)]
struct ApiError {
    err_title: String,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(self)).into_response()
    }
}

fn to_api_error(err_title: &str, err: impl std::fmt::Debug) -> ApiError {
    let message = format!("{:?}", err);
    tracing::error!(err_title, message);
    ApiError {
        err_title: err_title.to_string(),
        message,
    }
}

// -----------------------------------------------------------------------
// 路由 handlers
// -----------------------------------------------------------------------

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "jmcomic-downloader" }))
}

async fn sse_events(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    let rx = state.subscribe_events();
    let stream = async_stream::stream! {
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok((event_name, payload)) => {
                    let data = serde_json::to_string(&payload).unwrap_or_default();
                    yield Ok::<_, axum::Error>(Event::default()
                        .event(event_name)
                        .data(data));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(n, "SSE 客户端滞后，丢弃 {n} 条事件");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

async fn get_config(State(state): State<SharedState>) -> impl IntoResponse {
    Json(state.config().clone())
}

#[derive(Deserialize)]
struct SaveConfigBody {
    #[serde(flatten)]
    config: Config,
}

async fn save_config(
    State(state): State<SharedState>,
    Json(body): Json<SaveConfigBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let jm_client = ctx.jm_client();

    let proxy_changed;
    let file_logger_changed;
    let enable_file_logger = body.config.enable_file_logger;
    {
        let current = ctx.config();
        proxy_changed = current.proxy_mode != body.config.proxy_mode
            || current.proxy_host != body.config.proxy_host
            || current.proxy_port != body.config.proxy_port;
        file_logger_changed = current.enable_file_logger != enable_file_logger;
    }

    {
        let mut config_state = ctx.config_mut();
        *config_state = body.config;
        config_state
            .save(ctx.paths().data_dir.as_path())
            .map_err(|err| to_api_error("保存配置失败", err))?;
    }

    if proxy_changed {
        jm_client.reload_client();
    }

    if file_logger_changed {
        if enable_file_logger {
            logger::reload_file_logger()
                .map_err(|err| to_api_error("重新加载文件日志失败", err))?;
        } else {
            logger::disable_file_logger()
                .map_err(|err| to_api_error("禁用文件日志失败", err))?;
        }
    }

    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(
    State(state): State<SharedState>,
    Json(body): Json<LoginBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let profile = ctx
        .jm_client()
        .login(&body.username, &body.password)
        .await
        .map_err(|err| to_api_error("登录失败", err))?;
    Ok(Json(profile))
}

async fn get_user_profile(State(state): State<SharedState>) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let profile = ctx
        .jm_client()
        .get_user_profile()
        .await
        .map_err(|err| to_api_error("获取用户信息失败", err))?;
    Ok(Json(profile))
}

#[derive(Deserialize)]
struct SearchBody {
    keyword: String,
    page: i64,
    sort: SearchSort,
}

async fn search(
    State(state): State<SharedState>,
    Json(body): Json<SearchBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let resp = ctx
        .jm_client()
        .search(&body.keyword, body.page, body.sort)
        .await
        .map_err(|err| to_api_error("搜索失败", err))?;
    let result = types::SearchResultVariant::from_search_resp(ctx, resp)
        .map_err(|err| to_api_error("搜索失败", err))?;
    Ok(Json(result))
}

async fn get_comic(
    State(state): State<SharedState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let comic = utils::get_comic(ctx, id)
        .await
        .map_err(|err| to_api_error("获取漫画信息失败", err))?;
    Ok(Json(comic))
}

#[derive(Deserialize)]
struct FavoriteBody {
    folder_id: i64,
    page: i64,
    sort: FavoriteSort,
}

async fn get_favorite_folder(
    State(state): State<SharedState>,
    Json(body): Json<FavoriteBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let data = ctx
        .jm_client()
        .get_favorite_folder(body.folder_id, body.page, body.sort)
        .await
        .map_err(|err| to_api_error("获取收藏夹失败", err))?;
    let result = types::GetFavoriteResult::from_resp_data(ctx, data)
        .map_err(|err| to_api_error("获取收藏夹失败", err))?;
    Ok(Json(result))
}

async fn get_weekly_info(State(state): State<SharedState>) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let info = ctx
        .jm_client()
        .get_weekly_info()
        .await
        .map_err(|err| to_api_error("获取每周必看信息失败", err))?;
    Ok(Json(info))
}

#[derive(Deserialize)]
struct WeeklyBody {
    category_id: String,
    type_id: String,
}

async fn get_weekly(
    State(state): State<SharedState>,
    Json(body): Json<WeeklyBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let data = ctx
        .jm_client()
        .get_weekly(&body.category_id, &body.type_id)
        .await
        .map_err(|err| to_api_error("获取每周必看失败", err))?;
    let result = types::GetWeeklyResult::from_resp_data(ctx, data)
        .map_err(|err| to_api_error("获取每周必看失败", err))?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct CreateDownloadTaskBody {
    comic: Comic,
    chapter_id: i64,
}

async fn create_download_task(
    State(state): State<SharedState>,
    Json(body): Json<CreateDownloadTaskBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    ctx.download_manager()
        .create_download_task(body.comic, body.chapter_id)
        .map_err(|err| to_api_error("创建下载任务失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct CreateDownloadTasksBody {
    comic: Comic,
    chapter_ids: Vec<i64>,
}

async fn create_download_tasks(
    State(state): State<SharedState>,
    Json(body): Json<CreateDownloadTasksBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    ctx.download_manager()
        .create_download_tasks(body.comic, &body.chapter_ids);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ChapterIdBody {
    chapter_id: i64,
}

async fn pause_download_task(
    State(state): State<SharedState>,
    Json(body): Json<ChapterIdBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    ctx.download_manager()
        .pause_download_task(body.chapter_id)
        .map_err(|err| to_api_error("暂停下载任务失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

async fn resume_download_task(
    State(state): State<SharedState>,
    Json(body): Json<ChapterIdBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    ctx.download_manager()
        .resume_download_task(body.chapter_id)
        .map_err(|err| to_api_error("恢复下载任务失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_download_task(
    State(state): State<SharedState>,
    Json(body): Json<ChapterIdBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    ctx.download_manager()
        .delete_download_task(body.chapter_id)
        .map_err(|err| to_api_error("删除下载任务失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct DownloadComicBody {
    aid: i64,
}

async fn download_comic(
    State(state): State<SharedState>,
    Json(body): Json<DownloadComicBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let _jm = ctx.jm_client();
    let dm = ctx.download_manager();

    let comic = utils::get_comic(ctx, body.aid)
        .await
        .map_err(|err| to_api_error("获取漫画信息失败", err))?;

    let chapter_ids: Vec<i64> = comic
        .chapter_infos
        .iter()
        .map(|c| c.chapter_id)
        .collect();

    let owned = comic;
    dm.create_download_tasks(owned.clone(), &chapter_ids);
    let _ = owned;
    Ok(Json(json!({ "ok": true, "chapter_count": chapter_ids.len() })))
}

async fn download_all_favorites(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx: &dyn AppContext = state.as_ref();
    batch_ops::download_all_favorites(ctx)
        .await
        .map_err(|err| to_api_error("一键下载收藏夹失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

async fn update_downloaded_comics(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx: &dyn AppContext = state.as_ref();
    batch_ops::update_downloaded_comics(ctx)
        .await
        .map_err(|err| to_api_error("更新已下载漫画失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

async fn export_cbz(
    State(state): State<SharedState>,
    Json(comic): Json<Comic>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let synced = sync_comic(ctx, comic)
        .map_err(|err| to_api_error("同步漫画信息失败", err))?;
    export::cbz(ctx, &synced)
        .map_err(|err| to_api_error("导出 CBZ 失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

async fn export_pdf(
    State(state): State<SharedState>,
    Json(comic): Json<Comic>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let synced = sync_comic(ctx, comic)
        .map_err(|err| to_api_error("同步漫画信息失败", err))?;
    export::pdf(ctx, &synced)
        .map_err(|err| to_api_error("导出 PDF 失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ExportChaptersBody {
    comic: Comic,
    chapter_ids: Vec<i64>,
}

async fn export_cbz_chapters(
    State(state): State<SharedState>,
    Json(body): Json<ExportChaptersBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let synced = sync_comic(ctx, body.comic)
        .map_err(|err| to_api_error("同步漫画信息失败", err))?;
    export::cbz_chapters(ctx, &synced, body.chapter_ids)
        .map_err(|err| to_api_error("导出 CBZ (chapters) 失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

async fn export_pdf_chapters(
    State(state): State<SharedState>,
    Json(body): Json<ExportChaptersBody>,
) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let synced = sync_comic(ctx, body.comic)
        .map_err(|err| to_api_error("同步漫画信息失败", err))?;
    export::pdf_chapters(ctx, &synced, body.chapter_ids)
        .map_err(|err| to_api_error("导出 PDF (chapters) 失败", err))?;
    Ok(Json(json!({ "ok": true })))
}

async fn get_logs_dir_size(State(state): State<SharedState>) -> Result<impl IntoResponse, ApiError> {
    let ctx = state.as_ref().as_ref();
    let logs_dir = logger::logs_dir(ctx)
        .map_err(|err| to_api_error("获取日志目录失败", err))?;
    let size: u64 = std::fs::read_dir(&logs_dir)
        .map_err(|err| to_api_error("读取日志目录失败", err))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .map(|m| m.len())
        .sum();
    Ok(Json(json!({ "size": size })))
}


/// 跟桌面端 `get_synced_comic` 等价：把 Comic 字段跟本地已下载索引对齐
/// （download_dir / chapter.is_downloaded / chapter.chapter_download_dir 等）。
fn sync_comic(ctx: &dyn AppContext, mut comic: Comic) -> eyre::Result<Comic> {
    let id_to_dir_map = ctx.downloaded_comics_index().get_or_build(ctx)?;
    let config = ctx.config();
    let dir_fmt = config.dir_fmt.clone();
    let mode = config.chinese_normalization;
    comic.update_fields(&id_to_dir_map, &dir_fmt, mode)?;
    Ok(comic)
}
