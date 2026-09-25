//! GitHub 同步：走 Git Data API，**一次同步只产生一个 commit**。
//!
//! 为什么不用本地 git：
//! - 用户不需要装 git，仓库可以纯粹当存储用
//! - 不用处理 `.git` 目录，也不怕它跟着 vault 一起被同步来同步去
//! - 可以直接比对 Git 原生的 blob 哈希，只推真正变了的文件
//!
//! 同步是**双向**的，而它的核心是**三方比较**：
//! ```text
//! 基线 base   —— 上次同步时本地文件对应的那个 commit（配置里的 lastCommit）
//! 本地 local  —— 现在盘上有什么
//! 远端 remote —— 现在远端有什么
//! ```
//! 只有三份齐全，才能回答「这个差异是谁造成的」。少了 base 就只剩猜，
//! 而猜错的代价是删掉用户的笔记 —— 所以基线缺失时一律落到最保守的分支。
//! 判定规则集中在 `plan()`，改同步逻辑请只看那一个函数。

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::TimeZone;

use crate::config::{get_secret, AppConfig};
use crate::vault::is_ignored;

const API: &str = "https://api.github.com";
const UA: &str = "mnesphere";
const MAX_FILE: u64 = 10 * 1024 * 1024;

/// 空仓库的「立图」种子文件。
///
/// Git Data API 的 `POST /git/blobs` 与 `POST /git/trees` 在**零提交**的仓库上
/// 一律返回 409 `Git Repository is empty.` —— 因为 blob 必须挂在一棵已存在的提交图上。
/// 而 Contents API 不受这个限制，是唯一能在空仓库上落地的接口。
///
/// 所以空仓库时先用它埋一笔种子提交，把提交图立起来；随后我们建的**根提交**
/// （无 parents）会把种子整个甩掉 —— 远端最终只留干净的一笔提交，看不到这个文件。
const SEED_PATH: &str = ".mnesphere-seed";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// 从远端写进本地的
    pub pulled: Vec<String>,
    /// 远端删过、本地没动过 → 本地跟着删掉的
    pub local_deleted: Vec<String>,
    /// 本地 → 远端
    pub pushed: Vec<String>,
    /// 本地删过、远端没动过 → 远端跟着删掉的
    pub deleted: Vec<String>,
    /// 两边都改了。本地原件不动，远端那份已另存副本（见 `conflicts` 里的路径）
    pub conflicts: Vec<ConflictItem>,
    pub unchanged: usize,
    pub skipped: Vec<String>,
    pub commit: String,
    pub url: String,
    pub branch: String,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConflictItem {
    /// 冲突的文件（本地这份原封不动）
    pub path: String,
    /// 远端那份被存到了哪儿
    pub saved_as: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub full_name: String,
    pub default_branch: String,
    pub private: bool,
    pub html_url: String,
    /// 仓库存在但一个提交都没有。首次同步会自动把提交图立起来 —— 这不是错误。
    pub empty: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub login: String,
    pub name: String,
    /// Token 带的 scope。fine-grained token 不返回这个头，届时是空数组。
    pub scopes: Vec<String>,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

fn req(
    c: &reqwest::Client,
    method: reqwest::Method,
    path: &str,
    token: &str,
) -> reqwest::RequestBuilder {
    c.request(method, format!("{API}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", UA)
}

/// 把用户可能填进来的各种写法统一成 `owner/repo`。
///
/// 用户不会去记格式，往输入框里粘网址是**正常行为**，不该报 404。接受：
/// ```text
/// https://github.com/owner/repo        https://github.com/owner/repo.git
/// http://github.com/owner/repo         git@github.com:owner/repo.git
/// ssh://git@github.com/owner/repo      github.com/owner/repo
/// owner/repo
/// ```
pub fn normalize_repo(input: &str) -> Result<String, String> {
    let mut s = input.trim().to_string();
    if s.is_empty() {
        return Err("还没有填仓库地址".into());
    }

    if let Some(rest) = s.strip_prefix("git@") {
        // git@github.com:owner/repo(.git)
        if let Some((_, path)) = rest.split_once(':') {
            s = path.to_string();
        }
    } else {
        for p in ["https://", "http://", "ssh://"] {
            if let Some(rest) = s.strip_prefix(p) {
                s = rest.to_string();
                break;
            }
        }
        // 去掉开头的 host：`github.com/owner/repo` → `owner/repo`。
        // 用「含 `.` 或 `@`」来认 host —— GitHub 的账号名和仓库名都不允许这两种字符，
        // 所以这样判断不会误伤 `owner/repo`。
        if let Some((host, rest)) = s.split_once('/') {
            if host.contains('.') || host.contains('@') {
                s = rest.to_string();
            }
        }
    }

    let s = s.trim_end_matches('/').trim_end_matches(".git").to_string();
    let parts: Vec<&str> = s.split('/').filter(|x| !x.is_empty()).collect();
    if parts.len() != 2 {
        return Err(format!(
            "仓库要写成 `owner/repo` 的形式（也可以直接粘 GitHub 网址）。当前填的是：{input}"
        ));
    }
    Ok(format!("{}/{}", parts[0], parts[1]))
}

/// Git 原生 blob 哈希 = sha1("blob {len}\0" + content)
fn blob_sha(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn collect_files(vault: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = vec![];
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if is_ignored(&p) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                // 跳过编辑器临时文件
                if name.ends_with('~') || name.starts_with("~$") {
                    continue;
                }
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push((rel.to_string_lossy().replace('\\', "/"), p.clone()));
                }
            }
        }
    }
    walk(vault, vault, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

async fn gh_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    let txt = resp.text().await.unwrap_or_default();
    // 尽量只取 GitHub 的 message 字段 —— 整段 JSON 糊在界面上没人看得下去
    let msg = serde_json::from_str::<Value>(&txt)
        .ok()
        .and_then(|v| v["message"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| txt.chars().take(300).collect());
    match status.as_u16() {
        401 => {
            format!("认证失败（401）：Token 无效或已过期。\n去「仓库配置」里换一个新的 Token。\n{msg}")
        }
        403 => {
            format!("权限不足或被限流（403）：确认 Token 勾了 repo 权限，且没超出调用上限。\n{msg}")
        }
        404 => format!("找不到（404）：确认 `owner/repo` 拼写正确、且 Token 对它有权限。\n{msg}"),
        409 => format!("冲突（409）：{msg}"),
        422 => format!("请求被拒绝（422）：{msg}"),
        _ => format!("GitHub 返回 {status}：{msg}"),
    }
}

// ───────────────────────── 同步核心 ─────────────────────────

/// 远端树里的一个条目
struct RemoteEntry {
    sha: String,
    /// 只在「要不要下载」这个判断上用一次，省掉对超大文件的下载
    size: u64,
}

/// 一个文件在这次同步里的最终去向。
/// 没有 `Nothing` —— 计划表里**缺席**就是「无事可做」。
#[derive(Clone, Debug, PartialEq)]
enum Action {
    /// 远端 → 本地
    Download,
    /// 本地 → 远端。`resurrect` = 远端删了但它本地有改动，属于「把它救回来」
    Upload { resurrect: bool },
    /// 远端删过 → 本地也删
    DeleteLocal,
    /// 本地删过 → 远端也删
    DeleteRemote,
    /// 两边都改了 → 本地原件不动，远端那份落成副本
    Conflict,
}

/// 把远端来的路径当**不可信输入**处理。
///
/// 仓库里的内容不是我们写的 —— 任何拿到这个 Token 的人（或者哪天你手滑在网页上
/// 改了一把）都能往 tree 里塞一个 `../../../AppData/...` 的路径。不校验就等于
/// 在自己机器上开了一个任意文件写入接口。返回 `None` = 这个条目直接丢弃。
fn safe_rel_path(p: &str) -> Option<String> {
    if p.is_empty() || p == SEED_PATH || p.contains('\0') {
        return None;
    }
    let norm = p.replace('\\', "/");
    if norm.starts_with('/') {
        return None;
    }
    // Windows 盘符（`C:`）与 UNC 的起点
    let bytes = norm.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return None;
    }
    let mut out: Vec<&str> = Vec::new();
    for seg in norm.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return None;
        }
        out.push(seg);
    }
    Some(out.join("/"))
}

