//! Rozbalení argumentů (soubory / adresáře) na seznam obrázků.
use std::fs;
use std::path::{Path, PathBuf};

/// Přípony, které umí načíst `nacitani::dekoduj`.
pub const PRIPONY: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "webp", "gif", "bmp", "tif", "tiff", "svg", "svgz",
];

pub fn je_obrazek(cesta: &Path) -> bool {
    cesta.is_file()
        && cesta
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| PRIPONY.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn je_svg(cesta: &Path) -> bool {
    cesta
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "svg" | "svgz"))
}

/// Obrázky v adresáři seřazené podle jména (bez ohledu na velikost písmen).
fn v_adresari(adresar: &Path) -> Vec<PathBuf> {
    let mut vse: Vec<PathBuf> = match fs::read_dir(adresar) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| je_obrazek(p))
            .collect(),
        Err(e) => {
            eprintln!("viim: {}: {e}", adresar.display());
            Vec::new()
        }
    };
    vse.sort_by_key(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    vse
}

fn absolutni(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Seznam obrázků a index, kterým začít. Jeden soubor → listuje se celým jeho adresářem.
pub fn seznam(cesty: &[PathBuf]) -> (Vec<PathBuf>, usize) {
    if let [jediny] = cesty
        && jediny.is_file()
    {
        let soubor = absolutni(jediny);
        if let Some(adresar) = soubor.parent() {
            let vse = v_adresari(adresar);
            if let Some(i) = vse.iter().position(|p| *p == soubor) {
                return (vse, i);
            }
        }
        return (vec![soubor], 0);
    }
    let mut vse = Vec::new();
    for c in cesty {
        if c.is_dir() {
            vse.extend(v_adresari(&absolutni(c)));
        } else if c.is_file() {
            vse.push(absolutni(c));
        } else {
            eprintln!("viim: neexistuje: {}", c.display());
        }
    }
    (vse, 0)
}
