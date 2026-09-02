use std::path::{Path, PathBuf};

use crate::types::{DownloadFormat, ProxyMode};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

const API_DOMAIN_1: &str = "www.cdnzack.cc";
const API_DOMAIN_2: &str = "www.cdnhth.cc";
const API_DOMAIN_3: &str = "www.cdnhth.net";
const API_DOMAIN_4: &str = "www.cdnbea.net";
const API_DOMAIN_5: &str = "www.cdn-mspjmapiproxy.xyz";

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub username: String,
    pub password: String,
    pub download_dir: PathBuf,
    pub export_dir: PathBuf,
    pub download_format: DownloadFormat,
    pub dir_fmt: String,
    pub proxy_mode: ProxyMode,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub enable_file_logger: bool,
    pub chapter_concurrency: usize,
    pub chapter_download_interval_sec: u64,
    pub img_concurrency: usize,
    pub img_download_interval_sec: u64,
    pub download_all_favorites_interval_sec: u64,
    pub update_downloaded_comics_interval_sec: u64,
    pub api_domain_mode: ApiDomainMode,
    pub custom_api_domain: String,
    pub should_download_cover: bool,
    pub create_pdf_concurrency: usize,
    pub enable_merge_pdf: bool,
    /// 导出跳过模式
    pub export_skip_mode: ExportSkipMode,
    /// 章节归档格式：下载完成后是否将每个章节目录打包为压缩包
    pub chapter_archive_format: ChapterArchiveFormat,
    /// 章节下载缺失图片容忍阈值：当缺失图片数 ≤ 此值时，视为下载成功并降级为告警，
    /// 不会让整章作废。设为 `0` 则保留原行为（只要缺一张就整章失败）。
    pub missing_image_threshold: u32,
    /// 简繁中文归一化：用于消除同一本漫画在不同登录语言下因简繁差异开新目录的问题。
    /// 日文、韩文、英文等其他脚本不会被 OpenCC 错误连带转换。
    pub chinese_normalization: ChineseNormalization,
    /// 是否禁用 ERROR 级日志的 GUI 弹窗通知。
    ///
    /// 启用后，下载/同步等过程中产生的失败不再右下角弹通知，但仍会写入实时日志
    /// 与文件日志，方便事后排查。
    pub disable_error_notifications: bool,
}

impl Config {
    pub fn new(app: &AppHandle) -> eyre::Result<Self> {
        let app_data_dir = app.path().app_data_dir()?;
        let config_path = app_data_dir.join("config.json");

        let config = if config_path.exists() {
            let config_string = std::fs::read_to_string(config_path)?;
            match serde_json::from_str(&config_string) {
                // 如果能够直接解析为Config，则直接返回
                Ok(config) => config,
                // 否则，将默认配置与文件中已有的配置合并
                // 以免新版本添加了新的配置项，用户升级到新版本后，所有配置项都被重置
                Err(_) => Config::merge_config(&config_string, &app_data_dir),
            }
        } else {
            Config::default(&app_data_dir)
        };
        config.save(app)?;
        Ok(config)
    }

    pub fn save(&self, app: &AppHandle) -> eyre::Result<()> {
        let resource_dir = app.path().app_data_dir()?;
        let config_path = resource_dir.join("config.json");
        let config_string = serde_json::to_string_pretty(self)?;
        std::fs::write(config_path, config_string)?;
        Ok(())
    }

    pub fn get_api_domain(&self) -> String {
        match self.api_domain_mode {
            ApiDomainMode::Domain1 => API_DOMAIN_1.to_string(),
            ApiDomainMode::Domain2 => API_DOMAIN_2.to_string(),
            ApiDomainMode::Domain3 => API_DOMAIN_3.to_string(),
            ApiDomainMode::Domain4 => API_DOMAIN_4.to_string(),
            ApiDomainMode::Domain5 => API_DOMAIN_5.to_string(),
            ApiDomainMode::Custom => self.custom_api_domain.clone(),
        }
    }

    fn merge_config(config_string: &str, app_data_dir: &Path) -> Config {
        let Ok(mut json_value) = serde_json::from_str::<serde_json::Value>(config_string) else {
            return Config::default(app_data_dir);
        };
        let serde_json::Value::Object(ref mut map) = json_value else {
            return Config::default(app_data_dir);
        };
        let Ok(default_config_value) = serde_json::to_value(Config::default(app_data_dir)) else {
            return Config::default(app_data_dir);
        };
        let serde_json::Value::Object(default_map) = default_config_value else {
            return Config::default(app_data_dir);
        };
        for (key, value) in default_map {
            map.entry(key).or_insert(value);
        }
        let Ok(config) = serde_json::from_value(json_value) else {
            return Config::default(app_data_dir);
        };
        config
    }

    fn default(app_data_dir: &Path) -> Config {
        let cpu_core_num = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(1);

        Config {
            username: String::new(),
            password: String::new(),
            download_dir: app_data_dir.join("漫画下载"),
            export_dir: app_data_dir.join("漫画导出"),
            download_format: DownloadFormat::default(),
            dir_fmt: "{comic_title}/{chapter_title}".to_string(),
            proxy_mode: ProxyMode::default(),
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
            enable_file_logger: true,
            chapter_concurrency: 3,
            chapter_download_interval_sec: 0,
            img_concurrency: 20,
            img_download_interval_sec: 0,
            download_all_favorites_interval_sec: 0,
            update_downloaded_comics_interval_sec: 0,
            api_domain_mode: ApiDomainMode::Domain2,
            custom_api_domain: API_DOMAIN_2.to_string(),
            should_download_cover: true,
            create_pdf_concurrency: cpu_core_num,
            enable_merge_pdf: true,
            export_skip_mode: ExportSkipMode::default(),
            chapter_archive_format: ChapterArchiveFormat::default(),
            // 默认允许最多 5 张图片缺失（兼容长章节偶发的瞬时失败）
            missing_image_threshold: 5,
            // 默认把繁中转为简中，避免同一本漫画在不同登录语言下生成不同目录
            chinese_normalization: ChineseNormalization::ToSimplified,
            // 默认仍然弹 ERROR 通知；不喜欢打扰的用户可手动关闭
            disable_error_notifications: false,
        }
    }
}

/// 章节归档格式：下载完成后是否将每个章节目录打包为压缩包
#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum ChapterArchiveFormat {
    /// 不打包，保留原样
    #[default]
    None,
    /// 打包为 .zip
    Zip,
    /// 打包为 .cbz
    Cbz,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum ApiDomainMode {
    Domain1,
    #[default]
    Domain2,
    Domain3,
    Domain4,
    Domain5,
    Custom,
}

/// 中文简繁归一化模式：用于把漫画名/作者名/章节名落地到磁盘前做一次简繁转换，
/// 解决同一本漫画因登录语言不同（简中/繁中/日文等）被落到不同目录的问题。
///
/// 选择 None 时不做任何转换；选择 ToSimplified / ToTraditional 时调用 OpenCC
/// 按字符级转换，Hangul（韩文）和日文假名会被自动跳过不会被连带改写。
#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum ChineseNormalization {
    /// 不做任何转换
    None,
    /// 繁体 → 简体（默认；适合大多数大陆用户）
    #[default]
    ToSimplified,
    /// 简体 → 繁体
    ToTraditional,
}

/// 导出跳过模式
#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum ExportSkipMode {
    /// 每次重新导出所有章节
    #[default]
    None,
    /// 跳过本地已存在的导出文件
    SkipExisting,
    /// 跳过曾导出过的章节（即使本地文件已删除）
    SkipExported,
}
