//! 任务中心：持久化的任务状态，供 WebUI "任务中心" 页面查询。
//!
//! 背景：更新库存、下载任务的进度之前只靠 SSE 推送，刷新页面就丢了。
//! 这里用全局状态记录关键进度，WebUI 通过 `GET /task-center` 拉取快照，
//! 刷新页面后重新拉取即可恢复显示（服务端重启后重置，可接受）。

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;

fn now_str() -> String {
    // 秒级 Unix 时间戳的可读形式：YYYY-MM-DD HH:MM:SS（本地时区近似用 UTC+8）
    // 为避免引入 chrono 依赖，这里简单格式化；仅用于展示。
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 转成北京时间（UTC+8）显示
    let secs = secs + 8 * 3600;
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    // days 转日期（简化：从 1970-01-01 算起）
    let (y, mo, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, m, s)
}

fn days_to_ymd(mut days: u64) -> (i32, u32, u32) {
    let mut y = 1970i32;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let yd = if leap { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 1u32;
    for &md in &months {
        if days < md as u64 {
            break;
        }
        days -= md as u64;
        mo += 1;
    }
    (y, mo, days as u32 + 1)
}

/// 更新库存时单本失败的记录。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FailedComicInfo {
    pub comic_id: i64,
    pub comic_title: String,
    pub reason: String,
}

/// "更新库存" 的持久化状态。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInventoryStatus {
    /// idle | running | done
    pub status: String,
    pub total: i64,
    pub current: i64,
    pub current_comic_title: String,
    pub failed: Vec<FailedComicInfo>,
    /// 本轮新建的下载任务数（发现的新章节）。
    pub new_tasks_created: i64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl Default for UpdateInventoryStatus {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            total: 0,
            current: 0,
            current_comic_title: String::new(),
            failed: Vec::new(),
            new_tasks_created: 0,
            started_at: None,
            finished_at: None,
        }
    }
}

/// 单个下载任务的快照（从 DownloadManager 实时读取，不在此持久化）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTaskInfo {
    pub chapter_id: i64,
    pub comic_id: i64,
    pub comic_title: String,
    pub chapter_title: String,
    /// Pending | Downloading | Paused | Completed | Failed
    pub state: String,
    pub downloaded: u32,
    pub total: u32,
}

/// `GET /task-center` 返回的完整快照。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskCenterSnapshot {
    pub update_inventory: UpdateInventoryStatus,
    pub downloads: Vec<DownloadTaskInfo>,
}

#[derive(Debug, Default)]
struct TaskCenterInner {
    update_inventory: UpdateInventoryStatus,
}

/// 全局任务中心状态。
#[derive(Debug)]
pub struct TaskCenter {
    inner: Mutex<TaskCenterInner>,
}

impl TaskCenter {
    fn new() -> Self {
        Self {
            inner: Mutex::new(TaskCenterInner::default()),
        }
    }

    pub fn set_update_start(&self, total: i64) {
        let mut g = self.inner.lock();
        g.update_inventory = UpdateInventoryStatus {
            status: "running".to_string(),
            total,
            current: 0,
            current_comic_title: String::new(),
            failed: Vec::new(),
            new_tasks_created: 0,
            started_at: Some(now_str()),
            finished_at: None,
        };
    }

    pub fn set_update_progress(&self, current: i64, total: i64, title: &str) {
        let mut g = self.inner.lock();
        let st = &mut g.update_inventory;
        st.status = "running".to_string();
        st.current = current;
        st.total = total;
        st.current_comic_title = title.to_string();
        if st.started_at.is_none() {
            st.started_at = Some(now_str());
        }
    }

    pub fn add_update_failed(&self, comic_id: i64, title: &str, reason: &str) {
        let mut g = self.inner.lock();
        g.update_inventory.failed.push(FailedComicInfo {
            comic_id,
            comic_title: title.to_string(),
            reason: reason.chars().take(120).collect(),
        });
    }

    pub fn inc_update_new_tasks(&self, n: i64) {
        self.inner.lock().update_inventory.new_tasks_created += n;
    }

    pub fn set_update_done(&self) {
        let mut g = self.inner.lock();
        let st = &mut g.update_inventory;
        st.status = "done".to_string();
        st.finished_at = Some(now_str());
    }

    pub fn snapshot_update(&self) -> UpdateInventoryStatus {
        self.inner.lock().update_inventory.clone()
    }
}

static TASK_CENTER: OnceLock<TaskCenter> = OnceLock::new();

/// 获取全局任务中心（首次调用时初始化）。
pub fn task_center() -> &'static TaskCenter {
    TASK_CENTER.get_or_init(TaskCenter::new)
}
