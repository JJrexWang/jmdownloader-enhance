use serde::{Deserialize, Serialize};
use specta::Type;

/// 搜索排序方式。
///
/// HTTP API 接受两种写法:
/// - PascalCase: `Latest` / `View` / `Picture` / `Like`
/// - 小写 shorthand (JM 站点自身用的查询串): `mr` / `mv` / `mp` / `tf`
///
/// `serde(alias)` 兼容前端两种写法。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum SearchSort {
    #[serde(alias = "mr", alias = "Latest")]
    Latest,
    #[serde(alias = "mv", alias = "View")]
    View,
    #[serde(alias = "mp", alias = "Picture")]
    Picture,
    #[serde(alias = "tf", alias = "Like")]
    Like,
}

impl SearchSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            SearchSort::Latest => "mr",
            SearchSort::View => "mv",
            SearchSort::Picture => "mp",
            SearchSort::Like => "tf",
        }
    }
}
