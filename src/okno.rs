//! Okno prohlížeče: načítání na pozadí, cache, přepínání, ovládání, dialog pro otevření.
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use gtk::{gdk, gio, glib, graphene, prelude::*};

use crate::nacitani::{self, Dekodovano, Format, Mira};
use crate::platno::Platno;
use crate::soubory;

/// SVG se vykresluje min. ve 2× rozlišení → ostré i na monitoru se škálou 1,5/2.
const OSTROST: f64 = 2.0;
/// Násobek zoomu na jeden „cvak“ kolečka.
const KROK_ZOOM: f64 = 1.2;
/// Posun šipkou = 10 % okna (se Shiftem 50 %).
const KROK_POSUN: f64 = 0.1;
/// Kolik monitoru smí okno zabrat.
const PODIL: f64 = 0.9;
/// Paměťový strop cache dekódovaných obrázků.
const CACHE_BAJTY: usize = 768 * 1024 * 1024;

#[derive(Clone)]
pub struct Nastaveni {
    pub timeout: f64,
    pub max_w: Option<f64>,
    pub max_h: Option<f64>,
    pub ladeni: bool,
    /// Interval slideshow v sekundách.
    pub prezentace: f64,
    /// Spustit slideshow hned po zobrazení prvního obrázku.
    pub prezentace_hned: bool,
}

struct Polozka {
    index: usize,
    textura: gdk::Texture,
    w: f64,
    h: f64,
    svg_meritko: Option<f64>,
    bajty: usize,
}

/// Jednoduchá LRU cache textur s paměťovým stropem.
#[derive(Default)]
struct Cache {
    polozky: Vec<Polozka>, // od nejstarší po nejnovější
}

impl Cache {
    fn najdi(&mut self, index: usize) -> Option<&Polozka> {
        let pos = self.polozky.iter().position(|p| p.index == index)?;
        let p = self.polozky.remove(pos);
        self.polozky.push(p);
        self.polozky.last()
    }

    fn ma(&self, index: usize) -> bool {
        self.polozky.iter().any(|p| p.index == index)
    }

    fn vloz(&mut self, polozka: Polozka, aktualni: usize) {
        self.polozky.retain(|p| p.index != polozka.index);
        self.polozky.push(polozka);
        while self.polozky.iter().map(|p| p.bajty).sum::<usize>() > CACHE_BAJTY {
            match self.polozky.iter().position(|p| p.index != aktualni) {
                Some(pos) if self.polozky.len() > 1 => {
                    self.polozky.remove(pos);
                }
                _ => break,
            }
        }
    }
}

struct Vysledek {
    generace: u64,
    index: usize,
    ostreni: bool,
    vysledek: Result<Dekodovano, String>,
}

pub struct Prohlizec {
    app: gtk::Application,
    okno: gtk::ApplicationWindow,
    platno: Platno,
    nast: Nastaveni,
    max_w: f64,
    max_h: f64,
    soubory: RefCell<Vec<PathBuf>>,
    i: Cell<usize>,
    smer: Cell<isize>,
    generace: Cell<u64>,
    cache: RefCell<Cache>,
    bezi: RefCell<HashSet<(usize, bool)>>,
    chybne: RefCell<HashSet<usize>>,
    tx: async_channel::Sender<Vysledek>,
    zobrazen: Cell<Option<usize>>,
    prezentovano: Cell<bool>,
    /// Drží aplikaci naživu, než se otevře okno (načítání / dialog běží asynchronně).
    drzak: RefCell<Option<gio::ApplicationHoldGuard>>,
    velikost: Cell<Option<(i32, i32)>>,
    niri_id: Cell<Option<u64>>,
    zbytek: Cell<(f64, f64)>,
    mys: Cell<Option<(f64, f64)>>,
    tah: Cell<Option<(f64, f64)>>,
    casovac: RefCell<Option<glib::SourceId>>,
    svg_casovac: RefCell<Option<glib::SourceId>>,
    prezentace: Cell<bool>,
    prez_casovac: RefCell<Option<glib::SourceId>>,
    pub chyba: Rc<Cell<bool>>,
}

