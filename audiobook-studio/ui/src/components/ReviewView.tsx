import { createMemo, createSignal, For, onCleanup, Show } from 'solid-js';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { invoke } from '../tauri';
import { AudioEngine, fmtTime } from '../audio';
import Waveform, { KIND_COLOR } from './Waveform';
import {
  recordings, selectedRecordingId, setSelectedRecordingId, annotations, reviews,
  chapters, entries, occurrences, reviewerName, allVersions,
  refreshAll, refreshRecording, runAction, notify,
} from '../store';
import type { Annotation, AnnotationKind, Recording, VerifyResult } from '../api';

const engine = new AudioEngine();
let rafBind = false;

export default function ReviewView() {
  const [time, setTime] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [volume, setVolume] = createSignal(1);
  const [selRange, setSelRange] = createSignal<[number, number] | null>(null);
  const [loopSel, setLoopSel] = createSignal(false);
  const [annKind, setAnnKind] = createSignal<AnnotationKind>('mispron');
  const [comment, setComment] = createSignal('');
  const [entryId, setEntryId] = createSignal('');
  const [verify, setVerify] = createSignal<VerifyResult | null>(null);
  const [importBusy, setImportBusy] = createSignal(false);

  // 导入表单
  const [chNo, setChNo] = createSignal('1');
  const [chTitle, setChTitle] = createSignal('');
  const [itemNo, setItemNo] = createSignal('');
  const [srcPath, setSrcPath] = createSignal('');

  const recording = createMemo(() => recordings().find((r) => r.id === selectedRecordingId()));

  engine.onTime = (t, d) => {
    setTime(t);
    if (t >= d - 0.03 && !engine.isPlaying()) setPlaying(false);
  };

  const bindRaf = () => {
    if (rafBind) return;
    rafBind = true;
    const tick = () => {
      if (engine.isPlaying()) setTime(engine.currentTime());
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  };

  const load = async (r: Recording) => {
    setSelectedRecordingId(r.id);
    await refreshRecording();
    await runAction('加载音频', async () => {
      await engine.loadRecording(r.id, r.peaks);
      setTime(0);
    });
  };

  const togglePlay = () => {
    bindRaf();
    if (engine.isPlaying()) {
      engine.pause();
      setPlaying(false);
    } else {
      const r = selRange();
      if (r && loopSel()) engine.play(time() >= r[1] || time() < r[0] ? r[0] : time(), r);
      else if (r && (time() < r[0] || time() > r[1])) engine.play(r[0]);
      else engine.play();
      setPlaying(true);
    }
  };

  const stop = () => {
    engine.stop();
    setPlaying(false);
    setTime(0);
  };

  const addAnnotation = async () => {
    const r = selRange();
    const rec = recording();
    if (!r || !rec) {
      notify('err', '请先在波形上拖出一段时间');
      return;
    }
    const vid = entryId() ? (entries().find((x) => x.id === entryId())?.currentVersionId ?? null) : null;
    await runAction('添加批注', async () => {
      const a = await invoke<Annotation>('add_annotation', {
        input: {
          recordingId: rec.id,
          kind: annKind(),
          timeStart: round2(r[0]),
          timeEnd: round2(r[1]),
          comment: comment(),
          entryVersionId: vid,
          reviewer: reviewerName(),
        },
      });
      const vno = allVersions().find((x) => x.id === vid)?.versionNo ?? '?';
      notify('ok', `已记录 ${kindText(a.kind)} ${fmtTime(r[0])}–${fmtTime(r[1])}，锁定词典版本 v${vno}`);
      setComment('');
      setSelRange(null);
      await refreshRecording();
    });
  };

  const addOccurrence = async () => {
    const r = selRange();
    const rec = recording();
    const e = entries().find((x) => x.id === entryId());
    if (!r || !rec || !e) {
      notify('err', '请选择时间段并指定词条');
      return;
    }
    const ch = chapters().find((c) => c.id === rec.chapterId);
    await runAction('登记出现位置', async () => {
      await invoke('add_occurrence', {
        input: {
          recordingId: rec.id,
          headword: e.headword,
          chapter: ch?.chapterNo ?? null,
          role: null,
          timeStart: round2(r[0]),
          timeEnd: round2(r[1]),
        },
      });
      notify('ok', '出现位置已登记，读音变化时会据此生成待复核范围');
      await refreshRecording();
    });
  };

  const verifyFile = async () => {
    const rec = recording();
    if (!rec) return;
    await runAction('校验文件', async () => {
      const v = await invoke<VerifyResult>('verify_recording', { recordingId: rec.id });
      setVerify(v);
      await refreshRecording();
      if (v.exists && !v.changed) notify('ok', '文件未被外部修改');
      else if (v.exists && v.changed)
        notify('err', `文件已被替换！${v.oobAnnotations.length} 条批注越界（不会自动删除）`);
      else notify('err', '文件丢失');
    });
  };

  const pickFile = async () => {
    const f = await openDialog({ multiple: false, filters: [{ name: '音频', extensions: ['wav', 'mp3', 'm4a', 'flac', 'ogg'] }] });
    if (typeof f === 'string') setSrcPath(f);
  };

  const doImport = async () => {
    if (!srcPath() || !itemNo().trim()) {
      notify('err', '请选择文件并填写条次');
      return;
    }
    setImportBusy(true);
    await runAction('导入录音', async () => {
      // 前端先用 Web Audio 探测时长（非 WAV 时后端需要）
      let duration = await invoke<number | null>('probe_audio_duration', { path: srcPath() }) ?? undefined;
      if (duration === undefined) {
        // 非 WAV：用 Web Audio 解码得到时长
        const bytes = await invoke<number[]>('read_file_bytes', { path: srcPath() });
        const tmpCtx = new AudioContext();
        const buf = await tmpCtx.decodeAudioData(new Uint8Array(bytes).buffer.slice(0));
        duration = buf.duration;
        void tmpCtx.close();
      }
      const r = await invoke<Recording>('import_recording', {
        input: {
          chapterNo: Number(chNo()),
          chapterTitle: chTitle(),
          itemNo: itemNo().trim(),
          srcPath: srcPath(),
          duration,
          copyIntoProject: true,
        },
      });
      notify('ok', `已导入第${chNo()}章·条次${r.itemNo}，时长 ${fmtTime(r.duration)}，已复制进 media/`);
      setSrcPath('');
      setItemNo('');
      await refreshAll();
      await load(r);
    });
    setImportBusy(false);
  };

  onCleanup(() => engine.stop());

  const chapterNo = (id: string) => chapters().find((c) => c.id === id)?.chapterNo ?? '?';

  return (
    <div class="review">
      <div class="panel import-bar">
        <div class="row wrap">
          <input placeholder="章号" value={chNo()} onInput={(e) => setChNo(e.currentTarget.value)} style={{width: '60px'}} />
          <input placeholder="章节标题" value={chTitle()} onInput={(e) => setChTitle(e.currentTarget.value)} style={{width: '120px'}} />
          <input placeholder="条次，如 12b" value={itemNo()} onInput={(e) => setItemNo(e.currentTarget.value)} style={{width: '90px'}} />
          <button onClick={() => void pickFile()}>选择音频</button>
          <span class="muted small path">{srcPath() || '未选择'}</span>
          <button class="primary" disabled={importBusy()} onClick={() => void doImport()}>
            导入（复制进工程）
          </button>
        </div>
      </div>

      <div class="split review-split">
        <aside class="panel left rec-list">
          <h3>片段（按章节/条次）</h3>
          <For each={recordings()}>
            {(r) => (
              <div class={`rec ${r.id === selectedRecordingId() ? 'selected' : ''}`} onClick={() => void load(r)}>
                <div>第{chapterNo(r.chapterId)}章 · {r.itemNo}</div>
                <div class="muted small">{fmtTime(r.duration)} · {r.sha256.slice(0, 8)}</div>
              </div>
            )}
          </For>
        </aside>

        <section class="panel right">
          <Show when={recording()} fallback={<div class="placeholder">导入或选择左侧片段开始审听</div>}>
            {(r) => (
              <>
                <div class="rec-head">
                  <h3>第{chapterNo(r().chapterId)}章 · 条次 {r().itemNo}</h3>
                  <span class="muted small mono">{r().sha256}</span>
                  <button onClick={() => void verifyFile()}>校验是否被替换</button>
                  <Show when={verify()}>
                    {(v) => (
                      <span class={`verify ${v().exists && v().changed ? 'bad' : v().exists ? 'good' : 'bad'}`}>
                        {v().note}
                        <Show when={v().oobAnnotations.length > 0}>
                          （{v().oobAnnotations.length} 条批注越界）
                        </Show>
                      </span>
                    )}
                  </Show>
                </div>

                <Waveform
                  peaks={engine.peaks.length ? engine.peaks : r().peaks}
                  duration={r().duration}
                  time={time()}
                  annotations={annotations()}
                  reviews={reviews()}
                  onSeek={(t) => { engine.seek(t); setTime(t); }}
                  onSelectRange={(s, e) => setSelRange([s, e])}
                />

                <div class="transport">
                  <button class="primary" onClick={togglePlay}>{playing() ? '⏸ 暂停' : '▶ 播放'}</button>
                  <button onClick={stop}>⏹ 停止</button>
                  <label class="check">
                    <input type="checkbox" checked={loopSel()} onChange={(e) => setLoopSel(e.currentTarget.checked)} />
                    循环选区
                  </label>
                  <span class="time"><b>{fmtTime(time())}</b> / {fmtTime(r().duration)}</span>
                  <input
                    type="range" min="0" max="1" step="0.01" value={volume()}
                    onInput={(e) => { const v = Number(e.currentTarget.value); setVolume(v); engine.setVolume(v); }}
                  />
                </div>

                <Show when={selRange()}>
                  {(rg) => (
                    <div class="sel-bar">
                      选区 <b>{fmtTime(rg()[0])}–{fmtTime(rg()[1])}</b>（{(rg()[1] - rg()[0]).toFixed(2)}s）
                      <button class="link" onClick={() => setSelRange(null)}>清除</button>
                    </div>
                  )}
                </Show>

                <div class="ann-form">
                  <div class="row wrap">
                    <select value={annKind()} onChange={(e) => setAnnKind(e.currentTarget.value as AnnotationKind)}>
                      <option value="mispron">误读</option>
                      <option value="stress">重音</option>
                      <option value="noise">噪声</option>
                      <option value="other">其他</option>
                    </select>
                    <select value={entryId()} onChange={(e) => setEntryId(e.currentTarget.value)}>
                      <option value="">不关联词条</option>
                      <For each={entries()}>
                        {(e) => <option value={e.id}>{e.headword}</option>}
                      </For>
                    </select>
                    <input
                      class="comment"
                      placeholder="批注内容（误读时可记录实际听到的读法）"
                      value={comment()}
                      onInput={(e) => setComment(e.currentTarget.value)}
                    />
                    <button class="primary" onClick={() => void addAnnotation()}>落批注</button>
                    <button onClick={() => void addOccurrence()}>登记出现位置</button>
                  </div>
                </div>

                <div class="markup-lists">
                  <div class="ann-list">
                    <h4>批注（{annotations().length}）</h4>
                    <For each={annotations()}>
                      {(a) => <AnnotationRow a={a} onGoto={(t) => { engine.seek(t); setTime(t); }} />}
                    </For>
                  </div>
                  <div class="occ-list">
                    <h4>出现位置（{occurrences().length}）</h4>
                    <For each={occurrences()}>
                      {(o) => {
                        const e = entries().find((x) => x.id === o.entryId);
                        return (
                          <div class="occ" onClick={() => { engine.seek(o.timeStart); setTime(o.timeStart); }}>
                            <span class="dot" />
                            {e?.headword ?? o.entryId.slice(0, 6)}
                            {o.roleRef ? `（${o.roleRef}）` : ''}
                            <span class="muted">{fmtTime(o.timeStart)}–{fmtTime(o.timeEnd)}</span>
                          </div>
                        );
                      }}
                    </For>
                  </div>
                </div>
              </>
            )}
          </Show>
        </section>
      </div>
    </div>
  );
}

function AnnotationRow(props: { a: Annotation; onGoto: (t: number) => void }) {
  return (
    <div class={`ann ${props.a.resolved ? 'resolved' : ''}`} onClick={() => props.onGoto(props.a.timeStart)}>
      <span class="tag" style={{ background: KIND_COLOR[props.a.kind] }}>{kindText(props.a.kind)}</span>
      <b>{fmtTime(props.a.timeStart)}–{fmtTime(props.a.timeEnd)}</b>
      <span class="who">{props.a.reviewer}</span>
      <span class="comment-text">{props.a.comment}</span>
      <Show when={props.a.expectedIpa}>
        <span class="ipa">应读 {props.a.expectedIpa}</span>
      </Show>
      <Show when={versionOf(props.a.entryVersionId)}>
        {(v) => <span class="muted small">@v{v().versionNo}（{headwordOf(v().entryId)}）</span>}
      </Show>
      <Show when={!props.a.resolved}>
        <button class="link" onClick={async (ev) => {
          ev.stopPropagation();
          await invoke('resolve_annotation', { id: props.a.id });
          await refreshRecording();
        }}>标为已处理</button>
      </Show>
    </div>
  );
}

function versionOf(vid: string | null) {
  if (!vid) return undefined;
  return allVersions().find((v) => v.id === vid);
}
function headwordOf(entryId: string) {
  return entries().find((e) => e.id === entryId)?.headword ?? '';
}

function kindText(k: string) {
  return { mispron: '误读', stress: '重音', noise: '噪声', other: '其他' }[k] ?? k;
}
function round2(n: number) {
  return Math.round(n * 100) / 100;
}
