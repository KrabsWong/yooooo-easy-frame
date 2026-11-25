use crate::png::{self, PngInfo};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug)]
pub struct Frame {
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub modified: Option<SystemTime>,
    pub info: PngInfo,
}

impl Frame {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        if !is_png(&path) {
            return Err(format!("{} 不是 PNG 图片", path.display()));
        }

        let data = fs::read(&path).map_err(|error| format!("读取失败: {error}"))?;
        let info = png::parse(&data).map_err(|error| error.to_string())?;
        let metadata = fs::metadata(&path).map_err(|error| format!("读取元数据失败: {error}"))?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        Ok(Self {
            path,
            name,
            size_bytes: metadata.len(),
            modified: metadata.modified().ok(),
            info,
        })
    }
}

pub fn is_png(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}
