use std::{
    env, fs,
    ffi::OsStr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const MIN_FFMPEG_VERSION: (u32, u32) = (5, 0);
const UNSUPPORTED_MESSAGE: &str = "当前应用暂不支持当前系统。";
const EXPORT_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub struct Environment {
    pub ffmpeg: PathBuf,
    pub version: String,
}

pub fn check_environment() -> Result<Environment, String> {
    let ffmpeg = bundled_ffmpeg_path()?;

    let output = Command::new(&ffmpeg)
        .arg("-version")
        .output()
        .map_err(|_| UNSUPPORTED_MESSAGE.to_string())?;
    if !output.status.success() {
        return Err(UNSUPPORTED_MESSAGE.into());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let version_line = text.lines().next().unwrap_or_default();
    let version = parse_version(version_line)
        .ok_or_else(|| UNSUPPORTED_MESSAGE.to_string())?;

    if version < MIN_FFMPEG_VERSION {
        return Err(UNSUPPORTED_MESSAGE.into());
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
    let temp_output = temporary_output_path(output);
    fs::create_dir_all(&staging).map_err(|error| format!("创建临时目录失败: {error}"))?;

    let result = (|| {
        for (index, path) in frames.iter().enumerate() {
            let target = staging.join(format!("frame_{index:06}.png"));
            fs::copy(path, &target)
                .map_err(|error| format!("复制 {} 失败: {error}", path.display()))?;
        }

        let pattern = staging.join("frame_%06d.png");
        let compression_level = compression_level(quality);
        let framerate = format!("1000/{interval_ms}");

        let child = Command::new(ffmpeg)
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-framerate")
            .arg(framerate)
            .arg("-i")
            .arg(pattern)
            .arg("-vf")
            .arg(palette_filter(quality))
            .arg("-c:v")
            .arg("apng")
            .arg("-pix_fmt")
            .arg("pal8")
            .arg("-pred")
            .arg("mixed")
            .arg("-compression_level")
            .arg(compression_level.to_string())
            .arg("-plays")
            .arg("0")
            .arg("-f")
            .arg("apng")
            .arg(&temp_output)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("启动 ffmpeg 失败: {error}"))?;

        let status = wait_with_timeout(child, EXPORT_TIMEOUT)?;

        if status.status.success() {
            fs::rename(&temp_output, output)
                .map_err(|error| format!("保存导出文件失败: {error}"))?;
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

    let _ = fs::remove_file(&temp_output);
    let _ = fs::remove_dir_all(&staging);
    result
}

fn wait_with_timeout(mut child: Child, timeout: Duration) -> Result<Output, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("等待 ffmpeg 失败: {error}"));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    return child
                        .wait_with_output()
                        .map_err(|error| format!("等待 ffmpeg 失败: {error}"))
                        .and_then(|_| {
                            Err(format!(
                                "导出超时，已停止。限制为 {} 秒，请重试或减少帧数。",
                                timeout.as_secs()
                            ))
                        });
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => {
                let _ = child.kill();
                return Err(format!("检查 ffmpeg 状态失败: {error}"));
            }
        }
    }
}

fn palette_filter(quality: u8) -> String {
    format!(
        "format=rgba,split[a][b];[a]palettegen=max_colors={}:reserve_transparent=1:stats_mode=full[p];[b][p]paletteuse=dither={}:diff_mode=rectangle",
        palette_colors(quality),
        palette_dither(quality)
    )
}

fn temporary_output_path(output: &Path) -> PathBuf {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!(
        ".easy-frame-{}-{nanos}.png",
        std::process::id()
    ))
}

fn format_version(version: &(u32, u32)) -> String {
    format!("{}.{}", version.0, version.1)
}

fn compression_level(quality: u8) -> u8 {
    let quality = quality.clamp(1, 100);
    (1 + (u16::from(quality - 1) * 8 + 49) / 99) as u8
}

fn palette_colors(quality: u8) -> u16 {
    let quality = quality.clamp(1, 100);
    match quality {
        90..=100 => 256,
        75..=89 => 128,
        50..=74 => 96,
        25..=49 => 64,
        _ => 32,
    }
}

fn palette_dither(quality: u8) -> &'static str {
    if quality >= 98 {
        "sierra2_4a"
    } else {
        "none"
    }
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

    #[test]
    fn maps_quality_to_palette_colors() {
        assert_eq!(palette_colors(1), 32);
        assert_eq!(palette_colors(30), 64);
        assert_eq!(palette_colors(60), 96);
        assert_eq!(palette_colors(80), 128);
        assert_eq!(palette_colors(92), 256);
        assert_eq!(palette_colors(100), 256);
    }

    #[test]
    fn selects_dither_only_at_highest_quality() {
        assert_eq!(palette_dither(97), "none");
        assert_eq!(palette_dither(98), "sierra2_4a");
        assert_eq!(palette_dither(100), "sierra2_4a");
    }

    #[test]
    fn timeout_stops_long_running_export() {
        let child = Command::new("sh")
            .arg("-c")
            .arg("exec sleep 2")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let result = wait_with_timeout(child, Duration::from_millis(50));
        assert!(result.unwrap_err().contains("导出超时"));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn bundled_ffmpeg_path_is_inside_app_resources() {
        let executable = Path::new("/Applications/Easy Frame.app/Contents/MacOS/apng-builder");
        assert_eq!(
            ffmpeg_path_for_executable(executable),
            Path::new("/Applications/Easy Frame.app/Contents/Resources/ffmpeg")
        );
    }
}
fn staging_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!("apng-builder-{}-{nanos}", std::process::id()))
}

fn bundled_ffmpeg_path() -> Result<PathBuf, String> {
    let executable = env::current_exe()
        .map_err(|_| UNSUPPORTED_MESSAGE.to_string())?;
    let path = ffmpeg_path_for_executable(&executable);
    let mode = fs::metadata(&path)
        .map(|metadata| metadata.permissions().mode())
        .map_err(|_| UNSUPPORTED_MESSAGE.to_string())?;
    if mode & 0o111 == 0 {
        return Err(UNSUPPORTED_MESSAGE.into());
    }
    Ok(path)
}

fn ffmpeg_path_for_executable(executable: &Path) -> PathBuf {
    let directory = executable.parent().unwrap_or_else(|| Path::new("."));
    if directory.file_name() == Some(OsStr::new("MacOS")) {
        directory
            .parent()
            .unwrap_or(directory)
            .join("Resources")
            .join("ffmpeg")
    } else {
        directory.join("Resources").join("ffmpeg")
    }
}

fn parse_version(line: &str) -> Option<(u32, u32)> {
    let after_version = line.split_once("version")?.1;
    let version = after_version.split_whitespace().next()?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}
