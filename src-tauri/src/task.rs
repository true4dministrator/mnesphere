//! 任务：单清单文件 + 行级写回。
//!
//! 全部数据就一个文件 `任务/_tasks.md`（目录名跟别的模块一样可在配置里改）。
//! 不像打卡那样拆成「定义 + 数据」两份 —— 任务的**定义就是数据本身**：
//! 一行一条，行内容自带全部信息（标题 / 截止 / 优先级），没有第二份可拆。
//!
//! 一行的语法（`@` 和 `!` 都能省）：
//!
//! ```text
//! - [ ] 交材料 @2026-09-22 !高
//!   证书复印件 + 发票，一起交到教务
//! - [x] 买焊锡丝
//! ```
//!
//! **行级写回**（而不是整表重排）是刻意的。任务是手写的清单，用户会在编辑器里
//! 调顺序、插空行、写自己的小节标题，整表重排会把这些全抹掉，git diff 也会炸。
//! 代价是「行号会漂」—— 所以每行都带着解析时的原文当凭据，见 [`locate`]。

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

use crate::config::AppConfig;
use crate::vault::{abs_path, atomic_write};
use crate::AppState;

pub const TASK_FILE: &str = "_tasks.md";

/// 文件不存在时铺的骨架。纯粹是给人看的说明，解析时会自动跳过。
const HEADER: &str = "\
# 任务

> 这个文件由 mnesphere 维护，也可以直接手改。一行一条：
> `- [ ] 标题 @2026-09-22 !高`，日期和优先级都能省；
> 下一行缩进两格写备注；做完了把 `[ ]` 改成 `[x]`。
";

// ───────────────────────── 数据结构 ─────────────────────────

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// 解析时的 0-based 行号。只是「提示」，写回时不认死这个数，见 [`locate`]。
    pub line: usize,
    /// 任务行原文。写回时的唯一凭据。
    pub raw: String,
    /// 这一条占了几行（1 + 备注行数）。重写时整块替换。
    pub span: usize,
    pub title: String,
    /// `YYYY-MM-DD`，空串 = 未排期
    pub due: String,
    /// `high` / `normal` / `low`
    pub priority: String,
    pub note: String,
    pub done: bool,
}

// ───────────────────────── 语法 ─────────────────────────

fn re_task() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*[-*+]\s+\[( |x|X)\]\s*(.*)$").unwrap())
}

fn re_due() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"@(\d{4}-\d{2}-\d{2})").unwrap())
}

fn re_due_only() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap())
}

fn re_pri() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"!\s*(高|中|低|high|normal|low)").unwrap())
}

fn normalize_priority(s: &str) -> &'static str {
    match s.trim() {
        "高" | "high" => "high",
        "低" | "low" => "low",
        _ => "normal",
    }
}

/// 「中」是默认值，不写标记 —— 每条都顶着个 `!中` 只是噪音。
fn priority_mark(p: &str) -> &'static str {
    match p {
        "high" => "!高",
        "low" => "!低",
        _ => "",
    }
}

fn norm_due(d: Option<String>) -> String {
    let s = d.unwrap_or_default();
    let s = s.trim();
    if re_due_only().is_match(s) {
        s.to_string()
    } else {
        String::new()
    }
}

/// 任务标题不是文件名，没必要套 `sanitize_title` 那套（它会顺手吃掉 `|` `:` `*`
/// 这些在任务里完全不违法的字符）。这里只堵住真正会破坏行结构的东西：换行。
fn clean_title(s: &str) -> String {
    s.replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

// ───────────────────────── 解析 / 渲染 ─────────────────────────

pub fn parse_tasks(raw: &str) -> Vec<Task> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut out: Vec<Task> = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let l = lines[i];
        let Some(c) = re_task().captures(l) else {
            i += 1;
            continue;
        };
        let done = !c[1].trim().is_empty();
        let rest = c[2].to_string();

        let due = re_due()
            .captures(&rest)
            .map(|m| m[1].to_string())
            .unwrap_or_default();
        let priority = re_pri()
            .captures(&rest)
            .map(|m| normalize_priority(&m[1]).to_string())
            .unwrap_or_else(|| "normal".to_string());

        // 标记要从标题里摘掉，否则面板上会显示成「交材料 @2026-09-22 !高」。
        // 写回时再按统一顺序拼回去，等于顺手把用户手写的乱序归一化了。
        let mut title = re_pri().replace_all(&rest, "").to_string();
        title = re_due().replace_all(&title, "").to_string();
        let title = title.trim().trim_end_matches(['-', '—', '·']).trim().to_string();

        // 备注 = 紧跟其后的缩进行，直到空行 / 下一条任务 / 顶格段落为止
        let mut notes: Vec<String> = Vec::new();
        let mut j = i + 1;
        while j < lines.len() {
            let n = lines[j];
            if n.trim().is_empty() || re_task().is_match(n) {
                break;
            }
            if !(n.starts_with("  ") || n.starts_with('\t')) {
                break;
            }
            notes.push(n.trim().to_string());
            j += 1;
        }

        out.push(Task {
            line: i,
            raw: l.to_string(),
            span: 1 + notes.len(),
            title,
            due,
            priority,
            note: notes.join(" "),
            done,
        });
        i = j;
    }
    out
}

/// 把一条任务渲染回一行（标题 @截止 !优先级）+ 若干缩进备注行。
fn render_task(t: &Task) -> Vec<String> {
    let mut parts = vec![t.title.trim().to_string()];
    if !t.due.trim().is_empty() {
        parts.push(format!("@{}", t.due.trim()));
    }
    let pm = priority_mark(&t.priority);
    if !pm.is_empty() {
        parts.push(pm.to_string());
    }
    let mut out = vec![format!(
        "- [{}] {}",
        if t.done { "x" } else { " " },
        parts.join(" ")
    )];
    for l in t
        .note
        .replace('\r', "")
        .lines()
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
    {
        out.push(format!("  {l}"));
    }
    out
}

