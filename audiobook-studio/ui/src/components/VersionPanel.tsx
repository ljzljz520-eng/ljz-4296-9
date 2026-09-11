import { createSignal, For, Show } from 'solid-js';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { invoke } from '../tauri';
import {
  versions, selectedEntryId, refreshEntry, runAction, notify, currentApprovedVersion,
} from '../store';

export function VersionPanel() {
  const [ipa, setIpa] = createSignal('');
  const [syl, setSyl] = createSignal('');
  const [source, setSource] = createSignal('');
  const [note, setNote] = createSignal('');
  const [example, setExample] = createSignal<string | null>(null);
  const [approved, setApproved] = createSignal(true);

  const pickExample = async () => {
    const f = await openDialog({ multiple: false, filters: [{ name: '音频', extensions: ['wav', 'mp3', 'm4a', 'ogg', 'flac'] }] });
    if (typeof f === 'string') setExample(f);
  };

  const add = async () => {
    const id = selectedEntryId();
    if (!id || !ipa().trim()) return;
    await runAction('追加版本', async () => {
      const [v, ranges] = await invoke<[{ id: string }, number]>('add_version', {
        entryId: id,
        input: { ipa: ipa().trim(), syllabification: syl(), source: source(), note: note(), approved: approved(), exampleAudio: example() },
      });
      if (approved()) {
        notify('ok', `已批准新版本 v${v.id ? '' : ''}，生成 ${ranges} 条待复核范围（旧录音不会被直接判错）`);
      } else {
        notify('ok', '已保存为待定版本，批准前不影响录音与当前读音');
      }
      setIpa('');
      setSyl('');
      setNote('');
      await refreshEntry();
    });
  };

  const approve = async (id: string) => {
    await runAction('批准版本', async () => {
      const n = await invoke<number>('approve_version', { versionId: id });
      notify('ok', `版本已批准，生成 ${n} 条待复核范围`);
      await refreshEntry();
    });
  };
  const reject = async (id: string) => {
    await runAction('否决版本', async () => invoke('reject_version', { versionId: id }));
    await refreshEntry();
  };

  return (
    <div class="versions">
      <h4>版本历史（审听批注锁定具体版本号）</h4>
      <table class="data">
        <thead>
          <tr><th>版本</th><th>音标/拼读</th><th>来源</th><th>状态</th><th>备注</th><th></th></tr>
        </thead>
        <tbody>
          <For each={versions()}>
            {(v) => (
              <tr>
                <td>v{v.versionNo}</td>
                <td><b>{v.ipa}</b><div class="muted small">{v.syllabification}</div></td>
                <td class="small">{v.source}</td>
                <td>
                  <span class={`status s-${v.status}`}>
                    {v.status === 'approved' ? '已批准' : v.status === 'proposed' ? '待定' : '已否决'}
                  </span>
                </td>
                <td class="small muted">{v.note}</td>
                <td>
                  <Show when={v.status === 'proposed'}>
                    <button class="link" onClick={() => void approve(v.id)}>批准</button>
                    <button class="link danger" onClick={() => void reject(v.id)}>否决</button>
                  </Show>
                </td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
      <div class="version-form">
        <input placeholder="新音标/拼读 *" value={ipa()} onInput={(e) => setIpa(e.currentTarget.value)} />
        <input placeholder="音节拆分" value={syl()} onInput={(e) => setSyl(e.currentTarget.value)} />
        <input placeholder="来源" value={source()} onInput={(e) => setSource(e.currentTarget.value)} />
        <div class="row">
          <button type="button" onClick={() => void pickExample()}>示例音频</button>
          <span class="muted small">{example() ?? '未选择'}</span>
        </div>
        <input placeholder="备注" value={note()} onInput={(e) => setNote(e.currentTarget.value)} />
        <label class="check">
          <input type="checkbox" checked={approved()} onChange={(e) => setApproved(e.currentTarget.checked)} />
          批准（才会生效并生成待复核）
        </label>
        <button class="primary" onClick={() => void add()}>追加新版本</button>
        <Show when={currentApprovedVersion()}>
          <span class="muted small">提示：示例音频路径 {currentApprovedVersion()!.exampleAudio ?? '（未附）'}</span>
        </Show>
      </div>
    </div>
  );
}