/// Nejmenší logická šířka a výška ze všech připojených monitorů.
fn limity_monitoru() -> (f64, f64) {
    let Some(displej) = gdk::Display::default() else {
        return (1920.0, 1080.0);
    };
    let monitory = displej.monitors();
    let (mut w, mut h) = (f64::MAX, f64::MAX);
    for i in 0..monitory.n_items() {
        if let Some(m) = monitory.item(i).and_downcast::<gdk::Monitor>() {
            let g = m.geometry();
            w = w.min(g.width() as f64);
            h = h.min(g.height() as f64);
        }
    }
    if w == f64::MAX {
        (1920.0, 1080.0)
    } else {
        (w, h)
    }
}

fn textura(d: Dekodovano) -> gdk::Texture {
    let format = match d.format {
        Format::Rgba => gdk::MemoryFormat::R8g8b8a8,
        Format::Rgb => gdk::MemoryFormat::R8g8b8,
        Format::RgbaPremult => gdk::MemoryFormat::R8g8b8a8Premultiplied,
    };
    let bytes = glib::Bytes::from_owned(d.data);
    gdk::MemoryTexture::new(d.tw as i32, d.th as i32, format, &bytes, d.stride).upcast()
}

impl Prohlizec {
    pub fn new(app: &gtk::Application, nast: Nastaveni, chyba: Rc<Cell<bool>>) -> Rc<Self> {
        let okno = gtk::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .resizable(false) // pevná velikost → niri okno otevře jako plovoucí
            .build();
        let platno = Platno::default();
        okno.set_child(Some(&platno));

        let (mon_w, mon_h) = limity_monitoru();
        let prezentace_hned = nast.prezentace_hned;
        let (tx, rx) = async_channel::unbounded();
        let p = Rc::new(Self {
            app: app.clone(),
            okno,
            platno,
            max_w: nast.max_w.unwrap_or(mon_w * PODIL),
            max_h: nast.max_h.unwrap_or(mon_h * PODIL),
            nast,
            soubory: RefCell::default(),
            i: Cell::new(0),
            smer: Cell::new(1),
            generace: Cell::new(0),
            cache: RefCell::default(),
            bezi: RefCell::default(),
            chybne: RefCell::default(),
            tx,
            zobrazen: Cell::new(None),
            prezentovano: Cell::new(false),
            drzak: RefCell::new(Some(app.hold())),
            velikost: Cell::new(None),
            niri_id: Cell::new(None),
            zbytek: Cell::new((0.0, 0.0)),
            mys: Cell::new(None),
            tah: Cell::new(None),
            casovac: RefCell::default(),
            svg_casovac: RefCell::default(),
            prezentace: Cell::new(prezentace_hned),
            prez_casovac: RefCell::default(),
            chyba,
        });

        // výsledky z vláken dekodéru
        let pp = p.clone();
        glib::spawn_future_local(async move {
            while let Ok(v) = rx.recv().await {
                pp.prijmi(v);
            }
        });
        p.ovladani();
        p
    }

    // ---------- seznam souborů ----------

    /// Otevře soubory/adresáře (z příkazové řádky nebo z dialogu).
    pub fn otevri(self: &Rc<Self>, cesty: &[PathBuf]) {
        let (seznam, index) = soubory::seznam(cesty);
        if seznam.is_empty() {
            eprintln!("viim: žádné obrázky k zobrazení");
            if !self.prezentovano.get() {
                self.konec_s_chybou();
            }
            return;
        }
        self.generace.set(self.generace.get() + 1);
        *self.soubory.borrow_mut() = seznam;
        self.cache.borrow_mut().polozky.clear();
        self.bezi.borrow_mut().clear();
        self.chybne.borrow_mut().clear();
        self.zobrazen.set(None);
        self.i.set(index);
        self.smer.set(1);
        self.nacti_aktualni();
    }

    fn pocet(&self) -> usize {
        self.soubory.borrow().len()
    }

    fn cesta(&self, i: usize) -> PathBuf {
        self.soubory.borrow()[i].clone()
    }

    fn konec_s_chybou(&self) {
        self.chyba.set(true);
        self.drzak.borrow_mut().take();
        self.app.quit();
    }

    // ---------- načítání ----------

    fn nacti_aktualni(self: &Rc<Self>) {
        let i = self.i.get();
        if self.cache.borrow().ma(i) {
            self.zobraz(i);
        } else {
            self.pozadej(i, false, self.mira_okno());
        }
    }

    fn mira_okno(&self) -> Mira {
        Mira::Okno {
            max_w: self.max_w,
            max_h: self.max_h,
            ostrost: OSTROST,
        }
    }

