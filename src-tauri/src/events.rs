use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::{
    downloader::download_task_state::DownloadTaskState,
    types::{ChapterInfo, Comic},
};

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(tag = "event", content = "data")]
pub enum DownloadEvent {
    #[serde(rename_all = "camelCase")]
    Speed { speed: String },

    #[serde(rename_all = "camelCase")]
    Sleeping { chapter_id: i64, remaining_sec: u64 },

    #[serde(rename_all = "camelCase")]
    TaskCreate {
        state: DownloadTaskState,
        comic: Box<Comic>,
        chapter_info: Box<ChapterInfo>,
        downloaded_img_count: u32,
        total_img_count: u32,
    },

    #[serde(rename_all = "camelCase")]
    TaskDelete { chapter_id: i64 },

    #[serde(rename_all = "camelCase")]
    TaskUpdate {
        chapter_id: i64,
        state: DownloadTaskState,
        downloaded_img_count: u32,
        total_img_count: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(tag = "event", content = "data")]
pub enum DownloadAllFavoritesEvent {
    #[serde(rename_all = "camelCase")]
    GetFavoritesStart,

    /// 收藏夹全部拉到本地之后，进入「逐本处理」阶段。
    ///
    /// `current_comic_title` 是当前正在拉取/处理的那一本的标题，方便前端在 overview
    /// 卡片里直接展示「当前: 《xxx》」。`comic_id` 可能在拉取/解析阶段就失败了，所以是可选的。
    #[serde(rename_all = "camelCase")]
    GetComicsProgress {
        current: i64,
        total: i64,
        current_comic_title: String,
    },

    #[serde(rename_all = "camelCase")]
    StartCreateDownloadTasks {
        comic_id: i64,
        comic_title: String,
        current: i64,
        total: i64,
    },

    #[serde(rename_all = "camelCase")]
    CreatingDownloadTask { comic_id: i64, current: i64 },

    #[serde(rename_all = "camelCase")]
    EndCreateDownloadTasks { comic_id: i64 },

    /// 一本漫画在拉取或解析阶段失败，已被跳过。
    ///
    /// 区别于 `EndCreateDownloadTasks`：本事件代表「整本没处理成」，所以前端 overview
    /// 卡片要把这一本计入失败计数而不是已完成计数。
    #[serde(rename_all = "camelCase")]
    FailedComic {
        comic_id: Option<i64>,
        comic_title: String,
    },

    #[serde(rename_all = "camelCase")]
    GetComicsEnd,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(tag = "event", content = "data")]
pub enum UpdateDownloadedComicsEvent {
    #[serde(rename_all = "camelCase")]
    GetComicStart { total: i64 },

    /// 与 `DownloadAllFavoritesEvent::GetComicsProgress` 同义：
    /// `current_comic_title` 是当前正在处理的那本已下载漫画的标题。
    #[serde(rename_all = "camelCase")]
    GetComicProgress {
        current: i64,
        total: i64,
        current_comic_title: String,
    },

    #[serde(rename_all = "camelCase")]
    CreateDownloadTasksStart {
        comic_id: i64,
        comic_title: String,
        current: i64,
        total: i64,
    },

    #[serde(rename_all = "camelCase")]
    CreateDownloadTaskProgress { comic_id: i64, current: i64 },

    #[serde(rename_all = "camelCase")]
    CreateDownloadTasksEnd { comic_id: i64 },

    /// 一本已下载漫画在拉取阶段失败，已被跳过。
    #[serde(rename_all = "camelCase")]
    FailedComic {
        comic_id: i64,
        comic_title: String,
    },

    #[serde(rename_all = "camelCase")]
    GetComicEnd,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(tag = "event", content = "data")]
pub enum ExportCbzEvent {
    #[serde(rename_all = "camelCase")]
    Start {
        uuid: String,
        comic_title: String,
        total: u32,
    },
    #[serde(rename_all = "camelCase")]
    Progress { uuid: String, current: u32 },
    #[serde(rename_all = "camelCase")]
    Error { uuid: String },
    #[serde(rename_all = "camelCase")]
    End {
        uuid: String,
        comic_id: i64,
        chapter_export_dir: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(tag = "event", content = "data")]
pub enum ExportPdfEvent {
    #[serde(rename_all = "camelCase")]
    CreateStart {
        uuid: String,
        comic_title: String,
        total: u32,
    },
    #[serde(rename_all = "camelCase")]
    CreateProgress { uuid: String, current: u32 },
    #[serde(rename_all = "camelCase")]
    CreateError { uuid: String },
    #[serde(rename_all = "camelCase")]
    CreateEnd {
        uuid: String,
        comic_id: i64,
        chapter_export_dir: PathBuf,
    },

    #[serde(rename_all = "camelCase")]
    MergeStart {
        uuid: String,
        comic_title: String,
        total: u32,
    },
    #[serde(rename_all = "camelCase")]
    MergeProgress { uuid: String, current: u32 },
    #[serde(rename_all = "camelCase")]
    MergeError { uuid: String },
    #[serde(rename_all = "camelCase")]
    MergeEnd {
        uuid: String,
        comic_id: i64,
        chapter_export_dir: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    pub json_raw: String,
}
