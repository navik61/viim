//! Widget, který vykreslí texturu se zoomem a posunem (na GPU, filtr s mipmapami).
//!
//! Stav pohledu nezávisí na velikosti okna: `zoom` (1 = obrázek přesně vyplní okno)
//! a bod obrázku (`cx`, `cy`), který je ve středu okna.
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, prelude::*};

pub const ZOOM_MIN: f64 = 0.1;
pub const ZOOM_MAX: f64 = 100.0;

#[derive(Default)]
pub struct Pohled {
    pub textura: Option<gdk::Texture>,
    pub w: f64,
    pub h: f64,
    pub zoom: f64,
    pub cx: f64,
    pub cy: f64,
}

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct Platno {
        pub pohled: RefCell<Pohled>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Platno {
        const NAME: &'static str = "ViimPlatno";
        type Type = super::Platno;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Platno {}

    impl WidgetImpl for Platno {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let s = obj.meritko();
            let p = self.pohled.borrow();
            let Some(tex) = &p.textura else { return };
            let (sirka, vyska) = (obj.width() as f64, obj.height() as f64);
            let obdelnik = graphene::Rect::new(
                (sirka / 2.0 - p.cx * s) as f32,
                (vyska / 2.0 - p.cy * s) as f32,
                (p.w * s) as f32,
                (p.h * s) as f32,
            );
            // TRILINEAR = mipmapy → i silně zmenšené velké obrázky jsou hladké
            snapshot.append_scaled_texture(tex, gsk::ScalingFilter::Trilinear, &obdelnik);
        }
    }
}

glib::wrapper! {
    pub struct Platno(ObjectSubclass<imp::Platno>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Platno {
    fn default() -> Self {
        let p: Self = glib::Object::new();
        p.set_overflow(gtk::Overflow::Hidden);
        p
    }
}

impl Platno {
    fn pohled(&self) -> std::cell::RefMut<'_, Pohled> {
        self.imp().pohled.borrow_mut()
    }

    /// Nový obrázek, pohled na výchozí (celý obrázek).
    pub fn nastav(&self, textura: gdk::Texture, w: f64, h: f64) {
        *self.pohled() = Pohled {
            textura: Some(textura),
            w,
            h,
            zoom: 1.0,
            cx: w / 2.0,
            cy: h / 2.0,
        };
        self.queue_draw();
    }

    /// Jen vyměnit texturu (ostřejší SVG), pohled zůstane.
    pub fn vymen_texturu(&self, textura: gdk::Texture) {
        self.pohled().textura = Some(textura);
        self.queue_draw();
    }

    pub fn reset(&self) {
        let mut p = self.pohled();
        p.zoom = 1.0;
        p.cx = p.w / 2.0;
        p.cy = p.h / 2.0;
        drop(p);
        self.queue_draw();
    }

    /// Kolik logických px okna připadá na 1 px obrázku.
    pub fn meritko(&self) -> f64 {
        let p = self.imp().pohled.borrow();
        if p.w <= 0.0 || p.h <= 0.0 {
            return 1.0;
        }
        let (sirka, vyska) = (self.width().max(1) as f64, self.height().max(1) as f64);
        (sirka / p.w).min(vyska / p.h) * p.zoom
    }

    /// Nepouštět obraz mimo okno; menší obraz než okno zůstane vycentrovaný.
    fn omez(&self) {
        let s = self.meritko();
        let (sirka, vyska) = (self.width() as f64, self.height() as f64);
        let mut p = self.pohled();
        let omez_osu = |c: f64, rozmer: f64, okno: f64| {
            let pul = okno / 2.0 / s;
            if rozmer * s <= okno {
                rozmer / 2.0
            } else {
                c.clamp(pul, rozmer - pul)
            }
        };
        p.cx = omez_osu(p.cx, p.w, sirka);
        p.cy = omez_osu(p.cy, p.h, vyska);
    }

    /// Zoom tak, aby bod obrázku pod (mx, my) zůstal na místě; None = střed okna.
    pub fn zoomuj(&self, faktor: f64, mys: Option<(f64, f64)>) {
        let (sirka, vyska) = (self.width() as f64, self.height() as f64);
        let (mx, my) = mys.unwrap_or((sirka / 2.0, vyska / 2.0));
        let s = self.meritko();
        let (px, py) = {
            let p = self.imp().pohled.borrow();
            (p.cx + (mx - sirka / 2.0) / s, p.cy + (my - vyska / 2.0) / s)
        };
        {
            let mut p = self.pohled();
            p.zoom = (p.zoom * faktor).clamp(ZOOM_MIN, ZOOM_MAX);
        }
        let s = self.meritko();
        {
            let mut p = self.pohled();
            p.cx = px - (mx - sirka / 2.0) / s;
            p.cy = py - (my - vyska / 2.0) / s;
        }
        self.omez();
        self.queue_draw();
    }

    /// Posun pohledu o (dx, dy) logických px okna.
    pub fn posun(&self, dx: f64, dy: f64) {
        let s = self.meritko();
        {
            let mut p = self.pohled();
            p.cx += dx / s;
            p.cy += dy / s;
        }
        self.omez();
        self.queue_draw();
    }

    pub fn stred(&self) -> (f64, f64) {
        let p = self.imp().pohled.borrow();
        (p.cx, p.cy)
    }

    pub fn nastav_stred(&self, c: (f64, f64)) {
        let mut p = self.pohled();
        p.cx = c.0;
        p.cy = c.1;
    }

    /// Vejde se celý obraz do okna (není co posouvat)?
    pub fn vejde_se(&self) -> bool {
        let s = self.meritko();
        let p = self.imp().pohled.borrow();
        p.w * s <= self.width() as f64 + 0.5 && p.h * s <= self.height() as f64 + 0.5
    }
}