/// 本地清单：相对路径 → (blob sha, 绝对路径)。
///
/// 同时返回**「盘上有、但我们读不了」的路径集合**（超过 `MAX_FILE` 的、读权限出错的）。
///
/// ⚠️ 第二个返回值不是可选装饰，是安全阀。这些文件进不了 `local`，于是在判定里
/// 看起来就**跟「本地没有这个文件」一模一样** —— 而「本地没有」在下一条规则里
/// 正好意味着「远端那份可以删」。一个 15 MB 的录音会因此把你远端的原件抹掉。
/// 所以调用方必须拿它把这些路径从计划里摘出去。
fn collect_local(
    vault: &Path,
    skipped: &mut Vec<String>,
) -> (HashMap<String, (String, PathBuf)>, std::collections::HashSet<String>) {
    let mut out: HashMap<String, (String, PathBuf)> = HashMap::new();
    let mut unusable: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (rel, abs) in collect_files(vault) {
        let Ok(md) = abs.metadata() else {
            unusable.insert(rel.clone());
            skipped.push(format!("{rel}（读不到文件信息，已跳过）"));
            continue;
        };
        if md.len() > MAX_FILE {
            unusable.insert(rel.clone());
            skipped.push(format!("{rel}（{} MB，超过 10 MB）", md.len() / 1024 / 1024));
            continue;
        }
        match std::fs::read(&abs) {
            Ok(bytes) => {
                out.insert(rel, (blob_sha(&bytes), abs));
            }
            Err(e) => {
                unusable.insert(rel.clone());
                skipped.push(format!("{rel}（读取失败：{e}）"));
            }
        }
    }
    (out, unusable)
}

