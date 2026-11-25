use std::fs;
use std::path::Path;

#[cfg(test)]
use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const IHDR: &[u8; 4] = b"IHDR";
const ACTL: &[u8; 4] = b"acTL";
const IEND: &[u8; 4] = b"IEND";

#[derive(Clone)]
struct Chunk {
    kind: [u8; 4],
    data: Vec<u8>,
    crc: u32,
}

/// Move the animation control chunk directly after IHDR.
///
/// Some image loaders only inspect the first 80 bytes for `acTL`. In palette
/// APNGs, PLTE and tRNS push `acTL` beyond that window even though the file is
/// valid. Moving only `acTL` preserves all frame data and sequence numbers.
pub fn normalize_actl_position(path: &Path) -> Result<(), String> {
    let data = fs::read(path).map_err(|error| format!("读取 APNG 失败: {error}"))?;
    if !data.starts_with(&PNG_SIGNATURE) {
        return Err("不是有效的 PNG 文件".into());
    }

    let (chunks, trailing) = parse_chunks(&data)?;
    if chunks.first().map(|chunk| &chunk.kind) != Some(IHDR) {
        return Err("PNG 缺少 IHDR".into());
    }

    let positions: Vec<usize> = chunks
        .iter()
        .enumerate()
        .filter_map(|(index, chunk)| (chunk.kind == *ACTL).then_some(index))
        .collect();
    if positions.len() > 1 {
        return Err("APNG 包含多个 acTL".into());
    }
    let Some(position) = positions.first().copied() else {
        return Ok(());
    };
    if position == 1 {
        return Ok(());
    }

    let mut chunks = chunks;
    let actl = chunks.remove(position);
    chunks.insert(1, actl);

    let mut output = Vec::with_capacity(data.len());
    output.extend_from_slice(&PNG_SIGNATURE);
    for chunk in chunks {
        output.extend_from_slice(&(chunk.data.len() as u32).to_be_bytes());
        output.extend_from_slice(&chunk.kind);
        output.extend_from_slice(&chunk.data);
        output.extend_from_slice(&chunk.crc.to_be_bytes());
    }
    output.extend_from_slice(trailing);
    if output.len() != data.len() {
        return Err("APNG 重排后长度发生变化".into());
    }

    fs::write(path, output).map_err(|error| format!("写入 APNG 失败: {error}"))
}

fn parse_chunks(data: &[u8]) -> Result<(Vec<Chunk>, &[u8]), String> {
    let mut chunks = Vec::new();
    let mut offset = PNG_SIGNATURE.len();

    while offset + 12 <= data.len() {
        let length = u32::from_be_bytes(
            data[offset..offset + 4]
                .try_into()
                .map_err(|_| "PNG chunk 长度无效".to_string())?,
        ) as usize;
        let kind: [u8; 4] = data[offset + 4..offset + 8]
            .try_into()
            .map_err(|_| "PNG chunk 类型无效".to_string())?;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or_else(|| "PNG chunk 长度溢出".to_string())?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or_else(|| "PNG chunk 长度溢出".to_string())?;
        if chunk_end > data.len() {
            return Err("PNG 数据不完整".into());
        }

        let body = &data[data_start..data_end];
        let expected = u32::from_be_bytes(
            data[data_end..chunk_end]
                .try_into()
                .map_err(|_| "PNG CRC 无效".to_string())?,
        );
        let actual = crc32(&kind, body);
        if expected != actual {
            return Err(format!("PNG chunk {} CRC 错误", display_chunk(&kind)));
        }

        chunks.push(Chunk {
            kind,
            data: body.to_vec(),
            crc: actual,
        });
        offset = chunk_end;

        if kind == *IEND {
            return Ok((chunks, &data[offset..]));
        }
    }

    Err("PNG 数据不完整".into())
}

