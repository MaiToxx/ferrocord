//! Plaques animées (lot plaques) : le WebM d'une plaque nominative
//! (`assets/collectibles/{asset}asset.webm`) changé en images, comme un GIF.
//!
//! Le fichier est démultiplexé par `video::mkv` et ses images VP8 ou VP9
//! décodées par Windows (`video::codec`, « Extensions vidéo VP9 ») sur un fil
//! de décodage de l'ordonnanceur, jamais sur celui de l'interface. Sans ce
//! décodeur (extension absente, autre système) ou si le fichier est
//! illisible, le décodage échoue une fois pour toutes : l'image fixe de la
//! plaque reste affichée, rien n'est redemandé.
//!
//! Bornes : images ramenées à la taille d'affichage, au plus
//! [`anim::MAX_FRAMES`] images et [`MAX_BYTES`] octets de pixels (une boucle
//! plus longue garde une image sur `n`, à la même durée totale). Le son est
//! ignoré.
//!
//! Transparence (lot plaques2, retour d'utilisation : la plaque animée
//! couvrait toute la ligne d'un aplat opaque, là où l'image fixe laisse voir
//! la ligne) : une piste qui annonce `AlphaMode` porte, pour chaque image,
//! une image d'alpha du même codec dans les `BlockAdditions` de son groupe
//! (RFC 9559, 5.1.4.5.25 et « BlockAdditional » ; c'est ainsi que libvpx et
//! les navigateurs lisent le VP8 et le VP9 transparents). Un second décodeur
//! la lit ; sa luminance, sur toute l'étendue 0 à 255, est l'alpha, et la
//! couleur (non multipliée dans le fichier) est multipliée par lui avant
//! l'ajustement, comme egui l'attend. Si une image d'alpha manque ou ne se
//! décode pas, tout le fichier est refusé : mieux vaut l'image fixe qu'une
//! plaque opaque qui cache la ligne.

use super::anim::{self, Frames};
use super::decode::{self, DecodeError, Fit};
use crate::video::codec::{self, CodecError};
use crate::video::{convert, mkv, us_to_90k, Codec, EncodedFrame, Frame, FrameData};
use image::DynamicImage;
use std::time::Duration;

/// Octets de pixels (RVBA) d'une plaque animée, au plus : la moitié du
/// plafond d'une image animée. Une ligne de liste fait une quarantaine de Ko
/// par image ; une seule plaque joue à la fois (celle que survole le pointeur).
pub const MAX_BYTES: usize = 6 << 20;
/// Images illisibles tolérées avant d'abandonner (on attend une image clé).
const MAX_BAD: usize = 8;
/// Durée d'une image, bornée : ni boucle folle, ni image figée.
const MIN_DELAY: Duration = Duration::from_millis(8);
const MAX_DELAY: Duration = Duration::from_secs(60);
/// Durée de la dernière image quand rien ne la donne (fichier d'une seule image).
const DEFAULT_DELAY: Duration = Duration::from_millis(40);

/// Signature EBML : un fichier Matroska ou WebM.
pub fn is_webm(bytes: &[u8]) -> bool {
    bytes.starts_with(&mkv::MAGIC)
}

