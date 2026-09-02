use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use eyre::{eyre, OptionExt, WrapErr};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tracing::instrument;
use walkdir::WalkDir;

use crate::{
    extensions::{AppHandleExt, EyreReportToMessage, WalkDirEntryExt},
    responses::{GetComicRespData, RelatedListRespData},
};

use super::{ChapterInfo, DirFmtParams};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub struct Comic {
    pub id: i64,
    pub name: String,
    pub addtime: String,
    pub description: String,
    #[serde(rename = "total_views")]
    pub total_views: String,
    pub likes: String,
    pub chapter_infos: Vec<ChapterInfo>,
    #[serde(rename = "series_id")]
    pub series_id: String,
    #[serde(rename = "comment_total")]
    pub comment_total: String,
    pub author: Vec<String>,
    pub tags: Vec<String>,
    pub works: Vec<String>,
    pub actors: Vec<String>,
    #[serde(rename = "related_list")]
    pub related_list: Vec<RelatedListRespData>,
    pub liked: bool,
    #[serde(rename = "is_favorite")]
    pub is_favorite: bool,
    #[serde(rename = "is_aids")]
    pub is_aids: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_downloaded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comic_download_dir: Option<PathBuf>,
}

impl Comic {
    #[instrument(
        level = "error",
        skip_all,
        fields(comic_id = comic.id, comic_title = comic.name)
    )]
    pub fn from_comic_resp_data(app: &AppHandle, comic: GetComicRespData) -> eyre::Result<Comic> {
        let id_to_dir_map = app.get_downloaded_comics_index().get_or_build(app)?;
        Self::from_comic_resp_data_with_map(app, comic, id_to_dir_map)
    }

    #[instrument(
        level = "error",
        skip_all,
        fields(comic_id = comic.id, comic_title = comic.name)
    )]
    pub fn from_comic_resp_data_with_map(
        app: &AppHandle,
        comic: GetComicRespData,
        id_to_dir_map: Arc<HashMap<i64, PathBuf>>,
    ) -> eyre::Result<Comic> {
        let mut comic = Self::build_from_resp_data(comic);

        // TODO: 这是为了兼容v0.15.4及之前的版本，后续需要移除，计划在v0.17.0之后移除
        if let Some(comic_download_dir) = id_to_dir_map.get(&comic.id) {
            comic
                .create_chapter_metadata_for_old_version(comic_download_dir)
                .wrap_err("为旧版本创建章节元数据失败")?;
        }

        let dir_fmt = app.get_config().read().dir_fmt.clone();
        comic.update_fields(&id_to_dir_map, &dir_fmt)?;

        Ok(comic)
    }

    /// 从 `GetComicRespData` 构建 `Comic` 结构体（不含下载状态相关的字段填充），
    /// `from_comic_resp_data` 与 `from_comic_resp_data_with_map` 共用此实现。
    fn build_from_resp_data(comic: GetComicRespData) -> Comic {
        let mut chapter_infos: Vec<ChapterInfo> = comic
            .series
            .into_iter()
            .enumerate()
            .filter_map(|(index, s)| {
                let chapter_id = s.id.parse().ok()?;
                #[allow(clippy::cast_possible_wrap)]
                let order = (index + 1) as i64;
                let mut chapter_title = format!("第{order}话");
                if !s.name.is_empty() {
                    #[allow(clippy::format_push_string)]
                    chapter_title.push_str(&format!(" {}", &s.name));
                }
                let chapter_info = ChapterInfo {
                    chapter_id,
                    chapter_title,
                    order,
                    is_pdf_exported: false,
                    is_cbz_exported: false,
                    is_archived: false,
                    is_downloaded: None,
                    chapter_download_dir: None,
                };
                Some(chapter_info)
            })
            .collect();
        // 如果没有章节信息，就添加一个默认的章节信息
        if chapter_infos.is_empty() {
            chapter_infos.push(ChapterInfo {
                chapter_id: comic.id,
                chapter_title: "第1话".to_owned(),
                order: 1,
                is_pdf_exported: false,
                is_cbz_exported: false,
                is_archived: false,
                is_downloaded: None,
                chapter_download_dir: None,
            });
        }

        Comic {
            id: comic.id,
            name: comic.name,
            addtime: comic.addtime,
            description: comic.description,
            total_views: comic.total_views,
            likes: comic.likes,
            chapter_infos,
            series_id: comic.series_id,
            comment_total: comic.comment_total,
            author: comic.author,
            tags: comic.tags,
            works: comic.works,
            actors: comic.actors,
            related_list: comic.related_list,
            liked: comic.liked,
            is_favorite: comic.is_favorite,
            is_aids: comic.is_aids,
            is_downloaded: None,
            comic_download_dir: None,
        }
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    pub fn update_fields(
        &mut self,
        id_to_dir_map: &HashMap<i64, PathBuf>,
        dir_fmt: &str,
    ) -> eyre::Result<()> {
        if let Some(comic_download_dir) = id_to_dir_map.get(&self.id) {
            self.comic_download_dir = Some(comic_download_dir.clone());
            self.is_downloaded = Some(true);

            self.update_chapter_infos_fields(dir_fmt)
                .wrap_err("更新章节信息字段失败")?;
        }

        Ok(())
    }

    #[instrument(level = "error", skip_all, fields(metadata_path = %metadata_path.display()))]
    pub fn from_metadata(metadata_path: &Path, dir_fmt: &str) -> eyre::Result<Comic> {
        let comic_json = std::fs::read_to_string(metadata_path)?;
        let mut comic = serde_json::from_str::<Comic>(&comic_json)
            .wrap_err("将元数据文件反序列化为Comic失败")?;
        // 来自元数据的章节信息没有`download_dir`和`is_downloaded`字段，需要更新
        let parent = metadata_path
            .parent()
            .ok_or_eyre(format!("`{}`没有父目录", metadata_path.display()))?;
        let comic_download_dir = parent.to_path_buf();

        // TODO: 这是为了兼容v0.15.4及之前的版本，后续需要移除，计划在v0.17.0之后移除
        comic
            .create_chapter_metadata_for_old_version(&comic_download_dir)
            .wrap_err("为旧版本创建章节元数据失败")?;

        comic.comic_download_dir = Some(comic_download_dir);
        comic.is_downloaded = Some(true);

        comic.update_chapter_infos_fields(dir_fmt)?;

        Ok(comic)
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    pub fn get_comic_download_dir_name(&self) -> eyre::Result<String> {
        let comic_download_dir = self
            .comic_download_dir
            .as_ref()
            .ok_or_eyre("`comic_download_dir`字段为`None`")?;

        let comic_download_dir_name = comic_download_dir
            .file_name()
            .ok_or_eyre(format!(
                "获取`{}`的目录名失败",
                comic_download_dir.display()
            ))?
            .to_string_lossy()
            .to_string();

        Ok(comic_download_dir_name)
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    pub fn get_comic_export_dir(&self, app: &AppHandle) -> eyre::Result<PathBuf> {
        let (download_dir, export_dir) = {
            let config = app.get_config();
            let config = config.read();
            (config.download_dir.clone(), config.export_dir.clone())
        };

        let Some(comic_download_dir) = self.comic_download_dir.clone() else {
            return Err(eyre!("`comic_download_dir`字段为`None`"));
        };

        let relative_dir = comic_download_dir
            .strip_prefix(&download_dir)
            .wrap_err(format!(
                "无法从路径`{}`中移除前缀`{}`",
                comic_download_dir.display(),
                download_dir.display()
            ))?;

        let comic_export_dir = export_dir.join(relative_dir);
        Ok(comic_export_dir)
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    pub fn ensure_download_dir_fields(&mut self, app: &AppHandle) -> eyre::Result<()> {
        if self.has_download_dir_fields() {
            return Ok(());
        }

        self.update_download_dir_fields_by_fmt(app)
    }

    pub fn has_download_dir_fields(&self) -> bool {
        let comic_download_dir_ready = self.comic_download_dir.is_some();
        let chapter_download_dir_ready = self
            .chapter_infos
            .iter()
            .all(|chapter| chapter.chapter_download_dir.is_some());

        comic_download_dir_ready && chapter_download_dir_ready
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    pub fn save_comic_metadata(&self) -> eyre::Result<()> {
        let mut comic = self.clone();
        // 将漫画的is_downloaded和comic_download_dir字段设置为None
        // 这样能使这些字段在序列化时被忽略
        comic.is_downloaded = None;
        comic.comic_download_dir = None;
        for chapter in &mut comic.chapter_infos {
            // 将章节的is_downloaded和chapter_download_dir字段设置为None
            // 这样能使这些字段在序列化时被忽略
            chapter.is_downloaded = None;
            chapter.chapter_download_dir = None;
        }

        let comic_download_dir = self
            .comic_download_dir
            .as_ref()
            .ok_or_eyre("`comic_download_dir`字段为`None`")?;
        let metadata_path = comic_download_dir.join("元数据.json");

        std::fs::create_dir_all(comic_download_dir)
            .wrap_err(format!("创建目录`{}`失败", comic_download_dir.display()))?;

        let comic_json =
            serde_json::to_string_pretty(&comic).wrap_err("将Comic序列化为json失败")?;

        std::fs::write(&metadata_path, comic_json)
            .wrap_err(format!("写入文件`{}`失败", metadata_path.display()))?;

        Ok(())
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    pub fn get_cover_path(&self) -> eyre::Result<PathBuf> {
        let comic_download_dir = self
            .comic_download_dir
            .as_ref()
            .ok_or_eyre("`comic_download_dir`字段为`None`")?;

        let cover_path = comic_download_dir.join("cover.jpg");

        Ok(cover_path)
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    pub fn update_download_dir_fields_by_fmt(&mut self, app: &AppHandle) -> eyre::Result<()> {
        if self.chapter_infos.is_empty() {
            return Err(eyre!("没有章节信息，无法更新下载目录字段"));
        }

        let author = self.author.join(", ");
        let mut first_chapter_download_dir = None;

        for chapter_info in &mut self.chapter_infos {
            let chapter_title = &chapter_info.chapter_title;

            let dir_fmt_params = DirFmtParams {
                comic_id: self.id,
                comic_title: self.name.clone(),
                author: author.clone(),
                chapter_id: chapter_info.chapter_id,
                chapter_title: chapter_info.chapter_title.clone(),
                order: chapter_info.order,
            };

            let chapter_download_dir =
                ChapterInfo::get_chapter_download_dir_by_fmt(app, &dir_fmt_params)
                    .wrap_err(format!("章节`{chapter_title}`根据fmt获取章节下载目录失败"))?;

            if first_chapter_download_dir.is_none() {
                first_chapter_download_dir = Some(chapter_download_dir.clone());
            }

            chapter_info.chapter_download_dir = Some(chapter_download_dir);
        }

        let Some(first_chapter_download_dir) = first_chapter_download_dir else {
            return Err(eyre!(
                "处理完所有章节后first_chapter_download_dir仍然为None"
            ));
        };

        let comic_download_dir = first_chapter_download_dir.parent().ok_or_eyre(format!(
            "第一个章节下载目录`{}`没有父目录",
            first_chapter_download_dir.display()
        ))?;

        self.comic_download_dir = Some(comic_download_dir.to_path_buf());

        Ok(())
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    fn update_chapter_infos_fields(&mut self, dir_fmt: &str) -> eyre::Result<()> {
        let Some(comic_download_dir) = &self.comic_download_dir else {
            return Err(eyre!("`comic_download_dir`字段为`None`"));
        };

        if !comic_download_dir.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(comic_download_dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(Result::ok)
        {
            let entry_path = entry.path();

            if entry.is_chapter_metadata() {
                // 标准章节目录：chapter_dir/章节元数据.json + 图片
                let metadata_str = std::fs::read_to_string(entry_path)
                    .wrap_err(format!("读取`{}`失败", entry_path.display()))?;

                let chapter_json: serde_json::Value = serde_json::from_str(&metadata_str)
                    .wrap_err(format!(
                        "将`{}`反序列化为serde_json::Value失败",
                        entry_path.display()
                    ))?;

                let chapter_id = chapter_json
                    .get("chapterId")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_eyre(format!(
                        "`{}`没有`chapterId`字段",
                        entry_path.display()
                    ))?;

                if let Some(chapter_info) = self
                    .chapter_infos
                    .iter_mut()
                    .find(|chapter| chapter.chapter_id == chapter_id)
                {
                    let parent = entry_path.parent().ok_or_eyre(format!(
                        "`{}`没有父目录",
                        entry_path.display()
                    ))?;
                    chapter_info.chapter_download_dir = Some(parent.to_path_buf());
                    chapter_info.is_downloaded = Some(true);
                    chapter_info.is_archived = false;
                    chapter_info.is_pdf_exported = chapter_json
                        .get("isPdfExported")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    chapter_info.is_cbz_exported = chapter_json
                        .get("isCbzExported")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                }
            } else if entry.is_chapter_archive() {
                // 已打包的章节：chapter_dir_name.zip / .cbz，章节元数据.json 位于压缩包内部。
                // 优先尝试从压缩包内读取章节元数据；若压缩包内没有（例如早期版本打包时
                // 没有把 章节元数据.json 写进 zip 的情况），则用当前 dir_fmt 模板反向推算
                // 每个章节的预期章节目录名，再与 zip 文件名（去除扩展名和可选的
                // `__<chapter_id>` 后缀）做精确字符串匹配。该策略不依赖任何固定模式，
                // 因此无论用户怎么自定义 dir_fmt 都能识别归档所属的章节。
                let chapter_id = match read_chapter_id_from_archive(entry_path) {
                    Ok(id) => id,
                    Err(inner_err) => match self.match_chapter_id_from_archive_name(entry_path, dir_fmt) {
                        Some(id) => {
                            tracing::warn!(
                                "从归档`{}`读取`chapterId`失败（{}），已按 dir_fmt=`{dir_fmt}` 反推匹配到 chapterId={}；后续下载会自动把 章节元数据.json 写入压缩包",
                                entry_path.display(),
                                inner_err.to_message(),
                                id,
                            );
                            id
                        }
                        None => {
                            let err_title = format!(
                                "从归档`{}`读取`chapterId`失败",
                                entry_path.display()
                            );
                            let message = inner_err.to_message();
                            tracing::warn!(err_title, message);
                            continue;
                        }
                    },
                };

                if let Some(chapter_info) = self
                    .chapter_infos
                    .iter_mut()
                    .find(|chapter| chapter.chapter_id == chapter_id)
                {
                    chapter_info.chapter_download_dir = Some(entry_path.to_path_buf());
                    chapter_info.is_downloaded = Some(true);
                    chapter_info.is_archived = true;
                    // 已归档章节的导出状态需要打开压缩包读取，暂不支持，默认为 false
                    // TODO: 后续把 is_pdf_exported / is_cbz_exported 挪到漫画级元数据中即可避免开包
                    chapter_info.is_pdf_exported = false;
                    chapter_info.is_cbz_exported = false;
                }
            }
        }
        Ok(())
    }

    #[instrument(level = "error", skip_all, fields(comic_id = self.id, comic_title = self.name))]
    fn create_chapter_metadata_for_old_version(
        &self,
        comic_download_dir: &Path,
    ) -> eyre::Result<()> {
        let mut chapter_dirs = HashSet::new();
        for entry in std::fs::read_dir(comic_download_dir)?.filter_map(Result::ok) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            chapter_dirs.insert(entry.path());
        }

        for chapter_info in &self.chapter_infos {
            let old_chapter_dir = comic_download_dir.join(&chapter_info.chapter_title);
            let old_chapter_dir_exists = chapter_dirs.contains(&old_chapter_dir);
            let old_chapter_metadata_exists = old_chapter_dir.join("章节元数据.json").exists();
            if old_chapter_dir_exists && !old_chapter_metadata_exists {
                // 如果旧版本的章节目录存在，但没有元数据文件，就创建一个
                let mut info = chapter_info.clone();
                info.chapter_download_dir = Some(old_chapter_dir);
                info.is_downloaded = Some(true);
                info.save_chapter_metadata()?;
            }
        }

        Ok(())
    }

    /// 当压缩包里没有 `章节元数据.json` 时，按 zip 文件名回退匹配章节。
    ///
    /// 用 `dir_fmt` 模板给 `self.chapter_infos` 里每个章节渲一次「预期章节目录名」
    /// （即 dir_fmt 最后一段格式化后的结果），然后和 zip 文件名去除扩展名、可选的
    /// `__<chapter_id>` 后缀后做精确字符串匹配。该方案不依赖任何固定模式，所以无论
    /// 用户怎么自定义 dir_fmt（含 `{order} - ` 前缀、含自定义分隔符、含作者信息等）
    /// 都能识别归档所属的章节。
    ///
    /// 文件名同时支持两种形态：
    ///   - 新归档：`<chapter_dir_name>__<chapter_id>.zip` —— 先按 `__` 切分尾部解析
    ///     chapter_id，并校验剩余 stem 与预期章节目录名相等。
    ///   - 旧归档：`<chapter_dir_name>.zip` —— 直接比较 file_stem 与预期目录名。
    ///
    /// 全部失败返回 None，调用方按找不到 chapter_id 处理（warn 后跳过该归档）。
    fn match_chapter_id_from_archive_name(
        &self,
        archive_path: &Path,
        dir_fmt: &str,
    ) -> Option<i64> {
        let file_name = archive_path.file_name()?.to_str()?;
        let stem = archive_path.file_stem()?.to_str()?;
        if stem.is_empty() {
            return None;
        }

        let author = self.author.join(", ");

        // 形态 1：`<chapter_dir_name>__<chapter_id>.<ext>`
        //         （新归档约定；直接解析尾部 id 并校验 stem）
        if let Some(idx) = file_name.rfind("__") {
            let after = &file_name[idx + 2..];
            let id_part = after.split('.').next().unwrap_or("");
            if let Ok(parsed_id) = id_part.parse::<i64>() {
                let base_stem = &stem[..idx];
                // 在 chapter_infos 里找 parsed_id 对应的章节，
                // 同时校验其预期目录名 == base_stem（避免其他章节误中同名后缀）
                if let Some(chapter) = self
                    .chapter_infos
                    .iter()
                    .find(|c| c.chapter_id == parsed_id)
                {
                    let expected =
                        match Self::expected_chapter_dir_name(dir_fmt, &author, self, chapter) {
                            Ok(name) => name,
                            Err(err) => {
                                tracing::warn!(
                                    "按 dir_fmt 渲染章节`{}`的目录名失败：{}",
                                    chapter.chapter_id,
                                    err.to_message()
                                );
                                return Some(parsed_id);
                            }
                        };
                    if expected == base_stem {
                        return Some(parsed_id);
                    }
                    // stem 不匹配，回退到整体 file_stem 继续尝试（形态 2）
                }
            }
        }

        // 形态 2：`<chapter_dir_name>.zip` 或已被剥掉 `__<id>` 的 base_stem
        for chapter in &self.chapter_infos {
            let expected = match Self::expected_chapter_dir_name(dir_fmt, &author, self, chapter)
            {
                Ok(name) => name,
                Err(_) => continue,
            };
            if expected == stem {
                return Some(chapter.chapter_id);
            }
        }

        None
    }

    /// 用 dir_fmt 给定章节参数渲出「预期章节目录名」。失败时不 panic，仅记录日志后
    /// 让上层继续后续匹配策略。
    fn expected_chapter_dir_name(
        dir_fmt: &str,
        author: &str,
        comic: &Comic,
        chapter: &ChapterInfo,
    ) -> eyre::Result<String> {
        let params = DirFmtParams {
            comic_id: comic.id,
            comic_title: comic.name.clone(),
            author: author.to_string(),
            chapter_id: chapter.chapter_id,
            chapter_title: chapter.chapter_title.clone(),
            order: chapter.order,
        };
        ChapterInfo::compute_chapter_dir_name(dir_fmt, &params)
    }
}

/// 从章节归档（.zip / .cbz）中读取 `章节元数据.json` 的 `chapterId` 字段。
/// 失败时返回错误，调用方应当记日志并跳过该归档（不视为致命错误）。
///
/// 归档内的目录结构取决于打包方式：当前 pack_dir_as_archive 会把所有文件以
/// `<chapter_dir_name>/<filename>` 形式存放，因此 `章节元数据.json` 通常位于
/// 归档的第一级子目录下下。这里用 `zip::ZipArchive::file_names` 找到以
/// `章节元数据.json` 结尾的条目，再按名称打开，确保对打包路径变化保持兼容。
fn read_chapter_id_from_archive(archive_path: &Path) -> eyre::Result<i64> {
    use std::io::Read;

    let file = std::fs::File::open(archive_path)
        .wrap_err(format!("打开`{}`失败", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .wrap_err(format!("`{}`不是有效的 zip 归档", archive_path.display()))?;

    let metadata_entry_name = archive
        .file_names()
        .find(|name| name.ends_with("章节元数据.json"))
        .ok_or_else(|| {
            eyre::eyre!("`{}`中没有以`章节元数据.json`结尾的条目", archive_path.display())
        })?
        .to_string();

    let mut metadata_file = archive
        .by_name(&metadata_entry_name)
        .wrap_err(format!(
            "`{}`中的`{}`无法打开",
            archive_path.display(),
            metadata_entry_name
        ))?;
    let mut metadata_str = String::new();
    metadata_file
        .read_to_string(&mut metadata_str)
        .wrap_err(format!(
            "读取`{}`中的`{}`失败",
            archive_path.display(),
            metadata_entry_name
        ))?;

    let chapter_json: serde_json::Value = serde_json::from_str(&metadata_str)
        .wrap_err(format!(
            "将`{}`中的`章节元数据.json`反序列化为serde_json::Value失败",
            archive_path.display()
        ))?;

    let chapter_id = chapter_json
        .get("chapterId")
        .and_then(serde_json::Value::as_i64)
        .ok_or_eyre(format!(
            "`{}`中的`章节元数据.json`没有`chapterId`字段",
            archive_path.display()
        ))?;

    Ok(chapter_id)
}

