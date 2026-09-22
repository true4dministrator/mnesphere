//! 打卡：目标定义 + 按日期三态记录。
//!
//! 存储拆成两个文件，刻意为之：
//! - `打卡/_habits.md`   习惯**定义**，改动频率极低（frontmatter YAML）
//! - `打卡/YYYY-MM.md`   日期**矩阵**，每天都要写（固定格式的表格）
//!
//! 拆开的原因：如果混在一个文件里，每天写数据都要整体重写那个含 frontmatter 的文件，
//! 手滑一次就把习惯定义洗了。高频写的那一侧结构越简单越好。
//!
//! 三态取值：`done` / `missed` / 留空（未打卡）。
//! 「未到期」不是状态 —— 未来日期在表格里根本不占行。

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::config::AppConfig;
use crate::note::{join_frontmatter, parse_frontmatter, split_frontmatter};
use crate::vault::{abs_path, atomic_write};
use crate::AppState;

/// 连续多少天完全没有记录，就认为链条断了。没有这个上限，
/// 「未打卡不打断连续」会推出一条穿越整个历史的假连续。
const NONE_TOLERANCE: usize = 3;

pub const HABIT_FILE: &str = "_habits.md";

// ───────────────────────── 数据结构 ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Habit {
    pub name: String,
    pub color: String,
    pub created: String,
    pub note: String,
    pub archived: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DayCell {
    pub date: String,
    pub day: u32,
    pub weekday: u32,
    /// habit 名称 → 状态（"" / "done" / "missed"）
    pub states: BTreeMap<String, String>,
    pub future: bool,
    pub today: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MonthView {
    pub month: String,
    pub label: String,
    pub habits: Vec<Habit>,
    pub days: Vec<DayCell>,
    /// habit → 本月 done/missed/none 计数
    pub summary: BTreeMap<String, Counts>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub done: i32,
    pub missed: i32,
    pub none: i32,
    pub rate: f32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WeekBar {
    pub label: String,
    pub done: i32,
    pub total: i32,
    pub rate: f32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HabitStats {
    pub habit: String,
    pub current_streak: i32,
    pub longest_streak: i32,
    pub done: i32,
    pub missed: i32,
    pub none: i32,
    pub rate: f32,
    pub weeks: Vec<WeekBar>,
    pub first_date: String,
    pub total_days: i32,
}

// ───────────────────────── 定义文件 ─────────────────────────

fn habits_path(cfg: &AppConfig) -> std::path::PathBuf {
    abs_path(cfg, &format!("{}/{HABIT_FILE}", cfg.editor.checkin_dir))
}

const HABITS_DOC: &str =
    "# 习惯定义\n\n> 此文件由 mnesphere 管理。可以直接改 YAML 增删习惯，改完回到应用即可生效；\n> 但请保持 `habits:` 的列表结构，否则会解析失败。\n";

pub fn load_habits(cfg: &AppConfig) -> Vec<Habit> {
    let p = habits_path(cfg);
    let Ok(raw) = std::fs::read_to_string(&p) else {
        return vec![];
    };
    let (fm_raw, _) = split_frontmatter(&raw);
    let fm = parse_frontmatter(&fm_raw);
    match fm.get("habits") {
        Some(v) => serde_yaml::from_value::<Vec<Habit>>(v.clone()).unwrap_or_default(),
        None => vec![],
    }
}

pub fn save_habits(cfg: &AppConfig, habits: &[Habit]) -> Result<(), String> {
    let mut fm = serde_yaml::Value::Mapping(Default::default());
    let val = serde_yaml::to_value(habits).map_err(|e| e.to_string())?;
    fm.as_mapping_mut()
        .unwrap()
        .insert(serde_yaml::Value::String("habits".into()), val);
    let body = HABITS_DOC.to_string();
    atomic_write(&habits_path(cfg), &join_frontmatter(&fm, &body)).map_err(|e| e.to_string())
}

// ───────────────────────── 月文件 ─────────────────────────

fn month_path(cfg: &AppConfig, month: &str) -> std::path::PathBuf {
    abs_path(cfg, &format!("{}/{month}.md", cfg.editor.checkin_dir))
}

/// date → habit → state
type Matrix = BTreeMap<String, BTreeMap<String, String>>;

fn parse_month(raw: &str) -> Matrix {
    let (_, body) = split_frontmatter(raw);
    let mut out: Matrix = BTreeMap::new();
    let mut header: Vec<String> = vec![];
    let mut seen_sep = false;
    for line in body.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            continue;
        }
        let cells: Vec<String> = t
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        if cells.is_empty() {
            continue;
        }
        if header.is_empty() {
            header = cells;
            continue;
        }
        // 分隔行
        if !seen_sep && cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')) {
            seen_sep = true;
            continue;
        }
        let date = cells[0].trim().to_string();
        if date.len() < 8 {
            continue;
        }
        let mut row: BTreeMap<String, String> = BTreeMap::new();
        for (i, h) in header.iter().enumerate().skip(1) {
            let v = cells.get(i).cloned().unwrap_or_default();
            row.insert(h.clone(), normalize_state(&v));
        }
        out.insert(date, row);
    }
    out
}

