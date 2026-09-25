<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icons.svelte';
  import * as api from '../api';
  import type { PresetInfoLike, SyncReport } from '../api';
  import { ai, applyNoteSort, cfg, doc, refreshAll, reloadConfig, saveConfig, toast, ui } from '../state.svelte';
  import type { SettingsTab } from '../state.svelte';
  import { relTime } from '../time';
  import { applyTheme } from '../theme';

  /** 顶栏标题跟着左栏导航走，让人一眼知道自己在哪一区 */
  const SEC_LABEL: Record<SettingsTab, string> = {
    appearance: '外观',
    storage: '存储',
    ai: 'AI 助手',
    github: 'GitHub 同步',
    behavior: '行为',
    about: '关于'
  };

  /** 凭据/接口这类改动一次就够的东西默认收起来，免得手滑改坏。 */
  let open = $state({ ai: false, gh: false });

  let presets = $state<PresetInfoLike[]>([]);
  let aiKey = $state('');
  let ghToken = $state('');
  let ghHas = $state(false);
  let aiHas = $state(false);
  let info = $state<api.AppInfo | null>(null);
  let busy = $state('');
  let report = $state<SyncReport | null>(null);
  let autostart = $state(false);

  // ── GitHub 卡片的局部状态 ──
  /** 仓库连通性。empty = 仓库存在但一个提交都没有（**不是错误**，首次同步会自动建）。 */
  let ghState = $state<'idle' | 'ok' | 'empty' | 'missing'>('idle');
  /** 当前 Token 属于谁、带什么权限 —— 配错了用户自己能看出来 */
  let ghAcc = $state<api.AccountInfo | null>(null);
  /** 「仓库不存在」时用来建仓库的名字 */
  let newRepoName = $state('');
  /** 驱动相对时间刷新（每 30 秒动一次），否则「3 分钟前」会一直停在「3 分钟前」 */
  let now = $state(Date.now());

  const t = $derived(cfg.current?.theme);

  async function loadSettings() {
    presets = await api.themePresets();
    aiHas = await api.secretHas('ai_api_key');
    ghHas = await api.secretHas('github_token');
    info = await api.appInfo();
    try {
      autostart = await api.autostartEnabled();
    } catch {
      autostart = false;
    }
    // 顺手问一下「这个 Token 是谁的」——失败无所谓，卡片里少显示一行而已
    if (ghHas) ghAcc = await api.githubAccount().catch(() => null);
  }

  // ⚠️ 别把 onMount 的回调写成 async：那样返回的是 Promise，Svelte 认不出真正的
  // cleanup（类型上也会直接报错）。所以异步加载单独拎出来，回调本身保持同步。
  onMount(() => {
    void loadSettings();
    const id = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(id);
  });

  /** 改主题时立刻预览，不等到点保存 */
  function patchTheme(p: Record<string, unknown>) {
    if (!cfg.current) return;
    const next = { ...cfg.current, theme: { ...cfg.current.theme, ...p } } as api.AppConfig;
    cfg.current = next;
    applyTheme(next.theme);
  }

  /**
   * 左栏宽度：固定值 / 自适应。
   * 宽度是靠 `--panel-w` 这个 CSS 变量生效的，applyTheme 已经把数字写进去了；
   * 'auto' 交给 App.svelte 那个 effect 去称，这里只管存意图。
   */
  function setPanelWidth(w: number | 'auto') {
    patchTheme({ panelWidth: w });
    saveConfig({});
  }

  async function pickPreset(id: string) {
    const p = presets.find((x) => x.id === id);
    if (!p) return;
    patchTheme({ preset: id, accent: p.accent, accent2: p.accent2, bg: p.bg, fg: p.fg });
    await saveConfig({});
  }

  async function chooseWallpaper() {
    const p = await api.pickWallpaper();
    if (!p) return;
    patchTheme({ wallpaper: p });
    await saveConfig({});
  }

  async function chooseVault() {
    const p = await api.pickDirectory();
    if (!p) return;
    await saveConfig({ vault: p });
    await refreshAll();
    toast('vault 已切换', 'ok');
  }

  async function testAi() {
    busy = 'ai';
    try {
      const r = await api.aiTest();
      toast(r, 'ok');
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    } finally {
      busy = '';
    }
  }

  /** 从「GitHub 网址 / owner/repo / 纯仓库名」里抠出仓库名，用于预填建仓库输入框 */
  function repoNameOf(v: string): string {
    const s = v.trim().replace(/\.git$/i, '').replace(/\/+$/, '');
    const parts = s.split('/').filter(Boolean);
    return parts.length ? parts[parts.length - 1] ?? '' : '';
  }

  async function testGh() {
    busy = 'gh';
    ghState = 'idle';
    try {
      const r = await api.githubTest(cfg.current!.github.repo, cfg.current!.github.branch);
      // 后端已经把用户粘的网址归一过了 —— 用官方拼写回写配置，顺手把格式纠正掉
      if (r.fullName && r.fullName !== cfg.current!.github.repo) {
        await saveConfig({ github: { ...cfg.current!.github, repo: r.fullName } });
      }
      ghState = r.empty ? 'empty' : 'ok';
      toast(
        r.empty
          ? `${r.fullName} 目前是空仓库 —— 直接点「立即同步」，第一个提交会自动建好。`
          : `连通：${r.fullName}（分支 ${cfg.current!.github.branch || r.defaultBranch}，${r.private ? '私有' : '公开'}）`,
        'ok'
      );
    } catch (e) {
      const msg = typeof e === 'string' ? e : String(e);
      // 404 的语义是「这个仓库不存在」。与其把错误摔在用户脸上，不如递一条出路。
      // （后端 404 的文案固定带 `（404）`，这里靠它分流。）
      if (msg.includes('404')) {
        ghState = 'missing';
        newRepoName = repoNameOf(cfg.current!.github.repo);
      }
      toast(msg, 'error');
    } finally {
      busy = '';
      await reloadConfig();
    }
  }

  async function createRepo() {
    const name = newRepoName.trim();
    if (!name) return;
    busy = 'create';
    try {
      const r = await api.githubCreateRepo(name);
      // 建完直接写进配置 —— 用户的意图就是「用这个仓库」，不必再让他手动填一遍
      await saveConfig({
        github: { ...cfg.current!.github, repo: r.fullName, branch: r.defaultBranch || 'main' }
      });
      ghState = 'empty';
      toast(`已建好私有仓库 ${r.fullName}，正在推第一批文件…`, 'ok');
      // 建完立刻推一次，省掉「还得自己再点一下同步」这一步
      await doSync();
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    } finally {
      busy = '';
    }
  }

  /** 正在编辑、还没保存的文档 —— 后端对它们只读不写，免得把刚敲的字盖掉 */
  function protectList(): string[] {
    return doc.path && doc.dirty ? [doc.path] : [];
  }

  async function doSync() {
    busy = 'sync';
    report = null;
    try {
      report = await api.githubSync(protectList());
      ghState = 'ok';
      toast(report.message, 'ok');
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    } finally {
      busy = '';
      // 后端刚写过 lastSync / lastCommit —— 不回读的话界面会一直显示「从未」
      await reloadConfig();
    }
  }

  /** 只下载：把远端更新并进本地，一个字节都不往远端写 */
  async function doPull() {
    busy = 'pull';
    report = null;
    try {
      report = await api.githubPull(protectList());
      ghState = 'ok';
      toast(report.message, 'ok');
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    } finally {
      busy = '';
      await reloadConfig();
    }
  }

  async function toggleAutostart() {
    try {
      await api.setAutostart(autostart);
      await saveConfig({
        behavior: { ...cfg.current!.behavior, autostart }
      });
    } catch (e) {
      toast(String(e), 'error');
      autostart = !autostart;
    }
  }

  /** 用外部编辑器改过 vault 之后，手动刷一遍索引 */
  async function reread() {
    await refreshAll();
    toast('已重新读取 vault', 'ok');
  }
