# Viim

**Česky** · [English](README.en.md)

Minimalistický průhledný prohlížeč obrázků pro **niri (Wayland)** v Rustu + GTK4. Okno bez
rámečku má přesně velikost obrázku (bez ořezu), podporuje průhlednost PNG/SVG, zoom od kurzoru,
listování adresářem s načítáním na pozadí a slideshow.

## Účel
- Zobrazit PNG/SVG s průhledností v okně bez rámečku, **bez ořezu** (velikost okna se zná předem).
- Krátké zobrazení obrázku ze skriptu, např. připomínka: `viim -t 3 obrazek.png` (zavře se po 3 s).
- Běžné prohlížení adresáře se zoomem od kurzoru.
- Náhled obrázků pro [fzfex](https://github.com/navik61/fzfex) (velikost okna přes `-W`/`-H`).

## Instalace
Závislosti: `gtk4` (≥ 4.14) a Rust ≥ 1.85 (edition 2024). Na Arch Linuxu:
```sh
sudo pacman -S --needed gtk4 rust
git clone https://github.com/navik61/viim.git
cd viim
cargo build --release
install -Dm755 target/release/viim ~/.local/bin/viim
install -Dm644 viim.desktop ~/.local/share/applications/viim.desktop   # volitelně: „Otevřít pomocí“
```
Výchozí prohlížeč obrázků (volitelně):
```sh
for t in png jpeg webp gif bmp tiff svg+xml; do xdg-mime default viim.desktop image/$t; done
```
Pravidlo do `~/.config/niri/config.kdl` (plovoucí okno bez rámečku, nebere fokus):
```kdl
window-rule {
    match app-id=r"^viim$"
    open-floating true
    open-focused false
    focus-ring { off; }
    shadow { off; }
    border { off; }
}
```
Pokud chceš, aby okno po otevření fokus dostalo, vynech `open-focused false`.

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

Podporované formáty: PNG, JPEG, WebP, GIF (první snímek), BMP, TIFF, SVG.

Proměnné prostředí:
- `VIIM_AMD=0`: nevynucovat vykreslování na AMD GPU (viz *Jak to funguje*).
- `VIIM_DEBUG=1`: výpis polohy kurzoru při scrollu a velikosti každé nové textury
  (`textura: soubor ŠxV (MB)`).
- `VIIM_SKRIPT="n n b q"`: testovací háček, po zobrazení „stiskne“ zadané klávesy (po 700 ms,
  jména kláves GDK, např. `plus`, `Left`).

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
Jen okno; chyby na stderr. Návratový kód: `0` OK, `1` nic se nepodařilo zobrazit, `2` chybné
argumenty.

## Jak to funguje
- **Velikost okna** se spočítá z rozměrů obrázku (jen zmenšení do limitu, nikdy zvětšení) a okno
  se zobrazí až po načtení prvního obrázku. `resizable=false` → niri ho otevře jako plovoucí.
- **Zoom a posun** se dělají uvnitř okna (`src/platno.rs`), okno se nemění. Vykreslení na GPU přes
  `append_scaled_texture` s filtrem `Trilinear` (mipmapy → hladké i silné zmenšení).
- **Načítání** (`src/nacitani.rs`) běží v pevném poolu 2 vláken (`PRACANTU`) s frontou úloh.
  Úloha, která mezitím přestala být potřeba (jiný seznam, obrázek dál než ± 1 od aktuálního), se
  přeskočí bez dekódování, takže rychlé listování nespouští desítky dekódování naráz. Panika
  dekodéru se zachytí a soubor se bere jako nečitelný (vlákno přežije).
  - Rastr: crate `image`, zmenšený plošným průměrem (`imageops::thumbnail`) na velikost okna ×
    největší škála monitoru (fyzické px; menší obrázky a úspora < ~20 % → plné rozlišení). Po
    zoomu nad toto rozlišení se ~150 ms po posledním kroku dočte plné rozlišení a vymění se bez
    změny pohledu.
  - SVG: `resvg` (min. 2× rozlišení, po zoomu se přerenderuje ostřeji, max. 8192 px).
  - Výsledek jde kanálem do hlavní smyčky GTK.
- **Cache** (strop 768 MB, průběžně počítaná velikost) + předem načtený obrázek před a za
  aktuálním → `n`/`b` je okamžité. Při přeplnění se vyhodí obrázek **nejvzdálenější** od
  aktuálního (při shodě déle nepoužitý), takže předem načtení sousedé zůstanou.
- **Slideshow:** jeden `glib` časovač, který se naplánuje až po zobrazení obrázku → pomalé
  dekódování nic nepřeskočí, ruční `n`/`b` odpočet vynuluje, na konci seznamu jde dokola. Při
  jediném obrázku se časovač nespouští. Nečitelné soubory přeskočí stejně jako `n`.
- **Střed okna:** niri drží při změně velikosti levý horní roh, proto se okno posune
  `niri msg action move-floating-window` o polovinu rozdílu.
- **Poloha kurzoru** pro zoom: z události scrollu → dotaz na ukazatel → poslední `motion`/`enter`.
  Při `leave` se nemaže (některé nástroje krátce převezmou ukazatel).
- **GPU:** na hybridní grafice s AMD GPU (např. AMD iGPU + NVIDIA dGPU) se vynutí vykreslování na
  AMD (`VK_DRIVER_FILES`, `__EGL_VENDOR_LIBRARY_FILENAMES`), aby se dGPU mohla uspat. Jen když
  AMD GPU v systému je a proměnné nejsou nastavené; `VIIM_AMD=0` to vypne.

Konstanty (`src/okno.rs`): `OSTROST`, `KROK_ZOOM` (1,2), `KROK_POSUN` (10 %), `PODIL` (0,9),
`CACHE_BAJTY` (768 MB), `PRACANTU` (2); `src/nacitani.rs`: `RASTR_ZMENSIT_POD` (0,9),
`SVG_MAX_STRANA` (8192); `src/platno.rs`: `ZOOM_MIN`/`ZOOM_MAX`.

Release profil (`Cargo.toml`): `lto = "fat"`, `codegen-units = 1`, `strip = true` → binárka
~4,5 MB místo ~6,8 MB, rychlost stejná nebo vyšší. Záměrně **ne** `panic = "abort"` (panika
dekodéru na vadném souboru má shodit jen vlákno, ne aplikaci) a **ne** `opt-level = "s"`
(pomalejší dekódování). Build trvá déle (~1,5 min).

## Závislosti
Systém: `gtk4` (≥ 4.14), Rust ≥ 1.85 (edition 2024). Volitelně `niri` (dorovnání středu okna).
Crates: `gtk4`, `image`, `resvg`, `async-channel`, `serde_json`.

## Řešení problémů
- **Zoom jde od středu:** `VIIM_DEBUG=1 viim obr.png` – vypisuje `událost=` / `dotaz=` /
  `motion=`; všechny `None` = poloha kurzoru chybí.
- **Velká spotřeba paměti:** cache drží až 768 MB dekódovaných obrázků; fotky jsou zmenšené na
  velikost okna (~10 MB/kus), v plném rozlišení jen ty, do kterých se zoomovalo – sniž
  `CACHE_BAJTY`.
- **Fotka je po zvětšení chvíli neostrá:** plné rozlišení se dočítá ~150 ms po posledním kroku
  zoomu (`VIIM_DEBUG=1` ukáže `ostřejší`).
- **Okno při přepnutí „skáče“:** chybí `NIRI_SOCKET` nebo `niri` v PATH → bez dorovnání zůstane na
  místě levý horní roh.
- **Okno je černé nebo se nevykreslí:** zkus `VIIM_AMD=0 viim obr.png`.
- **SVG s textem vypadá jinak než v rsvg:** `resvg` používá systémová písma přes `fontdb`;
  chybějící font se nahradí.

## Podpora
Pokud se ti viim hodí, můžeš mi koupit kafe:

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/Q7Z72B5QVS)

## Licence
MIT, viz soubor [`LICENSE`](LICENSE).
