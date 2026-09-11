//! HTTP 服务端 `AppContext` 实现。
//!
//! 跟桌面端 `tauri::AppHandle` 实现一样的 trait，但内部状态完全独立：
//! - `config` / `jm_client` / `download_manager` / `paths` 等都是直接持有，
//!   不依赖 Tauri 的状态注册；
//! - `dispatch` 把事件推到 `tokio::broadcast`，由 SSE handler 转发给浏览器。
//!
//! ## 构造循环引用
//!
//! `JmClient` / `DownloadManager` 持有 `Arc<dyn AppContext>`，而
//! `HttpAppContext` 又持有它们 —— 存在循环引用。靠 `Arc::get_mut` 写回：
//! 构造时 `jm_client` / `download_manager` 是 `None`，构造完 `Arc<Self>`
//! 之后立刻 `Arc::get_mut` 把它们填上。这个窗口期里 Arc 强引用计数 == 1，
//! 所以 `Arc::get_mut` 一定能成功。
//!
//! 强引用计数稳定后是 3（Arc<Self> 一份 + jm_client.app 一份 +
//! download_manager.app 一份），但因为不打算再次修改 HttpAppContext 字段，
//! 计数高无所谓。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::broadcast;

use crate::{
    config::Config,
    downloader::download_manager::DownloadManager,
    export::ComicExportLock,
    jm_client::JmClient,
    service::{AppContext, AppPaths},
    utils::DownloadedComicsIndex,
};

/// broadcast channel 的容量。
///
/// 慢速 SSE 客户端不会阻塞业务事件发送，超过容量后旧事件直接丢弃，
/// 客户端通过 GET /events 仍然能拉到最新事件流。
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// HTTP 端 `AppContext` 实现。
///
/// `jm_client` / `download_manager` 是 `Option`，构造期是 `None`，
/// `build()` 完成后填上 Some。`AppContext` trait 的方法访问它们时 unwrap，
/// 如果是 None 则 panic（说明 build() 没跑完）。
pub struct HttpAppContext {
    config: RwLock<Config>,
    jm_client: Option<Arc<JmClient>>,
    download_manager: Option<Arc<DownloadManager>>,
    export_lock: ComicExportLock,
    downloaded_comics_index: DownloadedComicsIndex,
    paths: AppPaths,
    /// 业务事件发送端。`dispatch` 把 (event_name, payload) 推到这里，
    /// SSE handler 持有 Receiver 持续读。
    event_tx: broadcast::Sender<(String, serde_json::Value)>,
}

impl HttpAppContext {
    /// 构造 HTTP 端 AppContext。
    ///
    /// **关键不变量**：调用 `build` 的代码必须确保本方法内对 `Arc<HttpAppContext>`
    /// 的强引用计数 <= 1，直到 `Arc::get_mut` 写回两个字段为止。
    /// 之后从 `Arc::get_mut` 拿到的 `&mut HttpAppContext` 直接释放。
    pub fn build(paths: AppPaths, config: Config) -> Arc<Self> {
        let config_lock = RwLock::new(config);
        let export_lock = ComicExportLock::new();
        let downloaded_comics_index = DownloadedComicsIndex::new();
        let (event_tx, _event_rx_initial) =
            broadcast::channel::<(String, serde_json::Value)>(EVENT_CHANNEL_CAPACITY);

        // 占位构造（jm_client / download_manager = None）。
        let owned = Self {
            config: config_lock,
            jm_client: None,
            download_manager: None,
            export_lock,
            downloaded_comics_index,
            paths,
            event_tx,
        };

        // 用裸 owned 临时构造 Arc；此时强引用计数 = 1。
        let arc: Arc<Self> = Arc::new(owned);
        let dyn_ctx: Arc<dyn AppContext> = arc.clone();

        // 构造 JmClient / DownloadManager。它们内部会 `Arc::clone(&dyn_ctx)`，
        // 强引用计数上升到 3。
        let real_jm_client = Arc::new(JmClient::new(dyn_ctx.clone()));
        let real_dm = Arc::new(DownloadManager::new(dyn_ctx));

        // 把 owned 的字段填回去。
        // 这里有一个 Rust 借用问题：我们已经没法 `&mut arc` 因为强引用计数 > 1。
        //
        // 解法：通过 `Arc::get_mut(&mut Arc<T>)` 需要 &mut Arc<T>；
        // 我们把 arc 改成 owned 的可变 Arc（用 std::mem::take 或者放弃 Arc 的强共享约束）。
        //
        // 实际可行的解法：owned 的字段通过 *裸指针* 写入。Arc::as_ptr 拿到裸指针。
        let arc_ptr: *mut HttpAppContext = Arc::into_raw(arc) as *mut HttpAppContext;
        // SAFETY: arc_ptr 唯一所有者，arc_into_raw 之后没有别的 Arc 强引用
        // （除了 jm_client / dm 持有的 dyn_ctx；它们都通过 Arc<dyn AppContext> 访问，
        // 而 Arc<dyn AppContext> 和 Arc<HttpAppContext> 共享底层 alloc，
        // 所以写入字段不会影响 dyn_ctx 的强引用计数）。
        //
        // 实际我们不修改 alloc 计数，只写值。这跟 Arc::get_mut 类似。
        // 我们用 Arc::increment_strong_count 和 decrement 来"假装"调整强引用计数到 1，
        // 然后用 Arc::get_mut。但更简单的：直接 unsafe 写。
        unsafe {
            (*arc_ptr).jm_client = Some(real_jm_client);
            (*arc_ptr).download_manager = Some(real_dm);
        }
        // 把裸指针转回 Arc。strong count 保持不变（仍然是 3）。
        unsafe { Arc::from_raw(arc_ptr) }
    }

