# 更新日志

所有版本变更与本 fork 专属的优化都在此记录。GitHub Release 自动生成的 commit 列表是另一份详细记录。

## [0.18.0-enhance.11] - 2026-09-04

### 优化

- **收藏夹 / 周刊页「已下载徽标」开关**：在「设置 → 下载设置 → 性能」里新增两个 checkbox
  - 收藏页显示已下载徽标
  - 周刊页显示已下载徽标
  - **默认关闭**：本地库存大（≥几百本）时，每次翻页/章节完成会触发 hashmap 匹配 + 前端连锁 sync IPC，阻塞 UI 主线程造成卡顿。徽标信息本来在「本地库存」模块里也有，所以默认关闭换取流畅。需要徽标可手动开启。

### 改动

- `src-tauri/src/config.rs`：新增 `favorite_show_downloaded_badge` / `weekly_show_downloaded_badge`（bool，默认 `false`）
- `src-tauri/src/types/get_favorite_result.rs` 与 `get_weekly_result.rs`：`from_resp_data` / `update_fields` 改为接收 `Option<&HashMap>`；新增 `sync_one()` 帮助方法统一遵守开关
- `src-tauri/src/commands.rs`：`get_synced_comic_in_favorite` / `get_synced_comic_in_weekly` 改为调用 `*Result::sync_one`
- `src/panes/ProgressesPane/ProgressesPane.vue`：watcher 用对应配置门控 `syncComicInFavorite` / `syncComicInWeekly`
- `src/dialogs/SettingsDialog/components/DownloadSettings.vue`：新增「性能」分类与两个 checkbox（含提示 tooltip）
- `src/bindings.ts`：类型同步
- 版本号 `0.18.0-enhance.10` → `0.18.0-enhance.11`

## 自动化变更

- `.github/workflows/Publish.yml`：把 `draft: true` 改成 `draft: false`，CI 跑完直接发布 release，不再需要人工确认
