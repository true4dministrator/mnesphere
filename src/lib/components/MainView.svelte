<script lang="ts">
  import Editor from './Editor.svelte';
  import CheckinView from './CheckinView.svelte';
  import SettingsView from './SettingsView.svelte';
  import TaskView from './TaskView.svelte';
  import { ui } from '../state.svelte';
</script>

{#if ui.activity === 'checkin'}
  <CheckinView />
{:else if ui.activity === 'task'}
  <TaskView />
{:else if ui.activity === 'settings'}
  <SettingsView />
{:else}
  <Editor />
{/if}
