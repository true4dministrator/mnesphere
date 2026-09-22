<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icons.svelte';
  import * as api from '../api';
  import type { AppConfig } from '../api';
  import { closeWelcome, cfg, refreshAll, saveConfig, toast, welcome } from '../state.svelte';
  import { applyTheme, type Theme } from '../theme';

  let presets = $state<api.PresetInfoLike[]>([]);

  let vault = $state('');
  let theme = $state<Theme | null>(null);
  let aiBase = $state('');
  let aiModel = $state('');
  let aiKey = $state('');
  let ghRepo = $state('');
  let ghBranch = $state('main');
  let ghToken = $state('');
  let busy = $state(false);

  // 引导页只在 bootstrap 之后才可能出现，所以 cfg.current 一定已经就位。
  // 用 onMount 做一次性初始化，别用 $effect —— 那是「读自己又写自己」，容易打转。
  onMount(async () => {
    const c = cfg.current;
    if (c) {
      vault = c.vault;
      theme = { ...c.theme };
      aiBase = c.ai.baseUrl;
      aiModel = c.ai.model;
      ghRepo = c.github.repo;
      ghBranch = c.github.branch || 'main';
    }
    try {
      presets = await api.themePresets();
    } catch {
      /* 拿不到预设就只用当前配色，不影响走完引导 */
    }
  });

  function usePreset(p: api.PresetInfoLike) {
    if (!theme) return;
    theme = { ...theme, preset: p.id, accent: p.accent, accent2: p.accent2, bg: p.bg, fg: p.fg };
    applyTheme(theme);
  }

  function patchTheme(patch: Partial<Theme>) {
    if (!theme) return;
    theme = { ...theme, ...patch, preset: 'custom' };
    applyTheme(theme);
  }

  async function pickVault() {
    const p = await api.pickDirectory();
    if (p) vault = p;
  }

  async function pickWall() {
    const p = await api.pickWallpaper();
    if (p) patchTheme({ wallpaper: p });
  }

  const STEPS = ['存储位置', '外观', '连接（可跳过）'];

  async function finish() {
    if (busy) return;
    const c = cfg.current;
    if (!c) return;
    busy = true;
    try {
      if (theme) applyTheme(theme);
      const patch: Partial<AppConfig> = {
        vault,
        onboarded: true,
        ai: {
          ...c.ai,
          baseUrl: aiBase.trim() || c.ai.baseUrl,
          model: aiModel.trim() || c.ai.model
        },
        github: { ...c.github, repo: ghRepo.trim(), branch: ghBranch.trim() || 'main' }
      };
      if (theme) patch.theme = theme;
      await saveConfig(patch);

      if (aiKey.trim()) await api.secretSet('ai_api_key', aiKey.trim());
      if (ghToken.trim()) await api.secretSet('github_token', ghToken.trim());

      await refreshAll();
      closeWelcome();
      toast('设置好了，开始写吧', 'ok');
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    } finally {
      busy = false;
    }
  }

  function next() {
    if (welcome.step < 2) welcome.step += 1;
    else void finish();
  }

  function back() {
    if (welcome.step > 0) welcome.step -= 1;
  }

  /** 整段跳过：只写 onboarded，其余全保持默认。 */
  async function skipAll() {
    try {
      await saveConfig({ onboarded: true });
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    }
    closeWelcome();
  }
</script>

