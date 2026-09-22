//! mnesphere —— 本地优先的个人知识球。
//!
//! 这一层只做系统集成：窗口生命周期、托盘、单实例、命令注册。
//! 业务逻辑全在 vault / note / habit / ai / github 里。

mod ai;
mod config;
mod github;
mod habit;
mod note;
mod task;
mod vault;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use config::AppConfig;
use note::Index;

/// 自己写过的文件，短时间内忽略 watcher 回报 —— 否则「写 → 触发 → 重索引 → 再写」会打转。
const SELF_WRITE_WINDOW: Duration = Duration::from_millis(1500);

pub struct AppState {
    pub cfg: Mutex<AppConfig>,
    pub index: Mutex<Index>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    writes: Mutex<Vec<(PathBuf, Instant)>>,
}

impl AppState {
    pub fn new(cfg: AppConfig) -> Self {
        Self {
            cfg: Mutex::new(cfg),
            index: Mutex::new(Index::default()),
            watcher: Mutex::new(None),
            writes: Mutex::new(Vec::new()),
        }
    }

    pub fn mark_self_write(&self, p: &Path) {
        let mut g = self.writes.lock().unwrap();
        let now = Instant::now();
        g.retain(|(_, t)| now.duration_since(*t) < SELF_WRITE_WINDOW);
        g.push((normalize(p), now));
    }

    pub fn is_self_write(&self, p: &Path) -> bool {
        let target = normalize(p);
        let now = Instant::now();
        let mut g = self.writes.lock().unwrap();
        g.retain(|(_, t)| now.duration_since(*t) < SELF_WRITE_WINDOW);
        g.iter().any(|(q, _)| *q == target)
    }
}

fn normalize(p: &Path) -> PathBuf {
    let s = p.to_string_lossy().replace('\\', "/").to_lowercase();
    PathBuf::from(s)
}

/// 主题完全由前端 CSS 变量驱动，Rust 侧不插手渲染。
/// 保留这个钩子，是为了将来要做原生窗口底色 / 圆角时有个统一入口。
pub fn apply_theme_to_window(_app: &AppHandle, _cfg: &AppConfig) {}

// ───────────────────────── 窗口 ─────────────────────────

/// 唤起主窗口。
/// Windows 上有前台锁定策略：直接 set_focus 常常只闪一下就被压回去，
/// 所以先短暂置顶抢下焦点，再取消置顶。
fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_always_on_top(true);
        let _ = w.set_focus();
        let _ = w.set_always_on_top(false);
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "显示 mnesphere", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "同步到 GitHub", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open_vault", "打开 vault 目录", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &sync, &open, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("mnesphere")
        .menu(&menu)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "show" => show_main(app),
            "sync" => {
                let _ = tauri::Emitter::emit(app, "tray-sync", ());
                show_main(app);
            }
            "open_vault" => {
                if let Some(st) = app.try_state::<AppState>() {
                    let v = st.cfg.lock().unwrap().vault.clone();
                    let _ = std::process::Command::new("explorer").arg(v).spawn();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

// ───────────────────────── 杂项命令 ─────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    pub config_path: String,
    pub vault: String,
}

#[tauri::command]
fn app_info(state: tauri::State<'_, AppState>) -> AppInfo {
    let cfg = state.cfg.lock().unwrap();
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        config_path: config::config_path().to_string_lossy().to_string(),
        vault: cfg.vault.clone(),
    }
}

#[tauri::command]
fn rebuild_index(state: tauri::State<'_, AppState>) -> usize {
    let cfg = state.cfg.lock().unwrap().clone();
    note::rebuild_index(&state, &cfg);
    state.index.lock().unwrap().notes.len()
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let m = app.autolaunch();
    if enabled {
        m.enable().map_err(|e| e.to_string())
    } else {
        m.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn autostart_enabled(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

// ───────────────────────── 入口 ─────────────────────────

pub fn run() {
    tauri::Builder::default()
        // single-instance 必须第一个注册，否则会被其他插件抢先
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 第二次从桌面启动时：不新开窗口，唤起已有实例
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let to_tray = app
                    .try_state::<AppState>()
                    .map(|s| s.cfg.lock().unwrap().behavior.close_to_tray)
                    .unwrap_or(true);
                if to_tray {
                    // 关窗 ≠ 退出：藏进托盘，进程继续跑
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let cfg = AppConfig::load();
            let _ = vault::ensure_vault(&cfg);
            let start_hidden = cfg.behavior.start_hidden;

            let state = AppState::new(cfg.clone());
            note::rebuild_index(&state, &cfg);
            vault::sweep_tmp(&cfg.vault_path());
            app.manage(state);

            let handle = app.handle().clone();
            build_tray(&handle)?;

            {
                let st = app.state::<AppState>();
                let _ = vault::restart_watcher(&handle, &st);
            }

            // 主窗口在这里手动建 —— `tauri.conf.json` 里那个 `"create": false` 把 Tauri 的
            // 自动创建关掉了，为的就是能补一个 `enable_clipboard_access()`：
            // 它让 wry 挂上 PermissionRequested 处理器、自动放行 WebView2 的
            // CLIPBOARD_READ 权限，前端的 `navigator.clipboard.read()` 才读得到图片
            // （右键菜单里的「粘贴」要走这条；Ctrl+V 那条走 paste 事件，不依赖它）。
            //
            // 其余一切仍旧抄配置（`from_config`），配置保持唯一真相，别在这儿重复写窗口参数。
            // ⚠️ 必须建在 `app.manage(state)` 之后：建窗口会让前端立刻开始 invoke，
            //    命令状态还没挂上去的话会直接 panic。
            let win_cfg = app.config().app.windows.iter().find(|w| w.label == "main").cloned();
            if let Some(wc) = win_cfg {
                tauri::WebviewWindowBuilder::from_config(app.handle(), &wc)?
                    .enable_clipboard_access()
                    .build()?;
            }

            // 首启动引导：vault 目录不存在时也照样跑，前端会提示
            if cfg.onboarded && start_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 系统
            app_info,
            rebuild_index,
            hide_window,
            quit_app,
            set_autostart,
            autostart_enabled,
            // 配置与主题
            config::get_config,
            config::set_config,
            config::theme_presets,
            config::pick_wallpaper,
            config::pick_directory,
            config::path_exists,
            // 密钥
            config::secret_set,
            config::secret_has,
            config::secret_clear,
            // 文件系统
            vault::open_in_explorer,
            vault::reveal_path,
            vault::import_attachment,
            vault::save_pasted_image,
            vault::open_external,
            // 笔记
            note::list_notes_tree,
            note::list_diaries,
            note::read_doc,
            note::write_doc,
            note::update_diary_meta,
            note::create_note,
            note::create_diary,
            note::delete_doc,
            note::rename_doc,
            note::create_folder,
            note::rename_folder,
            note::delete_folder,
            note::search_notes,
            note::resolve_link,
            note::all_tags,
            note::index_stats,
            // 打卡
            habit::list_habits,
            habit::add_habit,
            habit::update_habit,
            habit::delete_habit,
            habit::get_month,
            habit::set_checkin,
            habit::cycle_checkin,
            habit::habit_stats,
            habit::today_board,
            habit::all_habits_overview,
            // 任务
            task::list_tasks,
            task::add_task,
            task::update_task,
            task::delete_task,
            // AI
            ai::ai_chat,
            ai::ai_test,
            // GitHub
            github::github_test,
            github::github_sync,
        ])
        .run(tauri::generate_context!())
        .expect("mnesphere 启动失败");
}