    /// Spustí dekódování ve vlákně (pokud už neběží).
    fn pozadej(&self, i: usize, ostreni: bool, mira: Mira) {
        if !self.bezi.borrow_mut().insert((i, ostreni)) {
            return;
        }
        let (cesta, tx, generace) = (self.cesta(i), self.tx.clone(), self.generace.get());
        std::thread::spawn(move || {
            let vysledek = nacitani::dekoduj(&cesta, mira);
            let _ = tx.send_blocking(Vysledek {
                generace,
                index: i,
                ostreni,
                vysledek,
            });
        });
    }

    fn prijmi(self: &Rc<Self>, v: Vysledek) {
        if v.generace != self.generace.get() {
            return; // mezitím otevřen jiný seznam
        }
        self.bezi.borrow_mut().remove(&(v.index, v.ostreni));
        let aktualni = self.i.get();
        match v.vysledek {
            Err(e) => {
                eprintln!("viim: {}: {e}", self.cesta(v.index).display());
                if !v.ostreni {
                    self.chybne.borrow_mut().insert(v.index);
                    if v.index == aktualni {
                        self.krok(self.smer.get());
                    }
                }
            }
            Ok(d) => {
                if v.ostreni {
                    // ostřejší SVG: jen pokud je opravdu lepší než to, co už je v cache
                    let lepsi = d.svg_meritko.unwrap_or(0.0)
                        > self
                            .cache
                            .borrow()
                            .polozky
                            .iter()
                            .find(|p| p.index == v.index)
                            .and_then(|p| p.svg_meritko)
                            .unwrap_or(0.0);
                    if !lepsi {
                        return;
                    }
                }
                let (w, h, svg_meritko) = (d.w, d.h, d.svg_meritko);
                let bajty = d.data.len();
                let tex = textura(d);
                self.cache.borrow_mut().vloz(
                    Polozka {
                        index: v.index,
                        textura: tex.clone(),
                        w,
                        h,
                        svg_meritko,
                        bajty,
                    },
                    aktualni,
                );
                if v.index == aktualni {
                    if v.ostreni {
                        if self.zobrazen.get() == Some(v.index) {
                            self.platno.vymen_texturu(tex);
                        }
                    } else if self.zobrazen.get() != Some(v.index) {
                        self.zobraz(v.index);
                    }
                }
            }
        }
    }

    /// Přepnout o `smer` (±1); nečitelné soubory přeskočí.
    fn krok(self: &Rc<Self>, smer: isize) {
        let n = self.pocet();
        if n == 0 {
            return;
        }
        if self.chybne.borrow().len() >= n {
            eprintln!("viim: žádný obrázek se nepodařilo načíst");
            if !self.prezentovano.get() {
                self.konec_s_chybou();
            }
            return;
        }
        self.smer.set(smer);
        let mut i = self.i.get();
        loop {
            i = (i as isize + smer).rem_euclid(n as isize) as usize;
            if !self.chybne.borrow().contains(&i) {
                break;
            }
        }
        self.i.set(i);
        self.nacti_aktualni();
    }

    fn zobraz(self: &Rc<Self>, i: usize) {
        let Some((tex, w, h)) = self
            .cache
            .borrow_mut()
            .najdi(i)
            .map(|p| (p.textura.clone(), p.w, p.h))
        else {
            return;
        };
        self.platno.nastav(tex, w, h);
        self.zobrazen.set(Some(i));

        // pevná velikost okna = obrázek zmenšený do limitu (nikdy nezvětšovat)
        let m = (self.max_w / w).min(self.max_h / h).min(1.0);
        let cil = (
            ((w * m).round() as i32).max(1),
            ((h * m).round() as i32).max(1),
        );
        self.platno.set_size_request(cil.0, cil.1);
        if let Some(stara) = self.velikost.get()
            && stara != cil
            && self.prezentovano.get()
        {
            self.drz_stred(cil.0 - stara.0, cil.1 - stara.1);
        }
        self.velikost.set(Some(cil));

        let n = self.pocet();
        let jmeno = self
            .cesta(i)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let titulek = if n > 1 {
            format!("{jmeno} ({}/{n})", i + 1)
        } else {
            jmeno
        };
        self.okno.set_title(Some(&titulek));

        if !self.prezentovano.get() {
            self.prezentovano.set(true);
            self.okno.present();
            self.drzak.borrow_mut().take(); // teď aplikaci drží okno
            self.spust_casovac();
            self.spust_skript();
        }
        self.naplanuj_dalsi();

        // načíst sousedy předem → přepnutí je okamžité
        if n > 1 {
            for j in [(i + 1) % n, (i + n - 1) % n] {
                if !self.cache.borrow().ma(j) && !self.chybne.borrow().contains(&j) {
                    self.pozadej(j, false, self.mira_okno());
                }
            }
        }
    }

