# mnesphere

**本地优先的个人知识球** —— 笔记 / 日记 / 打卡 / 任务收在一个 vault 里，全部以 Markdown 明文存储。

Windows 桌面应用，Tauri 2 + Rust + Svelte 5。无账号、无云端、无数据库：你的知识就是磁盘上那些 `.md` 文件，随时可以用别的编辑器打开、用 Git 备份、或者整个目录拷走。

## 功能

**笔记** —— Obsidian 式 `[[双链]]`，文件夹树；排序支持「名称」（中文按**拼音**，且 `第2章` 排在 `第10章` 前）或「修改时间」；`Ctrl+K` 全文检索。

**日记** —— 一天一篇，按 `日记/YYYY-MM/YYYY-MM-DD.md` 落盘，按月分组。

**打卡** —— 习惯定义 + 日期矩阵，三态：做到了 / 没做到 / 未打卡，未到的日期画虚线。

**任务** —— 单清单文件，行语法即可手写：`- [ ] 标题 [@2026-09-22] [!高]`，缩进两格的下一行算备注。左栏分「短期 / 长期 / 已完成」三视图。

**编辑与阅读** —— CodeMirror 6 编辑器 + 独立阅读态；图片直接拖入或 `Ctrl+V` 粘贴，自动按 `附件/YYYY-MM/` 归档；`$…$` / `$$…$$` 数学公式由 KaTeX 渲染。阅读态与编辑态看到的是同一份内容。

**AI 助手**（可选）—— 右侧抽屉，任何 OpenAI 兼容接口都行（默认 DeepSeek）。当前页会自动作为上下文，另有手动挂载列表。**回答与笔记走同一套 Markdown / 公式渲染**。

**GitHub 同步**（可选）—— 把 vault 推到你自己的私有仓库。

**外观** —— Hyprland 风格无边框自绘标题栏；配色由一套算法从基色推导（默认荧光青），字号与左栏字号可分别调。

## 数据与隐私

- 内容只存本地 `vault` 目录（**不要放在 OneDrive 等同步盘里**）。
- API Key / GitHub Token 存进 **系统凭据管理器**，不落盘、不进 vault、不进本仓库。
- 「关窗进托盘」而非退出；编辑内容有防抖自动保存。

## 构建

需要 [Rust](https://rustup.rs/) 工具链、Node 20+ 与 WebView2（Windows 10/11 自带）。

```bash
npm install
npm run tauri:dev     # 开发
npm run tauri:build   # 出包
```

产物：

- 免安装：`src-tauri/target/release/mnesphere.exe`
- 安装包：`src-tauri/target/release/bundle/nsis/mnesphere_<version>_x64-setup.exe`

## 结构

```
src/                前端（Svelte 5 runes）
  lib/markdown.ts   渲染管线：屏蔽代码 → 屏蔽数学 → markdown-it → 换回 KaTeX
  lib/math.ts       公式扫描与渲染
  lib/notesSort.ts  笔记树排序（唯一口径）
  lib/tasks.ts      任务分组 / 排序 / 日期文案（唯一口径）
src-tauri/src/      后端（Rust）
  vault.rs          路径与附件归档
  note.rs           笔记 / 日记索引与读写
  habit.rs          打卡
  task.rs           任务（行级写回，带行号漂移防护）
  ai.rs             流式对话
  github.rs         vault 同步
```

## 许可

GPL-3.0，见 [LICENSE](LICENSE)。
