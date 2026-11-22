# Viim

Průhledný prohlížeč obrázků pro niri (Wayland) v Rustu + GTK4. Nástupce Python verze
`~/Projects/Systemd/obrazek-view.py` – stejné ovládání, navíc načítání na pozadí, cache,
předem načtení sousedních obrázků a dialog pro otevření souboru/adresáře.

## Účel
- Zobrazit PNG/SVG s průhledností v okně bez rámečku, **bez ořezu** (velikost okna se zná předem).
- Připomínkové služby (`~/Projects/Systemd/README.md`) ho volají jako `viim -t 3 obrazek.png` (zavře se po 3 s).
- Běžné prohlížení adresáře se zoomem od kurzoru.

## Rychlé použití
```sh
viim                     # dialog pro výběr souboru
viim foto.png            # n/b listuje ostatní obrázky ve stejném adresáři
viim ~/Obrazky           # všechny obrázky v adresáři
viim a.png b.svg         # jen vybrané soubory
viim -t 3 Rotace.png     # zavře se po 3 s
viim -s 5 ~/Obrazky      # slideshow, obrázek každých 5 s
```

## Volby
| Volba | Výchozí | Význam |
|---|---|---|
| `cesta …` | – (otevře dialog) | soubor(y) nebo adresář; jeden soubor → listuje se celým jeho adresářem |
| `-t`, `--timeout SEK` | `0` (nezavírat) | zavřít po N s; jakákoli akce (klávesa, scroll, tažení) zavření zruší |
| `-s`, `--slideshow SEK` | vypnuto (interval 3 s) | spustit slideshow s intervalem SEK; klávesa `s` pak používá tento interval |
| `-H`, `--max-height PX` | 90 % nejnižšího monitoru | max. výška okna (logické px) |
| `-W`, `--max-width PX` | 90 % nejužšího monitoru | max. šířka okna (logické px) |
| `--app-id ID` | `viim` | app-id pro `window-rule` v niri |
| `-h`, `--help` | | nápověda |

Proměnné prostředí: `VIIM_DEBUG=1` – výpis polohy kurzoru při scrollu; `VIIM_SKRIPT="n n b q"` – testovací háček, po zobrazení „stiskne“ zadané klávesy (á 700 ms, jména kláves GDK, např. `plus`, `Left`).

## Ovládání
| Klávesa / myš | Akce |
|---|---|
| scroll nahoru / dolů | zvětšit / zmenšit od místa pod kurzorem |
| `+` / `-` | zvětšit / zmenšit od středu |
| `0` | zpět na původní velikost |
| šipky (se `Shift` větší krok) | posun obrazu |
| tažení myší | posun obrazu; když je vidět celý obraz, přesouvá se okno |
| `n` / `b` | další / předchozí obrázek |
| `s` | slideshow zap / vyp (`n`/`b` odpočet vynuluje) |
| `o` / `O` (Shift+o) | otevřít soubor / adresář (dialog; adresář → začne prvním obrázkem) |
| `q` / `Esc` | zavřít |

## Výstupy
Jen okno; chyby na stderr. Návratový kód: `0` OK, `1` nic se nepodařilo zobrazit, `2` chybné argumenty.

## Jak to funguje
- **Velikost okna** se spočítá z rozměrů obrázku (jen zmenšení do limitu, nikdy zvětšení) a okno se zobrazí až po načtení prvního obrázku. `resizable=false` → niri ho otevře jako plovoucí.
- **Zoom a posun** se dělají uvnitř okna (`src/platno.rs`), okno se nemění. Vykreslení na GPU přes `append_scaled_texture` s filtrem `Trilinear` (mipmapy → hladké i silné zmenšení).
- **Načítání** (`src/nacitani.rs`) běží ve vláknech: rastr crate `image` (plné rozlišení), SVG `resvg` (min. 2× rozlišení, po zoomu se přerenderuje ostřeji, max. 8192 px). Výsledek jde kanálem do hlavní smyčky GTK.
- **Cache** (LRU, strop 768 MB) + předem načtení obrázku před a za aktuálním → `n`/`b` je okamžité, návrat zpět nic nedekóduje.
- **Slideshow**: jeden `glib` časovač, který se naplánuje až po zobrazení obrázku → pomalé dekódování nic nepřeskočí, ruční `n`/`b` odpočet vynuluje, na konci seznamu jde dokola. Při jediném obrázku se časovač nespouští. Nečitelné soubory přeskočí stejně jako `n`.
- **Střed okna**: niri drží při změně velikosti levý horní roh, proto se okno posune `niri msg action move-floating-window` o polovinu rozdílu.
- **Poloha kurzoru** pro zoom: z události scrollu → dotaz na ukazatel → poslední `motion`/`enter`. Při `leave` se nemaže (démon `~/Projects/Kurzor` při dotyku touchpadu krátce převezme ukazatel).
- Vynucení integrované GPU (`VK_DRIVER_FILES`, `__EGL_VENDOR_LIBRARY_FILENAMES`), aby se dGPU NVIDIA mohla uspat.

Konstanty (`src/okno.rs`): `OSTROST`, `KROK_ZOOM` (1,2), `KROK_POSUN` (10 %), `PODIL` (0,9), `CACHE_BAJTY` (768 MB); `src/platno.rs`: `ZOOM_MIN`/`ZOOM_MAX`.

## Sestavení a instalace
```sh
cargo build --release
install -m755 target/release/viim ~/.local/bin/viim
```
Release profil (`Cargo.toml`): `lto = "fat"`, `codegen-units = 1`, `strip = true` → binárka ~4,5 MB místo ~6,8 MB, rychlost stejná nebo vyšší. Záměrně **ne** `panic = "abort"` (panika dekodéru na vadném souboru má shodit jen vlákno, ne aplikaci) a **ne** `opt-level = "s"` (pomalejší dekódování). Build trvá déle (~1,5 min).

Volitelně spouštěč pro „Otevřít v…“: `install -m644 viim.desktop ~/.local/share/applications/`.

Pravidlo v `~/.config/niri/config.kdl` (už existuje):
```kdl
window-rule {
    match app-id=r"^(viim|obrazek-view)$"
    open-floating true
    open-focused false
    focus-ring { off; }
    shadow { off; }
    border { off; }
}
```

## Závislosti
Systém: `gtk4` (≥ 4.14), Rust ≥ 1.85 (edition 2024). Crates: `gtk4`, `image`, `resvg`, `async-channel`, `serde_json`.

## Řešení problémů
- **Zoom jde od středu:** `VIIM_DEBUG=1 viim obr.png` – vypisuje `událost=` / `dotaz=` / `motion=`; všechny `None` = poloha kurzoru chybí.
- **Velká spotřeba paměti:** cache drží až 768 MB dekódovaných obrázků (velké PNG ~70 MB/kus) – sniž `CACHE_BAJTY`.
- **Okno při přepnutí „skáče“:** chybí `NIRI_SOCKET` nebo `niri` v PATH → bez dorovnání zůstane na místě levý horní roh.
- **SVG s textem vypadá jinak než v rsvg:** `resvg` používá systémová písma přes `fontdb`; chybějící font se nahradí.
