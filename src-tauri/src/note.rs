//! Markdown 文档模型与索引。
//!
//! 设计原则：**文件是唯一真相，索引只是缓存**。索引全放内存，启动扫一遍 vault，
//! 不做 SQLite —— 少一个状态源就少一半 bug。

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use crate::config::AppConfig;
use crate::vault::{abs_path, atomic_write, is_markdown, rel_path};
use crate::AppState;

// ───────────────────────── 数据结构 ─────────────────────────

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NoteMeta {
    pub path: String,
    pub stem: String,
    pub title: String,
    /// diary | note
    pub kind: String,
    /// 日记才有，YYYY-MM-DD
    pub date: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub words: usize,
    pub mtime: u64,
}

#[derive(Default)]
pub struct Index {
    pub notes: HashMap<String, NoteMeta>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
    pub date: String,
    pub words: usize,
    /// 文件最后修改时间（Unix 秒）。前端「按修改时间排」要用。
    ///
    /// 目录一律是 0 —— 目录的 mtime 会因子项增删而变，拿它排序会「抖」，
    /// 所以规则定死：**目录永远按名字排**，这个字段对目录没有意义。
    pub mtime: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DocContent {
    pub path: String,
    pub stem: String,
    pub title: String,
    pub kind: String,
    pub date: String,
    /// frontmatter 反序列化成 JSON 给前端；无 frontmatter 时为 null
    pub frontmatter: serde_json::Value,
    pub body: String,
    pub content: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub backlinks: Vec<NoteMeta>,
    pub words: usize,
    pub mtime: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub meta: NoteMeta,
    pub snippet: String,
    pub score: i32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub notes: usize,
    pub diaries: usize,
    pub words: usize,
    pub links: usize,
    pub orphans: usize,
}

// ───────────────────────── 正则 ─────────────────────────

fn re_wikilink() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"!?\[\[([^\[\]]{1,200}?)\]\]").unwrap())
}

fn re_tag() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?:^|[^\w#\p{L}\p{N}_])#([\p{L}\p{N}_][\p{L}\p{N}_\-/]{0,40})").unwrap())
}

fn re_fm() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // 开头 --- 到 下一个 --- 之间即 frontmatter
    R.get_or_init(|| Regex::new(r"(?s)\A---\r?\n(.*?)\r?\n---[ \t]*\r?\n?").unwrap())
}

fn re_date_name() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\d{4})-(\d{2})-(\d{2})$").unwrap())
}

// ───────────────────────── frontmatter ─────────────────────────

/// 拆出 frontmatter 原文与正文。frontmatter 原文会被解析成 YAML 映射，
/// 未知字段原样保留 —— 用户自己加的键不会被吃掉。
pub fn split_frontmatter(text: &str) -> (Option<String>, String) {
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    match re_fm().captures(t) {
        Some(c) => {
            let fm = c.get(1).map(|m| m.as_str().to_string());
            let body = t[c.get(0).unwrap().end()..].to_string();
            (fm, body)
        }
        None => (None, t.to_string()),
    }
}

pub fn parse_frontmatter(raw: &Option<String>) -> serde_yaml::Value {
    match raw {
        Some(s) if !s.trim().is_empty() => {
            serde_yaml::from_str::<serde_yaml::Value>(s).unwrap_or(serde_yaml::Value::Null)
        }
        _ => serde_yaml::Value::Null,
    }
}

pub fn join_frontmatter(fm: &serde_yaml::Value, body: &str) -> String {
    match fm {
        serde_yaml::Value::Mapping(m) if !m.is_empty() => {
            let y = serde_yaml::to_string(fm).unwrap_or_default();
            format!("---\n{}---\n\n{}", y, body.trim_start_matches(['\n', '\r']))
        }
        _ => body.to_string(),
    }
}

pub fn fm_get<'a>(fm: &'a serde_yaml::Value, key: &str) -> Option<&'a serde_yaml::Value> {
    fm.get(key)
}

