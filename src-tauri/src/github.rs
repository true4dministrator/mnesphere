//! GitHub 同步：走 Git Data API，**一次同步只产生一个 commit**。
//!
//! 为什么不用本地 git：
//! - 用户不需要装 git，仓库可以纯粹当存储用
//! - 不用处理 `.git` 目录，也不怕它跟着 vault 一起被同步来同步去
//! - 可以直接比对 Git 原生的 blob 哈希，只推真正变了的文件
//!
//! 冲突策略（V1）：只做单向「本地 → 远端」。不做三路合并 —— 那是三个月起步的活。

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{get_secret, AppConfig};
use crate::vault::is_ignored;

const API: &str = "https://api.github.com";
const UA: &str = "mnesphere";
const MAX_FILE: u64 = 10 * 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub pushed: Vec<String>,
    pub deleted: Vec<String>,
    pub unchanged: usize,
    pub skipped: Vec<String>,
    pub commit: String,
    pub url: String,
    pub branch: String,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub full_name: String,
    pub default_branch: String,
    pub private: bool,
    pub html_url: String,
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
    let short: String = txt.chars().take(500).collect();
    match status.as_u16() {
        401 => format!("认证失败（401）：Token 无效或已过期。\n{short}"),
        403 => format!("权限不足或被限流（403）：确认 Token 勾了 repo 权限，且没有超出调用上限。\n{short}"),
        404 => format!("仓库或分支不存在（404）：确认 owner/repo 拼写正确、Token 对它有权限。\n{short}"),
        409 => format!("冲突（409）：远端有本地不知道的变更。\n{short}"),
        422 => format!("请求被拒绝（422）：通常是分支保护规则或路径非法。\n{short}"),
        _ => format!("GitHub 返回 {status}：\n{short}"),
    }
}

// ───────────────────────── 命令 ─────────────────────────

#[tauri::command]
pub async fn github_test(repo: String, branch: Option<String>) -> Result<RepoInfo, String> {
    let token = get_secret("github_token").ok_or("还没有配置 GitHub Token")?;
    let repo = repo.trim().trim_end_matches(".git").to_string();
    if !repo.contains('/') {
        return Err("仓库要写成 owner/repo 的形式".into());
    }
    let c = client()?;
    let resp = req(&c, reqwest::Method::GET, &format!("/repos/{repo}"), &token)
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(gh_error(resp).await);
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let info = RepoInfo {
        full_name: v["full_name"].as_str().unwrap_or(&repo).to_string(),
        default_branch: v["default_branch"].as_str().unwrap_or("main").to_string(),
        private: v["private"].as_bool().unwrap_or(false),
        html_url: v["html_url"].as_str().unwrap_or("").to_string(),
    };

    // 顺带确认分支可写
    let br = branch.unwrap_or_else(|| info.default_branch.clone());
    let r2 = req(
        &c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/branches/{br}"),
        &token,
    )
    .send()
    .await
    .map_err(|e| e.to_string())?;
    if !r2.status().is_success() {
        return Err(format!(
            "仓库能访问，但分支 `{br}` 不存在。首次推送前请先在仓库里建一个初始提交（比如加个 README）。"
        ));
    }
    Ok(info)
}

