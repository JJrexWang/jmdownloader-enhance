use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};

use eyre::{OptionExt, WrapErr};
use parking_lot::RwLock;
use tauri::AppHandle;
use tracing::instrument;
use walkdir::WalkDir;

use crate::{
    extensions::{AppHandleExt, WalkDirEntryExt},
    types::Comic,
};

pub fn filename_filter(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\\' | '/' | '\n' => ' ',
            ':' => '：',
            '*' => '⭐',
            '?' => '？',
            '"' => '\'',
            '<' => '《',
            '>' => '》',
            '|' => '丨',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_string()
}

/// 计算MD5哈希并返回十六进制字符串
pub fn md5_hex(data: &str) -> String {
    format!("{:x}", md5::compute(data))
}

// ============================================================
// 已下载漫画索引缓存
//
// 用来避免每次都重新 walk 一遍下载目录。
// 命中条件：当前 download_dir 与构建缓存时一致。
// 失效时机：用户改了 download_dir 时自动失效；下载/删除漫画后
// 调用 [`invalidate`] 强制重建。
// ============================================================

#[derive(Default)]
pub struct DownloadedComicsIndex {
    inner: RwLock<Option<CachedMap>>,
}

struct CachedMap {
    download_dir: PathBuf,
    map: Arc<HashMap<i64, PathBuf>>,
}

impl DownloadedComicsIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_build(&self, app: &AppHandle) -> eyre::Result<Arc<HashMap<i64, PathBuf>>> {
        let download_dir = app.get_config().read().download_dir.clone();

        if let Some(cached) = self.inner.read().as_ref() {
            if cached.download_dir == download_dir {
                return Ok(Arc::clone(&cached.map));
            }
        }

        let map = Arc::new(build_id_to_dir_map(&download_dir)?);
        *self.inner.write() = Some(CachedMap {
            download_dir,
            map: Arc::clone(&map),
        });
        Ok(map)
    }

    pub fn invalidate(&self) {
        *self.inner.write() = None;
    }
}

#[instrument(level = "error", skip_all)]
fn build_id_to_dir_map(download_dir: &PathBuf) -> eyre::Result<HashMap<i64, PathBuf>> {
    let mut id_to_dir_map: HashMap<i64, PathBuf> = HashMap::new();
    if !download_dir.exists() {
        return Ok(id_to_dir_map);
    }

    for entry in WalkDir::new(download_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !entry.is_comic_metadata() {
            continue;
        }

        let metadata_str =
            std::fs::read_to_string(path).wrap_err(format!("读取`{}`失败", path.display()))?;
        let comic_json: serde_json::Value = serde_json::from_str(&metadata_str).wrap_err(
            format!("将`{}`反序列化为serde_json::Value失败", path.display()),
        )?;
        let id = comic_json
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .ok_or_eyre(format!("`{}`没有`id`字段", path.display()))?;

        let parent = path
            .parent()
            .ok_or_eyre(format!("`{}`没有父目录", path.display()))?;

        id_to_dir_map.entry(id).or_insert(parent.to_path_buf());
    }
    Ok(id_to_dir_map)
}

#[instrument(level = "error", skip_all)]
pub async fn get_comic(app: AppHandle, aid: i64) -> eyre::Result<Comic> {
    let jm_client = app.get_jm_client();

    let comic_resp_data = jm_client.get_comic(aid).await?;

    let comic = Comic::from_comic_resp_data(&app, comic_resp_data)?;

    Ok(comic)
}
