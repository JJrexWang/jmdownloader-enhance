//! 业务核心服务层抽象。
//!
//! 把现有的 `tauri::AppHandle` 依赖抽成 `AppContext` trait，
//! 让同一套业务逻辑既能跑在 Tauri 桌面运行时，也能跑在
//! HTTP 服务运行时。
//!
//! - `context`：`AppContext` trait + 桌面端 (AppHandle) 实现。
//! - `events`：事件名字符串常量（kebab-case）。
//! - `http`：HTTP 端 `HttpAppContext` 实现。

pub mod context;
pub mod events;
pub mod http;

pub use context::{AppContext, AppPaths};
pub use http::HttpAppContext;
