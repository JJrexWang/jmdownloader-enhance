//! 文本归一化工具：把从 API 拿到的作者名 / 漫画标题 / 章节标题等字符串
//! 在落地到磁盘目录之前做一次简繁中文归一化。
//!
//! 禁漫的 `get_comic(aid)` 接口对同一本漫画会随当前 session 语言偏好返回不同
// 简繁版本的 `name` 和 `author`，直接用这些字段做目录名会让同一个 comic_id
// 在不同时段被落到不同目录里。这里用 OpenCC 按字符级做简繁转换：
//   - CJK Unified Ideographs（汉字）按用户配置的归一化方向转换
//   - 日文假名（U+3040-U+30FF）、韩文 Hangul（U+AC00-U+D7AF）等其它脚本
//     会被 OpenCC 自动跳过，不会被错误连带改写
//!   - 拉丁字母、数字、标点等也不会被改
//!
//! 转换器是 Lazy 初始化的全局单例，避免每次都重新加载字典。

use std::sync::OnceLock;

use ferrous_opencc::{config::BuiltinConfig as FerrousConfig, OpenCC};

use crate::config::ChineseNormalization;

#[derive(PartialEq, Debug)]
enum OpenCCMode {
    ToSimplified,
    ToTraditional,
}

struct OpenCCInstance {
    mode: OpenCCMode,
    opencc: OpenCC,
}

static CONVERTER: OnceLock<std::sync::Mutex<Option<OpenCCInstance>>> = OnceLock::new();

fn converter_slot() -> &'static std::sync::Mutex<Option<OpenCCInstance>> {
    CONVERTER.get_or_init(|| std::sync::Mutex::new(None))
}

/// 按用户配置的归一化模式转换字符串。
///
/// `mode = ChineseNormalization::None` 时直接原样返回（不做任何处理）。
/// `mode = ToSimplified / ToTraditional` 时调用 OpenCC 按字符级转换。
/// 转换器是 Lazy 全局单例，整个进程只初始化一次。
pub fn normalize(s: &str, mode: ChineseNormalization) -> String {
    if matches!(mode, ChineseNormalization::None) {
        return s.to_string();
    }
    let target_mode = match mode {
        ChineseNormalization::None => unreachable!(),
        ChineseNormalization::ToSimplified => OpenCCMode::ToSimplified,
        ChineseNormalization::ToTraditional => OpenCCMode::ToTraditional,
    };
    let slot = converter_slot();
    let mut guard = slot.lock().expect("OpenCC converter mutex poisoned");
    let needs_init = match &*guard {
        Some(inst) => inst.mode != target_mode,
        None => true,
    };
    if needs_init {
        let builtin = match target_mode {
            OpenCCMode::ToSimplified => FerrousConfig::T2s,
            OpenCCMode::ToTraditional => FerrousConfig::S2t,
        };
        // 内置字典在 build 阶段就 embed 进 crate，第一次构造很快
        let opencc = OpenCC::from_config(builtin).expect("OpenCC 创建失败");
        *guard = Some(OpenCCInstance {
            mode: target_mode,
            opencc,
        });
    }
    let inst = guard.as_ref().expect("OpenCC converter 已初始化");
    inst.opencc.convert(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_returns_input_verbatim() {
        assert_eq!(normalize("夫婦挑戰賽", ChineseNormalization::None), "夫婦挑戰賽");
    }

    #[test]
    fn traditional_to_simplified() {
        assert_eq!(
            normalize("夫婦挑戰賽/夫婦遊戲", ChineseNormalization::ToSimplified),
            "夫妇挑战赛/夫妇游戏"
        );
    }

    #[test]
    fn simplified_to_traditional() {
        assert_eq!(
            normalize("夫妇挑战赛/夫妇游戏", ChineseNormalization::ToTraditional),
            "夫婦挑戰賽/夫婦遊戲"
        );
    }

    #[test]
    fn mixed_korean_chinese_keeps_korean() {
        assert_eq!(
            normalize("张三 안녕", ChineseNormalization::ToSimplified),
            "张三 안녕"
        );
        assert_eq!(
            normalize("繁體 한국어 简体", ChineseNormalization::ToSimplified),
            "繁体 한국어 简体"
        );
    }

    #[test]
    fn mixed_japanese_kana_chinese_keeps_kana() {
        assert_eq!(
            normalize("繁體 ひらがな 简体", ChineseNormalization::ToSimplified),
            "繁体 ひらがな 简体"
        );
    }

    #[test]
    fn english_preserved() {
        assert_eq!(
            normalize("English中文 English", ChineseNormalization::ToSimplified),
            "English中文 English"
        );
    }

    #[test]
    fn empty_string() {
        assert_eq!(normalize("", ChineseNormalization::ToSimplified), "");
    }
}
