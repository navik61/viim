//! viim – průhledný prohlížeč obrázků pro niri (GTK4).
mod nacitani;
mod okno;
mod platno;
mod soubory;

use std::cell::Cell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;

use gtk::{gdk, gio, glib, prelude::*};

const NAPOVEDA: &str = "\
Průhledný prohlížeč obrázků (PNG, SVG, JPEG, WebP, GIF, BMP, TIFF) v okně bez rámečku.

Použití: viim [-t SEK] [-s SEK] [-H MAX_VYSKA] [-W MAX_SIRKA] [--app-id ID] [cesta …]

  cesta                 soubor nebo adresář; jeden soubor → n/b listuje obrázky v jeho adresáři;
                        bez cesty se otevře dialog pro výběr souboru
  -t, --timeout SEK     zavřít po SEK sekundách (výchozí 0 = nezavírat); jakákoli akce
                        uživatele (klávesa, scroll, tažení) automatické zavření zruší
  -s, --slideshow SEK   spustit slideshow s intervalem SEK (klávesa s ho zapíná
                        s intervalem 3 s, nebo s tímto)
  -H, --max-height PX   max. výška okna v logických px (výchozí 90 % nejnižšího monitoru)
  -W, --max-width PX    max. šířka okna v logických px (výchozí 90 % nejužšího monitoru)
      --app-id ID       app-id okna pro pravidla niri (výchozí viim)
  -h, --help            tato nápověda

Ovládání:
  scroll nahoru / dolů   zvětšit / zmenšit od místa pod kurzorem
  + / -                  zvětšit / zmenšit od středu       0  původní velikost
  šipky                  posun obrazu (se Shiftem větší krok)
  tažení myší            posun obrazu; když je vidět celý obraz, přesouvá okno
  n / b                  další / předchozí obrázek
  s                      slideshow zap / vyp (n/b odpočet vynuluje)
  o / O (Shift+o)        otevřít soubor / adresář
  q / Esc                zavřít

Proměnná VIIM_DEBUG=1 vypisuje polohu kurzoru při scrollu.";

const CSS: &str = "window, window.background { background: transparent; box-shadow: none; }";

struct Argumenty {
    cesty: Vec<PathBuf>,
    nast: okno::Nastaveni,
    app_id: String,
}

fn chyba_arg(zprava: &str) -> ! {
    eprintln!("viim: {zprava}\nZkus viim --help");
    std::process::exit(2);
}

fn argumenty() -> Argumenty {
    let mut a = Argumenty {
        cesty: Vec::new(),
        nast: okno::Nastaveni {
            timeout: 0.0,
            max_w: None,
            max_h: None,
            ladeni: std::env::var_os("VIIM_DEBUG").is_some_and(|v| !v.is_empty()),
            prezentace: 3.0,
            prezentace_hned: false,
        },
        app_id: "viim".into(),
    };
    let mut it = std::env::args_os().skip(1);
    let mut jen_cesty = false;
    while let Some(arg) = it.next() {
        let s = arg.to_string_lossy().into_owned();
        if jen_cesty || !s.starts_with('-') || s == "-" {
            a.cesty.push(PathBuf::from(arg));
            continue;
        }
        let (prepinac, vlozena) = match s.split_once('=') {
            Some((p, h)) if s.starts_with("--") => (p.to_string(), Some(h.to_string())),
            _ => (s.clone(), None),
        };
        let mut hodnota = || -> String {
            vlozena
                .clone()
                .or_else(|| it.next().map(|v| v.to_string_lossy().into_owned()))
                .unwrap_or_else(|| chyba_arg(&format!("{prepinac} potřebuje hodnotu")))
        };
        let cislo = |v: String| -> f64 {
            v.parse::<f64>()
                .ok()
                .filter(|x| *x >= 0.0)
                .unwrap_or_else(|| chyba_arg(&format!("{prepinac}: neplatné číslo „{v}“")))
        };
        match prepinac.as_str() {
            "-h" | "--help" => {
                println!("{NAPOVEDA}");
                std::process::exit(0);
            }
            "-t" | "--timeout" => a.nast.timeout = cislo(hodnota()),
            "-s" | "--slideshow" => {
                let v = cislo(hodnota());
                if v <= 0.0 {
                    chyba_arg(&format!("{prepinac}: interval musí být > 0"));
                }
                a.nast.prezentace = v;
                a.nast.prezentace_hned = true;
            }
            "-H" | "--max-height" => a.nast.max_h = Some(cislo(hodnota())).filter(|x| *x > 0.0),
            "-W" | "--max-width" => a.nast.max_w = Some(cislo(hodnota())).filter(|x| *x > 0.0),
            "--app-id" => a.app_id = hodnota(),
            "--" => jen_cesty = true,
            _ => chyba_arg(&format!("neznámý přepínač {s}")),
        }
    }
    a
}

fn main() -> ExitCode {
    let args = argumenty();

    // Vykreslovat jen na integrované AMD GPU: jinak Vulkan/EGL loader otevře i ovladač NVIDIA
    // a dGPU se nikdy neuspí. Musí být před inicializací GTK (a před vznikem vláken).
    for (k, v) in [
        ("VK_DRIVER_FILES", "/usr/share/vulkan/icd.d/radeon_icd.json"),
        (
            "__EGL_VENDOR_LIBRARY_FILENAMES",
            "/usr/share/glvnd/egl_vendor.d/50_mesa.json",
        ),
    ] {
        if std::env::var_os(k).is_none() && std::path::Path::new(v).exists() {
            // SAFETY: zatím běží jen jedno vlákno
            unsafe { std::env::set_var(k, v) };
        }
    }

    glib::set_prgname(Some(args.app_id.as_str())); // app-id pro niri window-rule
    let app = gtk::Application::builder()
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let chyba = Rc::new(Cell::new(false));

    let ch = chyba.clone();
    app.connect_activate(move |app| {
        let css = gtk::CssProvider::new();
        css.load_from_string(CSS);
        if let Some(displej) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &displej,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let p = okno::Prohlizec::new(app, args.nast.clone(), ch.clone());
        if args.cesty.is_empty() {
            p.dialog(false);
        } else {
            p.otevri(&args.cesty);
        }
    });

    let rc = app.run_with_args::<&str>(&[]);
    if chyba.get() || rc != glib::ExitCode::SUCCESS {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
