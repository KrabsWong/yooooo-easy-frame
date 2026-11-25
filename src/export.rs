use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

pub const MIN_FFMPEG_VERSION: (u32, u32) = (5, 0);

#[derive(Clone, Debug)]
pub struct Environment {
    pub ffmpeg: PathBuf,
    pub version: String,
}

pub fn check_environment() -> Result<Environment, String> {
    let ffmpeg = find_in_path("ffmpeg").ok_or_else(|| {
        "未检测到 ffmpeg。请安装后重新启动应用：\nbrew install ffmpeg".to_string()
    })?;

    let output = Command::new(&ffmpeg)
        .arg("-version")
        .output()
        .map_err(|error| format!("无法运行 ffmpeg: {error}"))?;
    if !output.status.success() {
        return Err("ffmpeg 无法正常运行，请检查本地安装。".into());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let version_line = text.lines().next().unwrap_or_default();
    let version = parse_version(version_line).ok_or_else(|| {
        format!("无法识别 ffmpeg 版本：{version_line}\n请安装 ffmpeg 5.0 或更高版本。")
    })?;

    if version < MIN_FFMPEG_VERSION {
        return Err(format!(
            "检测到 ffmpeg {}，需要 {} 或更高版本。\nbrew upgrade ffmpeg",
            format_version(&version),
            format_version(&MIN_FFMPEG_VERSION)
        ));
    }

    Ok(Environment {
        ffmpeg,
        version: format_version(&version),
    })
}

pub fn export_apng(
    ffmpeg: &Path,
    frames: &[PathBuf],
    output: &Path,
    interval_ms: u16,
    quality: u8,
) -> Result<(), String> {
    if frames.is_empty() {
        return Err("请先添加序列帧图片。".into());
    }
    if interval_ms == 0 {
        return Err("帧间隔必须大于 0 ms。".into());
    }

    let staging = staging_dir();
    fs::create_dir_all(&staging).map_err(|error| format!("创建临时目录失败: {error}"))?;

    let result = (|| {
        for (index, path) in frames.iter().enumerate() {
            let target = staging.join(format!("frame_{index:06}.png"));
            fs::copy(path, &target)
                .map_err(|error| format!("复制 {} 失败: {error}", path.display()))?;
        }

        let pattern = staging.join("frame_%06d.png");
        let compression_level = compression_level(quality);
        let prediction = if quality >= 90 { "mixed" } else { "paeth" };
        let framerate = format!("1000/{interval_ms}");

        let status = Command::new(ffmpeg)
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-framerate")
            .arg(framerate)
            .arg("-i")
            .arg(pattern)
            .arg("-vf")
            .arg("format=rgba")
            .arg("-c:v")
            .arg("apng")
            .arg("-pred")
            .arg(prediction)
            .arg("-compression_level")
            .arg(compression_level.to_string())
            .arg("-plays")
            .arg("0")
            .arg(output)
            .output()
            .map_err(|error| format!("启动 ffmpeg 失败: {error}"))?;

        if status.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&status.stderr).trim().to_string();
            Err(if stderr.is_empty() {
                "ffmpeg 导出失败。".into()
            } else {
                format!("ffmpeg 导出失败: {stderr}")
            })
        }
    })();

    let _ = fs::remove_dir_all(&staging);
    result
}

fn staging_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!("apng-builder-{}-{nanos}", std::process::id()))
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn parse_version(line: &str) -> Option<(u32, u32)> {
    let after_version = line.split_once("version")?.1;
    let version = after_version.split_whitespace().next()?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

fn format_version(version: &(u32, u32)) -> String {
    format!("{}.{}", version.0, version.1)
}

fn compression_level(quality: u8) -> u8 {
    let quality = quality.clamp(1, 100);
    (1 + (u16::from(quality - 1) * 8 + 49) / 99) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffmpeg_version() {
        assert_eq!(parse_version("ffmpeg version 9.1 Copyright"), Some((9, 1)));
        assert_eq!(parse_version("ffmpeg version 4.4.2"), Some((4, 4)));
    }

    #[test]
    fn maps_quality_to_compression_level() {
        assert_eq!(compression_level(1), 1);
        assert_eq!(compression_level(100), 9);
    }
}
