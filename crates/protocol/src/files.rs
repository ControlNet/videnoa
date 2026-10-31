use serde::{Deserialize, Serialize};

/// `PUT /api/files/{path}`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UploadResponse {
    pub path: String,
    pub size: u64,
}

/// `GET /api/files/{path}/stat`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileStatResponse {
    pub path: String,
    pub size: u64,
    pub is_file: bool,
    pub is_dir: bool,
}