#[tauri::command]
pub async fn github_sync(
    state: tauri::State<'_, crate::AppState>,
    message: Option<String>,
) -> Result<SyncReport, String> {
    let cfg: AppConfig = {
        let c = state.cfg.lock().unwrap();
        c.clone()
    };
    if cfg.github.repo.trim().is_empty() {
        return Err("还没有填仓库地址".into());
    }
    let token = get_secret("github_token").ok_or("还没有配置 GitHub Token")?;

    let repo = cfg.github.repo.trim().trim_end_matches(".git").to_string();
    let branch = if cfg.github.branch.trim().is_empty() {
        "main".to_string()
    } else {
        cfg.github.branch.trim().to_string()
    };
    let vault = cfg.vault_path();
    let c = client()?;

    // 1) 本地文件清单 + 哈希
    let files = collect_files(&vault);
    let mut local: Vec<(String, Vec<u8>, String)> = vec![];
    let mut skipped: Vec<String> = vec![];
    for (rel, abs) in &files {
        let Ok(md) = abs.metadata() else { continue };
        if md.len() > MAX_FILE {
            skipped.push(format!("{rel}（{} MB，超过 10 MB）", md.len() / 1024 / 1024));
            continue;
        }
        match std::fs::read(abs) {
            Ok(bytes) => {
                let sha = blob_sha(&bytes);
                local.push((rel.clone(), bytes, sha));
            }
            Err(e) => skipped.push(format!("{rel}（读取失败：{e}）")),
        }
    }
    if local.is_empty() {
        return Err("vault 里没有可同步的文件".into());
    }

    // 2) 远端 ref → commit → tree
    let ref_resp = req(
        &c,
        reqwest::Method::GET,
        &format!("/repos/{repo}/git/ref/heads/{branch}"),
        &token,
    )
    .send()
    .await
    .map_err(|e| format!("连接失败：{e}"))?;

    let (parent_commit, base_tree) = if ref_resp.status().is_success() {
        let v: Value = ref_resp.json().await.map_err(|e| e.to_string())?;
        let head = v["object"]["sha"].as_str().unwrap_or("").to_string();
        if head.is_empty() {
            return Err("远端分支没有指向任何提交".into());
        }
        let cr = req(
            &c,
            reqwest::Method::GET,
            &format!("/repos/{repo}/git/commits/{head}"),
            &token,
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
        if !cr.status().is_success() {
            return Err(gh_error(cr).await);
        }
        let cv: Value = cr.json().await.map_err(|e| e.to_string())?;
        (
            head,
            cv["tree"]["sha"].as_str().unwrap_or("").to_string(),
        )
    } else if ref_resp.status().as_u16() == 404 || ref_resp.status().as_u16() == 409 {
        // 空仓库：没有 ref，需要建首个提交
        (String::new(), String::new())
    } else {
        return Err(gh_error(ref_resp).await);
    };

    // 3) 远端已有文件的哈希表
    let mut remote: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if !base_tree.is_empty() {
        let tr = req(
            &c,
            reqwest::Method::GET,
            &format!("/repos/{repo}/git/trees/{base_tree}?recursive=1"),
            &token,
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
        if !tr.status().is_success() {
            return Err(gh_error(tr).await);
        }
        let tv: Value = tr.json().await.map_err(|e| e.to_string())?;
        if let Some(arr) = tv["tree"].as_array() {
            for e in arr {
                if e["type"].as_str() == Some("blob") {
                    if let (Some(p), Some(s)) = (e["path"].as_str(), e["sha"].as_str()) {
                        remote.insert(p.to_string(), s.to_string());
                    }
                }
            }
        }
    }

    // 4) 差异
    let mut tree_entries: Vec<Value> = vec![];
    let mut pushed: Vec<String> = vec![];
    let mut unchanged = 0usize;

    for (rel, bytes, sha) in &local {
        match remote.get(rel) {
            Some(rs) if rs == sha => {
                unchanged += 1;
                continue;
            }
            _ => {}
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let br = req(
            &c,
            reqwest::Method::POST,
            &format!("/repos/{repo}/git/blobs"),
            &token,
        )
        .json(&json!({ "content": b64, "encoding": "base64" }))
        .send()
        .await
        .map_err(|e| format!("上传 {rel} 失败：{e}"))?;
        if !br.status().is_success() {
            let e = gh_error(br).await;
            return Err(format!("上传 {rel} 时出错：\n{e}"));
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

    // 本地删了的，远端也删
    let local_set: std::collections::HashSet<&String> = local.iter().map(|(r, _, _)| r).collect();
    let mut deleted: Vec<String> = vec![];
    for p in remote.keys() {
        if !local_set.contains(p) {
            tree_entries.push(json!({
                "path": p,
                "mode": "100644",
                "type": "blob",
                "sha": null
            }));
            deleted.push(p.clone());
        }
    }

    if tree_entries.is_empty() {
        let info = RepoInfo {
            full_name: repo.clone(),
            default_branch: branch.clone(),
            private: false,
            html_url: format!("https://github.com/{repo}"),
        };
        return Ok(SyncReport {
            pushed,
            deleted,
            unchanged,
            skipped,
            commit: String::new(),
            url: info.html_url,
            branch,
            message: "已经是最新的，没有需要提交的改动。".into(),
        });
    }

    // 5) tree
    let mut tree_body = json!({ "tree": tree_entries });
    if !base_tree.is_empty() {
        tree_body["base_tree"] = json!(base_tree);
    }
    let tr2 = req(
        &c,
        reqwest::Method::POST,
        &format!("/repos/{repo}/git/trees"),
        &token,
    )
    .json(&tree_body)
    .send()
    .await
    .map_err(|e| e.to_string())?;
    if !tr2.status().is_success() {
        return Err(gh_error(tr2).await);
    }
    let tv2: Value = tr2.json().await.map_err(|e| e.to_string())?;
    let new_tree = tv2["sha"].as_str().unwrap_or("").to_string();

    // 6) commit
    let msg = message.unwrap_or_else(|| {
        format!(
            "mnesphere {} · +{} ~{}",
            chrono::Local::now().format("%Y-%m-%d %H:%M"),
            pushed.len(),
            deleted.len()
        )
    });
    let mut cbody = json!({ "message": msg, "tree": new_tree });
    if !parent_commit.is_empty() {
        cbody["parents"] = json!([parent_commit]);
    }
    let cr2 = req(
        &c,
        reqwest::Method::POST,
        &format!("/repos/{repo}/git/commits"),
        &token,
    )
    .json(&cbody)
    .send()
    .await
    .map_err(|e| e.to_string())?;
    if !cr2.status().is_success() {
        return Err(gh_error(cr2).await);
    }
    let cv2: Value = cr2.json().await.map_err(|e| e.to_string())?;
    let new_commit = cv2["sha"].as_str().unwrap_or("").to_string();

    // 7) 移动 ref（空仓库则是创建）
    let rr = if parent_commit.is_empty() {
        req(
            &c,
            reqwest::Method::POST,
            &format!("/repos/{repo}/git/refs"),
            &token,
        )
        .json(&json!({ "ref": format!("refs/heads/{branch}"), "sha": new_commit }))
        .send()
        .await
    } else {
        req(
            &c,
            reqwest::Method::PATCH,
            &format!("/repos/{repo}/git/refs/heads/{branch}"),
            &token,
        )
        .json(&json!({ "sha": new_commit, "force": false }))
        .send()
        .await
    }
    .map_err(|e| e.to_string())?;

    if !rr.status().is_success() {
        return Err(gh_error(rr).await);
    }

    // 8) 记账
    {
        let mut g = state.cfg.lock().unwrap();
        g.github.last_sync = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        g.github.last_commit = new_commit.clone();
        g.github.branch = branch.clone();
        let _ = g.save();
    }

    // 先把用得到长度的字符串算出来，再交给结构体 —— 结构体字段按书写顺序求值，
    // 放在 message 里现算会撞上 pushed/deleted 已经被 move 进字段的借用检查。
    let report_msg = format!(
        "已推送 {} 个新增/修改，{} 个删除。",
        pushed.len(),
        deleted.len()
    );

    Ok(SyncReport {
        pushed,
        deleted,
        unchanged,
        skipped,
        commit: new_commit.chars().take(10).collect(),
        url: format!("https://github.com/{repo}"),
        branch,
        message: report_msg,
    })
}