pub fn fm_str(fm: &serde_yaml::Value, key: &str) -> String {
    match fm_get(fm, key) {
        Some(serde_yaml::Value::String(s)) => s.clone(),
        Some(serde_yaml::Value::Number(n)) => n.to_string(),
        Some(serde_yaml::Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

// ───────────────────────── 抽取 ─────────────────────────

pub fn extract_tags(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // 标题行剥掉前导 #，避免 "## 小标题" 被当成标签
        let scan = if t.starts_with('#') {
            t.trim_start_matches('#')
        } else {
            line
        };
        for c in re_tag().captures_iter(scan) {
            out.push(c[1].to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 双链目标。`![[img.png]]` 这种嵌入图片不算链接。
pub fn extract_links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in re_wikilink().captures_iter(text) {
        let whole = c.get(0).unwrap().as_str();
        let is_embed = whole.starts_with('!');
        let inner = c.get(1).unwrap().as_str();
        let target = inner.split('|').next().unwrap_or(inner).trim();
        if target.is_empty() {
            continue;
        }
        if is_embed {
            let lower = target.to_ascii_lowercase();
            if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".avif", ".pdf"]
                .iter()
                .any(|e| lower.ends_with(e))
            {
                continue;
            }
        }
        out.push(target.to_string());
    }
    out.sort();
    out.dedup();
    out
}

/// 中英混排的粗粒度字数：CJK 按字计，其余按空白切词。
pub fn count_words(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut latin = String::with_capacity(text.len());
    for ch in text.chars() {
        let u = ch as u32;
        let is_cjk = (0x3040..=0x30FF).contains(&u)
            || (0x3400..=0x4DBF).contains(&u)
            || (0x4E00..=0x9FFF).contains(&u)
            || (0xF900..=0xFAFF).contains(&u)
            || (0xAC00..=0xD7AF).contains(&u);
        if is_cjk {
            cjk += 1;
            latin.push(' ');
        } else {
            latin.push(ch);
        }
    }
    let lw = latin.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count();
    cjk + lw
}

/// 首行标题：`# xxx` 优先，其次 frontmatter.title，最后退回文件名。
pub fn derive_title(body: &str, fm: &serde_yaml::Value, stem: &str) -> String {
    for line in body.lines().take(30) {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            let r = rest.trim();
            if !r.is_empty() {
                return r.to_string();
            }
        }
    }
    let from_fm = fm_str(fm, "title");
    if !from_fm.is_empty() {
        return from_fm;
    }
    stem.to_string()
}

// ───────────────────────── 单篇解析 ─────────────────────────

pub fn parse_doc(cfg: &AppConfig, abs: &Path) -> Option<NoteMeta> {
    let raw = std::fs::read_to_string(abs).ok()?;
    let rel = rel_path(cfg, abs);
    let stem = abs.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let (fm_raw, body) = split_frontmatter(&raw);
    let fm = parse_frontmatter(&fm_raw);

    let is_diary = rel.starts_with(&format!("{}/", cfg.editor.diary_dir));
    let date = if re_date_name().is_match(&stem) {
        stem.clone()
    } else {
        fm_str(&fm, "date")
    };

    let mut tags = extract_tags(&body);
    if let Some(serde_yaml::Value::Sequence(seq)) = fm_get(&fm, "tags") {
        for v in seq {
            if let serde_yaml::Value::String(s) = v {
                tags.push(s.clone());
            }
        }
        tags.sort();
        tags.dedup();
    }

    let mtime = abs
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Some(NoteMeta {
        path: rel,
        stem: stem.clone(),
        title: derive_title(&body, &fm, &stem),
        kind: if is_diary { "diary".into() } else { "note".into() },
        date,
        tags,
        links: extract_links(&body),
        words: count_words(&body),
        mtime,
    })
}

// ───────────────────────── 索引维护 ─────────────────────────

pub fn rebuild_index(state: &AppState, cfg: &AppConfig) {
    let mut idx = Index::default();
    let root = cfg.vault_path();
    walk_md(&root, &mut |p| {
        if let Some(m) = parse_doc(cfg, p) {
            idx.notes.insert(m.path.clone(), m);
        }
    });
    *state.index.lock().unwrap() = idx;
}

pub fn reindex_one(state: &AppState, cfg: &AppConfig, abs: &Path) {
    let rel = rel_path(cfg, abs);
    let mut idx = state.index.lock().unwrap();
    if abs.exists() {
        if let Some(m) = parse_doc(cfg, abs) {
            idx.notes.insert(rel, m);
        }
    } else {
        idx.notes.remove(&rel);
    }
}

fn walk_md(dir: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if crate::vault::is_ignored(&p) {
                continue;
            }
            walk_md(&p, f);
        } else if is_markdown(&p) && !crate::vault::is_ignored(&p) {
            f(&p);
        }
    }
}

fn sorted_notes(state: &AppState) -> Vec<NoteMeta> {
    let idx = state.index.lock().unwrap();
    let mut v: Vec<NoteMeta> = idx.notes.values().cloned().collect();
    v.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    v
}

fn find_by_target(state: &AppState, target: &str) -> Option<NoteMeta> {
    let idx = state.index.lock().unwrap();
    let t = target.trim();
    // 1) 完整相对路径（可省略 .md）
    for cand in [t.to_string(), format!("{t}.md")] {
        let cand = cand.replace('\\', "/");
        if let Some(m) = idx.notes.get(&cand) {
            return Some(m.clone());
        }
    }
    // 2) 文件名（stem）匹配
    let stem = Path::new(t)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| t.to_string());
    let mut best: Option<NoteMeta> = None;
    for m in idx.notes.values() {
        if m.stem == stem {
            // 非日记优先
            if best.is_none() || (best.as_ref().unwrap().kind == "diary" && m.kind != "diary") {
                best = Some(m.clone());
            }
        }
    }
    best
}

