//! 文件系统层：原子写、路径工具、vault 初始化、文件监听。
//!
//! 两条纪律：
//! 1. **所有写入都是「临时文件 + rename」**，否则 watcher 会读到半截文件把索引搞崩。
//! 2. **自己写的变更必须被 watcher 忽略**，否则「写 → 触发 → 重索引 → 再写」会无限循环。

use notify::{RecursiveMode, Watcher};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::AppConfig;
use crate::AppState;

const TMP_TAG: &str = ".mnesphere-tmp";

/// 原子写：同目录临时文件 → rename 覆盖。
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    atomic_write_bytes(path, content.as_bytes())
}

/// 同上，只是内容不是 UTF-8 字符串（图片这类二进制）。
pub fn atomic_write_bytes(path: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".into());
    let tmp = path.with_file_name(format!(".{name}{TMP_TAG}{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
    }
    // Windows 上 std::fs::rename 走 MoveFileEx(REPLACE_EXISTING)，可以直接覆盖
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ───────────────────────── 路径工具 ─────────────────────────

/// 拼接 vault 内路径，并阻断 `..` 逃逸。
pub fn abs_path(cfg: &AppConfig, rel: &str) -> PathBuf {
    let clean = sanitize_rel(rel);
    cfg.vault_path().join(clean)
}

pub fn sanitize_rel(rel: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            // 绝对路径 / 盘符 / 父目录全部丢弃，防止越出 vault
            _ => {}
        }
    }
    out
}

/// vault 相对路径，统一正斜杠，便于前端和 GitHub 使用。
pub fn rel_path(cfg: &AppConfig, abs: &Path) -> String {
    let v = cfg.vault_path();
    match abs.strip_prefix(&v) {
        Ok(p) => p.to_string_lossy().replace('\\', "/"),
        Err(_) => abs.to_string_lossy().replace('\\', "/"),
    }
}

pub fn is_markdown(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("md") | Some("markdown") | Some("mdx")
    )
}

/// 这些路径的变更一律不理会。
pub fn is_ignored(p: &Path) -> bool {
    let s = p.to_string_lossy();
    if s.contains(TMP_TAG) || s.contains("~$") || s.ends_with(".swp") {
        return true;
    }
    for c in p.components() {
        if let Component::Normal(n) = c {
            let n = n.to_string_lossy();
            if n == ".mnesphere" || n == ".git" || n == "node_modules" || n == ".obsidian" {
                return true;
            }
        }
    }
    false
}

// ───────────────────────── vault 初始化 ─────────────────────────

pub fn ensure_vault(cfg: &AppConfig) -> Result<(), String> {
    let v = cfg.vault_path();
    std::fs::create_dir_all(&v).map_err(|e| format!("无法创建 vault 目录: {e}"))?;
    for sub in [
        cfg.editor.diary_dir.clone(),
        cfg.editor.notes_dir.clone(),
        cfg.editor.checkin_dir.clone(),
        cfg.editor.task_dir.clone(),
        cfg.editor.attachments_dir.clone(),
        ".mnesphere".to_string(),
    ] {
        std::fs::create_dir_all(v.join(sub)).map_err(|e| format!("无法创建子目录: {e}"))?;
    }
    // 让 .mnesphere 不进 git
    let gi = v.join(".gitignore");
    let want = ".mnesphere/\n.DS_Store\nThumbs.db\n";
    let cur = std::fs::read_to_string(&gi).unwrap_or_default();
    if !cur.contains(".mnesphere") {
        let merged = format!("{cur}{want}");
        let _ = atomic_write(&gi, &merged);
    }
    // 首次落地一篇欢迎笔记
    let readme = v.join(&cfg.editor.notes_dir).join("开始使用.md");
    if !readme.exists() {
        let _ = atomic_write(&readme, WELCOME);
    }
    Ok(())
}

const WELCOME: &str = r#"# 开始使用 mnesphere

这个文件本身就是一个普通的 Markdown 文件。整个 vault 里的**每一条内容都是 md**，
你可以随时用 Obsidian、VS Code 或者记事本直接打开它。

