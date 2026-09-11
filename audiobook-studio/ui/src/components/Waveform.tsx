import { createMemo, createSignal, onCleanup, For } from 'solid-js';
import type { Annotation, AnnotationKind, ReviewRange } from '../api';
import { fmtTime } from '../audio';

interface Props {
  peaks: number[];
  duration: number;
  time: number;
  annotations: Annotation[];
  reviews: ReviewRange[];
  pending?: boolean;
  onSeek: (t: number) => void;
  onSelectRange?: (s: number, e: number) => void;
  height?: number;
}

const KIND_COLOR: Record<AnnotationKind, string> = {
  mispron: '#f87171',
  stress: '#fbbf24',
  noise: '#94a3b8',
  other: '#a78bfa',
};

/**
 * Canvas 波形：中轴对称绘制峰值；叠加批注段（误读红/重音黄/噪声灰）、
 * 待复核段（蓝色斜纹描边）与播放头。拖拽选择时间段后回调上层落批注。
 */
export default function Waveform(props: Props) {
  let canvas!: HTMLCanvasElement;
  const [drag, setDrag] = createSignal<[number, number] | null>(null);
  const [hoverT, setHoverT] = createSignal<number | null>(null);

  const W = 1000;
  const H = createMemo(() => props.height ?? 160);

  function xToTime(clientX: number): number {
    const rect = canvas.getBoundingClientRect();
    const ratio = (clientX - rect.left) / rect.width;
    return Math.max(0, Math.min(1, ratio)) * props.duration;
  }

  const draw = () => {
    const c = canvas;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = W * dpr;
    c.height = H() * dpr;
    c.style.width = '100%';
    c.style.height = `${H()}px`;
    const ctx = c.getContext('2d')!;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, W, H());

    // 网格
    ctx.strokeStyle = '#1e293b';
    ctx.fillStyle = '#64748b';
    ctx.font = '10px sans-serif';
    const step = chooseStep(props.duration);
    for (let t = 0; t <= props.duration + 0.001; t += step) {
      const x = (t / props.duration) * W;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, H());
      ctx.stroke();
      ctx.fillText(fmtTime(t), x + 2, H() - 4);
    }

    // 待复核范围（蓝色描边底）
    for (const r of props.reviews) {
      const x1 = (r.timeStart / props.duration) * W;
      const x2 = (r.timeEnd / props.duration) * W;
      ctx.fillStyle = 'rgba(56,189,248,0.14)';
      ctx.fillRect(x1, 0, x2 - x1, H());
      ctx.strokeStyle = '#38bdf8';
      ctx.setLineDash([4, 3]);
      ctx.strokeRect(x1, 1, x2 - x1, H() - 2);
      ctx.setLineDash([]);
    }

    // 波形
    const peaks = props.peaks;
    const mid = H() / 2;
    ctx.fillStyle = '#34d399';
    if (peaks.length > 0) {
      const bw = W / peaks.length;
      for (let i = 0; i < peaks.length; i++) {
        const h = Math.max(1, peaks[i] * (mid - 6));
        ctx.fillRect(i * bw, mid - h, Math.max(1, bw - 0.4), h * 2);
      }
    } else {
      ctx.fillStyle = '#475569';
      ctx.fillRect(0, mid - 1, W, 2);
    }

    // 批注
    for (const a of props.annotations) {
      const x1 = (a.timeStart / props.duration) * W;
      const x2 = (a.timeEnd / props.duration) * W;
      ctx.fillStyle = KIND_COLOR[a.kind] + '55';
      ctx.fillRect(x1, 0, x2 - x1, H());
      ctx.fillStyle = KIND_COLOR[a.kind];
      ctx.fillRect(x1, 0, 2, H());
      ctx.fillRect(x2 - 1, 0, 2, H());
      ctx.save();
      ctx.fillStyle = KIND_COLOR[a.kind];
      ctx.font = 'bold 10px sans-serif';
      const label = labelOf(a);
      ctx.fillText(label.slice(0, 14), Math.min(x1 + 3, W - 80), 12);
      ctx.restore();
    }

    // 拖拽选区
    const d = drag();
    if (d) {
      const x1 = (d[0] / props.duration) * W;
      const x2 = (d[1] / props.duration) * W;
      ctx.fillStyle = 'rgba(250,204,21,0.22)';
      ctx.fillRect(Math.min(x1, x2), 0, Math.abs(x2 - x1), H());
      ctx.strokeStyle = '#facc15';
      ctx.strokeRect(Math.min(x1, x2), 0, Math.abs(x2 - x1), H());
    }

    // 播放头
    const px = (props.time / props.duration) * W;
    ctx.strokeStyle = '#f43f5e';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(px, 0);
    ctx.lineTo(px, H());
    ctx.stroke();
    ctx.lineWidth = 1;

    // hover 时间
    if (hoverT() !== null) {
      const x = (hoverT()! / props.duration) * W;
      ctx.fillStyle = '#e2e8f0';
      ctx.fillText(fmtTime(hoverT()!), x + 4, 14);
    }
  };

  createMemo(draw);
  onCleanup(() => {});

  function chooseStep(d: number) {
    if (d <= 10) return 1;
    if (d <= 30) return 5;
    if (d <= 120) return 10;
    return 30;
  }
  function labelOf(a: Annotation) {
    const prefix = { mispron: '误', stress: '重音', noise: '噪', other: '注' }[a.kind];
    return `${prefix}·${a.reviewer}`;
  }

  return (
    <canvas
      ref={canvas}
      onMouseDown={(e) => setDrag([xToTime(e.clientX), xToTime(e.clientX)])}
      onMouseMove={(e) => {
        setHoverT(xToTime(e.clientX));
        const d = drag();
        if (d) setDrag([d[0], xToTime(e.clientX)]);
      }}
      onMouseLeave={() => setHoverT(null)}
      onMouseUp={(e) => {
        const d = drag();
        setDrag(null);
        const t = xToTime(e.clientX);
        if (!d) return;
        const s = Math.min(d[0], t);
        const en = Math.max(d[0], t);
        if (en - s < 0.05) {
          props.onSeek(t); // 单击：跳转
        } else {
          props.onSelectRange?.(s, en);
        }
      }}
    />
  );
}

export { KIND_COLOR };
