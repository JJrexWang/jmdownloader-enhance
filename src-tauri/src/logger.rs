use std::io::Write;
use std::sync::{Arc, OnceLock};

use eyre::{OptionExt, WrapErr};
use notify::{RecommendedWatcher, Watcher};
use tracing::{instrument, Instrument, Subscriber};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    filter::filter_fn,
    fmt::{format::JsonFields, layer, time::LocalTime, MakeWriter},
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
    EnvFilter, Layer, Registry,
};

use crate::{
    events::LogEvent,
    extensions::EyreReportToMessage,
};

struct LogEventWriter {
    app: Arc<dyn crate::service::AppContext>,
}

impl Write for LogEventWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let json_raw = String::from_utf8_lossy(buf).to_string();
        let _ = crate::events::dispatch_event(self.app.as_ref(), LogEvent { json_raw });
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct LogEventWriterFactory {
    app: Arc<dyn crate::service::AppContext>,
}

impl MakeWriter<'_> for LogEventWriterFactory {
    type Writer = LogEventWriter;

    fn make_writer(&self) -> Self::Writer {
        LogEventWriter {
            app: self.app.clone(),
        }
    }
}

static RELOAD_FN: OnceLock<Box<dyn Fn() -> eyre::Result<()> + Send + Sync>> = OnceLock::new();
static GUARD: OnceLock<parking_lot::Mutex<Option<WorkerGuard>>> = OnceLock::new();

#[instrument(level = "error", skip_all)]
pub fn init(app: Arc<dyn crate::service::AppContext>) -> eyre::Result<()> {
    // 全局 env filter（由 RUST_LOG 控制），未设置时退回到
    // `info,jmcomic_downloader_lib=trace`——既安静又能在出问题时看清本 crate。
    let lib_module_path = module_path!();
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive(
            format!("{lib_module_path}=trace")
                .parse()
                .expect("hardcoded directive parse"),
        );
    // 输出到文件
    let (file_layer, guard) = create_file_layer(app.as_ref())?;
    let (reloadable_file_layer, reload_handle) = tracing_subscriber::reload::Layer::new(file_layer);
    // 输出到控制台
    let console_layer = layer()
        .with_writer(std::io::stdout)
        .with_timer(LocalTime::rfc_3339())
        .with_file(true)
        .with_line_number(true)
        .pretty();
    // 发送到前端
    let log_event_factory = LogEventWriterFactory { app: app.clone() };
    let log_event_layer = layer()
        .with_writer(log_event_factory)
        .with_timer(LocalTime::rfc_3339())
        .with_file(true)
        .with_line_number(true)
        .json()
        // 过滤掉来自这个文件的日志，避免无限递归
        .with_filter(filter_fn(|metadata| {
            metadata.module_path() != Some(lib_module_path)
        }));

    Registry::default()
        .with(env_filter)
        .with(reloadable_file_layer)
        .with(console_layer)
        .with(log_event_layer)
        .with(ErrorLayer::new(JsonFields::default()))
        .init();

    GUARD.get_or_init(|| parking_lot::Mutex::new(guard));
    let app_for_reload = Arc::clone(&app);
    RELOAD_FN.get_or_init(move || {
        Box::new(move || {
            let (file_layer, guard) = create_file_layer(app_for_reload.as_ref())?;
            reload_handle.reload(file_layer).wrap_err("reload失败")?;
            *GUARD.get().ok_or_eyre("GUARD未初始化")?.lock() = guard;
            Ok(())
        })
    });
    tauri::async_runtime::spawn(file_log_watcher(app));

    Ok(())
}

#[instrument(level = "error", skip_all)]
pub fn reload_file_logger() -> eyre::Result<()> {
    RELOAD_FN.get().ok_or_eyre("RELOAD_FN未初始化")?()
}

#[instrument(level = "error", skip_all)]
pub fn disable_file_logger() -> eyre::Result<()> {
    if let Some(guard) = GUARD.get().ok_or_eyre("GUARD未初始化")?.lock().take() {
        drop(guard);
    }
    Ok(())
}

#[instrument(level = "error", skip_all)]
fn create_file_layer<S>(
    app: &dyn crate::service::AppContext,
) -> eyre::Result<(Box<dyn Layer<S> + Send + Sync>, Option<WorkerGuard>)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let enable_file_logger = app.config().enable_file_logger;
    // 如果不启用文件日志，则返回一个占位用的sink layer，不创建也不输出日志文件
    if !enable_file_logger {
        let sink_layer = layer()
            .with_writer(std::io::sink)
            .with_timer(LocalTime::rfc_3339())
            .with_ansi(false)
            .with_file(true)
            .with_line_number(true)
            .json();
        return Ok((Box::new(sink_layer), None));
    }
    let logs_dir = logs_dir(app).wrap_err("获取日志目录失败")?;
    let file_appender = RollingFileAppender::builder()
        .filename_prefix("jmcomic-downloader")
        .filename_suffix("log")
        .rotation(Rotation::DAILY)
        .build(&logs_dir)
        .wrap_err("创建RollingFileAppender失败")?;
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = layer()
        .with_writer(non_blocking_appender)
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .json();
    Ok((Box::new(file_layer), Some(guard)))
}

#[instrument(level = "error", skip_all)]
async fn file_log_watcher(app: Arc<dyn crate::service::AppContext>) {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let event_handler_span = tracing::error_span!("file_log_watcher_event_handler");

    let event_handler = move |res| {
        let send_event_task = async {
            if let Err(err) = sender.send(res).await.map_err(eyre::Report::from) {
                let err_title = "发送日志文件watcher事件失败";
                let message = err.to_message();
                tracing::error!(err_title, message);
            }
        };

        tauri::async_runtime::block_on(send_event_task.instrument(event_handler_span.clone()));
    };

    let mut watcher = match RecommendedWatcher::new(event_handler, notify::Config::default())
        .map_err(eyre::Report::from)
    {
        Ok(watcher) => watcher,
        Err(err) => {
            let err_title = "创建日志文件watcher失败";
            let message = err.to_message();
            tracing::error!(err_title, message);
            return;
        }
    };

    let logs_dir = match logs_dir(app.as_ref()) {
        Ok(logs_dir) => logs_dir,
        Err(err) => {
            let err_title = "日志文件watcher获取日志目录失败";
            let message = err.to_message();
            tracing::error!(err_title, message);
            return;
        }
    };

    if let Err(err) = std::fs::create_dir_all(&logs_dir) {
        let err_title = "创建日志目录失败";
        let message = eyre::Report::from(err).to_message();
        tracing::error!(err_title, message);
        return;
    }

    if let Err(err) = watcher
        .watch(&logs_dir, notify::RecursiveMode::NonRecursive)
        .map_err(eyre::Report::from)
    {
        let err_title = "日志文件watcher监听日志目录失败";
        let message = err.to_message();
        tracing::error!(err_title, message);
        return;
    }

    while let Some(res) = receiver.recv().await {
        match res.map_err(eyre::Report::from) {
            Ok(event) => {
                if let notify::EventKind::Remove(_) = event.kind {
                    if let Err(err) = reload_file_logger() {
                        let err_title = "重置日志文件失败";
                        let message = err.to_message();
                        tracing::error!(err_title, message);
                    }
                }
            }
            Err(err) => {
                let err_title = "接收日志文件watcher事件失败";
                let message = err.to_message();
                tracing::error!(err_title, message);
            }
        }
    }
}

#[instrument(level = "error", skip_all)]
pub fn logs_dir(app: &dyn crate::service::AppContext) -> eyre::Result<std::path::PathBuf> {
    Ok(app.paths().logs_dir.clone())
}
