use super::*;
use eframe::egui::Color32;
use serde_json::{json, Value};

fn fixed(v: Value) -> Value {
    json!({"a": 0, "k": v})
}

fn ks() -> Value {
    json!({"a": fixed(json!([0, 0])), "p": fixed(json!([0, 0])), "s": fixed(json!([100, 100])), "r": fixed(json!(0)), "o": fixed(json!(100))})
}

fn comp(layers: Value, assets: Value) -> Vec<u8> {
    json!({"v": "5.7.0", "fr": 30, "ip": 0, "op": 30, "w": 100, "h": 100, "layers": layers, "assets": assets}).to_string().into_bytes()
}

fn shape_layer(shapes: Value) -> Value {
    json!({"ty": 4, "ind": 1, "ip": 0, "op": 30, "st": 0, "ks": ks(), "shapes": shapes})
}

fn fill(r: f64, g: f64, b: f64) -> Value {
    json!({"ty": "fl", "c": fixed(json!([r, g, b, 1])), "o": fixed(json!(100))})
}

fn tr(p: Value) -> Value {
    json!({"ty": "tr", "a": fixed(json!([0, 0])), "p": p, "s": fixed(json!([100, 100])), "r": fixed(json!(0)), "o": fixed(json!(100))})
}

/// Un disque rouge de 30 de diamètre qui va de x = 25 à x = 75 en 30 images.
fn moving_disc() -> Vec<u8> {
    let p = json!({"a": 1, "k": [
        {"t": 0, "s": [25, 50], "i": {"x": [1], "y": [1]}, "o": {"x": [0], "y": [0]}},
        {"t": 30, "s": [75, 50]}
    ]});
    let group = json!({"ty": "gr", "it": [
        {"ty": "el", "p": fixed(json!([0, 0])), "s": fixed(json!([30, 30]))},
        fill(1.0, 0.0, 0.0),
        tr(p)
    ]});
    comp(json!([shape_layer(json!([group]))]), json!([]))
}

fn px(img: &ColorImage, x: usize, y: usize) -> Color32 {
    img.pixels[y * img.size[0] + x]
}

fn first(bytes: &[u8], side: u32) -> ColorImage {
    frames(bytes, Fit::Contain(side, side), 1).unwrap_or_else(|e| panic!("{e}")).remove(0).0
}

#[test]
fn recognised_by_content() {
    assert!(is_lottie(&moving_disc()));
    assert!(is_lottie(b"\xef\xbb\xbf  {\"v\":\"5\",\"layers\":[]}"));
    assert!(!is_lottie(b"{\"message\": \"404: Not Found\"}"));
    assert!(!is_lottie(b"GIF89a \"layers\""));
    assert!(!is_lottie(b""));
    assert!(anim::readable(&moving_disc()));
}

#[test]
fn disc_moves_across_the_frames() {
    let f = frames(&moving_disc(), Fit::Contain(50, 50), anim::MAX_FRAMES).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(f.len(), 30);
    assert!(f.iter().all(|(img, d)| img.size == [50, 50] && (d.as_millis() == 33 || d.as_millis() == 34)));
    // Première image : disque à gauche (x = 25 sur 100, soit 12 sur 50).
    let (a, z) = (&f[0].0, &f[29].0);
    assert_eq!(px(a, 12, 25), Color32::from_rgb(255, 0, 0));
    assert_eq!(px(a, 37, 25), Color32::TRANSPARENT);
    assert_eq!(px(a, 12, 5), Color32::TRANSPARENT);
    // Dernière image : presque arrivé à droite.
    assert_eq!(px(z, 36, 25), Color32::from_rgb(255, 0, 0));
    assert_eq!(px(z, 12, 25), Color32::TRANSPARENT);
    // Au milieu de la boucle, au milieu du chemin.
    assert_eq!(px(&f[15].0, 25, 25), Color32::from_rgb(255, 0, 0));
    // Vectoriel : agrandi sans perte, bord net.
    let big = first(&moving_disc(), 400);
    assert_eq!(big.size, [400, 400]);
    assert_eq!(px(&big, 100, 200), Color32::from_rgb(255, 0, 0));
    assert_eq!(px(&big, 100, 130), Color32::TRANSPARENT);
    // L'animation entière est vue comme animée ; l'image fixe est la première.
    assert!(anim::is_animated(&moving_disc()));
    let still = still(&moving_disc(), 100).unwrap_or_else(|e| panic!("{e}")).into_rgba8();
    assert_eq!(still.dimensions(), (100, 100));
    assert_eq!(still.get_pixel(25, 50).0, [255, 0, 0, 255]);
    assert_eq!(still.get_pixel(75, 50).0, [0, 0, 0, 0]);
}

