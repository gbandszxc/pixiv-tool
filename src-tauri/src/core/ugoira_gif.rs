//! 按 Pixiv 帧表把原始尺寸 ZIP 转成循环 GIF。

use std::io::{BufWriter, Cursor, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use gif::{DisposalMethod, Encoder, Frame, Repeat};
use image::ImageReader;

use crate::pixiv::api::UgoiraDownloadFrame;
use crate::ugoira::sanitize_frame_name;

pub(super) fn write_gif(
    zip_bytes: &[u8],
    frames: &[UgoiraDownloadFrame],
    path: &Path,
) -> Result<()> {
    let (first, rest) = frames.split_first().context("动图没有可导出的帧")?;
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).context("读取动图 ZIP 失败")?;
    let mut bytes = Vec::new();
    let mut decode = |frame: &UgoiraDownloadFrame| -> Result<image::RgbaImage> {
        let name = sanitize_frame_name(&frame.file).context("动图包含无效的帧文件名")?;
        let mut entry = archive
            .by_name(&name)
            .with_context(|| format!("动图 ZIP 缺少帧 {name}"))?;
        ensure!(entry.is_file(), "动图 ZIP 条目 {name} 不是帧文件");
        bytes.clear();
        entry.read_to_end(&mut bytes)?;
        let pixels = ImageReader::new(Cursor::new(bytes.as_slice()))
            .with_guessed_format()?
            .decode()
            .with_context(|| format!("解码动图帧 {name} 失败"))?;
        Ok(pixels.into_rgba8())
    };
    let first_pixels = decode(first)?;
    let (width, height) = first_pixels.dimensions();
    let gif_width = u16::try_from(width).context("动图宽度超出 GIF 上限")?;
    let gif_height = u16::try_from(height).context("动图高度超出 GIF 上限")?;

    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(directory)?;
    {
        let mut writer = BufWriter::new(output.as_file_mut());
        let mut encoder = Encoder::new(&mut writer, gif_width, gif_height, &[])?;
        encoder.set_repeat(Repeat::Infinite)?;
        let images = std::iter::once(Ok(first_pixels)).chain(rest.iter().map(decode));
        for (frame, pixels) in frames.iter().zip(images) {
            let mut pixels = pixels?;
            ensure!(
                pixels.dimensions() == (width, height),
                "动图帧 {} 的尺寸与首帧不同",
                frame.file
            );
            let mut encoded = Frame::from_rgba_speed(gif_width, gif_height, &mut pixels, 10);
            encoded.dispose = DisposalMethod::Background;
            // GIF 以百分之一秒为单位，四舍五入并限制到格式可表示的范围。
            encoded.delay =
                ((u64::from(frame.delay) + 5) / 10).clamp(1, u64::from(u16::MAX)) as u16;
            encoder.write_frame(&encoded)?;
        }
        encoder.into_inner().context("完成 GIF 编码失败")?;
        writer.flush().context("写入 GIF 失败")?;
    }
    output.persist(path).context("提交 GIF 文件失败")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use gif::{ColorOutput, DecodeOptions};

    use super::*;

    #[test]
    fn gif_uses_frame_order_and_delays_and_commits_only_on_success() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, color) in [
            ("red.png", [255, 0, 0, 255]),
            ("blue.png", [0, 0, 255, 255]),
        ] {
            let mut png = Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(2, 3, image::Rgba(color))
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(png.get_ref()).unwrap();
        }
        let zip = archive.finish().unwrap().into_inner();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("animation.gif");
        let mut frames = vec![
            UgoiraDownloadFrame {
                file: "blue.png".into(),
                delay: 46,
            },
            UgoiraDownloadFrame {
                file: "red.png".into(),
                delay: 120,
            },
        ];
        write_gif(&zip, &frames, &path).unwrap();
        let gif = std::fs::read(&path).unwrap();
        assert!(gif.starts_with(b"GIF89a"));
        let mut options = DecodeOptions::new();
        options.set_color_output(ColorOutput::RGBA);
        let mut decoded = options.read_info(Cursor::new(&gif)).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 3));
        let first = decoded.read_next_frame().unwrap().unwrap();
        assert_eq!(&first.buffer[..4], &[0, 0, 255, 255]);
        assert_eq!(first.delay, 5);
        let second = decoded.read_next_frame().unwrap().unwrap();
        assert_eq!(&second.buffer[..4], &[255, 0, 0, 255]);
        assert_eq!(second.delay, 12);
        assert!(decoded.read_next_frame().unwrap().is_none());
        assert_eq!(decoded.repeat(), Repeat::Infinite);

        frames[1].file = "missing.png".into();
        assert!(write_gif(&zip, &frames, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), gif);
        assert_eq!(directory.path().read_dir().unwrap().count(), 1);
    }
}
