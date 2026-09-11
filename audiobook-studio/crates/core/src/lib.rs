//! ab-core：有声书工坊的纯 Rust 领域核心。
//!
//! 不依赖 Tauri / 浏览器环境，SQLite 工程可以直接在测试中创建，
//! 所有业务规则（同形词例外、越界校验、待复核生成、离线包合并）都在这里。

pub mod error;
pub mod clock;
pub mod wav;
pub mod db;
pub mod models;
pub mod dict;
pub mod recordings;
pub mod review;
pub mod package;
pub mod diff;

pub use error::{Error, Result};
pub use db::Studio;
