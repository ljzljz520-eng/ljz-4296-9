# 有声书工坊（Audiobook Studio）

有声书制作团队的桌面端工具：统一人物 / 地名 / 虚构术语读音，把审听意见精确落到录音片段。

- **桌面壳**：Rust + Tauri 2
- **界面**：SolidJS + TypeScript + Vite
- **工程存储**：本地 SQLite（`studio.db` + `media/`），无需联网
- **波形播放**：Web Audio API（`decodeAudioData` + `AudioBufferSourceNode`，支持精确 seek 与选区循环）
- **领域核心**：`crates/core` 为不依赖 Tauri 的纯 Rust crate，可直接 `cargo test`

## 目录结构

```
crates/
  core/          # ab-core：SQLite 工程、词典版本、例外解析、批注越界校验、
                 #          外部替换检测、待复核生成、离线包合并、差异报告
  tauri-app/     # ab-tauri：Tauri 命令层（薄封装），src/main.rs
ui/              # SolidJS 界面（波形审听 / 发音词典 / 待复核队列 / 离线包）
```

## 快速开始

### 1. 系统依赖（Tauri 2，Linux）

Debian/Ubuntu：

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev
```

macOS / Windows 见 <https://v2.tauri.app/start/prerequisites/>。

### 2. 运行

```bash
# 界面依赖
npm --prefix ui install

# 开发模式（自动起 Vite:1420 并打开 Tauri 窗口）
cargo install tauri-cli --version "^2"
cargo tauri dev

# 打包
cargo tauri build
```

### 3. 只跑核心领域测试（不需要任何系统 GUI 库）

```bash
cargo test -p ab-core
```

> 若环境里的 `cc` 被容器包装脚本拦截（某些沙箱），可把真实 gcc 放到 PATH 前面：
> `mkdir -p /tmp/bin && ln -sf /usr/bin/cc /tmp/bin/cc && PATH=/tmp/bin:$PATH cargo test`

## 数据模型要点

| 表 | 说明 |
|---|---|
| `entry` / `entry_version` | 词典条目与**不可变版本**。批注锁定具体 `entry_version_id` |
| `entry_exception` | 同形词例外，按 `scope_kind=role/chapter` + `scope_ref`（角色名/章节号）|
| `chapter` / `recording` | 章节与录音片段（章节号+条次唯一），保存 sha256、大小、时长、峰值 |
| `recording_occurrence` | 某词条在片段中的出现时间段（修改读音时据此定位受影响范围）|
| `annotation` | 审听批注：误读/重音/噪声/其他，精确 `[time_start, time_end]`，指向词典版本 |
| `review_range` | 待复核范围（读音变化/外来批注），状态 pending/ok/reannotated/rerecord |

## 关键业务规则

1. **同名异读走例外，不允许重复建条**。读音解析优先级：**角色例外 > 章节例外 > 当前批准版本**。
2. **修改读音不自动判旧录音错误**：批准的新版本只对命中的 `recording_occurrence`
   生成 `pending` 待复核范围；被角色/章节例外覆盖的出现不受默认读音变化影响。
   复核结论：旧录音可用 / 转批注 / 需重录。
3. **批注越界直接拒绝**：`start >= end`、负时间、`end > 片段时长` 返回 `OutOfBounds`。
4. **音频外部替换检测**：`verify_recording` 重算 sha256；变化时刷新摘要并列出
   超出新时长的批注（不自动删除，交审听者处理）。
5. **离线包 `.abpkg`（zip）**：含 `snapshot.json`、`manifest.json` 与可选媒体；
   导入合并**幂等**（按 UUID 去重，可安全重复导入），冲突（同 ID 内容不一致、
   越界批注、同键例外读音分歧）只报告不覆盖本地。
6. **多审听者**：每人可只导出自己的批注（reviewer 过滤）；主编合并后，外来未决批注
   自动生成待复核范围。
7. **差异报告**：比较两份快照（包或裸 JSON），输出新增/读音变化/录音替换/新批注/
   复核处置，含结构化结果与中文纯文本渲染。

## 已覆盖的测试场景（`crates/core/tests`）

- `homograph.rs`：同名异读例外与优先级、改读音只生成待复核、待定版本不生效、例外改版只影响命中出现
- `replacement_and_bounds.rs`：批注越界/零长度拒绝、外部替换检测与越界批注清单、文件丢失
- `merge.rs`：两审听者打包-合并-幂等、包内越界批注拦截为冲突、录音/例外分歧冲突、zip 往返
- `e2e.rs`：WAV 时长与峰值导入、改读音→批注→待复核→处置→差异报告全链路、修订号递增
