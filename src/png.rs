use std::fmt;

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

#[derive(Clone, Debug, Default)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
    pub compression: u8,
    pub filter: u8,
    pub interlace: u8,
    pub dpi_x: Option<f64>,
    pub dpi_y: Option<f64>,
    pub gamma: Option<f64>,
    pub srgb_intent: Option<u8>,
    pub modified: Option<String>,
    pub chunks: Vec<String>,
    pub text_entries: Vec<(String, String)>,
}

impl PngInfo {
    pub fn color_type_name(&self) -> &'static str {
        match self.color_type {
            0 => "Grayscale",
            2 => "Truecolor",
            3 => "Indexed",
            4 => "Grayscale + alpha",
            6 => "Truecolor + alpha",
            _ => "Unknown",
        }
    }

    pub fn interlace_name(&self) -> &'static str {
        match self.interlace {
            0 => "None",
            1 => "Adam7",
            _ => "Unknown",
        }
    }
}

#[derive(Debug)]
pub struct PngError(pub String);

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn parse(data: &[u8]) -> Result<PngInfo, PngError> {
    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return Err(PngError("不是有效的 PNG 文件".into()));
    }

    let mut info = PngInfo::default();
    let mut offset = 8usize;
    let mut saw_ihdr = false;

    while offset + 12 <= data.len() {
        let length = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let chunk_type = &data[offset + 4..offset + 8];
        let chunk_end = offset + 8 + length;
        if chunk_end + 4 > data.len() {
            return Err(PngError("PNG 数据不完整".into()));
        }
        let chunk_data = &data[offset + 8..chunk_end];

        if info.chunks.len() < 24 {
            let name = String::from_utf8_lossy(chunk_type).into_owned();
            if !matches!(name.as_str(), "IDAT" | "IEND") {
                info.chunks.push(format!("{name} ({length} B)"));
            }
        }

        match chunk_type {
            b"IHDR" if chunk_data.len() >= 13 => {
                info.width = u32::from_be_bytes(chunk_data[0..4].try_into().unwrap());
                info.height = u32::from_be_bytes(chunk_data[4..8].try_into().unwrap());
                info.bit_depth = chunk_data[8];
                info.color_type = chunk_data[9];
                info.compression = chunk_data[10];
                info.filter = chunk_data[11];
                info.interlace = chunk_data[12];
                saw_ihdr = true;
            }
            b"pHYs" if chunk_data.len() >= 9 => {
                let x = u32::from_be_bytes(chunk_data[0..4].try_into().unwrap()) as f64;
                let y = u32::from_be_bytes(chunk_data[4..8].try_into().unwrap()) as f64;
                if chunk_data[8] == 1 {
                    info.dpi_x = Some(x * 0.0254);
                    info.dpi_y = Some(y * 0.0254);
                }
            }
            b"gAMA" if chunk_data.len() >= 4 => {
                let gamma = u32::from_be_bytes(chunk_data[0..4].try_into().unwrap()) as f64;
                if gamma > 0.0 {
                    info.gamma = Some(100_000.0 / gamma);
                }
            }
            b"sRGB" if !chunk_data.is_empty() => {
                info.srgb_intent = Some(chunk_data[0]);
            }
            b"tIME" if chunk_data.len() >= 7 => {
                let year = u16::from_be_bytes([chunk_data[0], chunk_data[1]]);
                info.modified = Some(format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    year, chunk_data[2], chunk_data[3], chunk_data[4], chunk_data[5], chunk_data[6]
                ));
            }
            b"tEXt" => {
                if let Some((key, value)) = split_key_value(chunk_data, false) {
                    info.text_entries.push((key, value));
                }
            }
            b"zTXt" => {
                if let Some((key, value)) = split_key_value(chunk_data, true) {
                    info.text_entries.push((key, value));
                }
            }
            b"iTXt" => {
                if let Some((key, value)) = parse_itxt(chunk_data) {
                    info.text_entries.push((key, value));
                }
            }
            _ => {}
        }

        offset = chunk_end + 4;
        if chunk_type == b"IEND" {
            break;
        }
    }

    if !saw_ihdr {
        return Err(PngError("PNG 缺少 IHDR 数据".into()));
    }
    Ok(info)
}

fn split_key_value(data: &[u8], skip_compression_method: bool) -> Option<(String, String)> {
    let null = data.iter().position(|byte| *byte == 0)?;
    let key = String::from_utf8_lossy(&data[..null]).into_owned();
    let start = null + 1 + usize::from(skip_compression_method);
    let value = String::from_utf8_lossy(data.get(start..).unwrap_or_default()).into_owned();
    Some((key, value))
}

fn parse_itxt(data: &[u8]) -> Option<(String, String)> {
    let key_end = data.iter().position(|byte| *byte == 0)?;
    let key = String::from_utf8_lossy(&data[..key_end]).into_owned();
    let mut cursor = key_end + 1;
    if cursor + 2 > data.len() {
        return None;
    }
    cursor += 2;
    let lang_end = data[cursor..].iter().position(|byte| *byte == 0)? + cursor;
    cursor = lang_end + 1;
    let translated_end = data[cursor..].iter().position(|byte| *byte == 0)? + cursor;
    cursor = translated_end + 1;
    let text = String::from_utf8_lossy(&data[cursor..]).into_owned();
    Some((key, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_PIXEL_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
        1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 24, 87, 99, 248,
        207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 157, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];

    #[test]
    fn parses_ihdr() {
        let info = parse(ONE_PIXEL_PNG).unwrap();
        assert_eq!((info.width, info.height), (1, 1));
        assert_eq!(info.bit_depth, 8);
        assert_eq!(info.color_type_name(), "Truecolor + alpha");
    }

    #[test]
    fn rejects_non_png() {
        assert!(parse(b"not a png").is_err());
    }
}