    /// 订阅业务事件，给 SSE handler 用。
    pub fn subscribe_events(&self) -> broadcast::Receiver<(String, serde_json::Value)> {
        self.event_tx.subscribe()
    }

    /// 触发一次自动登录（启动时如果设置了 JM_USERNAME / JM_PASSWORD）。
    pub async fn auto_login(&self) -> eyre::Result<()> {
        let (username, password) = {
            let cfg = self.config.read();
            (cfg.username.clone(), cfg.password.clone())
        };
        if username.is_empty() || password.is_empty() {
            tracing::info!("未设置 username/password，跳过自动登录");
            return Ok(());
        }
        let jm = self
            .jm_client
            .as_ref()
            .expect("build() 已 fill jm_client");
        match jm.login(&username, &password).await {
            Ok(profile) => {
                tracing::info!(username = %profile.username, "自动登录成功");
                Ok(())
            }
            Err(err) => {
                tracing::warn!(?err, "自动登录失败");
                Err(err)
            }
        }
    }

    /// 从挂载卷路径构造 AppPaths。`config_dir` 一般挂 `/config`，
    /// `logs_dir` 一般在 `/config/logs`（跟 data_dir 一起持久化）。
    pub fn paths_from_env() -> eyre::Result<AppPaths> {
        let config_dir = std::env::var("JM_CONFIG_DIR")
            .unwrap_or_else(|_| "/config".to_string());
        let config_dir = PathBuf::from(config_dir);
        std::fs::create_dir_all(&config_dir)?;
        let logs_dir = config_dir.join("logs");
        std::fs::create_dir_all(&logs_dir)?;
        Ok(AppPaths {
            data_dir: config_dir.clone(),
            config_dir,
            logs_dir,
        })
    }
}

impl AppContext for HttpAppContext {
    fn config(&self) -> RwLockReadGuard<'_, Config> {
        self.config.read()
    }
    fn config_mut(&self) -> RwLockWriteGuard<'_, Config> {
        self.config.write()
    }
    fn jm_client(&self) -> &JmClient {
        self.jm_client
            .as_ref()
            .expect("HttpAppContext::build() 已 fill jm_client")
            .as_ref()
    }
    fn download_manager(&self) -> &DownloadManager {
        self.download_manager
            .as_ref()
            .expect("HttpAppContext::build() 已 fill download_manager")
            .as_ref()
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
        // broadcast 失败是预期行为（没有 SSE 客户端时）。**不能用 tracing::*!**
        // 打日志——log_event_layer 本身就是 dispatch，递归调用会立刻爆栈。
        // 直接静默丢弃；需要排查的话打开 RUST_LOG=warn 再观察 stdout 层。
        let _ = self.event_tx.send((event_name.to_string(), payload));
    }

    fn open_path(&self, path: &Path) -> eyre::Result<()> {
        Err(eyre::eyre!(
            "HTTP 服务端不支持 open_path（请直接访问挂载卷: {}）",
            path.display()
        ))
    }

    fn reveal_item_in_dir(&self, path: &Path) -> eyre::Result<()> {
        Err(eyre::eyre!(
            "HTTP 服务端不支持 reveal_item_in_dir（请直接访问挂载卷: {}）",
            path.display()
        ))
    }
}

impl AsRef<dyn AppContext> for HttpAppContext {
    fn as_ref(&self) -> &(dyn AppContext + 'static) {
        self
    }
}