// ───────────────────────── 树 ─────────────────────────

/// 只列"笔记"目录（不含日记 / 打卡）—— 呼应用户要求的模块隔离。
///
/// ⚠️ **这里不再排序**，输出顺序只是「按 path 插入」的中间产物。
/// 排序口径整个搬到了前端 `src/lib/notesSort.ts::sortNotesTree()`，理由：
/// 「按修改时间排」需要 `Intl.Collator` 那种能正确处理中文拼音的能力，
/// 后端要复刻就得拖进一整个拼音库。**排序口径只有一份，就是前端那一份。**
pub fn build_notes_tree(cfg: &AppConfig, state: &AppState) -> Vec<TreeNode> {
    let prefix = format!("{}/", cfg.editor.notes_dir);
    let mut items: Vec<NoteMeta> = sorted_notes(state)
        .into_iter()
        .filter(|m| m.path.starts_with(&prefix))
        .collect();

    // 顺手按 path 排一下，让输出本身是确定的（方便调试、也让前端拿到的不是随机序）。
    items.sort_by(|a, b| a.path.cmp(&b.path));
    let mut root: Vec<TreeNode> = Vec::new();

    // 先把磁盘上真实存在的目录铺进去。索引只认 .md 文件，光靠文件反推的话，
    // 用户新建的空文件夹在树里根本不会出现 —— 那就等于「建了看不见」。
    let abs_root = abs_path(cfg, &cfg.editor.notes_dir);
    scan_dirs(&abs_root, &prefix, &mut root);

    for m in items {
        let rel = m.path.trim_start_matches(&prefix);
        let parts: Vec<&str> = rel.split('/').collect();
        insert_node(&mut root, &parts, &m, &prefix);
    }

    for n in root.iter_mut() {
        rollup_words(n);
    }
    root
}

/// 递归扫盘，把目录节点先铺好（空目录也保留）。
fn scan_dirs(abs_dir: &Path, rel_prefix: &str, children: &mut Vec<TreeNode>) {
    let Ok(rd) = std::fs::read_dir(abs_dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() || crate::vault::is_ignored(&p) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let rel = format!("{rel_prefix}{name}");
        let mut node = TreeNode {
            name,
            path: rel.clone(),
            is_dir: true,
            children: vec![],
            date: String::new(),
            words: 0,
            mtime: 0,
        };
        scan_dirs(&p, &format!("{rel}/"), &mut node.children);
        children.push(node);
    }
}

/// 目录节点自己不带字数，把子树里的字数加起来给前端用。
fn rollup_words(n: &mut TreeNode) -> usize {
    if !n.is_dir {
        return n.words;
    }
    let mut sum = 0;
    for c in n.children.iter_mut() {
        sum += rollup_words(c);
    }
    n.words = sum;
    sum
}

