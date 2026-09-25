//! 配置与主题。内容数据一律 Markdown，这里只放"不是文本内容"的东西。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mnesphere")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// 默认 vault 放在用户主目录，**刻意避开** Documents —— 那目录常被 OneDrive 接管，
/// 文件锁 + 冲突副本会把这块搅烂。
pub fn default_vault() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mnesphere-vault")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub vault: String,
    pub theme: Theme,
    pub ai: AiConfig,
    pub github: GithubConfig,
    pub behavior: Behavior,
    pub editor: Editor,
    /// 首次启动引导是否已完成
    pub onboarded: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            vault: default_vault().to_string_lossy().to_string(),
            theme: Theme::default(),
            ai: AiConfig::default(),
            github: GithubConfig::default(),
            behavior: Behavior::default(),
            editor: Editor::default(),
            onboarded: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Theme {
    pub preset: String,
    pub accent: String,
    pub accent2: String,
    pub bg: String,
    pub fg: String,
    /// 壁纸绝对路径，空串表示不启用
    pub wallpaper: String,
    pub wallpaper_opacity: f32,
    pub wallpaper_blur: f32,
    /// 面板不透明度（1.0 = 完全不透，0.0 = 全透出壁纸）
    pub panel_opacity: f32,
    pub radius: f32,
    pub font_ui: String,
    pub font_mono: String,
    pub font_size: f32,
    /// 左侧文件栏（Panel）的字号。左栏文字密度大，常要跟正文分开调。
    pub panel_font: f32,
    /// 左栏宽度。-1 = 自适应（按内容称），正数 = 用户拖出来的固定值。
    /// 用 f32 而不是 Option，是为了跟同结构里其它数值字段保持一致，
    /// 前端拿到 -1 就当 'auto' 处理。
    pub panel_width: f32,
}

impl Default for Theme {
    fn default() -> Self {
        let p = preset("cyan");
        Self {
            preset: "cyan".into(),
            accent: p.0.into(),
            accent2: p.1.into(),
            bg: p.2.into(),
            fg: p.3.into(),
            wallpaper: String::new(),
            wallpaper_opacity: 1.0,
            wallpaper_blur: 0.0,
            panel_opacity: 0.86,
            radius: 12.0,
            font_ui: String::new(),
            font_mono: "JetBrains Mono".into(),
            font_size: 15.0,
            panel_font: 12.5,
            panel_width: 244.0,
        }
    }
}

/// 预设主题：(accent, accent2, bg, fg)
pub fn preset(id: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match id {
        "catppuccin" => ("#cba6f7", "#94e2d5", "#1e1e2e", "#cdd6f4"),
        "gruvbox" => ("#fabd2f", "#b8bb26", "#282828", "#ebdbb2"),
        "tokyo" => ("#7aa2f7", "#bb9af7", "#1a1b26", "#c0caf5"),
        "nord" => ("#88c0d0", "#a3be8c", "#2e3440", "#eceff4"),
        "rose" => ("#f5a0b8", "#f2cdcd", "#191724", "#e0def4"),
        // cyan —— 默认，沿用 zchlab 的荧光青
        _ => ("#22d3ee", "#5eead4", "#0a0e13", "#dbe6f0"),
    }
}

pub const PRESET_IDS: [&str; 6] = ["cyan", "catppuccin", "gruvbox", "tokyo", "nord", "rose"];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    /// 单次对话最多注入多少篇被引用笔记
    pub max_refs: usize,
    pub system_prompt: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com/v1".into(),
            model: "deepseek-chat".into(),
            temperature: 0.6,
            max_refs: 5,
            system_prompt: "你是 mnesphere 笔记库里的助手。回答简洁、具体，需要时引用笔记原文。\
如果回答涉及某个概念，用 [[双链]] 语法指出相关笔记名，方便用户跳转。"
                .into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct GithubConfig {
    /// owner/repo
    pub repo: String,
    pub branch: String,
    pub last_sync: String,
    pub last_commit: String,
}

