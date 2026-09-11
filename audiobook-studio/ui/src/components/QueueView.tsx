import { For, Show } from 'solid-js';
import { invoke } from '../tauri';
import {
  pendingReviews, recordings, entries, reviewerName, refreshAll, runAction, notify,
  setSelectedRecordingId,
} from '../store';
import { fmtTime } from '../audio';

/// 待复核队列：读音变化产生的范围在这里处置，不直接当作旧录音错误。
export default function QueueView() {
  const recTitle = (id: string) => {
    const r = recordings().find((x) => x.id === id);
    return r ? `片段 ${r.itemNo}` : id.slice(0, 6);
  };
  const headword = (id: string | null) =>
    entries().find((e) => e.id === id)?.headword ?? '';

  const resolve = async (id: string, resolution: 'ok' | 'reannotated' | 'rerecord') => {
    await runAction('复核处置', async () => {
      await invoke('resolve_review', { id, resolution, reviewer: reviewerName() });
      notify('ok',
        resolution === 'ok' ? '已确认旧录音可接受'
        : resolution === 'reannotated' ? '已记录为转批注'
        : '已标记需要重录');
      await refreshAll();
    });
  };

  const gotoRecording = async (recId: string) => {
    setSelectedRecordingId(recId);
    notify('ok', '已切换到该片段（到“波形审听”页查看）');
  };

  return (
    <div class="panel queue">
      <h3>待复核范围（{pendingReviews().length}）</h3>
      <p class="muted">
        读音修订后，系统不会自动判旧录音错误，而是把受影响的时间段排到这里，由复核者听辨后处置。
      </p>
      <table class="data">
        <thead>
          <tr>
            <th>片段</th><th>词条</th><th>时间段</th><th>原因</th><th>说明</th><th>生成时间</th><th>操作</th>
          </tr>
        </thead>
        <tbody>
          <For each={pendingReviews()}>
            {(r) => (
              <tr>
                <td>
                  <button class="link" onClick={() => void gotoRecording(r.recordingId)}>
                    {recTitle(r.recordingId)}
                  </button>
                </td>
                <td>{headword(r.entryId)}</td>
                <td class="mono">{fmtTime(r.timeStart)}–{fmtTime(r.timeEnd)}</td>
                <td>
                  <span class={`reason ${r.reason}`}>
                    {r.reason === 'pron_changed' ? '读音变化' : '外来批注'}
                  </span>
                </td>
                <td class="small">{r.detail}</td>
                <td class="small muted">{r.createdAt}</td>
                <td class="actions">
                  <button class="ok" onClick={() => void resolve(r.id, 'ok')}>旧录音可用</button>
                  <button onClick={() => void resolve(r.id, 'reannotated')}>转批注</button>
                  <button class="danger-btn" onClick={() => void resolve(r.id, 'rerecord')}>需重录</button>
                </td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
      <Show when={pendingReviews().length === 0}>
        <div class="placeholder">没有待复核范围。修订读音或合并别人的离线包后会出现。</div>
      </Show>
    </div>
  );
}