fn tasks_path(cfg: &AppConfig) -> std::path::PathBuf {
    abs_path(cfg, &format!("{}/{TASK_FILE}", cfg.editor.task_dir))
}

/// 文件不存在就返回骨架（但不落盘 —— 只有真的要写了才建文件）。
fn read_raw(cfg: &AppConfig) -> String {
    match std::fs::read_to_string(tasks_path(cfg)) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => HEADER.to_string(),
    }
}

pub fn load_tasks(cfg: &AppConfig) -> Vec<Task> {
    parse_tasks(&read_raw(cfg))
}

/// 按「原文 + 行号提示」定位任务行。
///
/// 行号会漂（用户在别处插了一行、或者资源管理器里手改过），所以不认死 `hint`，
/// 而是拿原文去比对：
/// 1. hint 那一行还是原文 → 直接用，绝大多数情况走这条；
/// 2. 否则全文找原文，唯一命中就用它（前面插了行也能对上）；
/// 3. 命中多处就取离 hint 最近的那个 —— 重复内容只能这么折中；
/// 4. 一处都没有 → 这条已经被改过或删了，让前端刷新。
fn locate(lines: &[&str], raw: &str, hint: usize) -> Result<usize, String> {
    if hint < lines.len() && lines[hint] == raw {
        return Ok(hint);
    }
    let hits: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| **l == raw)
        .map(|(i, _)| i)
        .collect();
    match hits.len() {
        0 => Err("这条任务已经被改过或删掉了，刷新一下再看看".into()),
        1 => Ok(hits[0]),
        _ => Ok(*hits
            .iter()
            .min_by_key(|i| i.abs_diff(hint))
            .unwrap()),
    }
}

// ───────────────────────── 命令 ─────────────────────────

#[tauri::command]
pub fn list_tasks(state: tauri::State<'_, AppState>) -> Vec<Task> {
    let cfg = state.cfg.lock().unwrap().clone();
    load_tasks(&cfg)
}

#[tauri::command]
pub fn add_task(
    state: tauri::State<'_, AppState>,
    title: String,
    due: Option<String>,
    priority: Option<String>,
    note: Option<String>,
) -> Result<Vec<Task>, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let clean = clean_title(&title);
    if clean.is_empty() {
        return Err("任务标题不能为空".into());
    }
    let p = tasks_path(&cfg);
    let mut raw = read_raw(&cfg);
    if !raw.ends_with('\n') {
        raw.push('\n');
    }
    // 追加到文件末尾，不按日期插队。插队会把后面每一行的行号都顶掉，
    // 手写清单里的空行和自定义顺序也会被搅乱。
    let t = Task {
        line: 0,
        raw: String::new(),
        span: 0,
        title: clean,
        due: norm_due(due),
        priority: priority
            .map(|s| normalize_priority(&s).to_string())
            .unwrap_or_else(|| "normal".to_string()),
        note: note.unwrap_or_default().trim().to_string(),
        done: false,
    };
    for l in render_task(&t) {
        raw.push_str(&l);
        raw.push('\n');
    }
    atomic_write(&p, &raw).map_err(|e| e.to_string())?;
    state.mark_self_write(&p);
    Ok(parse_tasks(&raw))
}

/// 改一条任务。只传要动的字段，其余保持原样。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn update_task(
    state: tauri::State<'_, AppState>,
    raw: String,
    hint: usize,
    title: Option<String>,
    due: Option<String>,
    priority: Option<String>,
    note: Option<String>,
    done: Option<bool>,
) -> Result<Vec<Task>, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let p = tasks_path(&cfg);
    let content = read_raw(&cfg);
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let idx = {
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        locate(&refs, &raw, hint)?
    };
    // 拿「当前磁盘上的这条」当底，而不是信前端传来的整份数据 ——
    // 前端漏传的字段就自然保持原值。
    let cur = parse_tasks(&content)
        .into_iter()
        .find(|t| t.line == idx)
        .ok_or("这一行现在不是任务了，刷新一下再看看")?;

    let mut next = cur.clone();
    next.line = 0;
    next.raw.clear();
    next.span = 0;
    if let Some(v) = title {
        let v = clean_title(&v);
        if !v.is_empty() {
            next.title = v;
        }
    }
    if let Some(v) = due {
        next.due = norm_due(Some(v));
    }
    if let Some(v) = priority {
        next.priority = normalize_priority(&v).to_string();
    }
    if let Some(v) = note {
        next.note = v.trim().to_string();
    }
    if let Some(v) = done {
        next.done = v;
    }

    lines.splice(idx..idx + cur.span.max(1), render_task(&next));
    let mut out = lines.join("\n");
    if content.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    atomic_write(&p, &out).map_err(|e| e.to_string())?;
    state.mark_self_write(&p);
    Ok(parse_tasks(&out))
}

#[tauri::command]
pub fn delete_task(
    state: tauri::State<'_, AppState>,
    raw: String,
    hint: usize,
) -> Result<Vec<Task>, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let p = tasks_path(&cfg);
    let content = read_raw(&cfg);
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let idx = {
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        locate(&refs, &raw, hint)?
    };
    let span = parse_tasks(&content)
        .into_iter()
        .find(|t| t.line == idx)
        .map(|t| t.span.max(1))
        .unwrap_or(1);
    lines.drain(idx..(idx + span).min(lines.len()));

    let mut out = lines.join("\n");
    if content.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    atomic_write(&p, &out).map_err(|e| e.to_string())?;
    state.mark_self_write(&p);
    Ok(parse_tasks(&out))
}