fn insert_node(children: &mut Vec<TreeNode>, parts: &[&str], m: &NoteMeta, prefix: &str) {
    if parts.len() == 1 {
        children.push(TreeNode {
            name: m.stem.clone(),
            path: m.path.clone(),
            is_dir: false,
            children: vec![],
            date: m.date.clone(),
            words: m.words,
            mtime: m.mtime,
        });
        return;
    }
    let dir_name = parts[0];
    let dir_path = format!("{prefix}{dir_name}");
    let pos = children.iter().position(|c| c.is_dir && c.name == dir_name);
    let idx = match pos {
        Some(i) => i,
        None => {
            children.push(TreeNode {
                name: dir_name.to_string(),
                path: dir_path,
                is_dir: true,
                children: vec![],
                date: String::new(),
                words: 0,
                mtime: 0,
            });
            children.len() - 1
        }
    };
    insert_node(&mut children[idx].children, &parts[1..], m, prefix);
}

// ───────────────────────── 命令 ─────────────────────────

#[tauri::command]
pub fn list_notes_tree(state: tauri::State<'_, AppState>) -> Vec<TreeNode> {
    let cfg = state.cfg.lock().unwrap().clone();
    build_notes_tree(&cfg, &state)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiaryMonth {
    pub month: String,
    pub label: String,
    pub items: Vec<NoteMeta>,
}

/// 日记按月分组。日记文件按 `日记/YYYY-MM/YYYY-MM-DD.md` 落盘。
#[tauri::command]
pub fn list_diaries(state: tauri::State<'_, AppState>) -> Vec<DiaryMonth> {
    let cfg = state.cfg.lock().unwrap().clone();
    let prefix = format!("{}/", cfg.editor.diary_dir);
    let mut map: HashMap<String, Vec<NoteMeta>> = HashMap::new();
    for m in sorted_notes(&state) {
        if !m.path.starts_with(&prefix) {
            continue;
        }
        let month = if m.date.len() >= 7 {
            m.date[..7].to_string()
        } else {
            m.path
                .trim_start_matches(&prefix)
                .split('/')
                .next()
                .unwrap_or("")
                .to_string()
        };
        map.entry(month).or_default().push(m);
    }
    let mut out: Vec<DiaryMonth> = map
        .into_iter()
        .map(|(month, mut items)| {
            items.sort_by(|a, b| b.date.cmp(&a.date));
            DiaryMonth {
                label: month.clone(),
                month,
                items,
            }
        })
        .collect();
    out.sort_by(|a, b| b.month.cmp(&a.month));
    out
}

#[tauri::command]
pub fn read_doc(state: tauri::State<'_, AppState>, rel: String) -> Result<DocContent, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let abs = abs_path(&cfg, &rel);
    let content = std::fs::read_to_string(&abs).map_err(|e| format!("读取失败: {e}"))?;
    let stem = abs.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let (fm_raw, body) = split_frontmatter(&content);
    let fm = parse_frontmatter(&fm_raw);

    let is_diary = rel.starts_with(&format!("{}/", cfg.editor.diary_dir));
    let date = if re_date_name().is_match(&stem) {
        stem.clone()
    } else {
        fm_str(&fm, "date")
    };
    let title = derive_title(&body, &fm, &stem);

    // 反链：谁的 links 指到了我
    let targets: Vec<String> = {
        let mut v = vec![stem.clone(), title.clone()];
        v.push(rel.trim_end_matches(".md").to_string());
        v
    };
    let backlinks: Vec<NoteMeta> = {
        let idx = state.index.lock().unwrap();
        idx.notes
            .values()
            .filter(|m| m.path != rel)
            .filter(|m| m.links.iter().any(|l| targets.iter().any(|t| t == l)))
            .cloned()
            .collect()
    };

    let mtime = abs
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(DocContent {
        path: rel.clone(),
        stem,
        title,
        kind: if is_diary { "diary".into() } else { "note".into() },
        date,
        frontmatter: serde_json::to_value(&fm).unwrap_or(serde_json::Value::Null),
        body: body.clone(),
        content,
        tags: extract_tags(&body),
        links: extract_links(&body),
        backlinks,
        words: count_words(&body),
        mtime,
    })
}

#[tauri::command]
pub fn write_doc(
    state: tauri::State<'_, AppState>,
    rel: String,
    content: String,
) -> Result<NoteMeta, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let abs = abs_path(&cfg, &rel);
    atomic_write(&abs, &content).map_err(|e| format!("保存失败: {e}"))?;
    state.mark_self_write(&abs);
    let m = parse_doc(&cfg, &abs).ok_or("保存后解析失败")?;
    state.index.lock().unwrap().notes.insert(rel, m.clone());
    Ok(m)
}

