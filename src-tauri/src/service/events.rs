// 事件名字符串常量。
//
// 之前这些事件名是 `tauri_specta::Event` derive 自动生成的；
// 抽到 `AppContext::emit` 之后，需要在这里显式维护一份名字表。
//
// 这些名字就是当前 Tauri 版本里前端 `listen()` 用的名字。
// 改名前务必同步前端代码。

pub const DOWNLOAD: &str = "downloadEvent";
pub const DOWNLOAD_ALL_FAVORITES: &str = "downloadAllFavoritesEvent";
pub const UPDATE_DOWNLOADED_COMICS: &str = "updateDownloadedComicsEvent";
pub const EXPORT_CBZ: &str = "exportCbzEvent";
pub const EXPORT_PDF: &str = "exportPdfEvent";
pub const LOG: &str = "logEvent";
