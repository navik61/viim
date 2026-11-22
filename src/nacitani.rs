//! Dekódování obrázků (běží ve vláknech na pozadí – nesmí sahat na GTK).
use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::soubory::je_svg;

/// Jak velké má být SVG vykreslení (rastrové obrázky se dekódují vždy v plném rozlišení).
#[derive(Clone, Copy, Debug)]
pub enum Mira {
    /// Vejít se do okna max_w × max_h (logické px), vykreslit `ostrost`× větší.
    Okno {
        max_w: f64,
        max_h: f64,
        ostrost: f64,
    },
    /// Pevné měřítko: px textury na px obrázku.
    Meritko(f64),
}

#[derive(Clone, Copy, Debug)]
pub enum Format {
    Rgba,
    Rgb,
    RgbaPremult,
}

/// Výsledek dekódování: surová data pro `gdk::MemoryTexture`.
pub struct Dekodovano {
    /// Vlastní (intrinsic) rozměry obrázku – v nich se počítá zoom a velikost okna.
    pub w: f64,
    pub h: f64,
    pub tw: u32,
    pub th: u32,
    pub stride: usize,
    pub format: Format,
    pub data: Vec<u8>,
    /// U SVG rozlišení textury (px textury na px obrázku), jinak None.
    pub svg_meritko: Option<f64>,
}

/// Strop velikosti SVG textury (paměť GPU).
const SVG_MAX_STRANA: f64 = 8192.0;

pub fn dekoduj(cesta: &Path, mira: Mira) -> Result<Dekodovano, String> {
    if je_svg(cesta) {
        svg(cesta, mira)
    } else {
        rastr(cesta)
    }
}

fn rastr(cesta: &Path) -> Result<Dekodovano, String> {
    let obr = image::ImageReader::open(cesta)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let (w, h) = (obr.width(), obr.height());
    let (format, data, bpp) = if obr.color().has_alpha() {
        (Format::Rgba, obr.into_rgba8().into_raw(), 4)
    } else {
        (Format::Rgb, obr.into_rgb8().into_raw(), 3)
    };
    Ok(Dekodovano {
        w: w as f64,
        h: h as f64,
        tw: w,
        th: h,
        stride: w as usize * bpp,
        format,
        data,
        svg_meritko: None,
    })
}

/// Systémová písma pro text v SVG – načtou se jednou (trvá to desítky ms).
fn pisma() -> Arc<resvg::usvg::fontdb::Database> {
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

fn svg(cesta: &Path, mira: Mira) -> Result<Dekodovano, String> {
    let data = std::fs::read(cesta).map_err(|e| e.to_string())?;
    let mut opt = resvg::usvg::Options {
        resources_dir: cesta.parent().map(|p| p.to_path_buf()),
        ..Default::default()
    };
    if data.windows(5).any(|o| o == b"<text") {
        opt.fontdb = pisma();
    }
    // from_data rozbalí i gzip (svgz)
    let strom = resvg::usvg::Tree::from_data(&data, &opt).map_err(|e| e.to_string())?;
    let (w, h) = (strom.size().width() as f64, strom.size().height() as f64);
    let meritko = match mira {
        Mira::Okno {
            max_w,
            max_h,
            ostrost,
        } => (max_w / w).min(max_h / h).min(1.0) * ostrost,
        Mira::Meritko(m) => m,
    }
    .min(SVG_MAX_STRANA / w.max(h));
    let tw = ((w * meritko).round() as u32).max(1);
    let th = ((h * meritko).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(tw, th).ok_or("SVG je příliš velké")?;
    let transform =
        resvg::tiny_skia::Transform::from_scale(tw as f32 / w as f32, th as f32 / h as f32);
    resvg::render(&strom, transform, &mut pixmap.as_mut());
    Ok(Dekodovano {
        w,
        h,
        tw,
        th,
        stride: tw as usize * 4,
        format: Format::RgbaPremult,
        data: pixmap.take(),
        svg_meritko: Some(meritko),
    })
}