fn crc32(kind: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn display_chunk(kind: &[u8; 4]) -> String {
    String::from_utf8_lossy(kind).into_owned()
}

#[cfg(test)]
fn unique_temp_path(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!("easy-frame-apng-{label}-{}-{nanos}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(12 + data.len());
        chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(data);
        chunk.extend_from_slice(&crc32(kind, data).to_be_bytes());
        chunk
    }

    fn build_png(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut data = PNG_SIGNATURE.to_vec();
        for (kind, body) in chunks {
            data.extend_from_slice(&encode_chunk(kind, body));
        }
        data
    }

    fn write_temp(data: &[u8]) -> std::path::PathBuf {
        let path = unique_temp_path("test");
        fs::write(&path, data).unwrap();
        path
    }

    fn chunk_kinds(data: &[u8]) -> Vec<String> {
        let (chunks, _) = parse_chunks(data).unwrap();
        chunks
            .iter()
            .map(|chunk| display_chunk(&chunk.kind))
            .collect()
    }

    #[test]
    fn moves_actl_after_ihdr_without_changing_other_chunks() {
        let ihdr = [0u8; 13];
        let actl = 8u32.to_be_bytes().into_iter().chain(0u32.to_be_bytes()).collect::<Vec<_>>();
        let source = build_png(&[
            (IHDR, &ihdr),
            (b"pHYs", &[0u8; 9]),
            (b"PLTE", &[0u8; 3]),
            (b"tRNS", &[0u8; 2]),
            (ACTL, &actl),
            (b"fcTL", &[0u8; 26]),
            (b"IDAT", &[0u8; 4]),
            (IEND, &[]),
        ]);
        let path = write_temp(&source);

        normalize_actl_position(&path).unwrap();
        let output = fs::read(&path).unwrap();

        assert_eq!(output.len(), source.len());
        assert_eq!(&output[12..16], IHDR);
        assert_eq!(&output[37..41], ACTL);
        assert_eq!(
            chunk_kinds(&output),
            vec!["IHDR", "acTL", "pHYs", "PLTE", "tRNS", "fcTL", "IDAT", "IEND"]
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn leaves_static_png_untouched() {
        let source = build_png(&[(IHDR, &[0u8; 13]), (b"IDAT", &[1, 2, 3]), (IEND, &[])]);
        let path = write_temp(&source);

        normalize_actl_position(&path).unwrap();

        assert_eq!(fs::read(&path).unwrap(), source);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn is_idempotent() {
        let actl = 1u32.to_be_bytes().into_iter().chain(0u32.to_be_bytes()).collect::<Vec<_>>();
        let source = build_png(&[
            (IHDR, &[0u8; 13]),
            (b"PLTE", &[0u8; 3]),
            (ACTL, &actl),
            (IEND, &[]),
        ]);
        let path = write_temp(&source);

        normalize_actl_position(&path).unwrap();
        let once = fs::read(&path).unwrap();
        normalize_actl_position(&path).unwrap();

        assert_eq!(fs::read(&path).unwrap(), once);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_duplicate_actl() {
        let actl = 1u32.to_be_bytes().into_iter().chain(0u32.to_be_bytes()).collect::<Vec<_>>();
        let source = build_png(&[
            (IHDR, &[0u8; 13]),
            (ACTL, &actl),
            (ACTL, &actl),
            (IEND, &[]),
        ]);
        let path = write_temp(&source);

        assert!(normalize_actl_position(&path)
            .unwrap_err()
            .contains("多个 acTL"));
        let _ = fs::remove_file(path);
    }

    #[test]
    #[ignore = "requires EASY_FRAME_APNG_FIXTURE"]
    fn normalizes_exported_fixture() {
        let source_path = env::var("EASY_FRAME_APNG_FIXTURE").expect("fixture path");
        let source = fs::read(&source_path).unwrap();
        let path = unique_temp_path("fixture");
        fs::write(&path, &source).unwrap();

        normalize_actl_position(&path).unwrap();
        let output = fs::read(&path).unwrap();

        assert_eq!(output.len(), source.len());
        assert!(output.len() >= 41);
        assert_eq!(&output[37..41], ACTL);
        parse_chunks(&output).unwrap();
        let _ = fs::remove_file(path);
    }
}