</script>

<section class="sv">
  <header class="sh">
    <h2>设置 · {SEC_LABEL[ui.settingsTab]}</h2>
    <span class="hint">改动即时生效，配置写在 {info?.configPath ?? '—'}</span>
  </header>

  <div class="scroll body">
    {#if cfg.current}
      <!-- ── 外观 ─────────────────────────────── -->
      {#if ui.settingsTab === 'appearance'}
      <div class="card">
        <h3>外观</h3>

        <div class="row">
          <span class="rlab">配色预设</span>
          <div class="swatches">
            {#each presets as p (p.id)}
              <button
                class="swatch"
                class:on={t?.preset === p.id}
                title={p.id}
                onclick={() => pickPreset(p.id)}
              >
                <i style="background:{p.bg}; border-color:{p.accent}"></i>
                <span>{p.id}</span>
              </button>
            {/each}
          </div>
        </div>

        <div class="row">
          <span class="rlab">强调色 / 辅色</span>
          <div class="inline">
            <input
              type="color"
              value={t?.accent}
              oninput={(e) => patchTheme({ accent: (e.target as HTMLInputElement).value, preset: 'custom' })}
              onchange={() => saveConfig({})}
            />
            <input
              type="color"
              value={t?.accent2}
              oninput={(e) => patchTheme({ accent2: (e.target as HTMLInputElement).value, preset: 'custom' })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{t?.accent} · {t?.accent2}</span>
          </div>
        </div>

        <div class="row">
          <span class="rlab">背景 / 前景</span>
          <div class="inline">
            <input
              type="color"
              value={t?.bg}
              oninput={(e) => patchTheme({ bg: (e.target as HTMLInputElement).value, preset: 'custom' })}
              onchange={() => saveConfig({})}
            />
            <input
              type="color"
              value={t?.fg}
              oninput={(e) => patchTheme({ fg: (e.target as HTMLInputElement).value, preset: 'custom' })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{t?.bg} · {t?.fg}</span>
          </div>
        </div>

        <div class="row">
          <span class="rlab">壁纸</span>
          <div class="inline grow">
            <input
              class="field"
              readonly
              value={t?.wallpaper || ''}
              placeholder="还没设置。选一张图铺满窗口内部。"
            />
            <button class="btn sm" onclick={chooseWallpaper}>选择图片</button>
            {#if t?.wallpaper}
              <button
                class="btn sm"
                onclick={async () => {
                  patchTheme({ wallpaper: '' });
                  await saveConfig({});
                }}>清除</button
              >
            {/if}
          </div>
        </div>

        <div class="row">
          <span class="rlab">壁纸不透明度</span>
          <div class="inline grow">
            <input
              type="range"
              min="0"
              max="1"
              step="0.02"
              value={t?.wallpaperOpacity}
              oninput={(e) => patchTheme({ wallpaperOpacity: +(e.target as HTMLInputElement).value })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{Math.round((t?.wallpaperOpacity ?? 1) * 100)}%</span>
          </div>
        </div>

        <div class="row">
          <span class="rlab">壁纸模糊</span>
          <div class="inline grow">
            <input
              type="range"
              min="0"
              max="24"
              step="1"
              value={t?.wallpaperBlur}
              oninput={(e) => patchTheme({ wallpaperBlur: +(e.target as HTMLInputElement).value })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{t?.wallpaperBlur ?? 0}px</span>
          </div>
        </div>

        <div class="row">
          <span class="rlab">面板不透明度</span>
          <div class="inline grow">
            <input
              type="range"
              min="0.35"
              max="1"
              step="0.02"
              value={t?.panelOpacity}
              oninput={(e) => patchTheme({ panelOpacity: +(e.target as HTMLInputElement).value })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{Math.round((t?.panelOpacity ?? 0.86) * 100)}%</span>
          </div>
        </div>

        <div class="row">
          <span class="rlab">圆角 / 正文字号</span>
          <div class="inline">
            <input
              type="range"
              min="0"
              max="22"
              step="1"
              value={t?.radius}
              oninput={(e) => patchTheme({ radius: +(e.target as HTMLInputElement).value })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{t?.radius}px</span>
            <input
              type="range"
              min="12"
              max="20"
              step="1"
              value={t?.fontSize}
              oninput={(e) => patchTheme({ fontSize: +(e.target as HTMLInputElement).value })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{t?.fontSize}px</span>
          </div>
        </div>

        <div class="row">
          <span class="rlab">左栏字号</span>
          <div class="inline grow">
            <input
              type="range"
              min="10.5"
              max="16"
              step="0.5"
              value={t?.panelFont}
              oninput={(e) => patchTheme({ panelFont: +(e.target as HTMLInputElement).value })}
              onchange={() => saveConfig({})}
            />
            <span class="mono dim">{t?.panelFont}px</span>
          </div>
        </div>

        <div class="row">
          <span class="rlab">左栏宽度</span>
          <div class="inline grow">
            {#if t?.panelWidth === 'auto'}
              <span class="dim">自适应中 —— 也可以直接拖分界线</span>
              <button class="btn sm" onclick={() => setPanelWidth(244)}>改成固定</button>
            {:else}
              <input
                type="range"
                min="180"
                max="520"
                step="2"
                value={typeof t?.panelWidth === 'number' ? t.panelWidth : 244}
                oninput={(e) => setPanelWidth(+(e.target as HTMLInputElement).value)}
                onchange={() => saveConfig({})}
              />
              <span class="mono dim">
                {typeof t?.panelWidth === 'number' ? Math.round(t.panelWidth) : 244}px
              </span>
              <button class="btn sm" onclick={() => setPanelWidth('auto')}>自适应</button>
            {/if}
          </div>
        </div>
      </div>
      {/if}

      <!-- ── 存储 ─────────────────────────────── -->
      {#if ui.settingsTab === 'storage'}
      <div class="card">
        <h3>存储</h3>
        <div class="row">
          <span class="rlab">vault 目录</span>
          <div class="inline grow">
            <input class="field" readonly value={cfg.current.vault} />
            <button class="btn sm" onclick={chooseVault}>更换</button>
          </div>
        </div>

        <div class="row">
          <span class="rlab">重新读取</span>
          <div class="inline grow">
            <button class="btn sm" onclick={reread}>重新读取 vault</button>
            <span class="dim">用外部编辑器改过文件之后，手动刷一遍索引</span>
          </div>
        </div>

        <p class="note">
          所有内容都是这个目录里的普通 Markdown 文件，你可以直接拿 Obsidian 打开。
          <b>别把 vault 放在 OneDrive / 坚果云这类同步盘的目录下</b>，文件锁和「冲突副本」会把这里搅烂。
        </p>
      </div>
      {/if}

      <!-- ── AI ───────────────────────────────── -->
      {#if ui.settingsTab === 'ai'}
      <div class="card">
        <h3>AI 助手</h3>

        <div class="statusline">
          <span class="badge" class:ok={aiHas}>{aiHas ? 'Key 已配置' : 'Key 未配置'}</span>
          <span class="mono dim ellip">{cfg.current.ai.model || '未指定模型'}</span>
          <span class="spacer"></span>
          <button class="btn sm" disabled={busy === 'ai'} onclick={testAi}>
            {busy === 'ai' ? '测试中…' : '测试连通性'}
          </button>
        </div>

        <button class="foldhead" onclick={() => (open.ai = !open.ai)}>
          <Icon name={open.ai ? 'chev-d' : 'chev-r'} size={13} />
          <span>接口配置</span>
          <span class="dim">地址 · 模型 · Key · 提示词</span>
        </button>

        {#if open.ai}
          <div class="foldbody">
            <div class="row">
              <span class="rlab">API 地址</span>
              <input
                class="field grow"
                bind:value={cfg.current.ai.baseUrl}
                onchange={() => saveConfig({})}
                placeholder="https://api.deepseek.com/v1"
              />
            </div>
            <div class="row">
              <span class="rlab">模型</span>
              <input
                class="field grow"
                bind:value={cfg.current.ai.model}
                onchange={() => saveConfig({})}
              />
            </div>
            <div class="row">
              <span class="rlab">温度</span>
              <div class="inline grow">
                <input
                  type="range"
                  min="0"
                  max="1.5"
                  step="0.1"
                  bind:value={cfg.current.ai.temperature}
                  onchange={() => saveConfig({})}
                />
                <span class="mono dim">{cfg.current.ai.temperature}</span>
              </div>
            </div>
            <div class="row">
              <span class="rlab">最多引用笔记</span>
              <div class="inline grow">
                <input
                  type="number"
                  class="field"
                  style="width:92px"
                  min="0"
                  max="12"
                  bind:value={cfg.current.ai.maxRefs}
                  onchange={() => saveConfig({})}
                />
                <span class="dim">篇</span>
              </div>
            </div>
            <div class="row">
              <span class="rlab">API Key</span>
              <div class="inline grow">
                <input
                  class="field"
                  type="password"
                  bind:value={aiKey}
                  placeholder={aiHas ? '已保存（存在系统凭据管理器）' : '还没有配置'}
                />
                <button
                  class="btn sm primary"
                  onclick={async () => {
                    if (!aiKey.trim()) return;
                    await api.secretSet('ai_api_key', aiKey.trim());
                    aiKey = '';
                    aiHas = true;
                    ai.hasKey = true;
                    toast('Key 已存进 Windows 凭据管理器', 'ok');
                  }}>保存</button
                >
                {#if aiHas}
                  <button
                    class="btn sm danger"
                    onclick={async () => {
                      await api.secretClear('ai_api_key');
                      aiHas = false;
                      ai.hasKey = false;
                      toast('已清除');
                    }}>清除</button
                  >
                {/if}
              </div>
            </div>
            <div class="row">
              <span class="rlab">系统提示词</span>
              <textarea
                class="field ta"
                rows="4"
                bind:value={cfg.current.ai.systemPrompt}
                onchange={() => saveConfig({})}
              ></textarea>
            </div>
          </div>
        {/if}

        <p class="note">
          Key 走 Windows 凭据管理器，<b>不会写进任何文件</b>，更不会被同步到 GitHub。
          协议是 OpenAI 兼容格式，所以 DeepSeek、通义、Kimi、本地 Ollama 都能直接接。
          {#if !open.ai}<b>要改地址或换 Key，展开上面的「接口配置」。</b>{/if}
        </p>
      </div>
      {/if}

      <!-- ── GitHub ───────────────────────────── -->
      {#if ui.settingsTab === 'github'}
      <div class="card">
        <h3>GitHub 同步</h3>

        <div class="statusline">
          <span class="badge" class:ok={ghHas}>{ghHas ? 'Token 已配置' : 'Token 未配置'}</span>
          {#if ghAcc}
            <span class="dim">
              账号 <b>{ghAcc.login}</b>{#if ghAcc.scopes.length}· 权限 {ghAcc.scopes.join(' ')}{/if}
            </span>
          {/if}
        </div>

        <div class="statusline">
          <button class="btn sm" disabled={busy === 'gh'} onclick={testGh}>
            {busy === 'gh' ? '检查中…' : '检查连通性'}
          </button>
          <button
            class="btn sm primary"
            disabled={busy === 'sync' || busy === 'pull' || busy === 'create'}
            onclick={doSync}
          >
            {busy === 'sync' ? '同步中…' : '立即同步'}
          </button>
          <button
            class="btn sm"
            disabled={busy === 'sync' || busy === 'pull' || busy === 'create'}
            onclick={doPull}
          >
            {busy === 'pull' ? '下载中…' : '仅下载'}
          </button>
          <span class="dim">
            上次：{cfg.current.github.lastSync ? relTime(cfg.current.github.lastSync, now) : '从未'}
          </span>
        </div>

        <p class="note">
          「同步」= 先把远端的更新拿下来，再把本地的改动传上去（换台电脑接着写，点它就行）。
          「仅下载」只往本地写，远端一个字节都不动。
        </p>

        <!-- 检查过之后才有话可说：正常 / 空仓库 / 不存在（附建仓入口） -->
        {#if ghState !== 'idle'}
          <div
            class="repoinfo"
            class:ok={ghState === 'ok'}
            class:warn={ghState === 'empty'}
            class:bad={ghState === 'missing'}
          >
            {#if ghState === 'ok'}
              <div class="rl">
                <Icon name="check" size={13} />
                <span
                  ><b>{cfg.current.github.repo}</b> · 分支 {cfg.current.github.branch || 'main'} ·
                  可以同步</span
                >
              </div>
            {:else if ghState === 'empty'}
              <div class="rl">
                <Icon name="info" size={13} />
                <span>仓库是空的 —— 首次同步会自动建好第一个提交和分支，直接点「立即同步」就行。</span>
              </div>
            {:else}
              <div class="rl">
                <Icon name="info" size={13} />
                <span>仓库 <b>{cfg.current.github.repo}</b> 不存在。</span>
              </div>
              <div class="rl pick">
                <input class="field" bind:value={newRepoName} placeholder="仓库名，例如 my-notes" />
                <button
                  class="btn sm primary"
                  disabled={busy === 'create' || !newRepoName.trim()}
                  onclick={createRepo}
                >
                  {busy === 'create' ? '创建中…' : '建为私有仓库'}
                </button>
              </div>
              <div class="rl dim">
                会在你的账号下建一个<b>私有</b>仓库，然后立刻把 vault 推上去。
                （同步的是日记和笔记，所以这里不给「公开」这个选项。）
              </div>
            {/if}
          </div>
        {/if}

        <button class="foldhead" onclick={() => (open.gh = !open.gh)}>
          <Icon name={open.gh ? 'chev-d' : 'chev-r'} size={13} />
          <span>仓库配置</span>
          <span class="dim">仓库 · 分支 · Token</span>
        </button>

        {#if open.gh}
          <div class="foldbody">
            <div class="row">
              <span class="rlab">仓库</span>
              <input
                class="field grow"
                bind:value={cfg.current.github.repo}
                onchange={() => saveConfig({})}
                placeholder="owner/repo，或直接粘 GitHub 网址"
              />
            </div>
            <div class="row">
              <span class="rlab">分支</span>
              <input
                class="field grow"
                bind:value={cfg.current.github.branch}
                onchange={() => saveConfig({})}
              />
            </div>
            <div class="row">
              <span class="rlab">Token</span>
              <div class="inline grow">
                <input
                  class="field"
                  type="password"
                  bind:value={ghToken}
                  placeholder={ghHas ? '已保存' : '需要 repo 权限的 Personal Access Token'}
                />
                <button
                  class="btn sm primary"
                  onclick={async () => {
                    if (!ghToken.trim()) return;
                    await api.secretSet('github_token', ghToken.trim());
                    ghToken = '';
                    ghHas = true;
                    toast('Token 已保存', 'ok');
                  }}>保存</button
                >
              </div>
            </div>
          </div>
        {/if}

        {#if report}
          <div class="report">
            <p>
              {#if report.commit}提交 <span class="mono">{report.commit}</span> · {/if}
              下载 {report.pulled.length} · 上传 {report.pushed.length} · 未变 {report.unchanged}
            </p>
            {#if report.pulled.length}
              <p class="dim">
                从远端写入本地 {report.pulled.length} 个：<span class="mono"
                  >{report.pulled.slice(0, 6).join('、')}</span
                >{#if report.pulled.length > 6} 等{/if}
              </p>
            {/if}
            {#if report.conflicts.length}
              <p class="conf">
                冲突 {report.conflicts.length} 个 —— 两边都改过，<b>本地那份一个字都没动</b>，
                远端那份另存成了副本，你自己合：
                {#each report.conflicts.slice(0, 4) as cf}
                  <br /><span class="mono">{cf.path}</span> → <span class="mono">{cf.savedAs}</span>
                {/each}
                {#if report.conflicts.length > 4}<br />…等 {report.conflicts.length} 个{/if}
              </p>
            {/if}
            {#if report.localDeleted.length}
              <p class="delli">
                远端删过这 {report.localDeleted.length} 个，本地没动过，所以本地也删了：<span
                  class="mono">{report.localDeleted.slice(0, 6).join('、')}</span
                >{#if report.localDeleted.length > 6} 等{/if}
              </p>
            {/if}
            {#if report.deleted.length}
              <p class="delli">
                本地删过这 {report.deleted.length} 个，远端没动过，所以远端也删了：<span class="mono"
                  >{report.deleted.slice(0, 6).join('、')}</span
                >{#if report.deleted.length > 6} 等{/if}
              </p>
            {/if}
            {#if report.skipped.length}
              <p class="dim">跳过 {report.skipped.length} 个：{report.skipped.slice(0, 4).join('、')}</p>
            {/if}
          </div>
        {/if}

        <p class="note">
          走 Git Data API，<b>一次同步只产生一个 commit</b>，不会把仓库刷成一堆流水账。
          同步是双向的：靠「上次同步的那个提交」当参照，逐文件判断差异是<b>谁</b>造成的 ——
          只有本地删过、且远端没动过的文件才会被删，别人推上去的东西不会被你顺手清掉。
          {#if !open.gh}<b>要换仓库或更新 Token，展开上面的「仓库配置」。</b>{/if}
        </p>
      </div>
      {/if}

      <!-- ── 行为 ─────────────────────────────── -->
      {#if ui.settingsTab === 'behavior'}
      <div class="card">
        <h3>行为</h3>
        <label class="check">
          <input
            type="checkbox"
            bind:checked={cfg.current.behavior.closeToTray}
            onchange={() => saveConfig({})}
          />
          <span>关闭窗口时收进托盘（<b>关窗 ≠ 退出</b>，从桌面再次启动会唤起已有实例，不会开第二个）</span>
        </label>
        <label class="check">
          <input
            type="checkbox"
            bind:checked={cfg.current.behavior.startHidden}
            onchange={() => saveConfig({})}
          />
          <span>开机自启时直接缩进托盘，不弹窗</span>
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={autostart} onchange={toggleAutostart} />
          <span>开机自动启动</span>
        </label>
        <label class="check">
          <input
            type="checkbox"
            bind:checked={cfg.current.behavior.hidePanelWhenAiOpen}
            onchange={() => saveConfig({})}
          />
          <span>AI 抽屉展开时自动收起左侧栏（窗口较窄时生效）</span>
        </label>
        <div class="row">
          <span class="rlab">启动默认界面</span>
          <div class="inline">
            <select
              class="field"
              style="width:120px"
              bind:value={cfg.current.behavior.defaultActivity}
              onchange={() => saveConfig({})}
            >
              <option value="notes">笔记</option>
              <option value="diary">日记</option>
              <option value="checkin">打卡</option>
              <option value="task">任务</option>
            </select>
          </div>
        </div>
        <label class="check">
          <input
            type="checkbox"
            bind:checked={cfg.current.behavior.autoCreateDiary}
            onchange={() => saveConfig({})}
          />
          <span>
            启动时自动创建「今天」的日记
            <b class="dim">
              （只管建文件，不跳过去 —— 想开局就进今天这篇，把上面的默认界面设成「日记」）
            </b>
          </span>
        </label>
        <div class="row">
          <span class="rlab">笔记排序</span>
          <div class="inline">
            <select
              class="field"
              style="width:120px"
              bind:value={cfg.current.behavior.noteSort}
              onchange={async () => {
                await saveConfig({});
                applyNoteSort();
              }}
            >
              <option value="name">名称</option>
              <option value="mtime">修改时间</option>
            </select>
          </div>
        </div>
        <div class="row">
          <span class="rlab">默认模式</span>
          <div class="inline">
            <select
              class="field"
              style="width:120px"
              bind:value={cfg.current.editor.defaultMode}
              onchange={() => saveConfig({})}
            >
              <option value="edit">编辑</option>
              <option value="read">阅读</option>
            </select>
          </div>
        </div>
      </div>
      {/if}

      <!-- ── 关于 ─────────────────────────────── -->
      {#if ui.settingsTab === 'about'}
      <div class="card">
        <h3>关于</h3>
        <div class="about">
          <div class="logo"></div>
          <div>
            <p class="name">mnesphere <span class="mono dim">v{info?.version}</span></p>
            <p class="dim">本地优先的个人知识球 —— 日记 / 打卡 / 双链笔记，全部以 Markdown 保存。</p>
            <p class="dim mono">
              Rust {info?.platform} · 数据目录 {info?.vault}
            </p>
          </div>
        </div>
      </div>
      {/if}
    {/if}
  </div>
</section>

<style>
  .sv {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
  .sh {
    display: flex;
    align-items: baseline;
    gap: 12px;
    height: 44px;
    flex: 0 0 44px;
    padding: 0 20px;
    border-bottom: 1px solid var(--line);
  }
  .sh h2 {
    font-size: 14px;
    font-weight: 500;
    line-height: 44px;
  }
  .hint {
    font-size: 11px;
    color: var(--fg-faint);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .body {
    flex: 1;
    min-height: 0;
    padding: 18px 20px 60px;
    max-width: 900px;
  }

  .card {
    padding: 16px 18px;
    border-radius: calc(var(--radius) * 0.9);
    /* 实心卡片色，不再借 --hover 那层白色蒙版 —— 那样叠出来是灰的。
       层次靠描边和顶部一道极淡的高光，而不是靠整体提亮。 */
    background: var(--card);
    border: 1px solid var(--line);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.028);
    margin-bottom: 14px;
  }
  .card h3 {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--accent);
    margin-bottom: 14px;
    letter-spacing: 0.3px;
  }

  /* 凭据区块：状态常驻可见，输入项默认收起来 */
  .statusline {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 10px;
  }
  .spacer {
    flex: 1;
  }
  .ellip {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 320px;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 9px;
    border-radius: 999px;
    background: var(--panel-2);
    border: 1px solid var(--line);
    color: var(--fg-faint);
    font-size: 11px;
    white-space: nowrap;
  }
  .badge.ok {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }

  .foldhead {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 8px 10px;
    margin-bottom: 4px;
    border-radius: 8px;
    background: var(--inset);
    border: 1px solid var(--line);
    color: var(--fg-mute);
    font-size: 12px;
    text-align: left;
  }
  .foldhead:hover {
    color: var(--fg);
    border-color: var(--line-2);
  }
  .foldhead > span:first-of-type {
    flex: 1;
  }
  .foldhead .dim {
    font-size: 11px;
  }

  .foldbody {
    padding: 12px 10px 4px;
    margin: 0 0 4px 4px;
    border-left: 2px solid var(--line);
    animation: unfold 0.16s ease-out;
  }
  @keyframes unfold {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    margin-bottom: 11px;
  }
  /* 行标签只是「这一项叫什么」，本身不绑定控件，所以用 span 而不是 label。
     真正的 label 只留给 .check 那种把 checkbox 包在里面的用法。 */
  .row > .rlab {
    flex: 0 0 118px;
    padding-top: 7px;
    font-size: 12px;
    color: var(--fg-mute);
  }
  .row .grow {
    flex: 1;
  }

  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  input[type='color'] {
    width: 34px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--line-2);
    border-radius: 7px;
    background: transparent;
    cursor: pointer;
  }
  input[type='range'] {
    flex: 1;
    min-width: 120px;
    accent-color: var(--accent);
    height: 20px;
  }
  input[type='checkbox'] {
    accent-color: var(--accent);
    width: 14px;
    height: 14px;
    cursor: pointer;
  }
  select.field {
    appearance: none;
    cursor: pointer;
  }
  .ta {
    height: auto;
    padding: 9px 11px;
    line-height: 1.7;
    resize: vertical;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .swatches {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .swatch {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    padding: 6px 10px;
    border-radius: 9px;
    border: 1px solid var(--line);
    font-size: 10.5px;
    color: var(--fg-mute);
  }
  .swatch i {
    width: 30px;
    height: 18px;
    border-radius: 5px;
    border: 2px solid;
  }
  .swatch.on {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--accent);
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
    font-size: 12.5px;
    color: var(--fg-dim);
    line-height: 1.65;
    cursor: pointer;
  }
  .check input {
    margin-top: 3px;
  }

  .note {
    margin-top: 10px;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--inset);
    border-left: 2px solid var(--accent-line);
    font-size: 11.5px;
    color: var(--fg-mute);
    line-height: 1.75;
  }
  .note b {
    color: var(--fg-dim);
    font-weight: 500;
  }

  .report {
    margin-top: 10px;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--accent-soft);
    font-size: 11.5px;
    color: var(--accent);
    line-height: 1.7;
  }

  /* 「远端有、本地没有」的删除项 —— 得比常规报告扎眼，它代表真的少了东西 */
  .report .delli {
    margin: 5px 0 0;
    padding-top: 6px;
    border-top: 1px dashed var(--line);
    color: var(--warn);
  }

  /* 冲突项：两边都动过，得有人来定夺。比常规报告重，但和「删除」区分开 */
  .report .conf {
    margin: 5px 0 0;
    padding-top: 6px;
    border-top: 1px dashed var(--line);
    color: var(--fg);
  }
  .report .conf b {
    color: var(--warn);
    font-weight: 500;
  }

  /* 仓库连通性回执：正常 / 空仓库 / 不存在，三态用左侧色条区分 */
  .repoinfo {
    display: flex;
    flex-direction: column;
    gap: 7px;
    margin-top: 10px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-left-width: 2px;
    border-radius: 8px;
    background: var(--card);
    font-size: 11.5px;
    line-height: 1.7;
  }
  .repoinfo .rl {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }
  .repoinfo .rl.pick .field {
    max-width: 220px;
  }
  .repoinfo .rl.dim {
    color: var(--fg-faint);
    align-items: flex-start;
  }
  .repoinfo.ok {
    border-left-color: var(--accent-2);
    color: var(--accent-2);
  }
  .repoinfo.warn {
    border-left-color: var(--warn);
    color: var(--warn);
  }
  .repoinfo.bad {
    border-left-color: var(--danger);
  }

  .about {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .logo {
    width: 42px;
    height: 42px;
    border-radius: 12px;
    border: 2px solid var(--accent-line);
    background:
      radial-gradient(circle at 50% 50%, transparent 30%, var(--accent-soft) 31%, transparent 62%),
      var(--panel-2);
    flex: 0 0 auto;
  }
  .about p {
    font-size: 12px;
    line-height: 1.7;
  }
  .about .name {
    font-size: 13px;
    color: var(--fg);
  }
  .dim {
    color: var(--fg-faint);
  }
</style>
