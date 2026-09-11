import { createSignal, onMount, Show } from 'solid-js';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke, isTauri } from '../tauri';
import {
  project, openProject, notify, runAction, reviewerName, persistReviewer, pendingReviews, toast,
} from '../store';
import DictionaryView from './DictionaryView';
import ReviewView from './ReviewView';
import QueueView from './QueueView';
import PackagesView from './PackagesView';

type Tab = 'review' | 'dict' | 'queue' | 'packages';

export default function App() {
  const [tab, setTab] = createSignal<Tab>('review');
  const [dirInput, setDirInput] = createSignal('');

  onMount(async () => {
    if (!isTauri) return;
    const p = await invoke<{ name: string; revision: number } | null>('current_project').catch(() => null);
    if (p) notify('ok', `已打开工程「${p.name}」r${p.revision}`);
  });

  const pickAndOpen = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === 'string') {
      setDirInput(dir);
      await runAction('打开工程', async () => {
        await openProject(dir);
        notify('ok', `工程「${project()?.name}」已打开（修订 r${project()?.revision}）`);
      });
    }
  };

  const openTyped = async () => {
    if (!dirInput()) return;
    await runAction('打开工程', async () => {
      await openProject(dirInput());
      notify('ok', `工程「${project()?.name}」已打开（修订 r${project()?.revision}）`);
    });
  };

  return (
    <div class="app">
      <header class="topbar">
        <div class="brand">🎙 有声书工坊</div>
        <Show when={project()} fallback={<span class="muted">未打开工程</span>}>
          <span class="proj">
            <b>{project()!.name}</b>{' '}
            <span class="rev">r{project()!.revision}</span>
          </span>
        </Show>
        <div class="grow" />
        <label class="reviewer">
          审听者
          <input value={reviewerName()} onInput={(e) => persistReviewer(e.currentTarget.value)} />
        </label>
        <input
          class="dir-input"
          placeholder="工程目录路径"
          value={dirInput()}
          onInput={(e) => setDirInput(e.currentTarget.value)}
        />
        <button onClick={openTyped}>打开路径</button>
        <button class="primary" onClick={pickAndOpen}>选择目录/新建</button>
      </header>

      <nav class="tabs">
        <button class={tab() === 'review' ? 'active' : ''} onClick={() => setTab('review')}>
          波形审听
        </button>
        <button class={tab() === 'dict' ? 'active' : ''} onClick={() => setTab('dict')}>
          发音词典
        </button>
        <button class={tab() === 'queue' ? 'active' : ''} onClick={() => setTab('queue')}>
          待复核 {pendingReviews().length > 0 && <span class="badge">{pendingReviews().length}</span>}
        </button>
        <button class={tab() === 'packages' ? 'active' : ''} onClick={() => setTab('packages')}>
          离线包与差异
        </button>
      </nav>

      <main class="content">
        <Show when={isTauri} fallback={
          <div class="placeholder">
            <h2>需要在桌面端运行</h2>
            <p>本程序通过 Tauri 访问本地 SQLite 工程与音频文件。请运行：</p>
            <pre>npm --prefix ui install &amp;&amp; cargo tauri dev</pre>
          </div>
        }>
          <Show when={project()} fallback={
            <div class="placeholder">
              <h2>开始</h2>
              <p>选择一个目录打开已有工程；若目录为空，会自动创建新的 SQLite 工程（studio.db + media/）。</p>
            </div>
          }>
            {tab() === 'review' && <ReviewView />}
            {tab() === 'dict' && <DictionaryView />}
            {tab() === 'queue' && <QueueView />}
            {tab() === 'packages' && <PackagesView />}
          </Show>
        </Show>
      </main>

      <div class={`toast ${toastClass()}`}>{toastText()}</div>
    </div>
  );
}

function toastClass() {
  return toast()?.kind === 'err' ? 'err' : 'ok';
}
function toastText() {
  return toast()?.text ?? '';
}
