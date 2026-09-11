use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("数据格式错误: {0}")]
    Format(String),
    #[error("找不到 {0}")]
    NotFound(String),
    #[error("冲突: {0}")]
    Conflict(String),
    #[error("批注越界: {0} (片段时长 {1}s, 批注 {2}s–{3}s)")]
    OutOfBounds(String, f64, f64, f64),
    #[error("时间范围非法: start={0}, end={1}")]
    BadRange(f64, f64),
    #[error("条目无可用批准版本: {0}")]
    NoApprovedVersion(String),
    #[error("离线包错误: {0}")]
    Package(String),
    #[error("压缩包错误: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
