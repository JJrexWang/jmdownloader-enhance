// 业务核心的服务层抽象。
//
// 把现有的 `tauri::AppHandle` 依赖抽成 `AppContext` trait，
// 让同一套业务逻辑既能跑在 Tauri 桌面运行时，也能跑在
// 未来的 HTTP 服务运行时。
//
// 当前阶段只定义 trait + 给 `tauri::AppHandle` 提供实现。
// HTTP 适配器（HttpAppContext）将在后续步骤添加。

pub mod context;
pub mod events;

pub use context::{AppContext, AppPaths};