#[tauri::command]
pub fn update_diary_meta(
    state: tauri::State<'_, AppState>,
    rel: String,
    key: String,
    value: String,
) -> Result<DocContent, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let abs = abs_path(&cfg, &rel);
    let content = std::fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    let (fm_raw, body) = split_frontmatter(&content);
    let mut fm = parse_frontmatter(&fm_raw);
    if !matches!(fm, serde_yaml::Value::Mapping(_)) {
        fm = serde_yaml::Value::Mapping(Default::default());
    }
    if value.trim().is_empty() {
        fm.as_mapping_mut().unwrap().remove(serde_yaml::Value::String(key.clone()));
    } else {
        fm.as_mapping_mut()
            .unwrap()
            .insert(serde_yaml::Value::String(key), serde_yaml::Value::String(value));
    }
    let out = join_frontmatter(&fm, &body);
    atomic_write(&abs, &out).map_err(|e| e.to_string())?;
    state.mark_self_write(&abs);
    if let Some(m) = parse_doc(&cfg, &abs) {
        state.index.lock().unwrap().notes.insert(rel.clone(), m);
    }
    read_doc(state, rel)
}

#[tauri::command]
pub fn create_note(
    state: tauri::State<'_, AppState>,
    title: String,
    folder: Option<String>,
) -> Result<String, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let clean = sanitize_title(&title);
    if clean.is_empty() {
        return Err("标题不能为空".into());
    }
    let dir = match folder {
        Some(f) if !f.trim().is_empty() => format!("{}/{}", cfg.editor.notes_dir, f.trim_matches('/')),
        _ => cfg.editor.notes_dir.clone(),
    };
    let mut rel = format!("{dir}/{clean}.md");
    let mut n = 1;
    while abs_path(&cfg, &rel).exists() {
        rel = format!("{dir}/{clean}-{n}.md");
        n += 1;
    }
    let body = format!("# {}\n\n", clean);
    atomic_write(&abs_path(&cfg, &rel), &body).map_err(|e| e.to_string())?;
    state.mark_self_write(&abs_path(&cfg, &rel));
    if let Some(m) = parse_doc(&cfg, &abs_path(&cfg, &rel)) {
        state.index.lock().unwrap().notes.insert(rel.clone(), m);
    }
    Ok(rel)
}

#[tauri::command]
pub fn create_diary(state: tauri::State<'_, AppState>, date: String) -> Result<String, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let d = if re_date_name().is_match(&date) {
        date.clone()
    } else {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    };
    let month = d[..7].to_string();
    let rel = format!("{}/{month}/{d}.md", cfg.editor.diary_dir);
    let abs = abs_path(&cfg, &rel);
    if abs.exists() {
        return Ok(rel);
    }
    // 0.1.2 起新日记不再预置任何标题和小标题：
    // 日期已经在面板和文件名的位置上了，正文再顶一个 `# 2026-09-19 周六` 是重复，
    // 那三个固定小标题则逼着人先删一遍。新日记就该是一张白纸。
    // 想预置内容的话，改配置里的 diary_template 即可（默认空）。
    //
    // 0.2.0 把 frontmatter 也收窄到只剩 `date`：`weather` / `mood` 全项目没有
    // 任何 UI 读（两个 chip 已经删了），留着只是每天打开都看见两行死字段。
    // 老日记里的这两行**不动** —— 读的时候不受影响，没必要回头改写用户的文件。
    // 编辑态里这一整块还会被折成一枚标签，想改也点得开。
    let fm = format!("---\ndate: {d}\n---\n\n");
    let body = cfg.editor.diary_template.as_str();
    atomic_write(&abs, &format!("{fm}{body}")).map_err(|e| e.to_string())?;
    state.mark_self_write(&abs);
    if let Some(m) = parse_doc(&cfg, &abs) {
        state.index.lock().unwrap().notes.insert(rel.clone(), m);
    }
    Ok(rel)
}

