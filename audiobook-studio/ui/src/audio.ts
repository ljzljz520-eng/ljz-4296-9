import { invoke } from './tauri';

/**
 * Web Audio 播放引擎：
 * - 通过 Tauri 命令读取媒体字节，decodeAudioData 解码任意浏览器支持的格式；
 * - 峰值由 WAV 后端预计算（recording.peaks），缺失时从解码数据现算；
 * - 精确 seek / 区间循环播放，方便审听反复确认问题片段。
 */
export class AudioEngine {
  private ctx: AudioContext | null = null;
  private buffer: AudioBuffer | null = null;
  private source: AudioBufferSourceNode | null = null;
  private gain: GainNode | null = null;
  private startedAt = 0;
  private offset = 0;
  private playing = false;
  private loop: [number, number] | null = null;
  private raf = 0;
  onTime: ((t: number, duration: number) => void) | null = null;
  peaks: number[] = [];

  private ensureCtx() {
    if (!this.ctx) {
      this.ctx = new AudioContext();
      this.gain = this.ctx.createGain();
      this.gain.gain.value = 1;
      this.gain.connect(this.ctx.destination);
    }
    return this.ctx;
  }

  async loadRecording(recordingId: string, fallbackPeaks: number[]) {
    this.stop();
    const ctx = this.ensureCtx();
    const payload = await invoke<{ bytes: number[]; fileName: string }>('read_media', {
      recordingId,
    });
    const u8 = new Uint8Array(payload.bytes);
    this.buffer = await ctx.decodeAudioData(u8.buffer.slice(0) as ArrayBuffer);
    this.peaks = fallbackPeaks.length > 32 ? fallbackPeaks : this.computePeaks(1200);
    this.offset = 0;
    this.emit();
  }

  /** 从 PCM 通道数据计算归一化峰值，桶数按可视宽度估算。 */
  private computePeaks(buckets: number): number[] {
    if (!this.buffer) return [];
    const data = this.buffer.getChannelData(0);
    const size = Math.max(1, Math.floor(data.length / buckets));
    const out: number[] = new Array(buckets).fill(0);
    for (let i = 0; i < data.length; i++) {
      const b = Math.min(buckets - 1, Math.floor(i / size));
      const v = Math.abs(data[i]);
      if (v > out[b]) out[b] = v;
    }
    return out;
  }

  get duration() {
    return this.buffer?.duration ?? 0;
  }

  private emit() {
    cancelAnimationFrame(this.raf);
    const tick = () => {
      this.onTime?.(this.currentTime(), this.duration);
      if (this.playing) this.raf = requestAnimationFrame(tick);
    };
    tick();
  }

  currentTime(): number {
    if (!this.playing || !this.ctx) return this.offset;
    return this.offset + (this.ctx.currentTime - this.startedAt);
  }

  play(from?: number, loopRange?: [number, number]) {
    if (!this.buffer || !this.ctx || !this.gain) return;
    this.stopSource();
    const src = this.ctx.createBufferSource();
    src.buffer = this.buffer;
    src.connect(this.gain);
    if (loopRange) {
      src.loop = true;
      src.loopStart = loopRange[0];
      src.loopEnd = loopRange[1];
      this.loop = loopRange;
    } else {
      this.loop = null;
    }
    const start = from ?? this.currentTime();
    src.onended = () => {
      if (this.source === src && !this.loop) {
        this.playing = false;
        this.offset = 0;
        this.onTime?.(0, this.duration);
      }
    };
    src.start(0, Math.min(start, this.duration - 0.001));
    this.source = src;
    this.startedAt = this.ctx.currentTime;
    this.offset = start;
    this.playing = true;
    this.emit();
  }

  pause() {
    if (!this.playing) return;
    this.offset = this.currentTime();
    this.loop = null;
    this.stopSource();
    this.playing = false;
    this.onTime?.(this.offset, this.duration);
  }

  seek(t: number) {
    const wasPlaying = this.playing;
    this.pause();
    this.offset = Math.max(0, Math.min(t, this.duration));
    this.onTime?.(this.offset, this.duration);
    if (wasPlaying) this.play();
  }

  setVolume(v: number) {
    if (this.gain) this.gain.gain.value = v;
  }

  private stopSource() {
    if (this.source) {
      try {
        this.source.onended = null;
        this.source.stop();
      } catch {
        /* 已停止 */
      }
      this.source.disconnect();
      this.source = null;
    }
  }

  stop() {
    this.stopSource();
    this.playing = false;
    this.offset = 0;
    this.loop = null;
    cancelAnimationFrame(this.raf);
  }

  isPlaying() {
    return this.playing;
  }
}

/** mm:ss.d */
export function fmtTime(t: number): string {
  if (!isFinite(t)) t = 0;
  const m = Math.floor(t / 60);
  const s = t - m * 60;
  return `${m}:${s.toFixed(1).padStart(4, '0')}`;
}
