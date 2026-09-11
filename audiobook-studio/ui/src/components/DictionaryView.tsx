import { createMemo, createSignal, For, Show } from 'solid-js';
import { invoke } from '../tauri';
import {
  entries, selectedEntryId, setSelectedEntryId, versions, exceptions,
  refreshEntry, refreshAll, runAction, notify,
} from '../store';
import type { Entry, EntryVersion } from '../api';
import { VersionPanel } from './VersionPanel';

// ResolvedPron 不经过全局 store，仅在此组件使用。
type Resolved = {
  entryId: string;
  headword: string;
  versionId: string;
  ipa: string;
  syllabification: string;
  matched: string;
  exceptionId: string | null;
};

export default function DictionaryView() {
  const [filter, setFilter] = createSignal('');
  const filtered = createMemo(() =>
    entries().filter(
      (e) => !filter() || e.headword.includes(filter()) || e.tags.includes(filter()),
    ),
  );

  // 新建词条
  const [nHead, setNHead] = createSignal('');
  const [nIpa, setNIpa] = createSignal('');
  const [nScope, setNScope] = createSignal('全书');
  const [nKind, setNKind] = createSignal('term');
  const [nSource, setNSource] = createSignal('');

  // 试读解析
  const [qChapter, setQChapter] = createSignal('');
  const [qRole, setQRole] = createSignal('');
  const [resolved, setResolved] = createSignal<Resolved | null>(null);

  const create = async () => {
    if (!nHead().trim() || !nIpa().trim()) return;
    await runAction('新建词条', async () => {
      const [e] = await invoke<[Entry, EntryVersion]>('create_entry', {
        input: {
          headword: nHead().trim(),
          ipa: nIpa().trim(),
          scope: nScope(),
          kind: nKind(),
          source: nSource(),
          approved: true,
        },
      });
      notify('ok', `已建词条「${e.headword}」v1`);
      setNHead('');
      setNIpa('');
      await refreshAll();
      setSelectedEntryId(e.id);
    });
  };

  const tryResolve = async () => {
    const e = selected();
    if (!e) return;
    const r = await invoke<Resolved>('resolve_pron', {
      query: {
        headword: e.headword,
        chapter: qChapter() ? Number(qChapter()) : null,
        role: qRole() || null,
      },
    });
    setResolved(r);
  };

  const selected = createMemo(() => entries().find((e) => e.id === selectedEntryId()));
  const current = createMemo(() => {
    const e = selected();
    return versions().find((v) => v.id === e?.currentVersionId);
  });

  return (
    <div class="split">
      <aside class="panel left">
        <div class="panel-head">
          <h3>词条（{entries().length}）</h3>
          <input placeholder="过滤原文/标签" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
        </div>
        <ul class="entry-list">
          <For each={filtered()}>
            {(e) => (
              <li class={e.id === selectedEntryId() ? 'selected' : ''} onClick={() => { setSelectedEntryId(e.id); void refreshEntry(); setResolved(null); }}>
                <span class="hw">{e.headword}</span>
                <span class="muted small">{e.scope}</span>
                <span class={`kind k-${e.kind}`}>{kindText(e.kind)}</span>
              </li>
            )}
          </For>
        </ul>
        <div class="new-entry">
          <h4>新建条目</h4>
          <input placeholder="原文（同形异读请用例外，勿重复建条）" value={nHead()} onInput={(e) => setNHead(e.currentTarget.value)} />
          <input placeholder="音标 / 拼读，如 qiū cí" value={nIpa()} onInput={(e) => setNIpa(e.currentTarget.value)} />
          <div class="row">
            <input placeholder="书目范围" value={nScope()} onInput={(e) => setNScope(e.currentTarget.value)} />
            <select value={nKind()} onChange={(e) => setNKind(e.currentTarget.value)}>
              <option value="term">术语</option>
              <option value="character">角色</option>
              <option value="place">地名</option>
              <option value="other">其他</option>
            </select>
          </div>
          <input placeholder="来源（作者/导演组/词典）" value={nSource()} onInput={(e) => setNSource(e.currentTarget.value)} />
          <button class="primary" onClick={create}>创建并批准 v1</button>
        </div>
      </aside>

      <section class="panel right">
        <Show when={selected()} fallback={<div class="placeholder">从左侧选择词条</div>}>
          {(e) => (
            <>
              <div class="detail-head">
                <h2>{e().headword}</h2>
                <span class="muted">{e().scope} · {kindText(e().kind)}{e().tags ? ` · ${e().tags}` : ''}</span>
                <Show when={current()}>
                  {(v) => (
                    <div class="current-ipa">
                      当前批准读音：<b>{v().ipa}</b>
                      <span class="muted"> v{v().versionNo}</span>
                    </div>
                  )}
                </Show>
              </div>

              <div class="resolve-box">
                <h4>语境试读（角色例外 &gt; 章节例外 &gt; 默认）</h4>
                <div class="row">
                  <input placeholder="章节号（可选）" value={qChapter()} onInput={(e) => setQChapter(e.currentTarget.value)} style={{width: '90px'}} />
                  <input placeholder="角色名（可选）" value={qRole()} onInput={(e) => setQRole(e.currentTarget.value)} />
                  <button onClick={() => void tryResolve()}>解析读音</button>
                </div>
                <Show when={resolved()}>
                  {(r) => (
                    <div class="resolved">
                      <span class={`match m-${r().matched}`}>{matchText(r().matched)}</span>
                      <b>{r().ipa}</b>
                      <span class="muted">{r().syllabification}</span>
                    </div>
                  )}
                </Show>
              </div>

              <VersionPanel />

              <ExceptionPanel />
            </>
          )}
        </Show>
      </section>
    </div>
  );
}