/// Images de la piste vidéo du WebM `bytes`, ajustées à `fit`, au plus `max`.
/// Aucun côté de la source ne dépasse `max_side`. `Unsupported` : pas de
/// piste VP8 ou VP9, ou pas de décodeur sur cette machine.
pub fn frames(bytes: &[u8], max_side: u32, fit: Fit, max: usize) -> Result<Frames, DecodeError> {
    let file = mkv::parse(bytes).ok_or(DecodeError::Unsupported)?;
    let track = file.video.as_ref().ok_or(DecodeError::Unsupported)?;
    let blocks = &file.video_blocks;
    if blocks.is_empty() {
        return Err(DecodeError::Corrupt("aucune image".into()));
    }
    let max_side = max_side.clamp(1, decode::MAX_SIDE);
    // Taille annoncée par la piste (0 : inconnue) : refusée avant tout décodage.
    if track.width > max_side || track.height > max_side {
        return Err(DecodeError::TooLarge { width: track.width, height: track.height });
    }
    let c = if track.codec == mkv::CodecId::Vp9 { Codec::Vp9 } else { Codec::Vp8 };
    let mut dec = codec::file_decoder(c).ok_or(DecodeError::Unsupported)?;
    // Piste transparente : un second décodeur, pour le flux d'alpha.
    let mut alpha_dec = if track.alpha { Some(codec::file_decoder(c).ok_or(DecodeError::Unsupported)?) } else { None };
    let max = max.clamp(1, anim::MAX_FRAMES);
    let mut plan: Option<(Fit, usize)> = None;
    let mut out: Frames = Vec::new();
    // Instant de chaque image gardée : les durées en sont les écarts.
    let mut times: Vec<u64> = Vec::new();
    let (mut total, mut bad, mut shown) = (0usize, 0usize, 0usize);
    let mut complete = true;
    for b in blocks {
        let Some(data) = b.start.checked_add(b.len).and_then(|end| bytes.get(b.start..end)) else { break };
        let f = EncodedFrame { codec: c, keyframe: b.key, ts90k: us_to_90k(b.pts_us), data: data.to_vec() };
        // L'alpha d'abord, pour chaque image même cachée ou sautée : son flux
        // a ses propres références, qui doivent toutes passer par le décodeur.
        let mask = match alpha_dec.as_mut() {
            Some(d) => {
                let data = b.alpha.and_then(|(start, len)| bytes.get(start..start.checked_add(len)?)).ok_or_else(|| DecodeError::Corrupt("image sans alpha".into()))?;
                let a = EncodedFrame { codec: c, keyframe: b.key, ts90k: f.ts90k, data: data.to_vec() };
                d.decode(&a).map_err(|e| DecodeError::Corrupt(format!("alpha : {e}")))?
            }
            None => None,
        };
        let frame = match dec.decode(&f) {
            Ok(Some(frame)) => frame,
            // Image cachée (référence), ou attente d'une image clé.
            Ok(None) => continue,
            Err(CodecError::Corrupt(_)) if bad < MAX_BAD => {
                bad += 1;
                continue;
            }
            // Fichier abîmé en cours de route : les images déjà lues sont gardées.
            Err(_) if !out.is_empty() => {
                complete = false;
                break;
            }
            Err(e) => return Err(DecodeError::Corrupt(e.to_string())),
        };
        let (w, h) = (frame.width, frame.height);
        if w == 0 || h == 0 || w > max_side || h > max_side {
            return Err(DecodeError::TooLarge { width: w, height: h });
        }
        let (fit, step) = *plan.get_or_insert_with(|| plan_for(blocks.len(), fit, (w, h)));
        shown += 1;
        // Image sautée : celle d'avant reste affichée pendant sa durée aussi.
        if (shown - 1) % step != 0 {
            continue;
        }
        if out.len() >= max {
            complete = false;
            break;
        }
        let mut rgba = Vec::new();
        convert::frame_to_rgba(&frame, &mut rgba).map_err(|e| DecodeError::Corrupt(e.to_string()))?;
        if alpha_dec.is_some() {
            let mask = mask.ok_or_else(|| DecodeError::Corrupt("alpha absent pour une image montrée".into()))?;
            apply_alpha(&mut rgba, (w, h), &mask)?;
        }
        let img = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| DecodeError::Corrupt("image décodée incohérente".into()))?;
        let img = anim::fit_frame_premultiplied(DynamicImage::ImageRgba8(img), fit);
        let size = img.size[0] * img.size[1] * 4;
        if !out.is_empty() && total + size > MAX_BYTES {
            complete = false;
            break;
        }
        total += size;
        out.push((img, DEFAULT_DELAY));
        times.push(b.pts_us);
    }
    if out.is_empty() {
        return Err(DecodeError::Corrupt("aucune image".into()));
    }
    // Durées : jusqu'à l'image gardée suivante ; la dernière va jusqu'à la fin
    // du fichier s'il la donne (et s'il a été lu en entier), sinon elle dure
    // autant que la précédente.
    let n = out.len();
    let clamp = |us: u64| Duration::from_micros(us).clamp(MIN_DELAY, MAX_DELAY);
    for i in 0..n - 1 {
        out[i].1 = clamp(times[i + 1].saturating_sub(times[i]));
    }
    let end = file.duration_us.saturating_sub(times[n - 1]);
    out[n - 1].1 = match (complete && end > 0, n) {
        (true, _) => clamp(end),
        (false, 1) => DEFAULT_DELAY,
        (false, _) => out[n - 2].1,
    };
    Ok(out)
}

