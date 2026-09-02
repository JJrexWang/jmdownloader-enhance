mod cbz;
mod pdf;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

pub use cbz::{cbz, cbz_chapters};
use eyre::WrapErr;
use parking_lot::Mutex;
pub use pdf::{pdf, pdf_chapters};
use tracing::instrument;

use crate::{extensions::PathIsImg, types::ChapterInfo};

/// 导出互斥锁管理器，确保同一漫画的导出操作串行执行
#[derive(Debug, Clone, Default)]
pub struct ComicExportLock {
    /// 正在导出的漫画 ID 集合
    locked_comic_ids: Arc<Mutex<HashSet<i64>>>,
}

impl ComicExportLock {
    pub fn new() -> Self {
        Self {
            locked_comic_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// 尝试获取漫画导出锁，返回是否成功
    pub fn try_acquire(&self, comic_id: i64) -> bool {
        let mut locked = self.locked_comic_ids.lock();
        if locked.contains(&comic_id) {
            return false;
        }
        locked.insert(comic_id);
        true
    }

    /// 释放漫画导出锁
    pub fn release(&self, comic_id: i64) {
        self.locked_comic_ids.lock().remove(&comic_id);
    }
}

pub struct ComicExportLockGuard {
    lock: ComicExportLock,
    comic_id: i64,
}

impl Drop for ComicExportLockGuard {
    fn drop(&mut self) {
        self.lock.release(self.comic_id);
    }
}

/// 导出格式
pub enum ExportFormat {
    Pdf,
    Cbz,
}

impl ExportFormat {
    pub fn extension(&self) -> &str {
        match self {
            ExportFormat::Pdf => "pdf",
            ExportFormat::Cbz => "cbz",
        }
    }
}

/// 获取已下载的章节
fn get_downloaded_chapters(chapter_infos: &[ChapterInfo]) -> Vec<ChapterInfo> {
    chapter_infos
        .iter()
        .filter(|chapter_info| chapter_info.is_downloaded.unwrap_or(false))
        .cloned()
        .collect()
}

/// 根据章节 ID 列表获取已下载的章节
fn get_downloaded_chapters_by_ids(
    chapter_infos: &[ChapterInfo],
    chapter_ids: &[i64],
) -> Vec<ChapterInfo> {
    let chapter_id_set: HashSet<_> = chapter_ids.iter().copied().collect();
    chapter_infos
        .iter()
        .filter(|chapter_info| {
            chapter_info.is_downloaded.unwrap_or(false)
                && chapter_id_set.contains(&chapter_info.chapter_id)
        })
        .cloned()
        .collect()
}

#[instrument(
    level = "error",
    skip_all,
    fields(images_dir = %images_dir.display(), must_be_common_img = must_be_common_img)
)]
fn get_image_paths(images_dir: &Path, must_be_common_img: bool) -> eyre::Result<Vec<PathBuf>> {
    let mut image_paths: Vec<PathBuf> = std::fs::read_dir(images_dir)
        .wrap_err(format!("读取目录`{}`失败", images_dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            if must_be_common_img {
                path.is_common_img()
            } else {
                path.is_img()
            }
        })
        .collect();
    image_paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(image_paths)
}

/// 章节存储形式：章节目录（默认）或章节归档（`.zip`/`.cbz`）。
fn chapter_archive_ext(chapter_download_dir: &Path) -> Option<String> {
    let ext = chapter_download_dir
        .extension()
        .and_then(|s| s.to_str())?
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "zip" | "cbz") {
        Some(ext)
    } else {
        None
    }
}

/// 为导出 PDF 而把章节归档里的图片解压到临时目录。返回 (临时目录路径, 临时目录中的图片路径列表)。
/// 调用方在使用完毕后负责删除临时目录。
fn extract_archive_to_temp(
    chapter_download_dir: &Path,
    must_be_common_img: bool,
) -> eyre::Result<(PathBuf, Vec<PathBuf>)> {
    use std::io::Read;
    use zip::ZipArchive;

    let temp_dir = std::env::temp_dir().join(format!(
        "jmcomic-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&temp_dir).wrap_err(format!(
        "创建临时目录`{}`失败",
        temp_dir.display()
    ))?;

    let file = std::fs::File::open(chapter_download_dir)
        .wrap_err(format!("打开`{}`失败", chapter_download_dir.display()))?;
    let mut archive = ZipArchive::new(file).wrap_err(format!(
        "`{}`不是有效的 zip 归档",
        chapter_download_dir.display()
    ))?;

    let mut image_paths = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).wrap_err(format!(
            "读取`{}`的第`{i}`个条目失败",
            chapter_download_dir.display()
        ))?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        // 归档内的目录结构形如 `<chapter_dir_name>/001.jpg`，
        // 我们只关心文件名部分。
        let file_name = name
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_default();
        let candidate = temp_dir.join(&file_name);

        let keep = if must_be_common_img {
            candidate.is_common_img()
        } else {
            candidate.is_img()
        };
        if !keep {
            continue;
        }

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .wrap_err(format!("读取`{}`中的`{}`失败", chapter_download_dir.display(), name.display()))?;
        std::fs::write(&candidate, &buf).wrap_err(format!(
            "写入`{}`失败",
            candidate.display()
        ))?;
        image_paths.push(candidate);
    }
    image_paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok((temp_dir, image_paths))
}

/// 包装一层：如果章节是归档，先解压到临时目录再调用 get_image_paths，
/// 并返回清理函数；如果是目录则直接调用 get_image_paths，清理函数为空操作。
fn get_image_paths_with_archive_support(
    chapter_download_dir: &Path,
    must_be_common_img: bool,
) -> eyre::Result<(Vec<PathBuf>, Option<PathBuf>)> {
    if chapter_archive_ext(chapter_download_dir).is_some() {
        let (temp_dir, paths) = extract_archive_to_temp(chapter_download_dir, must_be_common_img)?;
        Ok((paths, Some(temp_dir)))
    } else {
        let paths = get_image_paths(chapter_download_dir, must_be_common_img)?;
        Ok((paths, None))
    }
}
