// 事件名字符串常量。
//
// 之前这些事件名是 `tauri_specta::Event` derive 自动生成的；
// 抽到 `AppContext::emit` 之后，需要在这里显式维护一份名字表。
//
// 这些名字就是当前 Tauri 版本里前端 `listen()` 用的名字。
// 改名前务必同步前端代码。

pub const DOWNLOAD: &str = "download-event";
pub const DOWNLOAD_ALL_FAVORITES: &str = "download-all-favorites-event";
pub const UPDATE_DOWNLOADED_COMICS: &str = "update-downloaded-comics-event";
pub const EXPORT_CBZ: &str = "export-cbz-event";
pub const EXPORT_PDF: &str = "export-pdf-event";
pub const LOG: &str = "log-event";