#[test]
fn stroke_trim_and_draw_order() {
    // Trait horizontal épais, rogné à sa première moitié ; un carré vert est
    // listé après lui : il passe dessous.
    let line = json!({"ty": "gr", "it": [
        {"ty": "sh", "ks": fixed(json!({"c": false, "v": [[10, 50], [90, 50]], "i": [[0, 0], [0, 0]], "o": [[0, 0], [0, 0]]}))},
        {"ty": "tm", "s": fixed(json!(0)), "e": fixed(json!(50)), "o": fixed(json!(0)), "m": 1},
        {"ty": "st", "c": fixed(json!([0, 0, 1, 1])), "o": fixed(json!(100)), "w": fixed(json!(10)), "lc": 1, "lj": 1},
        tr(fixed(json!([0, 0])))
    ]});
    let square = json!({"ty": "gr", "it": [
        {"ty": "rc", "p": fixed(json!([30, 50])), "s": fixed(json!([20, 40])), "r": fixed(json!(0))},
        fill(0.0, 1.0, 0.0),
        tr(fixed(json!([0, 0])))
    ]});
    let img = first(&comp(json!([shape_layer(json!([line, square]))]), json!([])), 100);
    assert_eq!(px(&img, 30, 50), Color32::from_rgb(0, 0, 255), "le trait est au-dessus du carré");
    assert_eq!(px(&img, 30, 62), Color32::from_rgb(0, 255, 0));
    assert_eq!(px(&img, 45, 50), Color32::from_rgb(0, 0, 255));
    assert_eq!(px(&img, 70, 50), Color32::TRANSPARENT, "seconde moitié rognée");
    assert_eq!(px(&img, 45, 60), Color32::TRANSPARENT);
}

#[test]
fn solid_mask_parent_and_opacity() {
    // Aplat bleu à moitié opaque, découpé à sa moitié gauche, déplacé de 10
    // vers le bas par un calque nul parent.
    let mut solid_ks = ks();
    solid_ks["o"] = fixed(json!(50));
    let mut parent_ks = ks();
    parent_ks["p"] = fixed(json!([0, 10]));
    let mask = json!({"mode": "a", "inv": false, "pt": fixed(json!({"c": true, "v": [[0, 0], [50, 0], [50, 100], [0, 100]], "i": [[0, 0], [0, 0], [0, 0], [0, 0]], "o": [[0, 0], [0, 0], [0, 0], [0, 0]]}))});
    let layers = json!([
        {"ty": 1, "ind": 1, "parent": 2, "ip": 0, "op": 30, "st": 0, "ks": solid_ks, "sc": "#0000ff", "sw": 100, "sh": 100, "masksProperties": [mask]},
        {"ty": 3, "ind": 2, "ip": 0, "op": 30, "st": 0, "ks": parent_ks}
    ]);
    let img = first(&comp(layers, json!([])), 100);
    let p = px(&img, 25, 50);
    assert!(p.a().abs_diff(128) <= 2 && p.b().abs_diff(128) <= 2 && p.r() == 0, "{p:?}");
    assert_eq!(px(&img, 75, 50), Color32::TRANSPARENT, "hors du masque");
    assert_eq!(px(&img, 25, 5), Color32::TRANSPARENT, "décalé par le parent");
}

