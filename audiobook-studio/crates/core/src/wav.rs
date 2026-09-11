//! 最小化 WAV 解析：提取时长与波形峰值。
//!
//! 支持 PCM u8/i16/i24/i32 与 IEEE float32，多声道取各声道峰值的最大值。
//! 非 WAV（mp3/m4a 等）返回 [`WavInfo::None`]，时长由前端 Web Audio 解码后回传。

#[derive(Debug, Clone)]
pub struct WavInfo {
    pub duration: f64,
    pub channels: u16,
    pub sample_rate: u32,
    /// 单声道归一化峰值（每个峰值桶约对应 0.1 秒）。
    pub peaks: Vec<f32>,
}

impl WavInfo {
    pub const NONE_MARKER: &'static str = "";
}

pub fn parse(bytes: &[u8]) -> Option<WavInfo> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut fmt: Option<FmtChunk> = None;
    let mut data: Option<(usize, usize)> = None; // (offset, len)
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if body + size > bytes.len() {
            break;
        }
        match id {
            b"fmt " => fmt = Some(parse_fmt(&bytes[body..body + size])?),
            b"data" => {
                data = Some((body, size));
                break; // 峰值只需要第一个 data 块
            }
            _ => {}
        }
        pos = body + size + (size & 1);
    }
    let (fmt, (doff, dlen)) = (fmt?, data?);
    let bytes_per_sample = (fmt.bits / 8) as usize;
    let frame_size = bytes_per_sample * fmt.channels as usize;
    let frames = dlen.checked_div(frame_size).unwrap_or(0);
    let duration = frames as f64 / fmt.sample_rate as f64;

    let bucket = (fmt.sample_rate as usize / 10).max(1);
    let mut peaks = vec![0f32; (frames / bucket).max(1)];
    let data = &bytes[doff..doff + frames * frame_size];
    for (i, frame) in data.chunks_exact(frame_size).enumerate() {
        let mut peak = 0f32;
        for ch in frame.chunks_exact(bytes_per_sample) {
            peak = peak.max(sample_abs(ch, fmt.format, fmt.bits));
        }
        let b = (i / bucket).min(peaks.len() - 1);
        if peak > peaks[b] {
            peaks[b] = peak;
        }
    }
    Some(WavInfo { duration, channels: fmt.channels, sample_rate: fmt.sample_rate, peaks })
}

#[derive(Debug, Clone, Copy)]
struct FmtChunk {
    format: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
}

fn parse_fmt(b: &[u8]) -> Option<FmtChunk> {
    if b.len() < 16 {
        return None;
    }
    let format = u16::from_le_bytes(b[0..2].try_into().unwrap());
    let channels = u16::from_le_bytes(b[2..4].try_into().unwrap());
    let sample_rate = u32::from_le_bytes(b[4..8].try_into().unwrap());
    let bits = u16::from_le_bytes(b[14..16].try_into().unwrap());
    if channels == 0 || sample_rate == 0 {
        return None;
    }
    match (format, bits) {
        (1, 8) | (1, 16) | (1, 24) | (1, 32) | (3, 32) => Some(FmtChunk { format, channels, sample_rate, bits }),
        _ => None,
    }
}

fn sample_abs(ch: &[u8], format: u16, bits: u16) -> f32 {
    match (format, bits) {
        (1, 8) => ((ch[0] as f32 - 128.0) / 128.0).abs(),
        (1, 16) => (i16::from_le_bytes([ch[0], ch[1]]) as f32 / 32768.0).abs(),
        (1, 24) => {
            let v = (ch[0] as i32) | ((ch[1] as i32) << 8) | ((ch[2] as i32) << 16);
            let v = (v << 8) >> 8; // 符号扩展
            (v as f32 / 8_388_608.0).abs()
        }
        (1, 32) => (i32::from_le_bytes(ch[..4].try_into().unwrap()) as f64 / i32::MAX as f64) as f32,
        (3, 32) => f32::from_le_bytes(ch[..4].try_into().unwrap()).abs(),
        _ => 0.0,
    }.clamp(0.0, 1.0)
}

/// 测试辅助：生成单声道 16bit PCM WAV。
pub fn make_wav(sample_rate: u32, seconds: f64, amp: f32) -> Vec<u8> {
    let frames = (sample_rate as f64 * seconds) as usize;
    let data_len = frames * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        let half = (sample_rate as usize / 4).max(1);
        let v = (if (i / half).is_multiple_of(2) { amp } else { -amp } * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
