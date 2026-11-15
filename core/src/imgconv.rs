//! Conversão de imagem entre PNG, JPEG e WebP.

use image::{ImageFormat, Rgb, RgbImage};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    WebP,
}

impl Format {
    fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::WebP => "webp",
        }
    }
}

/// Primeiro caminho livre: `nome.ext`, `nome-1.ext`, `nome-2.ext`…
fn free_path(src: &Path, ext: &str) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new("."));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("imagem");
    let mut out = dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    while out.exists() {
        out = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    out
}

/// Converte e grava ao lado do original, sem sobrescrever nada.
pub fn convert_image(src: &Path, to: Format) -> Result<PathBuf, String> {
    let img = image::open(src).map_err(|e| format!("não é uma imagem que eu consiga abrir: {e}"))?;
    let out = free_path(src, to.ext());
    let result = match to {
        Format::Jpeg => {
            // JPEG não tem transparência: compõe sobre fundo branco.
            let rgba = img.to_rgba8();
            let mut rgb = RgbImage::new(rgba.width(), rgba.height());
            for (x, y, p) in rgba.enumerate_pixels() {
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                rgb.put_pixel(x, y, Rgb([mix(p[0]), mix(p[1]), mix(p[2])]));
            }
            rgb.save_with_format(&out, ImageFormat::Jpeg)
        }
        Format::Png => img.save_with_format(&out, ImageFormat::Png),
        Format::WebP => img.to_rgba8().save_with_format(&out, ImageFormat::WebP),
    };
    result.map_err(|e| format!("falha ao gravar: {e}"))?;
    Ok(out)
}

/// Decodifica uma imagem (ex.: capa do álbum) para RGBA, encolhendo para caber em
/// `max` px no lado maior. Devolve (largura, altura, pixels).
pub fn decode_rgba(bytes: &[u8], max: u32) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max || img.height() > max {
        img.resize(max, max, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nocto-img-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn png(d: &Path, px: [u8; 4]) -> PathBuf {
        let p = d.join("a.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba(px)).save(&p).unwrap();
        p
    }

    #[test]
    fn png_to_jpeg_creates_file() {
        let d = dir("jpg");
        let out = convert_image(&png(&d, [10, 20, 30, 255]), Format::Jpeg).unwrap();
        assert_eq!(out.extension().unwrap(), "jpg");
        assert!(out.exists());
    }
    #[test]
    fn transparent_png_to_jpeg_has_white_background() {
        let d = dir("alpha");
        let out = convert_image(&png(&d, [0, 0, 0, 0]), Format::Jpeg).unwrap();
        let px = image::open(out).unwrap().to_rgb8().get_pixel(1, 1).0;
        assert!(px.iter().all(|&c| c >= 250), "{px:?}");
    }
    #[test]
    fn png_to_webp_roundtrips() {
        let d = dir("webp");
        let out = convert_image(&png(&d, [10, 20, 30, 255]), Format::WebP).unwrap();
        assert_eq!(image::open(out).unwrap().to_rgba8().get_pixel(0, 0).0, [10, 20, 30, 255]);
    }
    #[test]
    fn not_an_image_is_error() {
        let d = dir("txt");
        let t = d.join("leia.txt");
        fs::write(&t, "oi").unwrap();
        assert!(convert_image(&t, Format::Png).is_err());
        assert!(convert_image(Path::new("nao_existe.png"), Format::Png).is_err());
    }
    #[test]
    fn does_not_overwrite() {
        let d = dir("dup");
        let src = png(&d, [1, 2, 3, 255]);
        let a = convert_image(&src, Format::Jpeg).unwrap();
        let b = convert_image(&src, Format::Jpeg).unwrap();
        let c = convert_image(&src, Format::Png).unwrap();
        assert_ne!(a, b);
        assert_ne!(c, src);
        assert!(a.exists() && b.exists() && c.exists());
    }
}

#[cfg(test)]
mod decode_tests {
    use super::*;

    #[test]
    fn decodes_png_bytes_to_rgba() {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([9, 8, 7, 255]))
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        let (w, h, px) = decode_rgba(&bytes, 512).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(&px[..4], &[9, 8, 7, 255]);
        assert!(decode_rgba(b"lixo", 512).is_none());
    }
    #[test]
    fn large_covers_are_shrunk() {
        let mut bytes = Vec::new();
        image::RgbaImage::new(600, 300).write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
        let (w, h, px) = decode_rgba(&bytes, 200).unwrap();
        assert_eq!((w, h), (200, 100));
        assert_eq!(px.len(), 200 * 100 * 4);
    }
}