/// Pose l'alpha `mask` (la luminance de l'image d'alpha, de même taille)
/// sur les pixels RVBA opaques `rgba` de `w` × `h`, couleur multipliée par
/// l'alpha.
fn apply_alpha(rgba: &mut [u8], (w, h): (u32, u32), mask: &Frame) -> Result<(), DecodeError> {
    let FrameData::I420 { y, .. } = &mask.data else { return Err(DecodeError::Corrupt("alpha : format inattendu".into())) };
    if (mask.width, mask.height) != (w, h) || y.len() * 4 != rgba.len() {
        return Err(DecodeError::Corrupt(format!("alpha de {} × {} pour une image de {w} × {h}", mask.width, mask.height)));
    }
    let mul = |c: u8, a: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
    for (px, &a) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(y) {
        *px = [mul(px[0], a), mul(px[1], a), mul(px[2], a), a];
    }
    Ok(())
}

/// Taille des images et pas de lecture (une image gardée sur `pas`) pour une
/// vidéo de `count` images de `src` pixels : au plus [`anim::MAX_FRAMES`]
/// images, et pour une image recadrée (`Fit::Fill`, la plaque) la taille qui
/// fait tenir la boucle entière dans [`MAX_BYTES`] : réduite jusqu'à la
/// moitié de chaque côté (le processeur graphique étire ; c'est un fond qui
/// bouge), puis allégée d'images plutôt que réduite davantage.
fn plan_for(count: usize, fit: Fit, src: (u32, u32)) -> (Fit, usize) {
    let cap = count.div_ceil(anim::MAX_FRAMES).max(1);
    match fit {
        Fit::Fill(w, h) => {
            let floor = (w.saturating_mul(h) / 4).max(1);
            let (fit, step) = anim::fill_plan(MAX_BYTES, floor, count.div_ceil(cap), (w, h), src);
            (fit, step * cap)
        }
        other => (other, cap),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::video::mkv::write::{webm, webm_alpha, AlphaFrame, Packet};

    /// Taille du jeu d'essai : une plaque large et basse.
    pub(crate) const W: u32 = 320;
    pub(crate) const H: u32 = 64;
    /// Images du jeu d'essai, à 25 images par seconde.
    pub(crate) const FRAMES: u64 = 8;

    /// Image grise unie dont la luminance suit `t` (croissante sur les dix premières).
    fn gray(w: u32, h: u32, t: u64) -> Frame {
        let (pw, ph) = (w as usize, h as usize);
        let c = pw.div_ceil(2) * ph.div_ceil(2);
        Frame { width: w, height: h, pts_us: t * 40_000, data: FrameData::I420 { y: vec![40 + (t * 20 % 200) as u8; pw * ph], u: vec![128; c], v: vec![128; c] } }
    }

    /// WebM VP8 de `n` images grises de `w` × `h`, 40 ms chacune, de plus en
    /// plus claires ; `None` sans le codec VP8 de Windows (« Extensions vidéo
    /// VP9 »). `codec` : identifiant annoncé par la piste.
    pub(crate) fn clip(codec: &str, n: u64, w: u32, h: u32) -> Option<Vec<u8>> {
        let mut enc = crate::video::codec::encoder(Codec::Vp8, w, h, 25, 400_000)?;
        let packets: Vec<Packet> = (0..n)
            .map(|t| {
                let mut out = enc.encode(&gray(w, h, t), t == 0).unwrap_or_else(|e| panic!("image {t} : {e}"));
                let f = out.remove(0);
                (1, t * 40, f.keyframe, f.data)
            })
            .collect();
        Some(webm(Some((codec, w, h)), None, &packets, n as f64 * 40.0, false))
    }

    /// La plaque animée du jeu d'essai, fabriquée une fois (sur un fil à
    /// part : Media Foundation veut le MTA) ; `None` sans le codec VP8.
    pub(crate) fn plate() -> Option<Vec<u8>> {
        static PLATE: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
        PLATE.get_or_init(|| std::thread::spawn(|| clip("V_VP8", FRAMES, W, H)).join().unwrap_or_else(|_| panic!("fabrication de la plaque d'essai"))).clone()
    }

    /// Image de `w` × `h` en trois bandes verticales (sombre, moyenne,
    /// claire), chacune d'une teinte différente, la moitié basse plus claire :
    /// un pas de ligne mal lu ou un plan de chrominance décalé la cisaille.
    fn bands(w: u32, h: u32) -> Frame {
        let (pw, ph) = (w as usize, h as usize);
        let (cw, ch) = (pw.div_ceil(2), ph.div_ceil(2));
        let third = |x: usize, width: usize| (x * 3 / width).min(2);
        let y = (0..pw * ph).map(|i| [50u8, 110, 170][third(i % pw, pw)] + if i / pw >= ph / 2 { 40 } else { 0 }).collect();
        let u = (0..cw * ch).map(|i| [90u8, 128, 170][third(i % cw, cw)]).collect();
        let v = (0..cw * ch).map(|i| [170u8, 128, 90][third(i % cw, cw)]).collect();
        Frame { width: w, height: h, pts_us: 0, data: FrameData::I420 { y, u, v } }
    }

    /// Image d'alpha : transparente sur la moitié gauche, opaque à droite.
    fn half_mask(w: u32, h: u32) -> Frame {
        let (pw, ph) = (w as usize, h as usize);
        let c = pw.div_ceil(2) * ph.div_ceil(2);
        Frame { width: w, height: h, pts_us: 0, data: FrameData::I420 { y: (0..pw * ph).map(|i| if i % pw < pw / 2 { 0 } else { 255 }).collect(), u: vec![128; c], v: vec![128; c] } }
    }

    /// WebM VP8 transparent de `n` images : `colour(t)` pour la couleur,
    /// `mask(t)` pour l'alpha, chacune dans son flux ; `None` sans le codec.
    fn alpha_clip(n: u64, (w, h): (u32, u32), colour: impl Fn(u64) -> Frame, mask: impl Fn(u64) -> Frame, flagged: bool) -> Option<Vec<u8>> {
        let mut enc = crate::video::codec::encoder(Codec::Vp8, w, h, 25, 800_000)?;
        let mut alpha = crate::video::codec::encoder(Codec::Vp8, w, h, 25, 800_000)?;
        let frames: Vec<AlphaFrame> = (0..n)
            .map(|t| {
                let c = enc.encode(&colour(t), t == 0).unwrap_or_else(|e| panic!("image {t} : {e}")).remove(0);
                let a = alpha.encode(&mask(t), t == 0).unwrap_or_else(|e| panic!("alpha {t} : {e}")).remove(0);
                (t * 40, c.keyframe, c.data, a.data)
            })
            .collect();
        Some(webm_alpha("V_VP8", (w, h), &frames, n as f64 * 40.0, flagged))
    }

    /// `f` sur un fil neuf (Media Foundation veut le MTA).
    fn apart<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(f).join().unwrap_or_else(|_| panic!("panique du fil d'essai"))
    }

    fn near(a: eframe::egui::Color32, want: [u8; 4], tol: u8) -> bool {
        a.to_array().iter().zip(want).all(|(x, y)| x.abs_diff(y) <= tol)
    }

    /// Retour d'utilisation (plaque animée opaque sur toute la ligne) : une
    /// piste transparente rend des pixels transparents là où son alpha est
    /// nul (la ligne se voit derrière), opaques ailleurs, couleur multipliée.
    #[test]
    fn plaque_transparente_composee_avec_son_alpha() {
        let (w, h) = (W, H);
        let Some(data) = apart(move || alpha_clip(4, (w, h), |t| gray(W, H, t + 5), |_| half_mask(W, H), true)) else {
            eprintln!("VP8 de Windows absent : sauté");
            return;
        };
        // À la taille de la source, puis à celle d'une ligne de liste.
        for (fw, fh) in [(W, H), (224, 42)] {
            let f = anim::decode_frames(&data, 1024, Fit::Fill(fw, fh)).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(f.len(), 4);
            for (i, (img, _)) in f.iter().enumerate() {
                let at = |x: u32, y: u32| img.pixels[(y * fw + x) as usize];
                // Gris de l'image `i` (luminance 140 + 20 i, BT.601 étendue réduite).
                let g = ((140.0 + 20.0 * i as f32 - 16.0) * 255.0 / 219.0).round() as u8;
                for y in [2, fh / 2, fh - 3] {
                    for x in [2, fw / 4, fw / 2 - 12] {
                        assert!(near(at(x, y), [0, 0, 0, 0], 12), "image {i}, ({x}, {y}) à gauche : {:?}", at(x, y));
                    }
                    for x in [fw / 2 + 12, fw * 3 / 4, fw - 3] {
                        assert!(near(at(x, y), [g, g, g, 255], 14), "image {i}, ({x}, {y}) à droite : {:?} pour {g}", at(x, y));
                    }
                }
                // Jamais plus de couleur que d'alpha (prémultiplié), nulle part.
                assert!(img.pixels.iter().all(|p| p.r().max(p.g()).max(p.b()) <= p.a()), "image {i}");
            }
        }
        // Alpha à demi : la couleur est multipliée (gris 124 × 128 / 255).
        let Some(half) = apart(move || {
            let mid = |_| {
                let mut m = half_mask(W, H);
                if let FrameData::I420 { y, .. } = &mut m.data {
                    y.fill(128);
                }
                m
            };
            alpha_clip(2, (w, h), |_| gray(W, H, 4), mid, true)
        }) else {
            return;
        };
        let f = anim::decode_frames(&half, 1024, Fit::Fill(W, H)).unwrap_or_else(|e| panic!("{e}"));
        let p = f[0].0.pixels[(H / 2 * W + W / 2) as usize];
        let g = ((120.0f32 - 16.0) * 255.0 / 219.0 * 128.0 / 255.0).round() as u8;
        assert!(near(p, [g, g, g, 128], 8), "{p:?} pour {g}");
    }

    /// Alpha annoncé mais absent, ou illisible : le fichier est refusé (la
    /// plaque garde son image fixe) plutôt que montré opaque. Non annoncé :
    /// les additions sont ignorées, l'image est opaque.
    #[test]
    fn alpha_manquant_ou_illisible_refuse() {
        let Some(packets) = apart(|| {
            let mut enc = crate::video::codec::encoder(Codec::Vp8, W, H, 25, 400_000)?;
            Some((0..3u64).map(|t| (t, enc.encode(&gray(W, H, t), t == 0).unwrap_or_else(|e| panic!("image {t} : {e}")).remove(0))).collect::<Vec<_>>())
        }) else {
            eprintln!("VP8 de Windows absent : sauté");
            return;
        };
        let with = |alpha: &dyn Fn(u64, &[u8]) -> Vec<u8>, flagged: bool| {
            let frames: Vec<AlphaFrame> = packets.iter().map(|(t, f)| (t * 40, f.keyframe, f.data.clone(), alpha(*t, &f.data))).collect();
            webm_alpha("V_VP8", (W, H), &frames, 120.0, flagged)
        };
        let decode = |data: Vec<u8>| apart(move || anim::decode_frames(&data, 1024, Fit::Fill(224, 42)));
        // Alpha qui n'est pas de la vidéo : rien à montrer pour l'alpha.
        let junk = decode(with(&|_, _| vec![0x55; 40], true));
        assert!(matches!(junk, Err(DecodeError::Corrupt(_))), "{:?}", junk.map(|f| f.len()));
        // Image d'alpha vide à partir de la deuxième image.
        let cut = decode(with(&|t, d| if t == 0 { d.to_vec() } else { Vec::new() }, true));
        assert!(matches!(cut, Err(DecodeError::Corrupt(_))), "{:?}", cut.map(|f| f.len()));
        // Non annoncé : opaque, les additions (même illisibles) ne comptent pas.
        let plain = decode(with(&|_, _| vec![0x55; 40], false)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(plain.len(), 3);
        assert!(plain.iter().all(|(img, _)| img.pixels.iter().all(|p| p.a() == 255)));
    }

    /// Bandes diagonales (retour d'utilisation) : écartées comme défaut de pas
    /// de ligne. Largeur non multiple de 16, hauteur impaire quand l'encodeur
    /// l'accepte : chaque pixel décodé est à sa place, luminance et teinte.
    #[test]
    fn largeur_non_multiple_de_16_sans_cisaillement() {
        let mut tried = 0;
        for (w, h) in [(250u32, 46u32), (250, 45), (251, 45), (178, 34)] {
            let made = apart(move || {
                let mut enc = crate::video::codec::encoder(Codec::Vp8, w, h, 25, 1_500_000)?;
                let packets: Vec<Packet> = (0..3u64).filter_map(|t| enc.encode(&bands(w, h), t == 0).ok().and_then(|mut o| (!o.is_empty()).then(|| o.remove(0))).map(|f| (1, t * 40, f.keyframe, f.data))).collect();
                (packets.len() == 3).then(|| webm(Some(("V_VP8", w, h)), None, &packets, 120.0, false))
            });
            let Some(data) = made else {
                eprintln!("{w} × {h} : l'encodeur VP8 de Windows ne le prend pas, sauté");
                continue;
            };
            tried += 1;
            // `Contain` à la taille de la source : les pixels décodés, sans ajustement.
            let f = anim::decode_frames(&data, 1024, Fit::Contain(w, h)).unwrap_or_else(|e| panic!("{w} × {h} : {e}"));
            let mut want = Vec::new();
            let src = bands(w, h);
            convert::frame_to_rgba(&src, &mut want).unwrap_or_else(|e| panic!("{e}"));
            for (i, (img, _)) in f.iter().enumerate() {
                let [gw, gh] = img.size;
                assert!(gw + 1 >= w as usize && gw <= w as usize && gh + 1 >= h as usize && gh <= h as usize, "{w} × {h} décodé en {gw} × {gh}");
                // Loin des bords de bande (le codec les adoucit) : chaque pixel à sa place.
                let mut checked = 0;
                for y in 0..gh {
                    for x in 0..gw {
                        let third = w as usize / 3;
                        let edge = (x % third).min(third - x % third) < 6 || x >= third * 3 - 6 || y.abs_diff(h as usize / 2) < 6;
                        if edge {
                            continue;
                        }
                        let k = (y * w as usize + x) * 4;
                        let p = img.pixels[y * gw + x];
                        assert!(near(p, [want[k], want[k + 1], want[k + 2], 255], 24), "{w} × {h}, image {i}, ({x}, {y}) : {p:?} au lieu de {:?}", &want[k..k + 3]);
                        checked += 1;
                    }
                }
                assert!(checked > gw * gh / 2, "{checked} pixels comparés");
            }
        }
        if tried == 0 {
            eprintln!("VP8 de Windows absent : sauté");
        }
    }

    fn gray_of(img: &eframe::egui::ColorImage) -> u8 {
        img.pixels[img.pixels.len() / 2].g()
    }

    /// Le WebM d'une plaque donne ses images à la taille d'affichage, dans
    /// l'ordre, avec leurs durées ; la boucle entière dure ce que dit le fichier.
    #[test]
    fn plaque_webm_decodee_a_la_taille_d_affichage() {
        let Some(data) = plate() else {
            eprintln!("VP8 de Windows absent : sauté");
            return;
        };
        assert!(is_webm(&data) && anim::readable(&data));
        // Par la porte du cache d'images (`decode_frames`), recadrée comme une ligne de liste.
        let f = anim::decode_frames(&data, 1024, Fit::Fill(224, 42)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.len(), FRAMES as usize);
        assert!(f.iter().all(|(img, d)| img.size == [224, 42] && *d == Duration::from_millis(40)), "{:?}", f.iter().map(|(i, d)| (i.size, *d)).collect::<Vec<_>>());
        // De plus en plus claires, opaques.
        for (i, pair) in f.windows(2).enumerate() {
            assert!(gray_of(&pair[1].0) > gray_of(&pair[0].0) + 8, "image {i} : {} puis {}", gray_of(&pair[0].0), gray_of(&pair[1].0));
        }
        assert!(f.iter().all(|(img, _)| img.pixels.iter().all(|p| p.a() == 255)));
        // Place plus grande que la source : pas agrandie, proportions demandées gardées.
        let big = anim::decode_frames(&data, 1024, Fit::Fill(960, 180)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(big[0].0.size, [320, 60]);
        // Source plus grande que le côté accepté : refusée avant décodage.
        assert_eq!(anim::decode_frames(&data, 256, Fit::Fill(224, 42)).err(), Some(DecodeError::TooLarge { width: W, height: H }));
        // Nombre d'images plafonné par l'appelant (`is_animated` n'en lit que deux).
        assert_eq!(frames(&data, 1024, Fit::Fill(224, 42), 2).map(|f| f.len()), Ok(2));
        assert!(anim::is_animated(&data));
    }

    /// Longue boucle : une image sur `pas` est gardée, la durée totale reste
    /// celle du fichier, et les bornes (images, octets) tiennent.
    #[test]
    fn longue_boucle_allegee_sans_changer_de_duree() {
        // Calcul seul : 600 images de 448 × 84 ne tiennent pas dans le budget
        // (réduites de moitié par côté, puis une sur six).
        let (fit, step) = plan_for(600, Fit::Fill(448, 84), (448, 84));
        let Fit::Fill(w, h) = fit else { panic!("{fit:?}") };
        let kept = 600usize.div_ceil(step);
        assert!(step >= 3 && kept <= anim::MAX_FRAMES, "pas {step}, {kept} images");
        assert!(kept * w as usize * h as usize * 4 <= MAX_BYTES, "{kept} × {w} × {h}");
        assert!((w as f32 / h as f32 - 448.0 / 84.0).abs() < 0.1, "{w} × {h}");
        // Courte boucle à la taille d'une ligne : rien n'est sauté ni réduit.
        assert_eq!(plan_for(90, Fit::Fill(224, 42), (448, 84)), (Fit::Fill(224, 42), 1));
        // Autre ajustement : seul le nombre d'images est plafonné.
        assert_eq!(plan_for(anim::MAX_FRAMES * 2 + 1, Fit::Cover(32), (64, 64)), (Fit::Cover(32), 3));
        // Sur un vrai fichier : 30 images, budget dépassé à dessein par une grande place.
        let Some(data) = std::thread::spawn(|| clip("V_VP8", 30, 320, 240)).join().unwrap_or_else(|_| panic!("fabrication du jeu d'essai")) else { return };
        let f = anim::decode_frames(&data, 1024, Fit::Fill(320, 240)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.len(), 30);
        assert_eq!(f.iter().map(|(_, d)| *d).sum::<Duration>(), Duration::from_millis(30 * 40));
        assert!(f.iter().map(|(i, _)| i.size[0] * i.size[1] * 4).sum::<usize>() <= MAX_BYTES);
    }

    /// Fichier illisible, piste inconnue, décodeur absent ou données qui ne
    /// sont pas de la vidéo : une erreur, jamais de panique, et l'appelant
    /// garde l'image fixe.
    #[test]
    fn fichiers_illisibles_refuses_sans_panique() {
        // Signature seule, piste d'un codec que rien ne lit ici.
        assert_eq!(anim::decode_frames(&mkv::MAGIC, 1024, Fit::Fill(224, 42)).err(), Some(DecodeError::Unsupported));
        let av1 = webm(Some(("V_AV1", W, H)), None, &[(1, 0, true, vec![0; 8])], 40.0, false);
        assert_eq!(anim::decode_frames(&av1, 1024, Fit::Fill(224, 42)).err(), Some(DecodeError::Unsupported));
        // Piste sans image.
        let empty = webm(Some(("V_VP8", W, H)), None, &[], 40.0, false);
        assert!(anim::decode_frames(&empty, 1024, Fit::Fill(224, 42)).is_err());
        // VP9 ou VP8 annoncé, données qui n'en sont pas : erreur (ou décodeur absent).
        for codec in ["V_VP9", "V_VP8"] {
            let fake = webm(Some((codec, W, H)), None, &[(1, 0, true, vec![0x55; 64]), (1, 40, false, vec![0x55; 64])], 80.0, false);
            let got = std::thread::spawn(move || anim::decode_frames(&fake, 1024, Fit::Fill(224, 42))).join().unwrap_or_else(|_| panic!("panique"));
            assert!(matches!(got, Err(DecodeError::Unsupported | DecodeError::Corrupt(_))), "{codec} : {got:?}");
        }
        // Fichier coupé au milieu : ce qui précède est rendu, ou une erreur ; pas de panique.
        if let Some(data) = plate() {
            let cut = &data[..data.len() * 2 / 3];
            let got = anim::decode_frames(cut, 1024, Fit::Fill(224, 42));
            assert!(got.as_ref().map_or(true, |f| !f.is_empty() && f.len() < FRAMES as usize), "{:?}", got.map(|f| f.len()));
        }
    }
}
