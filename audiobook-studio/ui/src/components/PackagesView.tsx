import { createSignal, Show, For } from 'solid-js';
import { save, open as openDialog } from '@tauri-apps/plugin-dialog';
import { invoke } from '../tauri';
import {
  reviewerName, refreshAll, runAction, notify, project, pendingReviews,
} from '../store';
import type { MergeReport, DiffReport } from '../api';

export default function PackagesView() {
  const [includeMedia, setIncludeMedia] = createSignal(true);
  const [onlyMine, setOnlyMine] = createSignal(false);
  const [report, setReport] = createSignal<MergeReport | null>(null);
  const [diff, setDiff] = createSignal<DiffReport | null>(null);
  const [diffText, setDiffText] = createSignal('');

  const doExport = async () => {
    const out = await save({
      defaultPath: `audiobook-${project()?.name ?? 'project'}-r${project()?.revision ?? 0}.abpkg`,
      filters: [{ name: '离线包', extensions: ['abpkg', 'zip'] }],
    });
    if (typeof out !== 'string') return;
    await runAction('导出离线包', async () => {
      const p = await invoke<string>('export_package', {
        outPath: out,
        options: { includeMedia: includeMedia(), reviewerFilter: onlyMine() ? reviewerName() : null },
        exportedBy: reviewerName(),
      });
      notify('ok', `已导出 ${p}（${includeMedia() ? '含媒体' : '仅数据'}${onlyMine() ? '，仅本人批注' : ''}）`);
    });
  };

  const doImport = async () => {
    const f = await openDialog({ multiple: false, filters: [{ name: '离线包', extensions: ['abpkg', 'zip'] }] });
    if (typeof f !== 'string') return;
    await runAction('合并离线包', async () => {
      const rep = await invoke<MergeReport>('import_package', { path: f });
      setReport(rep);
      await refreshAll();
      notify(rep.conflicts.length ? 'err' : 'ok',
        `合并完成：+${rep.annotationsAdded} 批注、+${rep.recordingsAdded} 录音、+${rep.reviewRangesAdded} 待复核、${rep.conflicts.length} 冲突`);
    });
  };

  const diffWithFile = async () => {
    const f = await openDialog({ multiple: false, filters: [{ name: '快照/离线包', extensions: ['abpkg', 'zip', 'json'] }] });
    if (typeof f !== 'string') return;
    await runAction('生成差异', async () => {
      const d = await invoke<DiffReport>('diff_against_file', { path: f });
      setDiff(d);
      setDiffText('');
    });
  };

  const diffTwoFiles = async () => {
    const a = await openDialog({ multiple: false, filters: [{ name: '基线快照', extensions: ['abpkg', 'zip', 'json'] }] });
    if (typeof a !== 'string') return;
    const b = await openDialog({ multiple: false, filters: [{ name: '目标快照', extensions: ['abpkg', 'zip', 'json'] }] });
    if (typeof b !== 'string') return;
    await runAction('比较两份快照', async () => {
      const text = await invoke<string>('diff_files', { basePath: a, targetPath: b });
      setDiffText(text);
      setDiff(null);
    });
  };

  return (
    <div class="packages">
      <section class="panel">
        <h3>离线打包（审听者可完全离线工作）</h3>
        <p class="muted">
          包内包含词典与版本、录音摘要、出现位置、批注（可只带本人）及可选媒体文件。
          对方在“打开工程”后导入 .abpkg 即可合并；合并幂等，可安全重复导入。
        </p>
        <div class="row wrap">
          <label class="check">
            <input type="checkbox" checked={includeMedia()} onChange={(e) => setIncludeMedia(e.currentTarget.checked)} />
            包含媒体文件
          </label>
          <label class="check">
            <input type="checkbox" checked={onlyMine()} onChange={(e) => setOnlyMine(e.currentTarget.checked)} />
            仅导出「{reviewerName()}」的批注
          </label>
          <button class="primary" onClick={() => void doExport()}>导出 .abpkg</button>
          <button onClick={() => void doImport()}>导入并合并 .abpkg</button>
        </div>
        <Show when={report()}>
          {(r) => (
            <div class="merge-report">
              <h4>合并报告</h4>
              <ul>
                <li>来源：{r().packagesApplied.join('，')}</li>
                <li>新增录音 {r().recordingsAdded}、版本 {r().versionsAdded}、例外 {r().exceptionsAdded}</li>
                <li>新增批注 {r().annotationsAdded}，冲突 {r().annotationsConflicted}，待复核 +{r().reviewRangesAdded}</li>
              </ul>
              <Show when={r().conflicts.length > 0}>
                <div class="conflicts">
                  <b>冲突（已保留本地版本，未覆盖）：</b>
                  <For each={r().conflicts}>{(c) => (
                    <div class="conflict">· [{c.kind}] {c.detail}</div>
                  )}</For>
                </div>
              </Show>
              <Show when={r().skipped.length > 0}>
                <div class="muted small">跳过：{r().skipped.join('；')}</div>
              </Show>
            </div>
          )}
        </Show>
      </section>

      <section class="panel">
        <h3>差异报告</h3>
        <div class="row">
          <button onClick={() => void diffWithFile()}>当前工程 对 某个包/快照</button>
          <button onClick={() => void diffTwoFiles()}>比较两份包/快照（纯文本）</button>
        </div>

        <Show when={diff()}>
          {(d) => (
            <div class="diff-report">
              <p class="summary">{d().summary}</p>
              <DiffGroup title="新增词条" items={d().entriesAdded} />
              <DiffGroup title="读音/例外变化" items={d().entriesChanged} />
              <DiffGroup title="新增录音" items={d().recordingsAdded} />
              <DiffGroup title="被替换录音" items={d().recordingsReplaced} />
              <DiffGroup title="新增批注（按审听者）" items={d().annotationsAdded} />
              <DiffGroup title="复核处置" items={d().reviewResolved} />
            </div>
          )}
        </Show>
        <Show when={diffText()}>
          <pre class="diff-text">{diffText()}</pre>
        </Show>
      </section>
    </div>
  );
}

function DiffGroup(props: { title: string; items: { key: string; title: string; detail: string }[] }) {
  return (
    <div class="diff-group">
      <h4>{props.title}（{props.items.length}）</h4>
      <Show when={props.items.length === 0}><span class="muted small">（无）</span></Show>
      <For each={props.items}>
        {(it) => <div class="diff-item">· <b>{it.title}</b> — {it.detail}</div>}
      </For>
    </div>
  );
}