/// 软删除：移动到 `.mnesphere/trash/`，可恢复，且该目录不进 git。
#[tauri::command]
pub fn delete_doc(state: tauri::State<'_, AppState>, rel: String) -> Result<(), String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let abs = abs_path(&cfg, &rel);
    if !abs.exists() {
        return Err("文件不存在".into());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let name = abs.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let dest_dir = cfg.vault_path().join(".mnesphere").join("trash");
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest = dest_dir.join(format!("{stamp}__{name}"));
    std::fs::rename(&abs, &dest).map_err(|e| format!("移入回收站失败: {e}"))?;
    state.mark_self_write(&abs);
    state.index.lock().unwrap().notes.remove(&rel);
    Ok(())
}

#[tauri::command]
pub fn rename_doc(
    state: tauri::State<'_, AppState>,
    rel: String,
    new_title: String,
) -> Result<String, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let abs = abs_path(&cfg, &rel);
    let clean = sanitize_title(&new_title);
    if clean.is_empty() {
        return Err("名称不能为空".into());
    }
    let dir = abs.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| cfg.vault_path());
    let mut dest = dir.join(format!("{clean}.md"));
    let mut n = 1;
    while dest.exists() && dest != abs {
        dest = dir.join(format!("{clean}-{n}.md"));
        n += 1;
    }
    std::fs::rename(&abs, &dest).map_err(|e| format!("重命名失败: {e}"))?;
    state.mark_self_write(&abs);
    state.mark_self_write(&dest);
    state.index.lock().unwrap().notes.remove(&rel);
    let new_rel = rel_path(&cfg, &dest);
    if let Some(m) = parse_doc(&cfg, &dest) {
        state.index.lock().unwrap().notes.insert(new_rel.clone(), m);
    }
    Ok(new_rel)
}

// ───────────────────────── 文件夹 ─────────────────────────
//
// 目录只允许在「笔记」目录下操作。日记按月自动分组、打卡有固定结构，
// 让用户在里面随便建目录只会把这两套约定搅乱。

fn notes_root_rel(cfg: &AppConfig) -> String {
    cfg.editor.notes_dir.clone()
}

/// 校验目标目录确实在笔记目录内，防止 `../..` 之类的相对路径越界。
fn guard_under_notes(cfg: &AppConfig, rel: &str) -> Result<(), String> {
    let root = notes_root_rel(cfg);
    let r = rel.trim_matches('/');
    if r == root || r.starts_with(&format!("{root}/")) {
        Ok(())
    } else {
        Err("只能操作「笔记」目录下的文件夹".into())
    }
}

#[tauri::command]
pub fn create_folder(
    state: tauri::State<'_, AppState>,
    parent: Option<String>,
    name: String,
) -> Result<String, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let clean = sanitize_title(&name);
    if clean.is_empty() {
        return Err("文件夹名不能为空".into());
    }
    let base = match parent {
        Some(p) if !p.trim().is_empty() => p.trim_matches('/').to_string(),
        _ => notes_root_rel(&cfg),
    };
    guard_under_notes(&cfg, &base)?;

    let rel = format!("{base}/{clean}");
    let abs = abs_path(&cfg, &rel);
    if abs.exists() {
        return Err(format!("已存在同名文件夹：{clean}"));
    }
    std::fs::create_dir_all(&abs).map_err(|e| format!("创建失败: {e}"))?;
    state.mark_self_write(&abs);
    Ok(rel)
}

#[tauri::command]
pub fn rename_folder(
    state: tauri::State<'_, AppState>,
    rel: String,
    new_name: String,
) -> Result<String, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    guard_under_notes(&cfg, &rel)?;
    let clean = sanitize_title(&new_name);
    if clean.is_empty() {
        return Err("文件夹名不能为空".into());
    }
    let abs = abs_path(&cfg, &rel);
    if !abs.is_dir() {
        return Err("文件夹不存在".into());
    }
    if abs == abs_path(&cfg, &notes_root_rel(&cfg)) {
        return Err("笔记根目录不能改名".into());
    }
    let parent = abs.parent().map(|p| p.to_path_buf()).ok_or("路径异常")?;
    let dest = parent.join(&clean);
    if dest.exists() {
        return Err(format!("已存在同名文件夹：{clean}"));
    }
    std::fs::rename(&abs, &dest).map_err(|e| format!("重命名失败: {e}"))?;
    state.mark_self_write(&abs);
    state.mark_self_write(&dest);
    // 里面的笔记路径全变了，整体重建索引最省心
    rebuild_index(&state, &cfg);
    Ok(rel_path(&cfg, &dest))
}