## 双链

用 `[[笔记名]]` 链接另一篇笔记，侧栏会出现反链。比如：[[打卡说明]]

## 图片

直接把图片**粘贴**（Ctrl+V）或拖进编辑器，会自动存到 `附件/年-月/` 并在光标处插好链接。

手写的话推荐用 Obsidian 那套 `![[附件/xxx.png]]` —— 它的路径从 vault 根算起，
笔记放在哪一层都不会断；换成 `![](附件/xxx.png)` 只有笔记正好在 vault 根时才对。

## 打卡

打卡数据在 `打卡/` 目录下，按月一个文件，格式是一张表格：

| 日期 | 晨跑 5km |
| --- | --- |
| 2026-09-19 | done |

取值只有三种：`done`（做到了）、`missed`（没做到）、留空（未打卡）。

## 同步

设置里填好仓库和 Token，就能把整个 vault 推到 GitHub —— 一次同步一个 commit。
"#;

// ───────────────────────── 文件监听 ─────────────────────────

pub fn restart_watcher(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let mut guard = state.watcher.lock().unwrap();
    *guard = None; // 丢掉旧 watcher，先停止再重建

    let vault = state.cfg.lock().unwrap().vault.clone();
    if vault.is_empty() || !Path::new(&vault).exists() {
        return Ok(());
    }

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut w = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| e.to_string())?;
    w.watch(Path::new(&vault), RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    *guard = Some(w);
    drop(guard);

    let app2 = app.clone();
    std::thread::spawn(move || {
        let mut pending: Vec<PathBuf> = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(350)) {
                Ok(Ok(ev)) => {
                    for p in ev.paths {
                        if !is_ignored(&p) {
                            pending.push(p);
                        }
                    }
                }
                Ok(Err(_)) => {}
                Err(RecvTimeoutError::Timeout) => {
                    if pending.is_empty() {
                        continue;
                    }
                    let st = app2.state::<AppState>();
                    let cfg = st.cfg.lock().unwrap().clone();
                    let mut touched: Vec<String> = Vec::new();
                    let batch: Vec<PathBuf> = pending.drain(..).collect();
                    for p in batch {
                        if st.is_self_write(&p) {
                            continue;
                        }
                        if is_markdown(&p) {
                            crate::note::reindex_one(&st, &cfg, &p);
                        }
                        let r = rel_path(&cfg, &p);
                        if !touched.contains(&r) {
                            touched.push(r);
                        }
                    }
                    if !touched.is_empty() {
                        let _ = app2.emit("vault-changed", touched);
                    }
                    // 打卡文件被外部改动也要重算
                    let _ = app2.emit("refresh-stats", ());
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    Ok(())
}

// ───────────────────────── 命令 ─────────────────────────

#[tauri::command]
pub fn open_in_explorer(path: String, reveal: Option<bool>) -> Result<(), String> {
    let p = PathBuf::from(&path);
    let target = if reveal.unwrap_or(false) && p.is_file() {
        p.parent().map(|x| x.to_path_buf()).unwrap_or(p.clone())
    } else {
        p.clone()
    };
    if !target.exists() {
        return Err(format!("路径不存在: {}", target.display()));
    }
    std::process::Command::new("explorer")
        .arg(target.as_os_str())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")) {
        return Err("只允许打开 http / https / mailto 链接".into());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", u])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(opener)
            .arg(u)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn reveal_path(state: tauri::State<'_, AppState>, rel: String) -> Result<String, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    Ok(abs_path(&cfg, &rel).to_string_lossy().to_string())
}

/// 在附件目录里挑一个不冲突的落脚点：按年月分目录归档，重名自动加 `-N`。
fn attachment_slot(cfg: &AppConfig, stem: &str, ext: &str, stamp: &str) -> Result<String, String> {
    let dir = format!("{}/{stamp}", cfg.editor.attachments_dir);
    std::fs::create_dir_all(abs_path(cfg, &dir)).map_err(|e| e.to_string())?;
    let mut candidate = format!("{dir}/{stem}{ext}");
    let mut n = 1;
    while abs_path(cfg, &candidate).exists() {
        candidate = format!("{dir}/{stem}-{n}{ext}");
        n += 1;
    }
    Ok(candidate)
}

/// 把外部文件复制进 vault 的附件目录，返回可用的相对路径。
#[tauri::command]
pub fn import_attachment(
    state: tauri::State<'_, AppState>,
    sources: Vec<String>,
) -> Result<Vec<String>, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let mut out = Vec::new();
    for s in sources {
        let src = PathBuf::from(&s);
        if !src.is_file() {
            continue;
        }
        let name = src.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let (stem, ext) = match name.rsplit_once('.') {
            Some((a, b)) => (a.to_string(), format!(".{b}")),
            None => (name.clone(), String::new()),
        };
        // 附件按年月归档，避免单目录堆成千上万个文件
        let stamp = chrono::Local::now().format("%Y-%m").to_string();
        let candidate = attachment_slot(&cfg, &stem, &ext, &stamp)?;
        std::fs::copy(&src, abs_path(&cfg, &candidate)).map_err(|e| e.to_string())?;
        state.mark_self_write(&abs_path(&cfg, &candidate));
        out.push(candidate);
    }
    Ok(out)
}

/// 只认魔数，不信前端报的 MIME：剪贴板里的类型标注未必靠谱，认错了后缀，
/// asset 协议回给 webview 的 content-type 就跟着错，图直接显示不出来。
fn sniff_image_ext(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if b.starts_with(b"BM") {
        return Some("bmp");
    }
    if b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

/// 粘贴进来的图片：前端把原始字节直接当 IPC body 发过来，这里落盘并返回 vault 相对路径。
///
/// 为什么参数写成 `Request` 而不是普通字段：图片是二进制，`Uint8Array` 作 payload 时
/// Tauri 会把 body 原样设成那些字节、content-type 设成 `application/octet-stream`
/// （见 tauri/scripts/process-ipc-message-fn.js）。也就是说整段 body 被图片占满，
/// 再也塞不下 JSON 字段 —— 文件名只能在这儿自己造。
#[tauri::command]
pub fn save_pasted_image(
    state: tauri::State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(b) => b.as_slice(),
        _ => return Err("粘贴过来的不是二进制数据".into()),
    };
    if bytes.is_empty() {
        return Err("粘贴内容为空".into());
    }
    // 一张 4K 截图就能到十几 MB，给个体积上限，免得手滑粘进来个几百 MB 的东西
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "图片太大了（{:.1} MB，上限 32 MB）",
            bytes.len() as f64 / 1_048_576.0
        ));
    }
    let ext = sniff_image_ext(bytes).ok_or("这段数据看着不像图片")?;

    let cfg = state.cfg.lock().unwrap().clone();
    let now = chrono::Local::now();
    let stamp = now.format("%Y-%m").to_string();
    let stem = format!("粘贴-{}", now.format("%Y%m%d-%H%M%S"));
    let rel = attachment_slot(&cfg, &stem, &format!(".{ext}"), &stamp)?;
    let abs = abs_path(&cfg, &rel);
    atomic_write_bytes(&abs, bytes).map_err(|e| e.to_string())?;
    state.mark_self_write(&abs);
    Ok(rel)
}

/// 引用计数归零的临时清理：删除一定时间前仍在的 tmp 残留。
pub fn sweep_tmp(vault: &Path) {
    // 用嵌套 fn 而不是闭包：闭包在自己体内拿不到自己的名字，递归不了。
    fn walk(dir: &Path) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if !is_ignored(&p) {
                        walk(&p);
                    }
                } else if p.to_string_lossy().contains(TMP_TAG) {
                    // 只清掉超过 1 天的残留
                    if let Ok(md) = p.metadata() {
                        if let Ok(modi) = md.modified() {
                            if let Ok(d) = modi.elapsed() {
                                if d.as_secs() > 86_400 {
                                    let _ = std::fs::remove_file(&p);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    walk(vault);
}
