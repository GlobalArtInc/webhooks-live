#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("the export cannot be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("the export cannot be written: {0}")]
    Write(#[from] std::io::Error),
    #[error("the ZIP archive cannot be written: {0}")]
    Archive(#[from] async_zip::error::ZipError),
}
