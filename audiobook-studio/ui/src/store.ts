import { createSignal } from 'solid-js';
import { invoke } from './tauri';
import type {
  ProjectInfo, Entry, EntryVersion, EntryException, Chapter, Recording,
  RecordingOccurrence, Annotation, ReviewRange,
} from './api';

/** 全局响应式状态。所有变更后调用 refresh* 从 SQLite 重新读取。 */
export const [project, setProject] = createSignal<ProjectInfo | null>(null);
export const [entries, setEntries] = createSignal<Entry[]>([]);
export const [selectedEntryId, setSelectedEntryId] = createSignal<string | null>(null);
export const [versions, setVersions] = createSignal<EntryVersion[]>([]);
export const [allVersions, setAllVersions] = createSignal<EntryVersion[]>([]);
export const [exceptions, setExceptions] = createSignal<EntryException[]>([]);

export const [chapters, setChapters] = createSignal<Chapter[]>([]);
export const [recordings, setRecordings] = createSignal<Recording[]>([]);
export const [selectedRecordingId, setSelectedRecordingId] = createSignal<string | null>(null);
export const [occurrences, setOccurrences] = createSignal<RecordingOccurrence[]>([]);
export const [annotations, setAnnotations] = createSignal<Annotation[]>([]);
export const [reviews, setReviews] = createSignal<ReviewRange[]>([]);
export const [pendingReviews, setPendingReviews] = createSignal<ReviewRange[]>([]);

export const [reviewerName, setReviewerName] = createSignal(
  localStorage.getItem('ab-reviewer') || '审听甲'
);
export function persistReviewer(name: string) {
  setReviewerName(name);
  localStorage.setItem('ab-reviewer', name);
}

export const [toast, setToast] = createSignal<{ kind: 'ok' | 'err'; text: string } | null>(null);
let toastTimer: ReturnType<typeof setTimeout> | undefined;
export function notify(kind: 'ok' | 'err', text: string) {
  setToast({ kind, text });
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => setToast(null), 4200);
}

export async function runAction<T>(label: string, fn: () => Promise<T>): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    notify('err', `${label}失败：${String(e)}`);
    return undefined;
  }
}

export async function openProject(dir: string) {
  const info = await invoke<ProjectInfo>('open_project', { dir });
  setProject(info);
  await refreshAll();
}

export async function refreshAll() {
  const [es, ch, rs, pr, allV] = await Promise.all([
    invoke<Entry[]>('list_entries'),
    invoke<Chapter[]>('list_chapters'),
    invoke<Recording[]>('list_recordings'),
    invoke<ReviewRange[]>('list_pending_reviews'),
    invoke<EntryVersion[]>('list_all_versions'),
  ]);
  setEntries(es);
  setChapters(ch);
  setRecordings(rs);
  setPendingReviews(pr);
  setAllVersions(allV);
  if (!selectedEntryId() && es[0]) setSelectedEntryId(es[0].id);
  if (!selectedRecordingId() && rs[0]) setSelectedRecordingId(rs[0].id);
  await Promise.all([refreshEntry(), refreshRecording()]);
}

export async function refreshEntry() {
  const id = selectedEntryId();
  if (!id) {
    setVersions([]);
    setExceptions([]);
    return;
  }
  const [vs, xs] = await Promise.all([
    invoke<EntryVersion[]>('list_versions', { entryId: id }),
    invoke<EntryException[]>('list_exceptions', { entryId: id }),
  ]);
  setVersions(vs);
  setExceptions(xs);
  // 当前版本指针可能变化
  setProject(await invoke<ProjectInfo>('current_project').then((p) => p).catch(() => project()));
  const es = await invoke<Entry[]>('list_entries');
  setEntries(es);
}

export async function refreshRecording() {
  const id = selectedRecordingId();
  if (!id) {
    setOccurrences([]);
    setAnnotations([]);
    setReviews([]);
    return;
  }
  const [os, as, rs] = await Promise.all([
    invoke<RecordingOccurrence[]>('list_occurrences', { recordingId: id }),
    invoke<Annotation[]>('list_annotations', { recordingId: id }),
    invoke<ReviewRange[]>('list_recording_reviews', { recordingId: id }),
  ]);
  setOccurrences(os);
  setAnnotations(as);
  setReviews(rs);
  setPendingReviews(await invoke<ReviewRange[]>('list_pending_reviews'));
  setRecordings(await invoke<Recording[]>('list_recordings'));
  setAllVersions(await invoke<EntryVersion[]>('list_all_versions'));
  setProject(await invoke<ProjectInfo | null>('current_project'));
}

export function currentRecording(): Recording | undefined {
  return recordings().find((r) => r.id === selectedRecordingId());
}
export function currentEntry(): Entry | undefined {
  return entries().find((e) => e.id === selectedEntryId());
}
export function currentApprovedVersion(): EntryVersion | undefined {
  const e = currentEntry();
  return versions().find((v) => v.id === e?.currentVersionId);
}
