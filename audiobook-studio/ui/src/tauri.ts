import { invoke as tauriInvoke } from '@tauri-apps/api/core';

export const isTauri = typeof (window as any).__TAURI_INTERNALS__ !== 'undefined';

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) {
    throw new Error('请在桌面端（Tauri）中打开，浏览器环境无法访问本地工程。');
  }
  return tauriInvoke<T>(cmd, args);
}
