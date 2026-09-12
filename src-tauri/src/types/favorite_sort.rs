use serde::{Deserialize, Serialize};
use specta::Type;

/// 收藏夹排序方式。
///
/// HTTP API 接受两种写法:
/// - PascalCase: `FavoriteTime` / `UpdateTime`
/// - 小写 shorthand (JM 站点自身用的查询串): `mr` / `mp`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum FavoriteSort {
    #[serde(alias = "mr", alias = "FavoriteTime")]
    FavoriteTime,
    #[serde(alias = "mp", alias = "UpdateTime")]
    UpdateTime,
}

impl FavoriteSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            FavoriteSort::FavoriteTime => "mr",
            FavoriteSort::UpdateTime => "mp",
        }
    }
}