function ExceptionPanel() {
  const [kind, setKind] = createSignal<'role' | 'chapter'>('role');
  const [ref, setRef] = createSignal('');
  const [ipa, setIpa] = createSignal('');
  const [syl, setSyl] = createSignal('');

  const add = async () => {
    const id = selectedEntryId();
    if (!id || !ref().trim() || !ipa().trim()) return;
    await runAction('保存例外', async () => {
      await invoke('upsert_exception', {
        entryId: id,
        input: { scopeKind: kind(), scopeRef: ref().trim(), ipa: ipa(), syllabification: syl() },
      });
      notify('ok', '例外已保存；命中该语境的旧片段已进入待复核');
      setRef('');
      setIpa('');
      setSyl('');
      await refreshEntry();
    });
  };

  const del = async (id: string) => {
    await runAction('删除例外', async () => {
      await invoke('delete_exception', { exceptionId: id });
      await refreshEntry();
    });
  };

  return (
    <div class="exceptions">
      <h4>同形词例外（按角色 / 章节）</h4>
      <table class="data">
        <thead><tr><th>类型</th><th>对象</th><th>例外读音</th><th>拼读</th><th></th></tr></thead>
        <tbody>
          <For each={exceptions()}>
            {(x) => (
              <tr>
                <td>{x.scopeKind === 'role' ? '角色' : '章节'}</td>
                <td>{x.scopeRef}</td>
                <td><b>{x.ipa}</b></td>
                <td class="muted">{x.syllabification}</td>
                <td><button class="link danger" onClick={() => void del(x.id)}>删除</button></td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
      <div class="row exception-form">
        <select value={kind()} onChange={(e) => setKind(e.currentTarget.value as 'role' | 'chapter')}>
          <option value="role">角色</option>
          <option value="chapter">章节</option>
        </select>
        <input placeholder="角色名 / 章节号" value={ref()} onInput={(e) => setRef(e.currentTarget.value)} />
        <input placeholder="例外音标" value={ipa()} onInput={(e) => setIpa(e.currentTarget.value)} />
        <input placeholder="拼读（可选）" value={syl()} onInput={(e) => setSyl(e.currentTarget.value)} />
        <button onClick={() => void add()}>保存例外</button>
      </div>
    </div>
  );
}

export function kindText(k: string) {
  return { term: '术语', character: '角色', place: '地名', other: '其他' }[k] ?? k;
}
export function matchText(m: string) {
  return { default: '默认读音', 'exception:role': '角色例外', 'exception:chapter': '章节例外' }[m] ?? m;
}
