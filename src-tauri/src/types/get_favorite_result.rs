use std::{collections::HashMap, path::PathBuf};

use eyre::WrapErr;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tracing::instrument;

use crate::{
    responses::{
        CategoryRespData, CategorySubRespData, ComicInFavoriteRespData, FavoriteFolderRespData,
        GetFavoriteRespData,
    },
    extensions::AppHandleExt,
};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetFavoriteResult {
    pub list: Vec<ComicInFavorite>,
    pub folder_list: Vec<FavoriteFolderRespData>,
    pub total: i64,
    pub count: i64,
}

impl GetFavoriteResult {
    #[instrument(level = "error", skip_all)]
    pub fn from_resp_data(
        app: &AppHandle,
        resp_data: GetFavoriteRespData,
    ) -> eyre::Result<GetFavoriteResult> {
        // 配置开关：关闭后整个 hashmap lookup 全部跳过，直接构造列表。
        // 这避免了大库存（几百到上千本）下每次翻页/换排序都触发哈希匹配，
        // 以及对应的前端连锁 sync 把 UI 主线程阻塞。
        let show_badge = app
            .get_config()
            .read()
            .favorite_show_downloaded_badge;
        let id_to_dir_map = if show_badge {
            Some(app.get_downloaded_comics_index().get_or_build(app)?)
        } else {
            None
        };

        let list = resp_data
            .list
            .into_iter()
            .map(|comic| ComicInFavorite::from_resp_data(comic, id_to_dir_map.as_deref()))
            .collect::<eyre::Result<_>>()?;

        let total: i64 = resp_data.total.parse().wrap_err("将total解析为i64失败")?;

        let get_favorite_result = GetFavoriteResult {
            list,
            folder_list: resp_data.folder_list,
            total,
            count: resp_data.count,
        };

        Ok(get_favorite_result)
    }

    /// 单独刷新某一条收藏夹项的字段（章节下载完成后由 sync 调用）。
    /// 同样遵守开关：关闭时直接返回原对象，不做 hashmap 匹配。
    pub fn sync_one(
        app: &AppHandle,
        mut comic: ComicInFavorite,
    ) -> eyre::Result<ComicInFavorite> {
        let show_badge = app
            .get_config()
            .read()
            .favorite_show_downloaded_badge;
        if show_badge {
            let id_to_dir_map = app.get_downloaded_comics_index().get_or_build(app)?;
            comic.update_fields(Some(&id_to_dir_map));
        } else {
            comic.update_fields(None);
        }
        Ok(comic)
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComicInFavorite {
    pub id: i64,
    pub author: String,
    pub description: Option<String>,
    pub name: String,
    pub latest_ep: Option<String>,
    pub latest_ep_aid: Option<String>,
    pub image: String,
    pub category: CategoryRespData,
    pub category_sub: CategorySubRespData,
    pub is_downloaded: bool,
    pub comic_download_dir: PathBuf,
}

impl ComicInFavorite {
    pub fn from_resp_data(
        resp_data: ComicInFavoriteRespData,
        id_to_dir_map: Option<&HashMap<i64, PathBuf>>,
    ) -> eyre::Result<ComicInFavorite> {
        let id: i64 = resp_data.id.parse().wrap_err("将id解析为i64失败")?;

        let mut comic = ComicInFavorite {
            id,
            author: resp_data.author,
            description: resp_data.description,
            name: resp_data.name,
            latest_ep: resp_data.latest_ep,
            latest_ep_aid: resp_data.latest_ep_aid,
            image: resp_data.image,
            category: resp_data.category,
            category_sub: resp_data.category_sub,
            is_downloaded: false,
            comic_download_dir: PathBuf::new(),
        };

        comic.update_fields(id_to_dir_map);

        Ok(comic)
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