/// 软删除：连同里面的内容一起移进 `.mnesphere/trash/`，可捞回。
#[tauri::command]
pub fn delete_folder(state: tauri::State<'_, AppState>, rel: String) -> Result<(), String> {
    let cfg = state.cfg.lock().unwrap().clone();
    guard_under_notes(&cfg, &rel)?;
    let abs = abs_path(&cfg, &rel);
    if !abs.is_dir() {
        return Err("文件夹不存在".into());
    }
    if abs == abs_path(&cfg, &notes_root_rel(&cfg)) {
        return Err("笔记根目录不能删除".into());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let name = abs.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let dest_dir = cfg.vault_path().join(".mnesphere").join("trash");
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest = dest_dir.join(format!("{stamp}__{name}"));
    std::fs::rename(&abs, &dest).map_err(|e| format!("移入回收站失败: {e}"))?;
    state.mark_self_write(&abs);
    rebuild_index(&state, &cfg);
    Ok(())
}

#[tauri::command]
pub fn search_notes(state: tauri::State<'_, AppState>, query: String) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let cfg = state.cfg.lock().unwrap().clone();
    let mut hits: Vec<SearchHit> = Vec::new();
    for m in sorted_notes(&state) {
        let abs = abs_path(&cfg, &m.path);
        let Ok(raw) = std::fs::read_to_string(&abs) else { continue };
        let (_, body) = split_frontmatter(&raw);
        let low_body = body.to_lowercase();
        let low_title = m.title.to_lowercase();

        let mut score = 0i32;
        if low_title.contains(&q) {
            score += 100;
        }
        if m.stem.to_lowercase().contains(&q) {
            score += 60;
        }
        if m.tags.iter().any(|t| t.to_lowercase().contains(&q)) {
            score += 40;
        }
        let hits_in_body = low_body.matches(&q).count() as i32;
        if hits_in_body > 0 {
            score += 10 + hits_in_body.min(20);
        }
        if score == 0 {
            continue;
        }

        let snippet = make_snippet(&body, &q);
        hits.push(SearchHit { meta: m, snippet, score });
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score));
    hits.truncate(80);
    hits
}

fn make_snippet(body: &str, q: &str) -> String {
    let low = body.to_lowercase();
    let Some(pos) = low.find(q) else {
        return body.chars().take(120).collect();
    };
    // 用字符边界安全切片
    let chars: Vec<char> = body.chars().collect();
    let byte_to_char = |b: usize| body[..b.min(body.len())].chars().count();
    let c = byte_to_char(pos);
    let start = c.saturating_sub(30);
    let end = (c + 90).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    s = s.replace('\n', " ");
    if start > 0 {
        s = format!("…{s}");
    }
    if end < chars.len() {
        s.push('…');
    }
    s
}

#[tauri::command]
pub fn resolve_link(state: tauri::State<'_, AppState>, target: String) -> Option<String> {
    find_by_target(&state, &target).map(|m| m.path)
}

#[tauri::command]
pub fn all_tags(state: tauri::State<'_, AppState>) -> Vec<(String, usize)> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for m in state.index.lock().unwrap().notes.values() {
        for t in &m.tags {
            *map.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let mut v: Vec<(String, usize)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

#[tauri::command]
pub fn index_stats(state: tauri::State<'_, AppState>) -> IndexStats {
    let idx = state.index.lock().unwrap();
    let notes = idx.notes.values().filter(|m| m.kind == "note").count();
    let diaries = idx.notes.values().filter(|m| m.kind == "diary").count();
    let words = idx.notes.values().map(|m| m.words).sum();
    let links: usize = idx.notes.values().map(|m| m.links.len()).sum();
    let orphans = idx
        .notes
        .values()
        .filter(|m| {
            m.links.is_empty()
                && !idx.notes.values().any(|o| o.path != m.path && o.links.contains(&m.stem))
        })
        .count();
    IndexStats { notes, diaries, words, links, orphans }
}

// ───────────────────────── 杂项 ─────────────────────────

pub fn sanitize_title(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\n' | '\r' | '\t'))
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .chars()
        .take(120)
        .collect()
}

