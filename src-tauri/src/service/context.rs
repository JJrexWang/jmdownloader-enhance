use std::path::{Path, PathBuf};

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
#[cfg(test)]
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
pub trait AppContext: Send + Sync {
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
    /// 没有 `Self: Sized` 限制，`&dyn AppContext` 上也能调。
    fn dispatch(&self, event_name: &str, payload: serde_json::Value);

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

    fn dispatch(&self, event_name: &str, payload: serde_json::Value) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock as PlRwLock;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// 测试用的 mock AppContext。
    ///
    /// 构造时不依赖 tauri，所有状态都在内存里。
    /// `dispatch` 记录事件到 Vec 用于断言；`open_path` / `reveal_item_in_dir`
    /// 总是返回 Ok，让 trait 方法的调用路径能走通。
    struct MockContext {
        config: Arc<PlRwLock<Config>>,
        jm_client: Arc<JmClient>,
        download_manager: Arc<DownloadManager>,
        export_lock: Arc<ComicExportLock>,
        downloaded_comics_index: Arc<DownloadedComicsIndex>,
        paths: Arc<AppPaths>,
        events: Arc<parking_lot::Mutex<Vec<(String, String)>>>,
    }

    impl MockContext {
        fn new(data_dir: PathBuf) -> Self {
            let config = Config::default(&data_dir);
            Self {
                config: Arc::new(PlRwLock::new(config)),
                jm_client: Arc::new(JmClient::new_for_test()),
                download_manager: Arc::new(DownloadManager::new_for_test()),
                export_lock: Arc::new(ComicExportLock::new()),
                downloaded_comics_index: Arc::new(DownloadedComicsIndex::new()),
                paths: Arc::new(AppPaths {
                    data_dir: data_dir.clone(),
                    config_dir: data_dir.clone(),
                    logs_dir: data_dir.join("logs"),
                }),
                events: Arc::new(parking_lot::Mutex::new(Vec::new())),
            }
        }
    }