#[test]
fn precomp_matte_and_gradient() {
    // Précomposition : un rectangle en dégradé du noir au blanc sur toute la
    // largeur, vu à travers un disque (cache alpha) centré.
    let grad = json!({"ty": "gf", "t": 1, "o": fixed(json!(100)), "s": fixed(json!([0, 0])), "e": fixed(json!([100, 0])), "g": {"p": 2, "k": fixed(json!([0, 0, 0, 0, 1, 1, 1, 1]))}});
    let inner = json!([shape_layer(json!([{"ty": "gr", "it": [
        {"ty": "rc", "p": fixed(json!([50, 50])), "s": fixed(json!([100, 100])), "r": fixed(json!(0))},
        grad,
        tr(fixed(json!([0, 0])))
    ]}]))]);
    let disc = json!({"ty": "gr", "it": [
        {"ty": "el", "p": fixed(json!([50, 50])), "s": fixed(json!([60, 60]))},
        fill(1.0, 1.0, 1.0),
        tr(fixed(json!([0, 0])))
    ]});
    let layers = json!([
        {"ty": 4, "ind": 1, "td": 1, "ip": 0, "op": 30, "st": 0, "ks": ks(), "shapes": [disc]},
        {"ty": 0, "ind": 2, "tt": 1, "refId": "c", "w": 100, "h": 100, "ip": 0, "op": 30, "st": 0, "ks": ks()}
    ]);
    let img = first(&comp(layers, json!([{"id": "c", "layers": inner}])), 100);
    let (left, right) = (px(&img, 30, 50), px(&img, 70, 50));
    assert!(left.a() == 255 && right.a() == 255);
    assert!(left.r() < 110 && right.r() > 150 && left.r() == left.g(), "{left:?} {right:?}");
    assert_eq!(px(&img, 5, 50), Color32::TRANSPARENT, "hors du cache");
    assert_eq!(px(&img, 50, 5), Color32::TRANSPARENT);
    // Cache inversé : l'inverse.
    let mut inv = json!([
        {"ty": 4, "ind": 1, "td": 1, "ip": 0, "op": 30, "st": 0, "ks": ks(), "shapes": [json!({"ty": "gr", "it": [
            {"ty": "el", "p": fixed(json!([50, 50])), "s": fixed(json!([60, 60]))}, fill(1.0, 1.0, 1.0), tr(fixed(json!([0, 0])))
        ]})]},
        {"ty": 1, "ind": 2, "tt": 2, "ip": 0, "op": 30, "st": 0, "ks": ks(), "sc": "#ff0000", "sw": 100, "sh": 100}
    ]);
    let img = first(&comp(inv.take(), json!([])), 100);
    assert_eq!(px(&img, 50, 50), Color32::TRANSPARENT);
    assert_eq!(px(&img, 5, 5), Color32::from_rgb(255, 0, 0));
}

#[test]
fn layer_outside_its_time_range_and_hold_keys() {
    // Calque visible de l'image 10 à 20 seulement ; opacité tenue (clé « h »).
    let mut l = shape_layer(json!([{"ty": "gr", "it": [
        {"ty": "rc", "p": fixed(json!([50, 50])), "s": fixed(json!([100, 100])), "r": fixed(json!(0))},
        fill(1.0, 0.0, 0.0),
        tr(fixed(json!([0, 0])))
    ]}]));
    l["ip"] = json!(10);
    l["op"] = json!(20);
    l["ks"]["o"] = json!({"a": 1, "k": [{"t": 10, "s": [100], "h": 1}, {"t": 15, "s": [0], "h": 1}]});
    let f = frames(&comp(json!([l]), json!([])), Fit::Contain(10, 10), anim::MAX_FRAMES).unwrap_or_else(|e| panic!("{e}"));
    let at = |n: usize| px(&f[n].0, 5, 5);
    assert_eq!(at(5), Color32::TRANSPARENT);
    assert_eq!(at(10), Color32::from_rgb(255, 0, 0));
    assert_eq!(at(14), Color32::from_rgb(255, 0, 0));
    assert_eq!(at(15), Color32::TRANSPARENT);
    assert_eq!(at(25), Color32::TRANSPARENT);
}

#[test]
fn budget_lowers_rate_then_size() {
    // Court et petit : tout tient, à 30 images par seconde au plus.
    assert_eq!(plan(2.0, 60.0, (160, 160), anim::MAX_FRAMES), (60, (160, 160)));
    assert_eq!(plan(1.0, 24.0, (160, 160), anim::MAX_FRAMES), (24, (160, 160)));
    // Grand : moins d'images, même taille (12 Mio / (320 × 320 × 4) = 30 images).
    assert_eq!(plan(2.0, 60.0, (320, 320), anim::MAX_FRAMES), (30, (320, 320)));
    // Trop long même à 12 images par seconde : l'image rapetisse.
    let (n, (w, h)) = plan(5.0, 60.0, (640, 640), anim::MAX_FRAMES);
    assert_eq!(n, 60);
    assert!(w == h && (200..640).contains(&w), "{w}");
    assert!(n * (w * h * 4) as usize <= anim::MAX_ANIM_BYTES);
    // Plafond du nombre d'images, et demande d'une seule.
    assert_eq!(plan(60.0, 30.0, (16, 16), anim::MAX_FRAMES).0, anim::MAX_FRAMES);
    assert_eq!(plan(3.0, 30.0, (100, 100), 1), (1, (100, 100)));
    assert_eq!(target(320.0, 160.0, Fit::Contain(100, 100)), (100, 50));
    assert_eq!(target(10.0, 10.0, Fit::Cover(64)), (64, 64));
}

