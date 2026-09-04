use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tracing::instrument;

use crate::{
    responses::{string_to_i64, ComicInWeeklyRespData, GetWeeklyRespData},
    types::{Category, CategorySub},
    extensions::AppHandleExt,
};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct GetWeeklyResult {
    pub total: i64,
    pub list: Vec<ComicInWeekly>,
}

impl GetWeeklyResult {
    #[instrument(level = "error", skip_all)]
    pub fn from_resp_data(app: &AppHandle, resp_data: GetWeeklyRespData) -> eyre::Result<Self> {
        // 配置开关：关闭后整个 hashmap lookup 全部跳过，直接构造列表。
        // 避免大库存下切分类/换页的连锁 IPC 阻塞 UI。
        let show_badge = app.get_config().read().weekly_show_downloaded_badge;
        let id_to_dir_map = if show_badge {
            Some(app.get_downloaded_comics_index().get_or_build(app)?)
        } else {
            None
        };

        let list = resp_data
            .list
            .into_iter()
            .map(|comic| ComicInWeekly::from_resp_data(comic, id_to_dir_map.as_deref()))
            .collect();

        let get_weekly_result = GetWeeklyResult {
            total: resp_data.total,
            list,
        };

        Ok(get_weekly_result)
    }

    /// 章节下载完成后由 sync 调用的单条同步：同样遵守开关。
    pub fn sync_one(app: &AppHandle, mut comic: ComicInWeekly) -> ComicInWeekly {
        let show_badge = app.get_config().read().weekly_show_downloaded_badge;
        if show_badge {
            if let Ok(id_to_dir_map) = app.get_downloaded_comics_index().get_or_build(app) {
                comic.update_fields(Some(&id_to_dir_map));
            }
        } else {
            comic.update_fields(None);
        }
        comic
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct ComicInWeekly {
    #[serde(deserialize_with = "string_to_i64")]
    pub id: i64,
    pub author: String,
    pub description: String,
    pub name: String,
    pub image: String,
    pub category: Category,
    pub category_sub: CategorySub,
    pub liked: bool,
    pub is_favorite: bool,
    pub update_at: i64,
    pub is_downloaded: bool,
    pub comic_download_dir: PathBuf,
}

impl ComicInWeekly {
    pub fn from_resp_data(
        resp_data: ComicInWeeklyRespData,
        id_to_dir_map: Option<&HashMap<i64, PathBuf>>,
    ) -> ComicInWeekly {
        let mut comic = ComicInWeekly {
            id: resp_data.id,
            author: resp_data.author,
            description: resp_data.description,
            name: resp_data.name,
            image: resp_data.image,
            category: resp_data.category,
            category_sub: resp_data.category_sub,
            liked: resp_data.liked,
            is_favorite: resp_data.is_favorite,
            update_at: resp_data.update_at,
            is_downloaded: false,
            comic_download_dir: PathBuf::new(),
        };

        comic.update_fields(id_to_dir_map);

        comic
    }

    pub fn update_fields(&mut self, id_to_dir_map: Option<&HashMap<i64, PathBuf>>) {
        if let Some(map) = id_to_dir_map {
            if let Some(comic_download_dir) = map.get(&self.id) {
                self.comic_download_dir = comic_download_dir.clone();
                self.is_downloaded = true;
            }
        }
    }
}
