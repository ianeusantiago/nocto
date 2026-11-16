//! Remove o fundo de uma imagem, 100% local e grátis: rede U²-Net (modelo "silueta",
//! ~44 MB, licença Apache-2.0) rodando em Rust puro (tract), sem DLL nem GPU.
//! O modelo é baixado uma vez, no primeiro uso, e só é carregado quando você pede:
//! com a ilha parada, isso não custa nada.

use image::{GenericImageView, Rgba, RgbaImage, imageops::FilterType};
use std::path::{Path, PathBuf};
use tract_onnx::prelude::*;

const MODEL_URL: &str = "https://github.com/danielgatis/rembg/releases/download/v0.0.0/silueta.onnx";
const MODEL_FILE: &str = "silueta.onnx";
const MODEL_BYTES: u64 = 44_173_029;
const SIDE: usize = 320;

/// Caminho do modelo na pasta de dados; baixa se ainda não estiver lá.
pub fn ensure_model(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join(MODEL_FILE);
    if std::fs::metadata(&path).map(|m| m.len() == MODEL_BYTES).unwrap_or(false) {
        return Ok(path);
    }
    let part = dir.join(format!("{MODEL_FILE}.part"));
    let mut body = ureq::get(MODEL_URL)
        .call()
        .map_err(|e| format!("não consegui baixar o modelo: {e}"))?
        .into_body();
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    std::io::copy(&mut body.as_reader(), &mut file).map_err(|e| format!("download interrompido: {e}"))?;
    drop(file);
    if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) != MODEL_BYTES {
        let _ = std::fs::remove_file(&part);
        return Err("download do modelo veio incompleto".into());
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Gera `nome-sem-fundo.png` ao lado do original (sem sobrescrever nada).
pub fn remove_background(src: &Path, model: &Path) -> Result<PathBuf, String> {
    let img = image::open(src).map_err(|e| format!("não é uma imagem que eu consiga abrir: {e}"))?;
    let (w, h) = img.dimensions();
    let mask = predict(&img, model)?;
    let mask = image::imageops::resize(&mask, w, h, FilterType::Triangle);
    let rgba = img.to_rgba8();
    let out_img = RgbaImage::from_fn(w, h, |x, y| {
        let p = rgba.get_pixel(x, y);
        let m = mask.get_pixel(x, y)[0] as u32;
        Rgba([p[0], p[1], p[2], (p[3] as u32 * m / 255) as u8])
    });
    let out = free_path(src);
    out_img.save_with_format(&out, image::ImageFormat::Png).map_err(|e| format!("falha ao gravar: {e}"))?;
    Ok(out)
}

/// Máscara 320×320 (0 = fundo, 255 = objeto).
fn predict(img: &image::DynamicImage, model: &Path) -> Result<image::GrayImage, String> {
    let err = |e: TractError| format!("falha no modelo: {e}");
    let plan = tract_onnx::onnx()
        .model_for_path(model)
        .map_err(err)?
        .with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into())
        .map_err(err)?
        .into_optimized()
        .map_err(err)?
        .into_runnable()
        .map_err(err)?;

    // Mesmo pré-processamento do rembg: escala por max, depois média/desvio do ImageNet.
    let small = img.resize_exact(SIDE as u32, SIDE as u32, FilterType::Lanczos3).to_rgb8();
    let max = small.pixels().flat_map(|p| p.0).max().unwrap_or(255).max(1) as f32;
    let mean = [0.485, 0.456, 0.406];
    let std = [0.229, 0.224, 0.225];
    let input: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, SIDE, SIDE), |(_, c, y, x)| {
        (small.get_pixel(x as u32, y as u32)[c] as f32 / max - mean[c]) / std[c]
    })
    .into();
    // Usa todos os núcleos só durante esse cálculo (a ilha parada continua sem custo).
    use tract_linalg::multithread::{Executor, multithread_tract_scope};
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let out = multithread_tract_scope(Executor::multithread(threads), || plan.run(tvec!(input.into()))).map_err(err)?;
    let view = out[0].to_plain_array_view::<f32>().map_err(err)?;
    let vals: Vec<f32> = view.iter().copied().take(SIDE * SIDE).collect();
    let (lo, hi) = vals.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let span = (hi - lo).max(1e-6);
    let px: Vec<u8> = vals.iter().map(|v| (((v - lo) / span) * 255.0).round() as u8).collect();
    image::GrayImage::from_raw(SIDE as u32, SIDE as u32, px).ok_or_else(|| "saída do modelo inesperada".into())
}

fn free_path(src: &Path) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new("."));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("imagem");
    let mut out = dir.join(format!("{stem}-sem-fundo.png"));
    let mut n = 1;
    while out.exists() {
        out = dir.join(format!("{stem}-sem-fundo-{n}.png"));
        n += 1;
    }
    out
}