fn normalize_state(s: &str) -> String {
    match s.trim().to_ascii_lowercase().as_str() {
        "done" | "x" | "[x]" | "1" | "true" | "✅" | "✓" => "done".into(),
        "missed" | "-" | "[-]" | "0" | "false" | "❌" | "✗" => "missed".into(),
        _ => String::new(),
    }
}

fn state_mark(s: &str) -> &'static str {
    match s {
        "done" => "done",
        "missed" => "missed",
        _ => "",
    }
}

fn month_days(month: &str) -> Vec<String> {
    let (y, m) = match month.split_once('-') {
        Some((a, b)) => (a.parse::<i32>().unwrap_or(2026), b.parse::<u32>().unwrap_or(1)),
        None => return vec![],
    };
    let mut out = vec![];
    let mut d = 1u32;
    while let Some(nd) = chrono::NaiveDate::from_ymd_opt(y, m, d) {
        out.push(nd.format("%Y-%m-%d").to_string());
        d += 1;
        if d > 31 {
            break;
        }
    }
    out
}

/// 表格行要写到哪一天：
/// - 过去的月份 → 整月
/// - 当前月份   → 1 号到今天
/// - 未来月份   → 不写（未来不是状态，是"还没有"）
fn last_row(month: &str, today: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    let days = month_days(month);
    if days.is_empty() {
        return None;
    }
    let last = chrono::NaiveDate::parse_from_str(days.last().unwrap(), "%Y-%m-%d").ok()?;
    let tm = today.format("%Y-%m").to_string();
    if month < tm.as_str() {
        Some(last)
    } else if month == tm {
        Some(today)
    } else {
        None
    }
}

pub fn load_month(cfg: &AppConfig, month: &str) -> Matrix {
    match std::fs::read_to_string(month_path(cfg, month)) {
        Ok(raw) => parse_month(&raw),
        Err(_) => BTreeMap::new(),
    }
}

/// 归档习惯的列会被丢弃，所以渲染时把**全部**习惯都列出来（含归档），
/// 保证历史数据不丢；前端只展示未归档的。
fn render_month_all(cfg: &AppConfig, month: &str, habits: &[Habit], data: &Matrix) -> String {
    let today = chrono::Local::now().date_naive();
    let mut out = String::new();
    out.push_str(&format!(
        "---\nmonth: \"{month}\"\n---\n\n# {month} 打卡记录\n\n> 由 mnesphere 维护。取值只有三种：`done` 做到了、`missed` 没做到、留空 未打卡。\n\n"
    ));
    out.push_str("| 日期 |");
    for h in habits {
        out.push_str(&format!(" {} |", h.name));
    }
    out.push_str("\n| --- |");
    for _ in habits {
        out.push_str(" --- |");
    }
    out.push('\n');
    if let Some(end) = last_row(month, today) {
        for d in month_days(month) {
            let Ok(nd) = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d") else { continue };
            if nd > end {
                break;
            }
            out.push_str(&format!("| {d} |"));
            for h in habits {
                let v = data
                    .get(&d)
                    .and_then(|r| r.get(&h.name))
                    .map(|s| state_mark(s))
                    .unwrap_or("");
                out.push_str(&format!(" {v} |"));
            }
            out.push('\n');
        }
    }
    let _ = cfg;
    out
}

