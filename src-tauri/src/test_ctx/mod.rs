//! 测试用的占位 `AppContext` 实现。
//!
//! 给 `JmClient::new_for_test()` / `DownloadManager::new_for_test()` 这类需要
//! 一个 `Arc<dyn AppContext>` 但不想拉起 Tauri runtime 的场景用。
//!
//! 不能真的下载 / dispatch，方法里只返回默认值；运行测试时只需要类型对得上。

use std::path::{Path, PathBuf};
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{
    config::Config,
    downloader::download_manager::DownloadManager,
    export::ComicExportLock,
    jm_client::JmClient,
    service::{AppContext, AppPaths},
    utils::DownloadedComicsIndex,
};

pub struct TestCtx {
    config: RwLock<Config>,
}

impl TestCtx {
    /// 给一个临时 data_dir 的 Default 实现 — 用 env::temp_dir() 派生避免依赖具体路径。
    pub fn new_for_test() -> Self {
        let tmp = std::env::temp_dir().join("jm_test_ctx_default");
        let _ = std::fs::create_dir_all(&tmp);
        Self::with_data_dir(tmp)
    }

    pub fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            config: RwLock::new(Config::default(&data_dir)),
        }
    }
}

impl Default for TestCtx {
    fn default() -> Self {
        Self::new_for_test()
    }
}

impl AppContext for TestCtx {
    fn config(&self) -> RwLockReadGuard<'_, Config> {
        self.config.read()
    }
    fn config_mut(&self) -> RwLockWriteGuard<'_, Config> {
        self.config.write()
    }
    fn jm_client(&self) -> &JmClient {
        unimplemented!("TestCtx::jm_client() — test mocks that need this should provide their own ctx")
    }
    fn download_manager(&self) -> &DownloadManager {
        unimplemented!("TestCtx::download_manager() — same as above")
    }
    fn export_lock(&self) -> &ComicExportLock {
        unimplemented!("TestCtx::export_lock()")
    }
    fn downloaded_comics_index(&self) -> &DownloadedComicsIndex {
        unimplemented!("TestCtx::downloaded_comics_index()")
    }
    fn paths(&self) -> &AppPaths {
        unimplemented!("TestCtx::paths()")
    }
    fn dispatch(&self, _event_name: &str, _payload: serde_json::Value) {}
    fn open_path(&self, _path: &Path) -> eyre::Result<()> {
        Ok(())
    }
    fn reveal_item_in_dir(&self, _path: &Path) -> eyre::Result<()> {
        Ok(())
    }
}