<div class="veil">
  <section class="card">
    <header>
      <span class="glyph"></span>
      <div class="ht">
        <h2>欢迎使用 mnesphere</h2>
        <p>本地优先的个人知识球 —— 日记、打卡、双链笔记，全都存成 Markdown。</p>
      </div>
    </header>

    <div class="steps">
      {#each STEPS as s, i (s)}
        <span class="sd" class:on={i === welcome.step} class:done={i < welcome.step}>
          <i>{i + 1}</i>{s}
        </span>
      {/each}
    </div>

    <div class="body">
      {#if welcome.step === 0}
        <div class="row">
          <span class="rlab">vault 目录</span>
          <div class="inline grow">
            <input class="field" readonly value={vault} />
            <button class="btn" onclick={pickVault}><Icon name="folder" size={13} /> 选择</button>
          </div>
        </div>
        <p class="note">
          所有内容都会存在这个目录里，一个文件就是一份真相。<br />
          <b>别放在 OneDrive / 坚果云等同步盘下</b> —— 同步盘的锁文件和冲突副本会把写入搞坏。
        </p>
        <div class="row">
          <span class="rlab">说明</span>
          <div class="feats">
            <span class="chip accent">Markdown 原件</span>
            <span class="chip">双链 [[笔记]]</span>
            <span class="chip">可推 GitHub</span>
            <span class="chip">无数据库</span>
          </div>
        </div>
      {:else if welcome.step === 1}
        <div class="row">
          <span class="rlab">配色预设</span>
          <div class="swatches">
            {#each presets as p (p.id)}
              <button
                class="swatch"
                class:on={theme?.preset === p.id}
                title={p.id}
                onclick={() => usePreset(p)}
              >
                <i style="background:{p.bg}; border-color:{p.accent}"></i>
                <span>{p.id}</span>
              </button>
            {/each}
          </div>
        </div>
        <div class="row">
          <span class="rlab">强调色</span>
          <div class="inline">
            <input
              type="color"
              value={theme?.accent}
              oninput={(e) => patchTheme({ accent: (e.target as HTMLInputElement).value })}
            />
            <input
              type="color"
              value={theme?.accent2}
              oninput={(e) => patchTheme({ accent2: (e.target as HTMLInputElement).value })}
            />
            <span class="mono dim">{theme?.accent} · {theme?.accent2}</span>
          </div>
        </div>
        <div class="row">
          <span class="rlab">壁纸</span>
          <div class="inline grow">
            <input class="field" readonly value={theme?.wallpaper || ''} placeholder="可选，铺在窗口内部" />
            <button class="btn" onclick={pickWall}><Icon name="image" size={13} /> 选图</button>
            {#if theme?.wallpaper}
              <button class="btn" onclick={() => patchTheme({ wallpaper: '' })}>清除</button>
            {/if}
          </div>
        </div>
        <div class="row">
          <span class="rlab">壁纸浓度</span>
          <div class="inline grow">
            <input
              type="range"
              min="0.15"
              max="1"
              step="0.05"
              value={theme?.wallpaperOpacity ?? 1}
              oninput={(e) =>
                patchTheme({ wallpaperOpacity: Number((e.target as HTMLInputElement).value) })}
            />
            <span class="mono dim">{Math.round((theme?.wallpaperOpacity ?? 1) * 100)}%</span>
          </div>
        </div>
        <p class="note">改动是即时预览的，现在看到什么样，进来就是什么样。</p>
      {:else}
        <div class="row">
          <span class="rlab">AI 接口</span>
          <div class="inline grow">
            <input class="field" bind:value={aiBase} placeholder="https://api.deepseek.com/v1" />
            <input class="field" bind:value={aiModel} placeholder="deepseek-chat" />
          </div>
        </div>
        <div class="row">
          <span class="rlab">API Key</span>
          <input class="field" type="password" bind:value={aiKey} placeholder="留空就不配，之后在设置里补" />
        </div>
        <div class="row">
          <span class="rlab">GitHub 仓库</span>
          <div class="inline grow">
            <input class="field" bind:value={ghRepo} placeholder="owner/repo" />
            <input class="field branch" bind:value={ghBranch} placeholder="main" />
          </div>
        </div>
        <div class="row">
          <span class="rlab">Token</span>
          <input class="field" type="password" bind:value={ghToken} placeholder="留空就不配" />
        </div>
        <p class="note">
          密钥会存进 Windows 凭据管理器，<b>不会</b>写进 vault，也不会跟着同步走。
          这两项现在不填完全没问题，随时能在设置里补。
        </p>
      {/if}
    </div>

    <footer>
      <button class="skip" onclick={skipAll}>跳过引导</button>
      <div class="nav">
        {#if welcome.step > 0}
          <button class="btn" onclick={back}>上一步</button>
        {/if}
        <button class="btn primary" disabled={busy} onclick={next}>
          {welcome.step === 2 ? (busy ? '正在保存…' : '开始使用') : '下一步'}
        </button>
      </div>
    </footer>
  </section>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    backdrop-filter: blur(10px);
  }

  .card {
    display: flex;
    flex-direction: column;
    width: min(620px, 100%);
    max-height: 100%;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--line-2);
    box-shadow: 0 30px 70px -30px rgba(0, 0, 0, 0.8);
    overflow: hidden;
  }

  header {
    display: flex;
    gap: 12px;
    padding: 20px 22px 14px;
  }
  .glyph {
    width: 14px;
    height: 14px;
    margin-top: 4px;
    border-radius: 4px;
    background: var(--accent);
    box-shadow: 0 0 14px -1px var(--accent-line);
    flex: 0 0 auto;
  }
  .ht h2 {
    font-size: 16px;
    font-weight: 500;
    color: var(--fg);
  }
  .ht p {
    margin-top: 4px;
    font-size: 12.5px;
    color: var(--fg-mute);
  }

  .steps {
    display: flex;
    gap: 16px;
    padding: 0 22px 14px;
    border-bottom: 1px solid var(--line);
  }
  .sd {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--fg-faint);
  }
  .sd i {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 1px solid var(--line-2);
    font-style: normal;
    font-size: 10px;
  }
  .sd.on {
    color: var(--accent);
  }
  .sd.on i {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
  .sd.done {
    color: var(--accent-2);
  }
  .sd.done i {
    border-color: var(--accent-line);
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 16px 22px 6px;
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 13px;
  }
  .rlab {
    flex: 0 0 96px;
    padding-top: 7px;
    font-size: 12px;
    color: var(--fg-mute);
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .inline.grow {
    flex: 1;
    min-width: 0;
  }
  .branch {
    max-width: 110px;
  }

  input[type='color'] {
    width: 30px;
    height: 26px;
    padding: 0;
    border: 1px solid var(--line-2);
    border-radius: 6px;
    background: none;
    cursor: pointer;
  }
  input[type='range'] {
    flex: 1;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .swatch {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 9px;
    border-radius: 999px;
    border: 1px solid var(--line);
    color: var(--fg-mute);
    font-size: 11.5px;
  }
  .swatch i {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 2px solid transparent;
  }
  .swatch.on {
    border-color: var(--accent-line);
    color: var(--accent);
    background: var(--accent-soft);
  }
  .swatch:hover {
    background: var(--hover);
  }

  .feats {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding-top: 3px;
  }

  .note {
    margin: 2px 0 14px 108px;
    font-size: 11.5px;
    line-height: 1.75;
    color: var(--fg-faint);
  }
  .note b {
    color: var(--warn);
    font-weight: 500;
  }

  .dim {
    color: var(--fg-faint);
    font-size: 11px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 12px 22px 16px;
    border-top: 1px solid var(--line);
  }
  .nav {
    display: flex;
    gap: 8px;
  }
  .skip {
    color: var(--fg-faint);
    font-size: 12px;
  }
  .skip:hover {
    color: var(--fg-mute);
    text-decoration: underline;
  }
</style>
