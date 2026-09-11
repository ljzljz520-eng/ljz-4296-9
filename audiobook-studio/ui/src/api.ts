// 与 ab-core/models.rs 对应的类型（camelCase）。

export interface ProjectInfo {
  name: string;
  revision: number;
  createdAt: string;
  updatedAt: string;
}

export interface Entry {
  id: string;
  headword: string;
  scope: string;
  kind: string;
  tags: string;
  currentVersionId: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface EntryVersion {
  id: string;
  entryId: string;
  versionNo: number;
  ipa: string;
  syllabification: string;
  exampleAudio: string | null;
  source: string;
  status: 'proposed' | 'approved' | 'rejected';
  note: string;
  createdAt: string;
  approvedAt: string | null;
}

export interface EntryException {
  id: string;
  entryId: string;
  scopeKind: 'role' | 'chapter';
  scopeRef: string;
  ipa: string;
  syllabification: string;
  note: string;
  createdAt: string;
}

export interface Chapter {
  id: string;
  chapterNo: number;
  title: string;
}

export interface Recording {
  id: string;
  chapterId: string;
  itemNo: string;
  filePath: string;
  relPath: string | null;
  sha256: string;
  sizeBytes: number;
  duration: number;
  peaks: number[];
  importedAt: string;
  mtime: number;
}

export interface RecordingOccurrence {
  id: string;
  recordingId: string;
  entryId: string;
  roleRef: string | null;
  timeStart: number;
  timeEnd: number;
}

export type AnnotationKind = 'mispron' | 'stress' | 'noise' | 'other';

export interface Annotation {
  id: string;
  recordingId: string;
  kind: AnnotationKind;
  timeStart: number;
  timeEnd: number;
  comment: string;
  entryVersionId: string | null;
  expectedIpa: string | null;
  reviewer: string;
  createdAt: string;
  resolved: number;
}

export interface ReviewRange {
  id: string;
  recordingId: string;
  entryId: string | null;
  timeStart: number;
  timeEnd: number;
  reason: string;
  detail: string;
  fromVersionId: string | null;
  toVersionId: string | null;
  status: 'pending' | 'ok' | 'reannotated' | 'rerecord';
  createdAt: string;
  resolvedBy: string | null;
  resolution: string | null;
}

export interface VerifyResult {
  recordingId: string;
  changed: boolean;
  oldSha: string;
  newSha: string | null;
  exists: boolean;
  oldDuration: number;
  newDuration: number | null;
  oobAnnotations: Annotation[];
  note: string;
}

export interface NewEntry {
  headword: string;
  scope?: string;
  kind?: string;
  tags?: string;
  ipa: string;
  syllabification?: string;
  source?: string;
  approved?: boolean;
  note?: string;
}

export interface NewVersion {
  ipa: string;
  syllabification?: string;
  source?: string;
  note?: string;
  approved?: boolean;
}

export interface NewException {
  scopeKind: 'role' | 'chapter';
  scopeRef: string;
  ipa: string;
  syllabification?: string;
  note?: string;
}

export interface NewRecording {
  chapterNo: number;
  chapterTitle?: string;
  itemNo: string;
  srcPath: string;
  duration?: number;
  copyIntoProject?: boolean;
}

export interface NewAnnotation {
  recordingId: string;
  kind: AnnotationKind;
  timeStart: number;
  timeEnd: number;
  comment: string;
  entryVersionId?: string | null;
  reviewer?: string;
}

export interface MergeReport {
  packagesApplied: string[];
  recordingsAdded: number;
  annotationsAdded: number;
  annotationsConflicted: number;
  versionsAdded: number;
  exceptionsAdded: number;
  reviewRangesAdded: number;
  skipped: string[];
  conflicts: { kind: string; id: string; detail: string }[];
}

export interface PackageOptions {
  includeMedia: boolean;
  reviewerFilter?: string | null;
}

export interface DiffItem {
  key: string;
  title: string;
  detail: string;
}

export interface DiffReport {
  baseRevision: number | null;
  targetRevision: number | null;
  entriesAdded: DiffItem[];
  entriesChanged: DiffItem[];
  recordingsAdded: DiffItem[];
  recordingsReplaced: DiffItem[];
  annotationsAdded: DiffItem[];
  reviewResolved: DiffItem[];
  summary: string;
}