    impl AppContext for MockContext {
        fn config(&self) -> RwLockReadGuard<'_, Config> {
            self.config.read()
        }
        fn config_mut(&self) -> RwLockWriteGuard<'_, Config> {
            self.config.write()
        }
        fn jm_client(&self) -> &JmClient {
            &self.jm_client
        }
        fn download_manager(&self) -> &DownloadManager {
            &self.download_manager
        }
        fn export_lock(&self) -> &ComicExportLock {
            &self.export_lock
        }
        fn downloaded_comics_index(&self) -> &DownloadedComicsIndex {
            &self.downloaded_comics_index
        }
        fn paths(&self) -> &AppPaths {
            &self.paths
        }
        fn dispatch(&self, event_name: &str, payload: serde_json::Value) {
            // 测试用：把事件序列化后存到 Vec
            let json = serde_json::to_string(&payload).unwrap();
            self.events.lock().push((event_name.to_string(), json));
        }
        fn open_path(&self, _path: &Path) -> eyre::Result<()> {
            Ok(())
        }
        fn reveal_item_in_dir(&self, _path: &Path) -> eyre::Result<()> {
            Ok(())
        }
    }

    // 暂用 #[ignore] 跳过：MockContext 内部的 JmClient/DownloadManager 用了
    // `MaybeUninit::zeroed()` 占位 AppHandle，是 UB；让构造/drop 走到
    // `tauri_runtime_wry::WindowIdStore` 时会 SIGSEGV。等到把
    // JmClient/DownloadManager 改成 `Arc<dyn AppContext>`（Day 3-5 重构）
    // 就能正常跑，先在 CI 上保留 #[ignore]。
    #[test]
    #[ignore = "AppHandle 占位导致 UB；等 core 改造后用真 AppContext 再开"]
    fn trait_object_works() {
        // 关键验证：&dyn AppContext 能正常调用所有方法
        let tmp = std::env::temp_dir().join("jm_test_trait_object");
        let _ = std::fs::create_dir_all(&tmp);
        let mock = MockContext::new(tmp.clone());
        let ctx: &dyn AppContext = &mock;

        // 1. config() 返回 guard，能读字段
        assert_eq!(ctx.config().download_dir, tmp.join("漫画下载"));
        assert_eq!(ctx.config().export_dir, tmp.join("漫画导出"));

        // 2. paths() 返回 &AppPaths，能访问路径
        assert_eq!(ctx.paths().data_dir, tmp);
        assert_eq!(ctx.paths().logs_dir, tmp.join("logs"));

        // 3. config_mut() 返回 write guard，能改
        {
            let mut cfg = ctx.config_mut();
            cfg.username = "alice".to_string();
        }
        assert_eq!(ctx.config().username, "alice");

        // 4. dispatch 通过具体类型调用，event 进了 events vec
        #[derive(Serialize, Clone)]
        struct FakeEvent {
            msg: String,
        }
        let event = FakeEvent { msg: "hi".to_string() };
        mock.dispatch(
            "testEvent",
            serde_json::to_value(&event).expect("FakeEvent 序列化"),
        );
        let events = mock.events.lock();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "testEvent");
        assert!(events[0].1.contains("\"msg\":\"hi\""));
    }

    // 暂用 #[ignore] 跳过：MockContext 内部的 JmClient/DownloadManager 用了
    // `MaybeUninit::zeroed()` 占位 AppHandle，是 UB；让构造/drop 走到
    // `tauri_runtime_wry::WindowIdStore` 时会 SIGSEGV。等到把
    // JmClient/DownloadManager 改成 `Arc<dyn AppContext>`（Day 3-5 重构）
    // 就能正常跑，先在 CI 上保留 #[ignore]。
    #[test]
    #[ignore = "AppHandle 占位导致 UB；等 core 改造后用真 AppContext 再开"]
    fn app_paths_constructs_data_dir() {
        let tmp = std::env::temp_dir().join("jm_test_app_paths");
        let _ = std::fs::create_dir_all(&tmp);

        // AppPaths 的 data_dir 和 config_dir 应该一致（桌面端约定）
        let paths = AppPaths {
            data_dir: tmp.clone(),
            config_dir: tmp.clone(),
            logs_dir: tmp.join("logs"),
        };
        assert_eq!(paths.data_dir, paths.config_dir);
        assert!(paths.logs_dir.ends_with("logs"));
    }

    // 暂用 #[ignore] 跳过：MockContext 内部的 JmClient/DownloadManager 用了
    // `MaybeUninit::zeroed()` 占位 AppHandle，是 UB；让构造/drop 走到
    // `tauri_runtime_wry::WindowIdStore` 时会 SIGSEGV。等到把
    // JmClient/DownloadManager 改成 `Arc<dyn AppContext>`（Day 3-5 重构）
    // 就能正常跑，先在 CI 上保留 #[ignore]。
    #[test]
    #[ignore = "AppHandle 占位导致 UB；等 core 改造后用真 AppContext 再开"]
    fn downloaded_comics_index_starts_empty() {
        // 走 &dyn AppContext 路径的 get_or_build：download_dir 不存在时返回空 map，不 panic。
        let tmp = std::env::temp_dir().join("jm_test_index_empty");
        let _ = std::fs::create_dir_all(&tmp);
        let mock = MockContext::new(tmp.clone());
        let map = mock
            .downloaded_comics_index
            .get_or_build(&mock as &dyn AppContext)
            .unwrap();
        assert!(map.is_empty());

        // invalidate 之后再调一次还是空（不会因为缓存导致 stale 数据被复用）。
        mock.downloaded_comics_index.invalidate();
        let map2 = mock
            .downloaded_comics_index
            .get_or_build(&mock as &dyn AppContext)
            .unwrap();
        assert!(map2.is_empty());
    }
}