#[test]
fn hostile_files_fail_without_panic() {
    let bad: Vec<Vec<u8>> = vec![
        b"{\"layers\": 3}".to_vec(),
        b"{\"layers\": [], \"w\": 0, \"h\": 100, \"fr\": 30, \"op\": 10}".to_vec(),
        b"{\"layers\": [], \"w\": 100, \"h\": 100, \"fr\": 0, \"op\": 10}".to_vec(),
        b"{\"layers\": [], \"w\": 100, \"h\": 100, \"fr\": 30, \"ip\": 10, \"op\": 5}".to_vec(),
        b"{\"layers\": [], \"w\": 1e9, \"h\": 100, \"fr\": 30, \"op\": 5}".to_vec(),
        b"{\"layers\": [{\"ty\": 4}], \"w\": 100, \"h\": 100, \"fr\": 30, \"op\": 5".to_vec(),
        comp(json!([{"ty": 0, "refId": "soi", "ks": ks(), "ip": 0, "op": 30}]), json!([{"id": "soi", "layers": [{"ty": 0, "refId": "soi", "ks": ks(), "ip": 0, "op": 30}]}])),
    ];
    for (i, b) in bad.iter().enumerate() {
        let r = anim::decode_frames(b, 256, Fit::Contain(32, 32));
        // La précomposition qui se contient elle-même se dessine vide, sans boucle.
        assert_eq!(r.is_ok(), i == 6, "cas {i}");
    }
    // Calques sans rien de connu, formes inconnues, parent en boucle : image vide.
    let odd = comp(
        json!([
            {"ty": 99, "ks": ks()},
            {"ty": 4, "ind": 1, "parent": 2, "ip": 0, "op": 30, "ks": ks(), "shapes": [{"ty": "zz"}, {"ty": "fl", "c": fixed(json!([1, 0, 0, 1])), "o": fixed(json!(100))}, {"ty": "sr", "pt": fixed(json!(1e9)), "or": fixed(json!(5))}]},
            {"ty": 3, "ind": 2, "parent": 1, "ip": 0, "op": 30, "ks": ks()}
        ]),
        json!([]),
    );
    assert!(first(&odd, 20).pixels.iter().all(|p| *p == Color32::TRANSPARENT));
    // Trop de formes : refusé à la lecture.
    let many: Vec<Value> = (0..25_000).map(|_| json!({"ty": "zz"})).collect();
    assert!(frames(&comp(json!([shape_layer(Value::Array(many))]), json!([])), Fit::Contain(8, 8), 2).is_err());
    // Groupes imbriqués sans fin.
    let mut deep = json!({"ty": "gr", "it": []});
    for _ in 0..60 {
        deep = json!({"ty": "gr", "it": [deep]});
    }
    assert!(frames(&comp(json!([shape_layer(json!([deep]))]), json!([])), Fit::Contain(8, 8), 2).is_err());
}

/// Outil de mise au point : dessine le fichier `LOTTIE_FILE` en PNG dans le
/// dossier `LOTTIE_OUT` (images 0, un quart, moitié, trois quarts), et dit
/// ce que la lecture a ignoré. `cargo test --lib lottie_file_to_png -- --ignored --nocapture`.
#[test]
#[ignore]
fn lottie_file_to_png() {
    let (Ok(file), Ok(out)) = (std::env::var("LOTTIE_FILE"), std::env::var("LOTTIE_OUT")) else { return };
    let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{file} : {e}"));
    assert!(is_lottie(&bytes), "pas reconnu comme Lottie");
    let t = std::time::Instant::now();
    let f = frames(&bytes, Fit::Contain(320, 320), anim::MAX_FRAMES).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("{} images de {:?}, {:?} chacune, rendues en {:?}", f.len(), f[0].0.size, f[0].1, t.elapsed());
    let stem = std::path::Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or("lottie").to_string();
    for q in 0..4 {
        let img = &f[f.len() * q / 4].0;
        let rgba: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect();
        let png = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba).unwrap_or_else(|| panic!("image"));
        png.save(format!("{out}/{stem}-{q}.png")).unwrap_or_else(|e| panic!("{e}"));
    }
}