    /// Po zoomu SVG přerenderovat ve vyšším rozlišení (se zpožděním, ať se nerenderuje
    /// při každém cvaku kolečka).
    fn svg_doostrit(self: &Rc<Self>) {
        let i = self.i.get();
        if self.zobrazen.get() != Some(i) {
            return;
        }
        let Some(ma) = self
            .cache
            .borrow()
            .polozky
            .iter()
            .find(|p| p.index == i)
            .and_then(|p| p.svg_meritko)
        else {
            return;
        };
        let skala = self.okno.surface().map(|s| s.scale()).unwrap_or(1.0);
        let potreba = self.platno.meritko() * skala;
        if potreba <= ma * 1.05 {
            return;
        }
        if let Some(id) = self.svg_casovac.borrow_mut().take() {
            id.remove();
        }
        let p = self.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(150), move || {
            p.svg_casovac.borrow_mut().take();
            if p.i.get() == i {
                p.pozadej(i, true, Mira::Meritko(potreba));
            }
        });
        *self.svg_casovac.borrow_mut() = Some(id);
    }

    // ---------- slideshow ----------

    /// Zapnout / vypnout slideshow.
    fn prepni_prezentaci(self: &Rc<Self>) {
        let zapnout = !self.prezentace.get();
        self.prezentace.set(zapnout);
        if zapnout {
            self.naplanuj_dalsi();
        } else if let Some(id) = self.prez_casovac.borrow_mut().take() {
            id.remove();
        }
    }

    /// Další obrázek za interval. Odpočet začíná až po zobrazení obrázku, takže pomalé
    /// dekódování nic nepřeskočí a ruční n/b odpočet vynuluje.
    fn naplanuj_dalsi(self: &Rc<Self>) {
        if let Some(id) = self.prez_casovac.borrow_mut().take() {
            id.remove();
        }
        if !self.prezentace.get() || self.pocet() < 2 || self.zobrazen.get().is_none() {
            return;
        }
        let p = self.clone();
        let id = glib::timeout_add_local_once(
            Duration::from_secs_f64(self.nast.prezentace),
            move || {
                p.prez_casovac.borrow_mut().take();
                if p.prezentace.get() {
                    p.krok(1);
                }
            },
        );
        *self.prez_casovac.borrow_mut() = Some(id);
    }

    // ---------- automatické zavření ----------

    fn spust_casovac(self: &Rc<Self>) {
        if self.nast.timeout <= 0.0 {
            return;
        }
        let p = self.clone();
        let id =
            glib::timeout_add_local_once(Duration::from_secs_f64(self.nast.timeout), move || {
                p.casovac.borrow_mut().take();
                p.okno.close();
            });
        *self.casovac.borrow_mut() = Some(id);
    }

    /// Uživatel s oknem pracuje → nezavírat ho samo.
    fn aktivita(&self) {
        if let Some(id) = self.casovac.borrow_mut().take() {
            id.remove();
        }
    }

    // ---------- niri: držet střed okna při změně velikosti ----------

    /// niri při změně velikosti plovoucího okna drží levý horní roh → posunout okno
    /// o polovinu rozdílu, aby zůstal na místě střed (Wayland aplikace se samy posouvat
    /// nemůžou, jde to jen přes IPC niri).
    fn drz_stred(&self, dw: i32, dh: i32) {
        if std::env::var_os("NIRI_SOCKET").is_none() {
            return;
        }
        if self.niri_id.get().is_none() {
            self.niri_id.set(niri_id_okna());
        }
        let Some(id) = self.niri_id.get() else { return };
        let (zx, zy) = self.zbytek.get();
        let (dx, dy) = (-dw as f64 / 2.0 + zx, -dh as f64 / 2.0 + zy);
        let (ix, iy) = (dx.round(), dy.round());
        self.zbytek.set((dx - ix, dy - iy));
        let args = [
            "msg".to_string(),
            "action".into(),
            "move-floating-window".into(),
            "--id".into(),
            id.to_string(),
            format!("--x={:+}", ix as i64),
            format!("--y={:+}", iy as i64),
        ];
        std::thread::spawn(move || {
            let _ = Command::new("niri")
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        });
    }

    // ---------- ovládání ----------

    fn ovladani(self: &Rc<Self>) {
        let klavesy = gtk::EventControllerKey::new();
        let p = self.clone();
        klavesy.connect_key_pressed(move |_, klavesa, _, stav| p.klavesa(klavesa, stav));
        self.okno.add_controller(klavesy);

        // poslední známá poloha kurzoru; při „leave“ se NEmaže – démon Kurzor při každém
        // dotyku touchpadu na okamžik převezme ukazatel (overlay sonda) → falešný leave
        let pohyb = gtk::EventControllerMotion::new();
        let p = self.clone();
        pohyb.connect_motion(move |_, x, y| p.mys.set(Some((x, y))));
        let p = self.clone();
        pohyb.connect_enter(move |_, x, y| p.mys.set(Some((x, y))));
        self.platno.add_controller(pohyb);

        let kolecko = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let p = self.clone();
        kolecko.connect_scroll(move |ctl, _, dy| {
            p.aktivita();
            let dy = if ctl.unit() == gdk::ScrollUnit::Surface {
                dy / 20.0
            } else {
                dy
            }; // touchpad
            let mys = p.kurzor(ctl);
            p.platno.zoomuj(KROK_ZOOM.powf(-dy), mys); // nahoru (dy < 0) = zvětšit
            p.svg_doostrit();
            glib::Propagation::Stop
        });
        self.platno.add_controller(kolecko);

        let tazeni = gtk::GestureDrag::new();
        let p = self.clone();
        tazeni.connect_drag_begin(move |g, x, y| {
            p.aktivita();
            if p.platno.vejde_se() {
                // není co posouvat → tažením se přesouvá okno
                p.tah.set(None);
                if let (Some(povrch), Some(zarizeni)) = (p.okno.surface(), g.device())
                    && let Some(top) = povrch.dynamic_cast_ref::<gdk::Toplevel>()
                {
                    top.begin_move(
                        &zarizeni,
                        g.current_button() as i32,
                        x,
                        y,
                        g.current_event_time(),
                    );
                }
                g.set_state(gtk::EventSequenceState::Denied);
            } else {
                p.tah.set(Some(p.platno.stred()));
            }
        });
        let p = self.clone();
        tazeni.connect_drag_update(move |_, dx, dy| {
            if let Some(start) = p.tah.get() {
                p.platno.nastav_stred(start);
                p.platno.posun(-dx, -dy);
            }
        });
        self.platno.add_controller(tazeni);
    }

    /// Poloha kurzoru v souřadnicích plátna: 1) z události scrollu, 2) dotaz na polohu
    /// ukazatele nad oknem, 3) poslední motion/enter; nic → None (zoom od středu).
    fn kurzor(&self, ctl: &gtk::EventControllerScroll) -> Option<(f64, f64)> {
        let nativ = self.platno.native()?;
        let (tx, ty) = nativ.surface_transform();
        let na_platno = |x: f64, y: f64| {
            nativ
                .compute_point(
                    &self.platno,
                    &graphene::Point::new((x - tx) as f32, (y - ty) as f32),
                )
                .map(|b| (b.x() as f64, b.y() as f64))
        };
        let z_udalosti = ctl
            .current_event()
            .and_then(|e| e.position())
            .and_then(|(x, y)| na_platno(x, y));
        let z_dotazu = self.okno.surface().and_then(|povrch| {
            let mys = povrch.display().default_seat()?.pointer()?;
            let (x, y, _) = povrch.device_position(&mys)?;
            na_platno(x, y)
        });
        if self.nast.ladeni {
            eprintln!(
                "scroll: událost={z_udalosti:?} dotaz={z_dotazu:?} motion={:?}",
                self.mys.get()
            );
        }
        z_udalosti.or(z_dotazu).or(self.mys.get())
    }

    fn klavesa(self: &Rc<Self>, klavesa: gdk::Key, stav: gdk::ModifierType) -> glib::Propagation {
        use gdk::Key;
        self.aktivita();
        let p = &self.platno;
        let krok = if stav.contains(gdk::ModifierType::SHIFT_MASK) {
            0.5
        } else {
            KROK_POSUN
        };
        let (sirka, vyska) = (p.width() as f64, p.height() as f64);
        match klavesa {
            Key::q | Key::Q | Key::Escape => self.okno.close(),
            Key::n | Key::N => self.krok(1),
            Key::b | Key::B => self.krok(-1),
            Key::s | Key::S => self.prepni_prezentaci(),
            Key::o => self.dialog(false),
            Key::O => self.dialog(true),
            Key::Left => p.posun(-sirka * krok, 0.0),
            Key::Right => p.posun(sirka * krok, 0.0),
            Key::Up => p.posun(0.0, -vyska * krok),
            Key::Down => p.posun(0.0, vyska * krok),
            Key::equal | Key::plus | Key::KP_Add => {
                p.zoomuj(KROK_ZOOM, None);
                self.svg_doostrit();
            }
            Key::minus | Key::KP_Subtract => p.zoomuj(1.0 / KROK_ZOOM, None),
            Key::_0 | Key::KP_0 => p.reset(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// Testovací háček: `VIIM_SKRIPT="n b n q"` po zobrazení postupně (á 700 ms)
    /// „stiskne“ zadané klávesy. Slouží k automatickému testu bez klávesnice.
    pub fn spust_skript(self: &Rc<Self>) {
        let Ok(skript) = std::env::var("VIIM_SKRIPT") else {
            return;
        };
        let klavesy: Vec<gdk::Key> = skript
            .split_whitespace()
            .filter_map(gdk::Key::from_name)
            .collect();
        for (k, klavesa) in klavesy.into_iter().enumerate() {
            let p = self.clone();
            glib::timeout_add_local_once(Duration::from_millis(700 * (k as u64 + 1)), move || {
                p.klavesa(klavesa, gdk::ModifierType::empty());
                if let Some((w, h)) = p.velikost.get() {
                    eprintln!(
                        "skript: {:?} → {} {w}x{h}",
                        klavesa.name(),
                        p.okno.title().unwrap_or_default()
                    );
                }
            });
        }
    }

    // ---------- dialog pro otevření ----------

    /// Dialog pro výběr souboru (`adresar = false`) nebo adresáře (`true`).
    pub fn dialog(self: &Rc<Self>, adresar: bool) {
        let dialog = gtk::FileDialog::builder()
            .title(if adresar {
                "Otevřít adresář s obrázky"
            } else {
                "Otevřít obrázek"
            })
            .modal(true)
            .build();
        let filtr = gtk::FileFilter::new();
        filtr.set_name(Some("Obrázky"));
        for pripona in soubory::PRIPONY {
            filtr.add_suffix(pripona);
        }
        let filtry = gio::ListStore::new::<gtk::FileFilter>();
        filtry.append(&filtr);
        dialog.set_filters(Some(&filtry));
        dialog.set_default_filter(Some(&filtr));
        let pocatek = if self.pocet() > 0 {
            self.cesta(self.i.get()).parent().map(Path::to_path_buf)
        } else {
            std::env::current_dir().ok()
        };
        if let Some(adr) = pocatek {
            dialog.set_initial_folder(Some(&gio::File::for_path(adr)));
        }

        let rodic = self.prezentovano.get().then(|| self.okno.clone());
        let p = self.clone();
        let hotovo = move |vysledek: Result<gio::File, glib::Error>| match vysledek
            .ok()
            .and_then(|f| f.path())
        {
            Some(cesta) => p.otevri(&[cesta]),
            None if !p.prezentovano.get() => {
                p.drzak.borrow_mut().take(); // zrušeno při startu bez souboru → konec
                p.app.quit();
            }
            None => {}
        };
        if adresar {
            dialog.select_folder(rodic.as_ref(), gio::Cancellable::NONE, hotovo);
        } else {
            dialog.open(rodic.as_ref(), gio::Cancellable::NONE, hotovo);
        }
    }
}

/// Id našeho okna v niri (podle PID).
fn niri_id_okna() -> Option<u64> {
    let vystup = Command::new("niri")
        .args(["msg", "-j", "windows"])
        .output()
        .ok()?;
    let okna: Vec<serde_json::Value> = serde_json::from_slice(&vystup.stdout).ok()?;
    let pid = std::process::id() as u64;
    okna.iter().find(|o| o["pid"].as_u64() == Some(pid))?["id"].as_u64()
}
