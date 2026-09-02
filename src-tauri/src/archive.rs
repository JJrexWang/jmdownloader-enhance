//! 章节归档工具：将章节目录打包为 `.zip` / `.cbz` 压缩包。
//!
//! 用于：
//! - 下载完成后直接把章节目录打包，减少磁盘上的小文件数量；
//! - 后续在导出 PDF / CBZ 时复用，避免再次打包。

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use eyre::WrapErr;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::config::ChapterArchiveFormat;

/// `.cbz` 与 `.zip` 的差异仅在扩展名：cbz 本质就是 zip，
/// 只是 ComicBook 社区约定的扩展名，方便阅读器识别。
fn extension_for(format: ChapterArchiveFormat) -> &'static str {
    match format {
        ChapterArchiveFormat::Zip => "zip",
        ChapterArchiveFormat::Cbz => "cbz",
        ChapterArchiveFormat::None => "",
    }
}

/// 把 `src_dir` 下的所有文件打包成 `dest_path`（zip / cbz）。
///
/// - 打包过程中会保留 `src_dir` 的目录结构（以 `src_dir` 的目录名为根）。
/// - `章节元数据.json` 与图片等文件会被一同打包。
/// - 打包完成后不会删除 `src_dir`，由调用方决定。
pub fn pack_dir_as_archive(
    src_dir: &Path,
    dest_path: &Path,
    format: ChapterArchiveFormat,
) -> eyre::Result<()> {
    if matches!(format, ChapterArchiveFormat::None) {
        return Err(eyre::eyre!("`ChapterArchiveFormat::None` 不应触发打包"));
    }

    let zip_file = File::create(dest_path)
        .wrap_err(format!("创建文件`{}`失败", dest_path.display()))?;
    let mut zip_writer = ZipWriter::new(zip_file);

    let options = SimpleFileOptions::default();

    let src_dir_name = src_dir
        .file_name()
        .ok_or_else(|| eyre::eyre!("`{}`没有目录名", src_dir.display()))?
        .to_string_lossy()
        .into_owned();

    // 先写一个条目占位，避免空目录
    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        let entry_path = entry.path();
        let Some(relative_path) = entry_path.strip_prefix(src_dir).ok() else {
            continue;
        };
        if relative_path.as_os_str().is_empty() {
            continue;
        }

        let mut archive_path = PathBuf::from(&src_dir_name);
        archive_path.push(relative_path);

        let archive_path_str = archive_path
            .to_str()
            .ok_or_else(|| eyre::eyre!("归档路径`{}`不是合法 UTF-8", archive_path.display()))?
            .to_string();

        if entry.file_type().is_dir() {
            // 确保空目录也能被记录
            zip_writer
                .add_directory(format!("{archive_path_str}/"), options)
                .wrap_err(format!(
                    "在`{}`中创建目录条目`{archive_path_str}`失败",
                    dest_path.display()
                ))?;
            continue;
        }

        zip_writer
            .start_file(&archive_path_str, options)
            .wrap_err(format!(
                "在`{}`中创建文件条目`{archive_path_str}`失败",
                dest_path.display()
            ))?;

        let mut file = File::open(entry_path)
            .wrap_err(format!("打开`{}`失败", entry_path.display()))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .wrap_err(format!("读取`{}`失败", entry_path.display()))?;
        zip_writer
            .write_all(&buffer)
            .wrap_err(format!("写入`{}`失败", entry_path.display()))?;
    }

    zip_writer
        .finish()
        .wrap_err(format!("关闭`{}`失败", dest_path.display()))?;

    Ok(())
}

/// 给定章节目录路径、章节 ID 与归档格式，返回归档文件应当写入的路径。
///
/// 文件名采用 `<dir_name>__<chapter_id>.<ext>` 这种约定：在原目录名后面追加一个
/// `__<chapter_id>` 后缀。这样即便压缩包内部没有 `章节元数据.json`（例如旧版本
/// 打包流程遗漏的情况），`update_chapter_infos_fields` 的文件名回退逻辑也能
/// 直接从后缀里拿到 chapter_id，避免依赖目录名格式的脆弱匹配。
pub fn chapter_archive_path(
    chapter_download_dir: &Path,
    chapter_id: i64,
    format: ChapterArchiveFormat,
) -> eyre::Result<PathBuf> {
    let dir_name = chapter_download_dir
        .file_name()
        .ok_or_else(|| eyre::eyre!("`{}`没有目录名", chapter_download_dir.display()))?
        .to_string_lossy()
        .into_owned();
    let parent = chapter_download_dir
        .parent()
        .ok_or_else(|| eyre::eyre!("`{}`没有父目录", chapter_download_dir.display()))?;
    Ok(parent.join(format!("{dir_name}__{chapter_id}.{}", extension_for(format))))
}
