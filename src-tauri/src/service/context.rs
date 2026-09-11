use std::path::{Path, PathBuf};

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use serde::Serialize;
use tauri::Manager;

use crate::{
    config::Config,
    downloader::download_manager::DownloadManager,
    export::ComicExportLock,
    jm_client::JmClient,
    utils::DownloadedComicsIndex,
};

/// 应用运行时需要的关键路径。
///
/// 替代代码里散落的 `app.path().app_data_dir()` / `app.path().app_config_dir()`
/// 等调用。HTTP 适配器下直接用挂载卷路径构造，避免引入 Tauri 的路径解析器。
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// 配置文件 / Cookies / 缓存的根目录
    pub data_dir: PathBuf,
    /// 配置文件（config.json）所在目录
    pub config_dir: PathBuf,
    /// 日志目录
    pub logs_dir: PathBuf,
}

impl AppPaths {
    /// 桌面端用的构造器：从 tauri 的 `app_data_dir` 派生所有子目录。
    pub fn from_app_handle(app: &tauri::AppHandle) -> eyre::Result<Self> {
        let data_dir = app.path().app_data_dir()?;
        let config_dir = data_dir.clone();
        let logs_dir = data_dir.join("logs");
        Ok(Self {
            data_dir,
            config_dir,
            logs_dir,
        })
    }
}

/// 跨传输的运行时上下文。
///
/// 现有所有 `fn xxx(app: AppHandle)` 都改成 `fn xxx(ctx: &dyn AppContext)`，
/// 函数体几乎不动（只是把 `app.get_xxx()` 换成 `ctx.xxx()`）。
///
/// 同一个业务函数既能跑在 Tauri 桌面端（`AppHandle` 实现），
/// 也能跑在 HTTP 服务端（`HttpAppContext` 实现，后续步骤添加）。
pub trait AppContext: Sync {
    fn config(&self) -> RwLockReadGuard<'_, Config>;
    fn config_mut(&self) -> RwLockWriteGuard<'_, Config>;
    fn jm_client(&self) -> &JmClient;
    fn download_manager(&self) -> &DownloadManager;
    fn export_lock(&self) -> &ComicExportLock;
    fn downloaded_comics_index(&self) -> &DownloadedComicsIndex;
    fn paths(&self) -> &AppPaths;

    /// 替代 `Event::emit(&app)` 的统一事件发送。
    ///
    /// 桌面端：转发给 tauri 的 `Emitter::emit`，所有 webview 收到事件。
    /// HTTP 端：推入 broadcast channel，SSE handler 转发给浏览器。
    ///
    /// 不返回 Result：错误仅 log warn，调用方可以 fire-and-forget。
    ///
    /// 注意：因为是泛型方法，这里加了 `where Self: Sized`，
    /// 也就是说 `&dyn AppContext` 上不能调用，必须拿到具体类型
    /// （`AppHandle` 或未来的 `HttpAppContext`）才能 dispatch。
    /// 实际上事件都是从持有具体 ctx 的地方（commands / 后台任务）发出的，
    /// 领域层方法不会 dispatch，所以这个限制不痛。
    fn dispatch<E: Serialize + Clone>(&self, event_name: &str, payload: E)
    where
        Self: Sized;

    /// 替代 `app.opener().open_path(...)`：用系统默认应用打开文件/目录。
    ///
    /// 桌面端：调用 tauri-plugin-opener 的 `open_path`。
    /// HTTP 端：通常无意义，留给实现方决定（公开 zip 流 / 返回 501）。
    fn open_path(&self, path: &Path) -> eyre::Result<()>;

    /// 替代 `app.opener().reveal_item_in_dir(...)`：在文件管理器里高亮某项。
    ///
    /// 桌面端：调用 tauri-plugin-opener 的 `reveal_item_in_dir`。
    /// HTTP 端：同上。
    fn reveal_item_in_dir(&self, path: &Path) -> eyre::Result<()>;
}

/// 桌面端实现：完全保留现有行为，所有方法都是 tauri's Manager 的一层薄包装。
impl AppContext for tauri::AppHandle {
    fn config(&self) -> RwLockReadGuard<'_, Config> {
        // 通过 inner() 显式取 &RwLock<Config>，让 read() 拿到 &self 的生命周期，
        // 避免链式调用产生的临时值 borrow 问题。
        self.state::<RwLock<Config>>().inner().read()
    }
    fn config_mut(&self) -> RwLockWriteGuard<'_, Config> {
        self.state::<RwLock<Config>>().inner().write()
    }
    fn jm_client(&self) -> &JmClient {
        self.state::<JmClient>().inner()
    }
    fn download_manager(&self) -> &DownloadManager {
        self.state::<DownloadManager>().inner()
    }
    fn export_lock(&self) -> &ComicExportLock {
        self.state::<ComicExportLock>().inner()
    }
    fn downloaded_comics_index(&self) -> &DownloadedComicsIndex {
        self.state::<DownloadedComicsIndex>().inner()
    }
    fn paths(&self) -> &AppPaths {
        self.state::<AppPaths>().inner()
    }

    fn dispatch<E: Serialize + Clone>(&self, event_name: &str, payload: E) {
        // tauri 2 的 emit 是 `tauri::Emitter` trait（带运行时泛型）的方法，
        // 完全限定避免和我们自己的 dispatch 方法名冲突。
        if let Err(err) =
            <Self as tauri::Emitter<tauri::Wry>>::emit(self, event_name, payload)
        {
            tracing::warn!("emit {event_name} 失败: {err}");
        }
    }

    fn open_path(&self, path: &Path) -> eyre::Result<()> {
        use tauri_plugin_opener::OpenerExt;
        self.opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|err| eyre::eyre!("打开路径失败: {err}"))
    }

    fn reveal_item_in_dir(&self, path: &Path) -> eyre::Result<()> {
        use tauri_plugin_opener::OpenerExt;
        self.opener()
            .reveal_item_in_dir(path.to_string_lossy().into_owned())
            .map_err(|err| eyre::eyre!("在文件管理器中显示失败: {err}"))
    }
}