fn write_month2(cfg: &AppConfig, month: &str, habits: &[Habit], data: &Matrix) -> Result<(), String> {
    atomic_write(&month_path(cfg, month), &render_month_all(cfg, month, habits, data))
        .map_err(|e| e.to_string())
}

fn all_months(cfg: &AppConfig) -> Vec<String> {
    let dir = abs_path(cfg, &cfg.editor.checkin_dir);
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if let Some(stem) = n.strip_suffix(".md") {
                if stem.len() == 7 && stem.as_bytes()[4] == b'-' {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

// ───────────────────────── 统计 ─────────────────────────

fn finalize(c: &mut Counts) {
    let denom = c.done + c.missed;
    c.rate = if denom > 0 {
        c.done as f32 / denom as f32
    } else {
        0.0
    };
}

fn compute_stats(cfg: &AppConfig, habit: &str) -> HabitStats {
    let months = all_months(cfg);
    let today = chrono::Local::now().date_naive();

    // date → state（只取这一习惯）
    let mut series: BTreeMap<String, String> = BTreeMap::new();
    for m in &months {
        let data = load_month(cfg, m);
        for (d, row) in data {
            if let Some(s) = row.get(habit) {
                series.insert(d, s.clone());
            }
        }
    }

    let mut done = 0i32;
    let mut missed = 0i32;
    let mut none = 0i32;
    for s in series.values() {
        match s.as_str() {
            "done" => done += 1,
            "missed" => missed += 1,
            _ => none += 1,
        }
    }

    let dates: Vec<chrono::NaiveDate> = series
        .keys()
        .filter_map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();
    let first_date = dates.first().map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
    let total_days = dates.len() as i32;

    // 当前连续：从今天倒着走。done 记数；missed 断；none 跳过但有上限
    let mut current = 0i32;
    let mut cursor = today;
    let mut none_run = 0usize;
    let created = load_habits(cfg)
        .into_iter()
        .find(|h| h.name == habit)
        .map(|h| h.created)
        .unwrap_or_default();
    let created_date = chrono::NaiveDate::parse_from_str(&created, "%Y-%m-%d").ok();
    loop {
        if let Some(cd) = created_date {
            if cursor < cd {
                break;
            }
        }
        let key = cursor.format("%Y-%m-%d").to_string();
        match series.get(&key).map(|s| s.as_str()).unwrap_or("") {
            "done" => {
                current += 1;
                none_run = 0;
            }
            "missed" => break,
            _ => {
                none_run += 1;
                if none_run > NONE_TOLERANCE {
                    break;
                }
            }
        }
        cursor -= chrono::Duration::days(1);
        // 安全阀：最多回溯 5 年
        if (today - cursor).num_days() > 365 * 5 {
            break;
        }
    }

    // 最长连续：只统计 done 的连续段（missed 与 none 都断）
    let mut longest = 0i32;
    let mut run = 0i32;
    let sorted: Vec<chrono::NaiveDate> = {
        let mut v: Vec<chrono::NaiveDate> = dates.clone();
        v.sort();
        v
    };
    let mut prev: Option<chrono::NaiveDate> = None;
    for d in sorted {
        let contiguous = prev.map(|p| (d - p).num_days() == 1).unwrap_or(false);
        if !contiguous {
            run = 0;
        }
        if series.get(&d.format("%Y-%m-%d").to_string()).map(|s| s.as_str()) == Some("done") {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
        prev = Some(d);
    }

    // 近 12 周
    let mut weeks: Vec<WeekBar> = vec![];
    let monday = today
        - chrono::Duration::days(chrono::Datelike::weekday(&today).num_days_from_monday() as i64);
    for i in (0..12).rev() {
        let start = monday - chrono::Duration::weeks(i as i64);
        let mut wd = 0;
        let mut wt = 0;
        for k in 0..7 {
            let d = start + chrono::Duration::days(k);
            if d > today {
                break;
            }
            wt += 1;
            let key = d.format("%Y-%m-%d").to_string();
            if series.get(&key).map(|s| s.as_str()) == Some("done") {
                wd += 1;
            }
        }
        weeks.push(WeekBar {
            label: start.format("%m-%d").to_string(),
            done: wd,
            total: wt,
            rate: if wt > 0 { wd as f32 / wt as f32 } else { 0.0 },
        });
    }

    let mut counts = Counts { done, missed, none, rate: 0.0 };
    finalize(&mut counts);

    HabitStats {
        habit: habit.to_string(),
        current_streak: current,
        longest_streak: longest,
        done,
        missed,
        none,
        rate: counts.rate,
        weeks,
        first_date,
        total_days,
    }
}

// ───────────────────────── 命令 ─────────────────────────

#[tauri::command]
pub fn list_habits(state: tauri::State<'_, AppState>) -> Vec<Habit> {
    let cfg = state.cfg.lock().unwrap().clone();
    let mut h = load_habits(&cfg);
    h.sort_by(|a, b| a.created.cmp(&b.created).then(a.name.cmp(&b.name)));
    h
}

#[tauri::command]
pub fn add_habit(
    state: tauri::State<'_, AppState>,
    name: String,
    color: Option<String>,
    note: Option<String>,
) -> Result<Vec<Habit>, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let clean = crate::note::sanitize_title(&name);
    if clean.is_empty() {
        return Err("目标名称不能为空".into());
    }
    let mut habits = load_habits(&cfg);
    if habits.iter().any(|h| h.name == clean) {
        return Err(format!("已存在同名目标：{clean}"));
    }
    habits.push(Habit {
        name: clean,
        color: color.unwrap_or_else(|| "cyan".into()),
        created: chrono::Local::now().format("%Y-%m-%d").to_string(),
        note: note.unwrap_or_default(),
        archived: false,
    });
    save_habits(&cfg, &habits)?;
    state.mark_self_write(&habits_path(&cfg));
    Ok(habits)
}

#[tauri::command]
pub fn update_habit(
    state: tauri::State<'_, AppState>,
    old_name: String,
    name: Option<String>,
    color: Option<String>,
    note: Option<String>,
    archived: Option<bool>,
) -> Result<Vec<Habit>, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let mut habits = load_habits(&cfg);
    let Some(pos) = habits.iter().position(|h| h.name == old_name) else {
        return Err("目标不存在".into());
    };
    let new_name = name
        .map(|n| crate::note::sanitize_title(&n))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| old_name.clone());

    if new_name != old_name {
        if habits.iter().any(|h| h.name == new_name) {
            return Err(format!("已存在同名目标：{new_name}"));
        }
        // 改名要同步重写所有月文件的表头，否则历史数据会跟列名对不上
        for m in all_months(&cfg) {
            let mut data = load_month(&cfg, &m);
            for row in data.values_mut() {
                if let Some(v) = row.remove(&old_name) {
                    row.insert(new_name.clone(), v);
                }
            }
            write_month2(&cfg, &m, &habits, &data)?;
        }
    }

    let h = &mut habits[pos];
    h.name = new_name;
    if let Some(c) = color {
        h.color = c;
    }
    if let Some(n) = note {
        h.note = n;
    }
    if let Some(a) = archived {
        h.archived = a;
    }
    save_habits(&cfg, &habits)?;
    state.mark_self_write(&habits_path(&cfg));
    Ok(habits)
}

#[tauri::command]
pub fn delete_habit(state: tauri::State<'_, AppState>, name: String) -> Result<Vec<Habit>, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let mut habits = load_habits(&cfg);
    habits.retain(|h| h.name != name);
    save_habits(&cfg, &habits)?;
    state.mark_self_write(&habits_path(&cfg));
    // 月文件里残留的列下次写入时会自动消失
    Ok(habits)
}

fn apply_cell(cfg: &AppConfig, habit: &str, date: &str, value: &str) -> Result<(), String> {
    let nd = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| "日期格式错误")?;
    if nd > chrono::Local::now().date_naive() {
        return Err("不能给未来的日期打卡".into());
    }
    let month = date[..7].to_string();
    let habits = load_habits(cfg);
    if !habits.iter().any(|h| h.name == habit) {
        return Err("目标不存在".into());
    }
    let mut data = load_month(cfg, &month);
    let mut row = data.remove(date).unwrap_or_default();
    let v = normalize_state(value);
    if v.is_empty() {
        row.remove(habit);
    } else {
        row.insert(habit.to_string(), v);
    }
    if !row.is_empty() {
        data.insert(date.to_string(), row);
    }
    write_month2(cfg, &month, &habits, &data)?;
    Ok(())
}