/// commit sha → tree sha
async fn commit_tree(
    c: &reqwest::Client,
    repo: &str,
    commit: &str,
    token: &str,
) -> Result<String, String> {
    let r = req(
        c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/git/commits/{commit}"),
        token,
    )
    .send()
    .await
    .map_err(|e| format!("连接失败：{e}"))?;
    if !r.status().is_success() {
        return Err(gh_error(r).await);
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    Ok(v["tree"]["sha"].as_str().unwrap_or("").to_string())
}

/// tree sha → {路径: 条目}。`recursive=1` 一次拿全，不用逐层递归。
async fn tree_entries(
    c: &reqwest::Client,
    repo: &str,
    tree: &str,
    token: &str,
) -> Result<HashMap<String, RemoteEntry>, String> {
    let r = req(
        c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/git/trees/{tree}?recursive=1"),
        token,
    )
    .send()
    .await
    .map_err(|e| format!("连接失败：{e}"))?;
    if !r.status().is_success() {
        return Err(gh_error(r).await);
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    // 截断必须当成硬错误。树接口在 10 万条目 / 7 MB 处会截断，而**截断后的清单看起来
    // 就像「远端少了一堆文件」** —— 拿它去判定，会把用户本地的文件当成远端新增，
    // 也可能把该拉的漏掉。宁可停下来报错，也不要拿残缺的清单去动用户的文件。
    if v["truncated"].as_bool().unwrap_or(false) {
        return Err("仓库文件太多，GitHub 的树接口把清单截断了，这次同步不安全，已中止。\n\
                    告诉我一声，我改成逐层遍历再试。"
            .into());
    }
    let mut out: HashMap<String, RemoteEntry> = HashMap::new();
    if let Some(arr) = v["tree"].as_array() {
        for e in arr {
            if e["type"].as_str() != Some("blob") {
                continue;
            }
            let Some(raw) = e["path"].as_str() else { continue };
            // 过不了安全校验的条目直接丢，且**不报错** —— 一个畸形路径不该让整次同步失败
            let Some(p) = safe_rel_path(raw) else { continue };
            // 本地采集时就被忽略的目录（`.git` / `node_modules` / `.mnesphere`…），
            // 远端也一律不管。否则「本地没有」会被判定成「可以删远端」或「该拉下来」，
            // 白白指着这些永远不该进 vault 的东西做动作。
            if is_ignored(Path::new(&p)) {
                continue;
            }
            let sha = e["sha"].as_str().unwrap_or("").to_string();
            if sha.is_empty() {
                continue;
            }
            out.insert(
                p,
                RemoteEntry {
                    sha,
                    size: e["size"].as_u64().unwrap_or(0),
                },
            );
        }
    }
    Ok(out)
}

/// 远端分支当前状态：`(head commit, tree sha, 清单)`。
/// 空仓库返回三个空值 —— 那不是错误，是「首次同步」的正常起点。
async fn remote_state(
    c: &reqwest::Client,
    repo: &str,
    branch: &str,
    token: &str,
) -> Result<(String, String, HashMap<String, RemoteEntry>), String> {
    let r = req(
        c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/git/ref/heads/{branch}"),
        token,
    )
    .send()
    .await
    .map_err(|e| format!("连接失败：{e}"))?;

    // 409 = 仓库一个提交都没有；404 = 仓库非空但没有这个分支。
    // 混掉这两种会做出危险的事，所以先在 remote_state 里分干净。
    match r.status().as_u16() {
        409 => return Ok((String::new(), String::new(), HashMap::new())),
        404 => {
            return Err(format!(
                "仓库里没有分支 `{branch}`。\n\
                 检查一下分支名，或者先在仓库里建一个初始提交。"
            ))
        }
        _ => {}
    }
    if !r.status().is_success() {
        return Err(gh_error(r).await);
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    let head = v["object"]["sha"].as_str().unwrap_or("").to_string();
    if head.is_empty() {
        return Err("远端分支没有指向任何提交".into());
    }
    let tree = commit_tree(c, repo, &head, token).await?;
    if tree.is_empty() {
        return Err("远端提交里没有 tree".into());
    }
    let entries = tree_entries(c, repo, &tree, token).await?;
    Ok((head, tree, entries))
}

/// 基线清单。拿不到就返回空表 —— 空基线只会让判定退化成最保守的分支，
/// **不会造成任何删除**，所以这里可以放心地「悄悄降级」。
async fn baseline_entries(
    c: &reqwest::Client,
    repo: &str,
    last_commit: &str,
    token: &str,
) -> HashMap<String, RemoteEntry> {
    let lc = last_commit.trim();
    if lc.is_empty() {
        return HashMap::new();
    }
    // 配置里记的 commit 可能已经在远端消失了（比如你重建了仓库），
    // 这时候当作「没有基线」比报错更合适。
    let Ok(tree) = commit_tree(c, repo, lc, token).await else {
        return HashMap::new();
    };
    if tree.is_empty() {
        return HashMap::new();
    }
    tree_entries(c, repo, &tree, token).await.unwrap_or_default()
}

/// 下载一个 blob。GitHub 的 base64 是带换行的，得先滤掉空白再解码。
async fn fetch_blob(
    c: &reqwest::Client,
    repo: &str,
    sha: &str,
    token: &str,
) -> Result<Vec<u8>, String> {
    let r = req(
        c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/git/blobs/{sha}"),
        token,
    )
    .send()
    .await
    .map_err(|e| format!("下载内容失败：{e}"))?;
    if !r.status().is_success() {
        return Err(gh_error(r).await);
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    let b64 = v["content"].as_str().unwrap_or("");
    let clean: String = b64.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(clean.as_bytes())
        .map_err(|e| format!("解码远端内容失败：{e}"))
}

/// 给冲突的远端版本找个不撞车的落点：`名字 (远端冲突).md`、`名字 (远端冲突 2).md`…
fn conflict_path(vault: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = p
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    for i in 1..=99usize {
        let name = if i == 1 {
            format!("{stem} (远端冲突){ext}")
        } else {
            format!("{stem} (远端冲突 {i}){ext}")
        };
        let cand = vault.join(&dir).join(&name);
        if !cand.exists() {
            return cand;
        }
    }
    vault.join(&dir).join(format!(
        "{stem} (远端冲突 {}){ext}",
        chrono::Local::now().format("%Y%m%d%H%M%S")
    ))
}

/// 三方判定的**唯一实现**。整个同步的正确性都压在这张表上。
///
/// ```text
///  本地 vs 基线 │ 远端 vs 基线 │ 结论
///  ────────────┼─────────────┼───────────────────────────
///   没变       │  没变       │ 无事
///   没变       │  变了       │ 下载（远端改了）
///   变了       │  没变       │ 上传（本地改了）
///   变了       │  变了       │ 冲突 → 两边都留
/// ```
/// 删除同理：一侧消失了，只有**另一侧没动过**时才敢跟着删。
/// 谁都改过、谁都没基线的时候，一律选「不删」—— 保数据比保整洁重要。
fn plan(
    local: &HashMap<String, (String, PathBuf)>,
    remote: &HashMap<String, RemoteEntry>,
    base: &HashMap<String, RemoteEntry>,
) -> BTreeMap<String, Action> {
    let mut keys: BTreeSet<&String> = BTreeSet::new();
    keys.extend(local.keys());
    keys.extend(remote.keys());
    keys.extend(base.keys());

    let mut out: BTreeMap<String, Action> = BTreeMap::new();
    for k in keys {
        let l = local.get(k).map(|(s, _)| s.as_str());
        let r = remote.get(k).map(|e| e.sha.as_str());
        let b = base.get(k).map(|e| e.sha.as_str());

        let act = match (l, r) {
            (Some(l), Some(r)) => {
                if l == r {
                    continue; // 内容一致，无事
                }
                match b {
                    Some(b) if b == l => Action::Download, // 只有远端变了
                    Some(b) if b == r => Action::Upload { resurrect: false }, // 只有本地变了
                    Some(_) => Action::Conflict,           // 两边都变了
                    // 没有基线 → 分不清是谁改的。不猜，本地原件不动，
                    // 远端那份另存成副本给用户自己合。
                    None => Action::Conflict,
                }
            }
            (Some(l), None) => match b {
                // 基线里有、现在远端没有 → 是远端删的
                Some(b) if b == l => Action::DeleteLocal, // 本地也没动过，跟着删
                // 远端删了但本地改过 → 把本地这份保下来（标记成「救回来」）
                Some(_) => Action::Upload { resurrect: true },
                None => Action::Upload { resurrect: false }, // 本地新加的
            },
            (None, Some(r)) => match b {
                // 基线里有、现在本地没有 → 是本地删的
                Some(b) if b == r => Action::DeleteRemote, // 远端也没动过，远端跟着删
                // 本地删了但远端改过 → 拉回来，不替用户丢东西
                Some(_) => Action::Download,
                None => Action::Download, // 远端新加的
            },
            // 两边都没有：只可能是基线里有过、两边都删了 → 无事
            (None, None) => continue,
        };
        out.insert(k.clone(), act);
    }
    out
}

/// 写一个下载下来的文件：先补目录、再落盘、然后记一笔「这是我写的」。
///
/// `mark_self_write` 必须在写入**之前或之后紧邻**调用 —— 它是靠时间窗口匹配的。
/// 漏了这一步，文件监听会把我们自己的写入当成「用户改了文件」，
/// 于是刚同步完状态栏就亮黄点，看着像同步没生效。
fn write_local(state: &crate::AppState, abs: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
    }
    std::fs::write(abs, bytes).map_err(|e| format!("写入失败：{e}"))?;
    state.mark_self_write(abs);
    Ok(())
}

// ───────────────────────── 命令 ─────────────────────────

/// 看看这个 Token 是谁的、有什么权限。设置页用它显示「当前账号」，
/// 也方便用户自己发现「Token 过期了 / scope 不对」。
#[tauri::command]
pub async fn github_account() -> Result<AccountInfo, String> {
    let token = get_secret("github_token").ok_or("还没有配置 GitHub Token")?;
    let c = client()?;
    let resp = req(&c, reqwest::Method::GET, "/user", &token)
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(gh_error(resp).await);
    }
    // headers 必须在 json() 之前取（json() 会吃掉 resp）
    let scopes: Vec<String> = resp
        .headers()
        .get("x-oauth-scopes")
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(AccountInfo {
        login: v["login"].as_str().unwrap_or("").to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        scopes,
    })
}

/// 在当前账号下建一个**私有**仓库。
///
/// 恒 `private: true`，界面不给公开选项 —— vault 里装的是日记和笔记，
/// 要公开请自己去 GitHub 建，别让同步功能背这个锅。
///
/// `auto_init: false`（保持零提交）：首次同步会自己把提交图立起来，
/// 这样远端不会多出一笔无关的 README 提交。
#[tauri::command]
pub async fn github_create_repo(
    name: String,
    description: Option<String>,
) -> Result<RepoInfo, String> {
    let token = get_secret("github_token").ok_or("还没有配置 GitHub Token")?;
    let name = name.trim().trim_end_matches(".git").to_string();
    if name.is_empty() {
        return Err("仓库名不能为空".into());
    }
    if name.starts_with('.') {
        return Err("仓库名不能以 `.` 开头".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err("仓库名只能用字母、数字、`-`、`_`、`.`（GitHub 的限制）".into());
    }

    let c = client()?;
    let body = json!({
        "name": name,
        "private": true,
        "auto_init": false,
        "has_issues": false,
        "has_wiki": false,
        "has_projects": false,
        "description": description.unwrap_or_else(|| "mnesphere 知识库同步仓库".into()),
    });
    let resp = req(&c, reqwest::Method::POST, "/user/repos", &token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !resp.status().is_success() {
        if resp.status().as_u16() == 422 {
            return Err(format!(
                "建不了 `{name}`：可能这个名字已经被占了，或者不符合 GitHub 的命名规则。"
            ));
        }
        return Err(gh_error(resp).await);
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(RepoInfo {
        full_name: v["full_name"].as_str().unwrap_or("").to_string(),
        default_branch: v["default_branch"].as_str().unwrap_or("main").to_string(),
        private: v["private"].as_bool().unwrap_or(true),
        html_url: v["html_url"].as_str().unwrap_or("").to_string(),
        empty: true,
    })
}

#[tauri::command]
pub async fn github_test(repo: String, branch: Option<String>) -> Result<RepoInfo, String> {
    let token = get_secret("github_token").ok_or("还没有配置 GitHub Token")?;
    let repo = normalize_repo(&repo)?;
    let c = client()?;

    // 1) 仓库本体
    let resp = req(&c, reqwest::Method::GET, &format!("/repos/{repo}"), &token)
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(gh_error(resp).await);
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let default_branch = v["default_branch"].as_str().unwrap_or("main").to_string();
    let info = RepoInfo {
        full_name: v["full_name"].as_str().unwrap_or(&repo).to_string(),
        default_branch: default_branch.clone(),
        private: v["private"].as_bool().unwrap_or(false),
        html_url: v["html_url"].as_str().unwrap_or("").to_string(),
        empty: false,
    };

    // 2) 分支在不在
    let br = branch
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| default_branch.clone());
    let r2 = req(
        &c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/branches/{br}"),
        &token,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;
    if r2.status().is_success() {
        return Ok(info);
    }

    // 3) 分支没有 —— 要区分「仓库本身就是空的」和「分支名写错了」。
    //    空仓库没有任何分支，而这不是错误：首次同步会自动把提交图立起来。
    let r3 = req(
        &c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/branches"),
        &token,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;
    if r3.status().is_success() {
        let arr: Value = r3.json().await.map_err(|e| e.to_string())?;
        if arr.as_array().map(|a| a.is_empty()).unwrap_or(false) {
            return Ok(RepoInfo { empty: true, ..info });
        }
    }

    Err(format!(
        "仓库能访问，但没有分支 `{br}`。\n\
         要么分支名填错了，要么这个仓库还没建过初始提交。"
    ))
}

/// 空仓库的「立图」：用 Contents API 埋一笔种子提交，让提交图存在。
///
/// 之后调用方会建一个**根提交**（无 parents）覆盖它，种子文件不会留在远端。
async fn seed_empty_repo(
    c: &reqwest::Client,
    repo: &str,
    branch: &str,
    token: &str,
) -> Result<(), String> {
    let content = base64::engine::general_purpose::STANDARD.encode(b"mnesphere sync seed\n");
    // 刻意**不带 branch** —— 让它落在仓库的默认分支上。这是唯一确定能成功的写法，
    // 不必赌「指定一个还不存在的分支」会怎样。用户要的分支若是别的，下面再补建。
    let r = req(
        c,
        reqwest::Method::PUT,
        &format!("/repos/{repo}/contents/{SEED_PATH}"),
        token,
    )
    .json(&json!({ "message": "mnesphere: init", "content": content }))
    .send()
    .await
    .map_err(|e| format!("初始化仓库失败：{e}"))?;
    if !r.status().is_success() {
        return Err(format!(
            "仓库是空的，初始化第一个提交失败了：\n{}",
            gh_error(r).await
        ));
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    let seed_commit = v["commit"]["sha"].as_str().unwrap_or("").to_string();
    if seed_commit.is_empty() {
        return Err("初始化提交后没有拿到 commit sha".into());
    }

    // 用户要的分支若不是默认分支，把它也指到这笔种子提交上
    let db = {
        let r = req(c, reqwest::Method::GET, &format!("/repos/{repo}"), token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let v: Value = r.json().await.map_err(|e| e.to_string())?;
        v["default_branch"].as_str().unwrap_or("main").to_string()
    };
    if db != branch {
        let r2 = req(
            c,
            reqwest::Method::POST,
            &format!("/repos/{repo}/git/refs"),
            token,
        )
        .json(&json!({ "ref": format!("refs/heads/{branch}"), "sha": seed_commit }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
        if !r2.status().is_success() {
            return Err(format!(
                "仓库初始化了，但建不出分支 `{branch}`：\n{}",
                gh_error(r2).await
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct PushOutcome {
    pushed: Vec<String>,
    deleted: Vec<String>,
    commit: String,
}

/// 双向同步：**先拉，后推**。
///
/// 顺序不能反。本机可能落后于远端（另一台机器推过），不先把远端的差异并进来，
/// 推送就会基于一颗过期的树去做判断 —— 那正是旧版「远端有、本地没有就删」
/// 会误杀别人文件的根源。
///
/// `protect`：正在编辑、还没保存的文档路径。这些文件**只读不写** ——
/// 内存里那份才是用户眼下的真实意图，拿磁盘内容覆盖它等于凭空吞掉刚敲的字。
#[tauri::command]
pub async fn github_sync(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    protect: Option<Vec<String>>,
    message: Option<String>,
) -> Result<SyncReport, String> {
    sync_impl(&app, &state, protect.unwrap_or_default(), true, message).await
}

/// 只下载：把远端的更新并进本地，**一个字节都不往远端写**。
/// 适合「换了台机器，先把我写的东西拿下来」。
#[tauri::command]
pub async fn github_pull(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    protect: Option<Vec<String>>,
) -> Result<SyncReport, String> {
    sync_impl(&app, &state, protect.unwrap_or_default(), false, None).await
}

async fn sync_impl(
    app: &tauri::AppHandle,
    state: &crate::AppState,
    protect: Vec<String>,
    do_push: bool,
    message: Option<String>,
) -> Result<SyncReport, String> {
    use tauri::Emitter;

    let cfg: AppConfig = state.cfg.lock().unwrap().clone();
    if cfg.github.repo.trim().is_empty() {
        return Err("还没有填仓库地址".into());
    }
    let token = get_secret("github_token").ok_or("还没有配置 GitHub Token")?;
    let repo = normalize_repo(&cfg.github.repo)?;
    let branch = if cfg.github.branch.trim().is_empty() {
        "main".to_string()
    } else {
        cfg.github.branch.trim().to_string()
    };
    let vault = cfg.vault_path();
    let c = client()?;

    // ── 1) 三份清单 ───────────────────────────────
    let mut skipped: Vec<String> = Vec::new();
    let (local, unusable) = collect_local(&vault, &mut skipped);
    let (remote_head, base_tree, remote) = remote_state(&c, &repo, &branch, &token).await?;
    let base = baseline_entries(&c, &repo, &cfg.github.last_commit, &token).await;

    // ── 2) 判定 ───────────────────────────────────
    let guard: std::collections::HashSet<String> =
        protect.iter().map(|p| p.replace('\\', "/")).collect();
    let mut acts = plan(&local, &remote, &base);

    // 盘上有、但读不动（超限/没权限）的文件：从计划里整个摘掉。
    // 不摘的话它们在判定里等同于「本地没有」，会导致远端原件被删或被覆盖。
    for p in &unusable {
        acts.remove(p);
    }

    // 正在编辑的文件只读不写。要覆盖它的，降级成「冲突副本」——
    // 副本是**另一个**文件，原件一个字节都不动；要删它的，直接取消。
    let mut protected_notes: Vec<String> = Vec::new();
    for p in &guard {
        match acts.get(p) {
            Some(Action::Download) => {
                acts.insert(p.clone(), Action::Conflict);
                protected_notes.push(format!("{p}（正在编辑，远端版本另存为副本）"));
            }
            Some(Action::DeleteLocal) => {
                acts.remove(p);
                protected_notes.push(format!("{p}（正在编辑，已取消删除）"));
            }
            _ => {}
        }
    }

    let unchanged = {
        let mut all: BTreeSet<&String> = BTreeSet::new();
        all.extend(local.keys());
        all.extend(remote.keys());
        all.extend(base.keys());
        all.len().saturating_sub(acts.len())
    };

    // ── 3) 拉取：下载 / 删本地 / 落冲突副本 ────────
    let mut pulled: Vec<String> = Vec::new();
    let mut local_deleted: Vec<String> = Vec::new();
    let mut conflicts: Vec<ConflictItem> = Vec::new();
    let mut touched: Vec<String> = Vec::new();

    for (rel, act) in &acts {
        match act {
            Action::Download | Action::Conflict => {
                let Some(entry) = remote.get(rel) else { continue };
                // 超大文件不下：宁可留着差异，也不要一次同步把内存吃满
                if entry.size > MAX_FILE {
                    skipped.push(format!(
                        "{rel}（远端 {} MB，超过 10 MB）",
                        entry.size / 1024 / 1024
                    ));
                    continue;
                }
                let bytes = match fetch_blob(&c, &repo, &entry.sha, &token).await {
                    Ok(b) => b,
                    Err(e) => {
                        skipped.push(format!("{rel}（下载失败：{e}）"));
                        continue;
                    }
                };
                if matches!(act, Action::Conflict) {
                    let dst = conflict_path(&vault, rel);
                    if let Err(e) = write_local(state, &dst, &bytes) {
                        skipped.push(format!("{rel}（副本写入失败：{e}）"));
                        continue;
                    }
                    let saved_as = dst
                        .strip_prefix(&vault)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| rel.clone());
                    touched.push(saved_as.clone());
                    conflicts.push(ConflictItem {
                        path: rel.clone(),
                        saved_as,
                    });
                    continue;
                }
                let abs = match local.get(rel) {
                    Some((_, abs)) => abs.clone(),
                    // 远端新增的文件，本地还没有落点
                    None => vault.join(rel),
                };
                if let Err(e) = write_local(state, &abs, &bytes) {
                    skipped.push(format!("{rel}（写入失败：{e}）"));
                    continue;
                }
                touched.push(rel.clone());
                pulled.push(rel.clone());
            }
            Action::DeleteLocal => {
                let Some((_, abs)) = local.get(rel) else { continue };
                if let Err(e) = std::fs::remove_file(abs) {
                    skipped.push(format!("{rel}（删除失败：{e}）"));
                    continue;
                }
                state.mark_self_write(abs);
                touched.push(rel.clone());
                local_deleted.push(rel.clone());
            }
            _ => {}
        }
    }

    // ── 4) 推送 ───────────────────────────────────
    let mut uploads: Vec<String> = Vec::new();
    let mut remote_deletes: Vec<String> = Vec::new();
    let mut resurrected = 0usize;
    for (rel, act) in &acts {
        match act {
            Action::Upload { resurrect } => {
                if *resurrect {
                    resurrected += 1;
                }
                uploads.push(rel.clone());
            }
            Action::DeleteRemote => remote_deletes.push(rel.clone()),
            _ => {}
        }
    }
    uploads.sort();
    remote_deletes.sort();

    let mut outcome = PushOutcome::default();
    if do_push && (!uploads.is_empty() || !remote_deletes.is_empty()) {
        outcome = push_impl(
            &c,
            &repo,
            &branch,
            &token,
            &vault,
            &remote_head,
            &base_tree,
            &uploads,
            &remote_deletes,
            message,
        )
        .await?;
    }

    // ── 5) 记账 ───────────────────────────────────
    //
    // ⚠️ 基线（lastCommit）在**没推送**的时候也必须更新。
    // 这一条是「另一台机器拉完东西之后，本机下次同步把全部文件误判成远端改过」的
    // 唯一解药：基线的语义是「本地文件当前对应的那个 commit」，不是「我最后一次提交的 sha」。
    let final_head = if outcome.commit.is_empty() {
        remote_head.clone()
    } else {
        outcome.commit.clone()
    };
    {
        let mut g = state.cfg.lock().unwrap();
        g.github.last_sync = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if !final_head.is_empty() {
            g.github.last_commit = final_head.clone();
        }
        g.github.branch = branch.clone();
        let _ = g.save();
    }

    // ── 6) 收尾：索引 + 通知前端 ──────────────────
    //
    // 写盘都走过 mark_self_write，文件监听不会替我们发事件了 —— 得自己喊一声，
    // 否则界面上的笔记树/日记列表还是旧的，看着像「拉取没生效」。
    if !touched.is_empty() {
        crate::note::rebuild_index(state, &cfg);
        let _ = app.emit("vault-changed", touched.clone());
    }

    // ── 7) 报告 ───────────────────────────────────
    let mut parts: Vec<String> = Vec::new();
    if !pulled.is_empty() {
        parts.push(format!("下载 {}", pulled.len()));
    }
    if !outcome.pushed.is_empty() {
        parts.push(format!("上传 {}", outcome.pushed.len()));
    }
    if !local_deleted.is_empty() {
        parts.push(format!("删本地 {}", local_deleted.len()));
    }
    if !outcome.deleted.is_empty() {
        parts.push(format!("删远端 {}", outcome.deleted.len()));
    }
    if !conflicts.is_empty() {
        parts.push(format!("冲突 {}", conflicts.len()));
    }
    // 「仅下载」不推东西。本地攒着的改动得说一声，不然用户会以为同步完了就万事大吉。
    let pending_upload = !do_push && !uploads.is_empty();
    let mut report_msg = if parts.is_empty() {
        if pending_upload {
            format!(
                "远端没有更新。本地另有 {} 个改动没上传 ——「仅下载」不会往远端写。",
                uploads.len()
            )
        } else {
            "两边没有差异，无事可做。".to_string()
        }
    } else {
        format!("同步完成：{}。", parts.join("、"))
    };
    if pending_upload && !parts.is_empty() {
        report_msg.push_str(&format!("本地另有 {} 个改动没上传（仅下载）。", uploads.len()));
    }
    if resurrected > 0 {
        report_msg.push_str(&format!(
            "其中 {resurrected} 个文件远端删过但本地有改动，已重新上传。"
        ));
    }
    if !conflicts.is_empty() {
        report_msg.push_str("冲突文件的远端版本已另存为副本，本地原件未动。");
    }

    // 顺带把「怎么处理的」也塞进 skipped，设置页的报告区会显示出来
    skipped.extend(protected_notes);

    Ok(SyncReport {
        pulled,
        local_deleted,
        pushed: outcome.pushed,
        deleted: outcome.deleted,
        conflicts,
        unchanged,
        skipped,
        commit: outcome.commit.chars().take(10).collect(),
        url: format!("https://github.com/{repo}"),
        branch,
        message: report_msg,
    })
}

/// 把本地的新增/修改推上去，并把「本地确实删过」的文件从远端摘掉。
///
/// `uploads` / `remote_deletes` 由 `plan()` 定夺 —— 这里不做任何判断，只执行。
#[allow(clippy::too_many_arguments)]
async fn push_impl(
    c: &reqwest::Client,
    repo: &str,
    branch: &str,
    token: &str,
    vault: &Path,
    remote_head: &str,
    base_tree: &str,
    uploads: &[String],
    remote_deletes: &[String],
    message: Option<String>,
) -> Result<PushOutcome, String> {
    // 空仓库：blob 与 tree 都要求「提交图已经存在」。先用 Contents API 埋一笔种子
    // （它是唯一能在零提交仓库上落地的接口），随后我们的**根提交**会把种子整个甩掉，
    // 远端最终只留干净的一笔。
    if remote_head.is_empty() {
        seed_empty_repo(c, repo, branch, token).await?;
    }

    let mut tree_entries: Vec<Value> = Vec::new();
    let mut pushed: Vec<String> = Vec::new();

    for rel in uploads {
        let abs = vault.join(rel);
        let bytes = std::fs::read(&abs).map_err(|e| format!("读取 {rel} 失败：{e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let br = req(c, reqwest::Method::POST, &format!("/repos/{repo}/git/blobs"), token)
            .json(&json!({ "content": b64, "encoding": "base64" }))
            .send()
            .await
            .map_err(|e| format!("上传 {rel} 失败：{e}"))?;
        if !br.status().is_success() {
            if br.status().as_u16() == 409 {
                return Err("仓库还是空的 —— 初始化第一个提交没生效，请再点一次同步。".into());
            }
            return Err(format!("上传 {rel} 时出错：\n{}", gh_error(br).await));
        }
        let bv: Value = br.json().await.map_err(|e| e.to_string())?;
        let bsha = bv["sha"].as_str().unwrap_or("").to_string();
        if bsha.is_empty() {
            return Err(format!("上传 {rel} 后没有拿到 blob sha"));
        }
        tree_entries.push(json!({
            "path": rel,
            "mode": "100644",
            "type": "blob",
            "sha": bsha
        }));
        pushed.push(rel.clone());
    }

    let mut deleted: Vec<String> = Vec::new();
    for rel in remote_deletes {
        // sha: null 就是 git 里「删除」的表达方式
        tree_entries.push(json!({
            "path": rel,
            "mode": "100644",
            "type": "blob",
            "sha": null
        }));
        deleted.push(rel.clone());
    }

    // tree
    let mut tree_body = json!({ "tree": tree_entries });
    if !base_tree.is_empty() {
        tree_body["base_tree"] = json!(base_tree);
    }
    let tr = req(c, reqwest::Method::POST, &format!("/repos/{repo}/git/trees"), token)
        .json(&tree_body)
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !tr.status().is_success() {
        if tr.status().as_u16() == 409 {
            return Err("仓库还是空的 —— 初始化第一个提交没生效，请再点一次同步。".into());
        }
        return Err(gh_error(tr).await);
    }
    let tv: Value = tr.json().await.map_err(|e| e.to_string())?;
    let new_tree = tv["sha"].as_str().unwrap_or("").to_string();

    // commit
    let msg = message.unwrap_or_else(|| {
        format!(
            "mnesphere {} · +{} -{}",
            chrono::Local::now().format("%Y-%m-%d %H:%M"),
            pushed.len(),
            deleted.len()
        )
    });
    let mut cbody = json!({ "message": msg, "tree": new_tree });
    if !remote_head.is_empty() {
        cbody["parents"] = json!([remote_head]);
    }
    let cr = req(c, reqwest::Method::POST, &format!("/repos/{repo}/git/commits"), token)
        .json(&cbody)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !cr.status().is_success() {
        return Err(gh_error(cr).await);
    }
    let cv: Value = cr.json().await.map_err(|e| e.to_string())?;
    let new_commit = cv["sha"].as_str().unwrap_or("").to_string();
    if new_commit.is_empty() {
        return Err("提交后没有拿到 commit sha".into());
    }

    // 移动 ref。空仓库那条路上 ref 已被种子提交建出来了，所以一律走 PATCH。
    // force 只在「刚埋过种子」时才开 —— 种子不是新提交的祖先，不开会 422。
    // 正常仓库保持 force=false：万一远端在我们读完 head 之后又被推过，
    // GitHub 会拒绝，而不是把别人的提交悄悄盖上。
    let rr = req(
        c,
        reqwest::Method::PATCH,
        &format!("/repos/{repo}/git/refs/heads/{branch}"),
        token,
    )
    .json(&json!({ "sha": new_commit, "force": remote_head.is_empty() }))
    .send()
    .await
    .map_err(|e| e.to_string())?;
    if !rr.status().is_success() {
        return Err(gh_error(rr).await);
    }

    Ok(PushOutcome {
        pushed,
        deleted,
        commit: new_commit,
    })
}

// ───────────────────────── 状态查询 ─────────────────────────

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GhStatus {
    /// 配了仓库没有
    pub configured: bool,
    /// 自上次同步以来 vault 里有东西动过
    pub dirty: bool,
    pub repo: String,
    pub url: String,
    pub branch: String,
    pub last_sync: String,
    pub last_commit: String,
}

/// 递归找出目录树里最新的 mtime（Unix 秒）。
fn newest_mtime(dir: &Path, acc: &mut Option<u64>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if is_ignored(&p) {
            continue;
        }
        if p.is_dir() {
            newest_mtime(&p, acc);
        } else if let Ok(md) = p.metadata() {
            if let Ok(t) = md.modified() {
                let secs = t
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if acc.map(|a| secs > a).unwrap_or(true) {
                    *acc = Some(secs);
                }
            }
        }
    }
}

/// 状态栏用：配没配、有没有未同步的改动、上次同步是什么时候。
///
/// 「有没有改动」刻意**不做精确 diff**（那要全量读文件算哈希，几百个文件太贵），
/// 只比 mtime —— 对「要不要点一下同步」这个判断足够了。
///
/// 另一个好处是**完全无状态**：不用在每条写入路径上埋标记（那种做法一定会漏掉
/// 「用户拿别的编辑器改了文件」这条外部路径），也不需要额外的后台线程。
#[tauri::command]
pub fn github_status(state: tauri::State<'_, crate::AppState>) -> Result<GhStatus, String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let repo = normalize_repo(&cfg.github.repo).unwrap_or_default();
    let configured = !repo.is_empty();

    let mut dirty = false;
    if configured && !cfg.github.last_sync.is_empty() {
        // last_sync 存的是**本地时间**，得按本地时区还原成时间戳再比
        let cutoff =
            chrono::NaiveDateTime::parse_from_str(&cfg.github.last_sync, "%Y-%m-%d %H:%M:%S")
                .ok()
                .and_then(|n| chrono::Local.from_local_datetime(&n).single())
                .map(|dt| dt.timestamp());

        if let Some(cutoff) = cutoff {
            let mut newest: Option<u64> = None;
            newest_mtime(&cfg.vault_path(), &mut newest);
            if let Some(t) = newest {
                // +1 秒容差：同步与写盘可能落在同一秒，不该因此谎报「有改动」
                dirty = (t as i64) > cutoff + 1;
            }
        }
    }

    Ok(GhStatus {
        configured,
        dirty,
        repo: repo.clone(),
        url: if configured {
            format!("https://github.com/{repo}")
        } else {
            String::new()
        },
        branch: cfg.github.branch.clone(),
        last_sync: cfg.github.last_sync.clone(),
        last_commit: cfg.github.last_commit.clone(),
    })
}


// ───────────────────────── 单元测试 ─────────────────────────
//
// `plan()` 是整个同步的脑子，而它同时又是**纯函数** —— 沙箱里起不了 GUI，
// 但这一层可以真跑，所以危险分支必须有断言，不能靠读代码自我安慰。
//
// 最要紧的一条是 `first_sync_never_deletes_remote`：它复现的正是用户那台
// 第二台机器上的数据毁灭路径（旧实现会把远端 21 个文件全判成「本地已删」）。

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(pairs: &[(&str, &str)]) -> HashMap<String, (String, PathBuf)> {
        pairs
            .iter()
            .map(|(p, s)| (p.to_string(), (s.to_string(), PathBuf::from(p))))
            .collect()
    }

    fn tree(pairs: &[(&str, &str)]) -> HashMap<String, RemoteEntry> {
        pairs
            .iter()
            .map(|(p, s)| {
                (
                    p.to_string(),
                    RemoteEntry {
                        sha: s.to_string(),
                        size: 10,
                    },
                )
            })
            .collect()
    }

    fn acts(
        l: &[(&str, &str)],
        r: &[(&str, &str)],
        b: &[(&str, &str)],
    ) -> BTreeMap<String, Action> {
        plan(&loc(l), &tree(r), &tree(b))
    }

    fn count(a: &BTreeMap<String, Action>, f: impl Fn(&Action) -> bool) -> usize {
        a.values().filter(|x| f(x)).count()
    }

    /// 首次同步（没有基线）：远端有的要拉下来，本地有的要推上去，
    /// **绝不能因为「远端有、本地没有」就删远端**。
    #[test]
    fn first_sync_never_deletes_remote() {
        let a = acts(
            &[("b.md", "s1"), ("c.md", "s2"), ("d.md", "s3")],
            &[("x.md", "s9"), ("y.md", "s8")],
            &[],
        );
        assert_eq!(count(&a, |x| matches!(x, Action::DeleteRemote)), 0, "{a:?}");
        assert_eq!(count(&a, |x| matches!(x, Action::Download)), 2, "{a:?}");
        assert_eq!(count(&a, |x| matches!(x, Action::Upload { .. })), 3, "{a:?}");
    }

    /// 轮替的常态：本地没动、远端变了 → 下载。两台机器交替写就从这里走过去。
    #[test]
    fn remote_changed_only_downloads() {
        let a = acts(&[("a.md", "same")], &[("a.md", "newer")], &[("a.md", "same")]);
        assert_eq!(a.get("a.md"), Some(&Action::Download), "{a:?}");
    }

    /// 本地改了、远端没动 → 上传
    #[test]
    fn local_changed_only_uploads() {
        let a = acts(&[("a.md", "mine")], &[("a.md", "same")], &[("a.md", "same")]);
        assert_eq!(a.get("a.md"), Some(&Action::Upload { resurrect: false }), "{a:?}");
    }

    /// 两边都改了 → 冲突，谁也不许自动胜出
    #[test]
    fn both_changed_conflicts() {
        let a = acts(&[("a.md", "mine")], &[("a.md", "theirs")], &[("a.md", "base")]);
        assert_eq!(a.get("a.md"), Some(&Action::Conflict), "{a:?}");
    }

    /// 没有基线又两边都有、且内容不同 → 同样按冲突处理（不猜）
    #[test]
    fn no_baseline_and_differing_content_is_conflict() {
        let a = acts(&[("a.md", "mine")], &[("a.md", "theirs")], &[]);
        assert_eq!(a.get("a.md"), Some(&Action::Conflict), "{a:?}");
    }

    /// 内容一致 → 不进计划表
    #[test]
    fn identical_content_is_noop() {
        let a = acts(&[("a.md", "x")], &[("a.md", "x")], &[("a.md", "x")]);
        assert!(a.is_empty(), "{a:?}");
    }

    /// 本地删过、远端没动 → 远端跟着删
    #[test]
    fn local_delete_propagates_when_remote_untouched() {
        let a = acts(&[], &[("a.md", "keep")], &[("a.md", "keep")]);
        assert_eq!(a.get("a.md"), Some(&Action::DeleteRemote), "{a:?}");
    }

    /// 远端删过、本地没动 → 本地跟着删
    #[test]
    fn remote_delete_propagates_when_local_untouched() {
        let a = acts(&[("a.md", "base")], &[], &[("a.md", "base")]);
        assert_eq!(a.get("a.md"), Some(&Action::DeleteLocal), "{a:?}");
    }

    /// 远端删了但本地改过 → 保住本地（标记 resurrect），**不删本地**
    #[test]
    fn remote_delete_does_not_eat_local_edits() {
        let a = acts(&[("a.md", "mine")], &[], &[("a.md", "base")]);
        assert_eq!(a.get("a.md"), Some(&Action::Upload { resurrect: true }), "{a:?}");
    }

    /// 本地删了但远端改过 → 拉回来，不替用户丢东西
    #[test]
    fn local_delete_does_not_eat_remote_edits() {
        let a = acts(&[], &[("a.md", "theirs")], &[("a.md", "base")]);
        assert_eq!(a.get("a.md"), Some(&Action::Download), "{a:?}");
    }

    /// 两种删除要在同一次同步里各走各的，不能互相带偏。
    /// `mine.md` = **本地删掉**的（远端还留着原样）→ 远端也删；
    /// `theirs.md` = **远端删掉**的（本地还留着原样）→ 本地也删。
    #[test]
    fn deletes_go_both_ways_independently() {
        let a = acts(
            &[("theirs.md", "same")], // 本地这边：theirs.md 还在，mine.md 已经没了
            &[("mine.md", "same")],   // 远端那边：mine.md 还在，theirs.md 已经没了
            &[("mine.md", "same"), ("theirs.md", "same")],
        );
        assert_eq!(a.get("mine.md"), Some(&Action::DeleteRemote), "{a:?}");
        assert_eq!(a.get("theirs.md"), Some(&Action::DeleteLocal), "{a:?}");
    }

    /// 远端路径是**不可信输入**：越界的一律不要
    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for bad in [
            "../../etc/passwd",
            "..\\..\\Windows\\System32\\x.dll",
            "/etc/passwd",
            "C:/Windows/x.dll",
            "C:////Windows////x.dll",
            "a/../../b.md",
            "a//b.md",
            ".mnesphere-seed",
            "",
        ] {
            assert!(safe_rel_path(bad).is_none(), "应收起却放行了: {bad}");
        }
    }

    #[test]
    fn accepts_normal_relative_paths() {
        for (inn, want) in [
            ("笔记/2026-09-24.md", "笔记/2026-09-24.md"),
            ("附件/2026-09/a b.png", "附件/2026-09/a b.png"),
            ("任务/_tasks.md", "任务/_tasks.md"),
        ] {
            assert_eq!(safe_rel_path(inn).as_deref(), Some(want));
        }
    }
}