impl Default for GithubConfig {
    fn default() -> Self {
        Self {
            repo: String::new(),
            branch: "main".into(),
            last_sync: String::new(),
            last_commit: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Behavior {
    /// 关闭窗口 → 隐藏到托盘
    pub close_to_tray: bool,
    pub autostart: bool,
    /// 开机自启时直接缩到托盘
    pub start_hidden: bool,
    pub hide_panel_when_ai_open: bool,
    /// 启动后默认停在哪个模块：notes / diary / checkin / task。
    ///
    /// ⚠️ 与 `auto_create_diary` 是**同一批**加的，两者用「本字段为空串」当缺失信号
    /// 在 `migrate()` 里一次性补齐。理由见那里的注释。
    pub default_activity: String,
    /// 启动时是否自动创建「今天」那篇日记（并打开）。
    /// 关掉它之后，启动就只切到默认界面，不再替用户建文件。
    pub auto_create_diary: bool,
    /// 左侧笔记树的排序方式：`name`（按名字）/ `mtime`（按修改时间，最新在前）。
    ///
    /// 只是**视图偏好**，排序本身在前端做（见 `src/lib/notesSort.ts`）。
    pub note_sort: String,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            autostart: false,
            start_hidden: false,
            hide_panel_when_ai_open: true,
            default_activity: "diary".into(),
            auto_create_diary: true,
            note_sort: "name".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Editor {
    pub default_mode: String,
    pub autosave_ms: u64,
    pub diary_dir: String,
    pub notes_dir: String,
    pub checkin_dir: String,
    /// 任务清单目录。注意它不在 notes_dir / diary_dir 下，所以不会混进笔记树；
    /// 但索引是整 vault 走的，`任务/_tasks.md` 会进搜索 —— 跟打卡文件一样，先这样。
    pub task_dir: String,
    pub attachments_dir: String,
    /// 新建日记时往正文里预置的内容。默认**留空** —— 新日记就该是一张白纸，
    /// 固定的三段式（今天做了什么 / 想法 / 明天）只会让人先删一遍。
    pub diary_template: String,
}

/// 0.1.1 及更早的出厂默认模板。留在这里只为一件事：迁移。
///
/// `Editor` 上挂了 `#[serde(default)]`，但它只在 JSON 里**缺**这个字段时才兜底；
/// 而 config.json 早在首次启动时就把这个串实打实写进去了，所以光改
/// `Default::default()` 对老用户一点用都没有 —— 必须显式比对、显式替换。
const LEGACY_DIARY_TEMPLATE: &str = "## 今天做了什么\n\n\n## 想法\n\n\n## 明天\n\n";

impl Default for Editor {
    fn default() -> Self {
        Self {
            default_mode: "edit".into(),
            autosave_ms: 900,
            diary_dir: "日记".into(),
            notes_dir: "笔记".into(),
            checkin_dir: "打卡".into(),
            task_dir: "任务".into(),
            attachments_dir: "附件".into(),
            diary_template: String::new(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let p = config_path();
        match std::fs::read_to_string(&p) {
            Ok(s) => match serde_json::from_str::<AppConfig>(&s) {
                Ok(mut c) => {
                    c.migrate();
                    c
                }
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }

    /// 一次性配置迁移。每加一条，记得写完就把老用户的那份改到新形态并落盘 ——
    /// 不然改了 `Default` 也只在「第一次装这个应用的人」身上生效。
    fn migrate(&mut self) {
        let mut dirty = false;

        // 日记模板：只有还停在旧出厂默认值的才动。用户自己改过的模板一律保留。
        if self.editor.diary_template == LEGACY_DIARY_TEMPLATE {
            self.editor.diary_template = Editor::default().diary_template;
            dirty = true;
        }

        // panel_width 是 0.2.0 之后才加进 Theme 的。老用户的 config.json 里
        // **没有这个字段** —— serde(default) 会把它填成 0.0。0 不是合法宽度，
        // 当成「没设过」补成出厂 244。用户真拖过之后存的是正数，不会被碰。
        if self.theme.panel_width <= 0.0 {
            self.theme.panel_width = Theme::default().panel_width;
            dirty = true;
        }

        // 0.2.1 新增：启动默认界面 + 是否自动建今日日记。
        //
        // 这两个字段是同一批加的，且**都不可能是用户手填的空值**，所以拿
        // 「default_activity 为空串」当整批的「缺失信号」是安全的：
        // 补一次之后它非空，这块就再也不触发 —— 于是用户日后自己关掉
        // auto_create_diary，不会被迁移逻辑又翻回 true（那种「设置里的开关
        // 自己弹回去」的 bug 最难查）。
        if self.behavior.default_activity.is_empty() {
            self.behavior.default_activity = Behavior::default().default_activity;
            self.behavior.auto_create_diary = Behavior::default().auto_create_diary;
            dirty = true;
        }

        // 0.2.2 新增：笔记树排序方式。同样拿「空串」当缺失信号 ——
        // 它跟上面那批是**不同批次**，所以必须各用一个信号，不能共用
        // （共用的话 0.2.1 已经迁移过的用户就吃不到了）。
        if self.behavior.note_sort.is_empty() {
            self.behavior.note_sort = Behavior::default().note_sort;
            dirty = true;
        }

        if dirty {
            if let Err(e) = self.save() {
                eprintln!("[mnesphere] 写回迁移后的配置失败：{e}");
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let p = config_path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::vault::atomic_write(&p, &s).map_err(|e| e.to_string())
    }

    pub fn vault_path(&self) -> PathBuf {
        PathBuf::from(&self.vault)
    }

    pub fn sub(&self, name: &str) -> String {
        match name {
            "diary" => self.editor.diary_dir.clone(),
            "notes" => self.editor.notes_dir.clone(),
            "checkin" => self.editor.checkin_dir.clone(),
            "task" => self.editor.task_dir.clone(),
            "attachments" => self.editor.attachments_dir.clone(),
            _ => name.to_string(),
        }
    }
}

// ───────────────────────── 密钥：系统凭据管理器 ─────────────────────────

const KEYRING_SERVICE: &str = "space.zchlab.mnesphere";

pub fn set_secret(name: &str, value: &str) -> Result<(), String> {
    let e = keyring::Entry::new(KEYRING_SERVICE, name).map_err(|e| e.to_string())?;
    if value.is_empty() {
        let _ = e.delete_credential();
        return Ok(());
    }
    e.set_password(value).map_err(|e| e.to_string())
}

pub fn get_secret(name: &str) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, name)
        .ok()?
        .get_password()
        .ok()
}

pub fn has_secret(name: &str) -> bool {
    get_secret(name).map(|s| !s.is_empty()).unwrap_or(false)
}

// ───────────────────────── 命令 ─────────────────────────

#[tauri::command]
pub fn get_config(state: tauri::State<'_, crate::AppState>) -> AppConfig {
    // 每次读盘，保证手工编辑 config.json 也能生效
    let cfg = AppConfig::load();
    *state.cfg.lock().unwrap() = cfg.clone();
    cfg
}

#[tauri::command]
pub fn set_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    mut config: AppConfig,
) -> Result<AppConfig, String> {
    let old_vault = {
        let g = state.cfg.lock().unwrap();
        // `last_sync` / `last_commit` 是**运行态字段** —— 只有同步流程会写它们。
        // 前端手上那份配置是异步拉来的快照，这两个字段很可能已经过期：
        // 用户同步完（后端写了新时间）之后再随便改个设置，前端就会把它抹回空。
        // 所以这里一律以服务端内存值为准，不接受前端传来的这两个值。
        config.github.last_sync = g.github.last_sync.clone();
        config.github.last_commit = g.github.last_commit.clone();
        g.vault.clone()
    };
    config.save()?;
    {
        let mut g = state.cfg.lock().unwrap();
        *g = config.clone();
    }
    if old_vault != config.vault {
        crate::vault::ensure_vault(&config)?;
        crate::note::rebuild_index(&state, &config);
        crate::vault::restart_watcher(&app, &state)?;
    }
    crate::apply_theme_to_window(&app, &config);
    Ok(config)
}

#[tauri::command]
pub fn theme_presets() -> Vec<PresetInfo> {
    PRESET_IDS
        .iter()
        .map(|id| {
            let (a, a2, bg, fg) = preset(id);
            PresetInfo {
                id: (*id).into(),
                accent: a.into(),
                accent2: a2.into(),
                bg: bg.into(),
                fg: fg.into(),
            }
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    pub id: String,
    pub accent: String,
    pub accent2: String,
    pub bg: String,
    pub fg: String,
}

#[tauri::command]
pub fn pick_wallpaper(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let f = app
        .dialog()
        .file()
        .add_filter("图片", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"])
        .blocking_pick_file()?;
    f.into_path().ok().map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn pick_directory(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let f = app.dialog().file().blocking_pick_folder()?;
    f.into_path().ok().map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn path_exists(p: String) -> bool {
    Path::new(&p).exists()
}

// ───────────────────────── 密钥命令 ─────────────────────────

#[tauri::command]
pub fn secret_set(name: String, value: String) -> Result<(), String> {
    set_secret(&name, &value)
}

#[tauri::command]
pub fn secret_has(name: String) -> bool {
    has_secret(&name)
}

#[tauri::command]
pub fn secret_clear(name: String) -> Result<(), String> {
    set_secret(&name, "")
}