#[tauri::command]
pub fn set_checkin(
    state: tauri::State<'_, AppState>,
    habit: String,
    date: String,
    value: String,
) -> Result<(), String> {
    let cfg = state.cfg.lock().unwrap().clone();
    apply_cell(&cfg, &habit, &date, &value)?;
    state.mark_self_write(&month_path(&cfg, &date[..7]));
    Ok(())
}

/// 一次点击在三种状态间循环：未打卡 → 做到了 → 没做到 → 未打卡
#[tauri::command]
pub fn cycle_checkin(
    state: tauri::State<'_, AppState>,
    habit: String,
    date: String,
) -> Result<String, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    if date.len() < 7 {
        return Err("日期格式错误".into());
    }
    let data = load_month(&cfg, &date[..7]);
    let cur = data
        .get(&date)
        .and_then(|r| r.get(&habit))
        .cloned()
        .unwrap_or_default();
    let next = match cur.as_str() {
        "" => "done",
        "done" => "missed",
        _ => "",
    };
    apply_cell(&cfg, &habit, &date, next)?;
    state.mark_self_write(&month_path(&cfg, &date[..7]));
    Ok(next.to_string())
}

fn build_month(cfg: &AppConfig, state: &AppState, month: String) -> MonthView {
    let habits = load_habits(cfg);
    let active: Vec<Habit> = habits.iter().filter(|h| !h.archived).cloned().collect();
    let data = load_month(cfg, &month);
    let today = chrono::Local::now().date_naive();
    let this_month = today.format("%Y-%m").to_string();
    let _ = state;

    let mut days: Vec<DayCell> = vec![];
    let mut summary: BTreeMap<String, Counts> = BTreeMap::new();
    for h in &active {
        summary.insert(h.name.clone(), Counts::default());
    }

    for d in month_days(&month) {
        let Ok(nd) = chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d") else { continue };
        let future = month.as_str() > this_month.as_str() || nd > today;
        let mut states: BTreeMap<String, String> = BTreeMap::new();
        for h in &active {
            let s = data
                .get(&d)
                .and_then(|r| r.get(&h.name))
                .map(|s| if s == "done" || s == "missed" { s.clone() } else { String::new() })
                .unwrap_or_default();
            let c = summary.entry(h.name.clone()).or_default();
            if !future {
                match s.as_str() {
                    "done" => c.done += 1,
                    "missed" => c.missed += 1,
                    _ => c.none += 1,
                }
            }
            states.insert(h.name.clone(), s);
        }
        days.push(DayCell {
            date: d.clone(),
            day: chrono::Datelike::day(&nd),
            weekday: chrono::Datelike::weekday(&nd).num_days_from_monday(),
            states,
            future,
            today: d == today.format("%Y-%m-%d").to_string(),
        });
    }

    for c in summary.values_mut() {
        finalize(c);
    }

    let label = if month.len() >= 7 {
        format!("{} 年 {} 月", &month[..4], &month[5..7])
    } else {
        month.clone()
    };

    MonthView {
        label,
        month,
        habits: active,
        days,
        summary,
    }
}

#[tauri::command]
pub fn get_month(state: tauri::State<'_, AppState>, month: String) -> MonthView {
    let cfg = state.cfg.lock().unwrap().clone();
    build_month(&cfg, &state, month)
}

#[tauri::command]
pub fn habit_stats(state: tauri::State<'_, AppState>, habit: String) -> HabitStats {
    let cfg = state.cfg.lock().unwrap().clone();
    compute_stats(&cfg, &habit)
}

/// 本月视图，供日记页底部的快捷打卡条使用
#[tauri::command]
pub fn today_board(state: tauri::State<'_, AppState>) -> MonthView {
    let cfg = state.cfg.lock().unwrap().clone();
    let month = chrono::Local::now().format("%Y-%m").to_string();
    build_month(&cfg, &state, month)
}

#[tauri::command]
pub fn all_habits_overview(state: tauri::State<'_, AppState>) -> HashMap<String, HabitStats> {
    let cfg = state.cfg.lock().unwrap().clone();
    let habits = load_habits(&cfg);
    habits
        .iter()
        .filter(|h| !h.archived)
        .map(|h| (h.name.clone(), compute_stats(&cfg, &h.name)))
        .collect()
}
