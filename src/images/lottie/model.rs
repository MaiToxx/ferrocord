//! Lecture d'une animation Lottie (JSON de Bodymovin) : calques, formes et
//! propriétés animées, dans la forme dont le dessin a besoin. Ce qui n'est
//! pas compris est ignoré (calque ou forme `Other`), jamais une erreur :
//! une animation partiellement rendue vaut mieux que rien.
//!
//! Bornes : le JSON vient du réseau. Nombre total de calques et de formes,
//! profondeur des groupes et points par tracé sont plafonnés ; au-delà, la
//! lecture échoue.

use serde_json::Value;
use std::collections::HashMap;

/// Calques et formes, au plus, toutes compositions confondues.
const MAX_NODES: usize = 20_000;
/// Points d'un tracé, au plus.
const MAX_POINTS: usize = 4_000;
/// Images clés d'une propriété, au plus.
const MAX_KEYS: usize = 2_000;
/// Groupes imbriqués, au plus.
const MAX_DEPTH: usize = 24;

pub struct Comp {
    pub w: f64,
    pub h: f64,
    /// Première image et image de fin (exclue).
    pub ip: f64,
    pub op: f64,
    /// Images par seconde.
    pub fr: f64,
    pub layers: Vec<Layer>,
    /// Précompositions, par identifiant.
    pub assets: HashMap<String, Vec<Layer>>,
}

pub struct Layer {
    pub kind: Kind,
    pub ind: Option<i64>,
    pub parent: Option<i64>,
    pub ip: f64,
    pub op: f64,
    /// Décalage du temps du calque, et étirement.
    pub st: f64,
    pub sr: f64,
    pub hidden: bool,
    pub ks: Transform,
    pub masks: Vec<Mask>,
    /// Ce calque est découpé par le calque qui le précède.
    pub matte: Option<Matte>,
    /// Ce calque sert de cache au suivant : il n'est pas dessiné lui-même.
    pub is_matte: bool,
    /// Temps de la précomposition, en secondes, quand il est remappé.
    pub time_remap: Option<Prop>,
}

pub enum Kind {
    Precomp { id: String, w: f64, h: f64 },
    Solid { color: [f64; 3], w: f64, h: f64 },
    Null,
    Shape(Vec<Item>),
    /// Image, texte, caméra… : rien n'est dessiné.
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Matte {
    Alpha,
    AlphaInverted,
    Luma,
    LumaInverted,
}

pub struct Mask {
    pub path: PathProp,
    pub inverted: bool,
    /// `a` (ajout), `s` (soustraction), `i` (intersection) ; `n` est écarté à la lecture.
    pub mode: char,
}

pub struct Transform {
    pub anchor: Prop,
    pub pos: Position,
    pub scale: Prop,
    pub rot: Prop,
    pub opacity: Prop,
    pub skew: Prop,
    pub skew_axis: Prop,
}

pub enum Position {
    Joined(Prop),
    Split(Prop, Prop),
}

pub enum Item {
    Group { items: Vec<Item>, tr: Option<Transform> },
    Path(PathProp),
    Ellipse { p: Prop, s: Prop },
    Rect { p: Prop, s: Prop, r: Prop },
    Star { p: Prop, points: Prop, rot: Prop, outer: Prop, inner: Prop, polygon: bool },
    Fill { color: Prop, opacity: Prop, even_odd: bool },
    Stroke { color: Prop, opacity: Prop, line: Line },
    GradientFill { grad: Gradient, opacity: Prop, even_odd: bool },
    GradientStroke { grad: Gradient, opacity: Prop, line: Line },
    Trim { start: Prop, end: Prop, offset: Prop, sequential: bool },
    Other,
}

pub struct Line {
    pub width: Prop,
    /// 1 : coupé, 2 : rond, 3 : carré.
    pub cap: u8,
    /// 1 : onglet, 2 : rond, 3 : biseau.
    pub join: u8,
    pub miter: f64,
    /// Tirets et espaces, dans l'ordre, puis décalage.
    pub dashes: Vec<Prop>,
    pub dash_offset: Option<Prop>,
}

pub struct Gradient {
    pub radial: bool,
    pub start: Prop,
    pub end: Prop,
    /// Nombre d'arrêts de couleur ; `stops` les porte (position, r, v, b),
    /// suivis des arrêts d'alpha (position, alpha).
    pub count: usize,
    pub stops: Prop,
}

/// Propriété numérique, animée ou non (une seule clé).
pub struct Prop {
    pub keys: Vec<Key>,
}

pub struct Key {
    pub t: f64,
    pub v: Vec<f64>,
    /// Valeur d'arrivée (ancien format ; sinon la valeur de la clé suivante).
    pub end: Option<Vec<f64>>,
    pub hold: bool,
    /// Courbe d'accélération : points de contrôle de sortie et d'entrée.
    pub ease: [f64; 4],
    /// Tangentes de la trajectoire (position).
    pub to: Option<Vec<f64>>,
    pub ti: Option<Vec<f64>>,
}

#[derive(Clone, Default)]
pub struct PathData {
    pub v: Vec<(f64, f64)>,
    pub i: Vec<(f64, f64)>,
    pub o: Vec<(f64, f64)>,
    pub closed: bool,
}

pub struct PathProp {
    pub keys: Vec<PathKey>,
}

pub struct PathKey {
    pub t: f64,
    pub shape: PathData,
    pub end: Option<PathData>,
    pub hold: bool,
    pub ease: [f64; 4],
}

struct Reader {
    nodes: usize,
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        // Certains exports rangent un nombre seul dans un tableau.
        Value::Array(a) => a.first().and_then(num),
        _ => None,
    }
}

fn nums(v: &Value) -> Vec<f64> {
    match v {
        Value::Array(a) => a.iter().take(512).filter_map(|x| x.as_f64().filter(|x| x.is_finite())).collect(),
        other => num(other).into_iter().collect(),
    }
}

fn flag(v: &Value) -> bool {
    num(v).is_some_and(|x| x != 0.0)
}

/// Courbe d'accélération d'une clé (`o` puis `i`, chacun `{x, y}` en nombre ou en tableau).
fn ease(k: &Value) -> [f64; 4] {
    let part = |v: &Value, d: f64| num(v).unwrap_or(d);
    [part(&k["o"]["x"], 0.0), part(&k["o"]["y"], 0.0), part(&k["i"]["x"], 1.0), part(&k["i"]["y"], 1.0)]
}

impl Prop {
    pub fn constant(v: &[f64]) -> Prop {
        Prop { keys: vec![Key { t: 0.0, v: v.to_vec(), end: None, hold: true, ease: [0.0, 0.0, 1.0, 1.0], to: None, ti: None }] }
    }

    /// Lit `{a, k}` ; absente ou illisible : `default`.
    fn read(v: &Value, default: &[f64]) -> Option<Prop> {
        let k = &v["k"];
        let animated = k.as_array().is_some_and(|a| a.first().is_some_and(Value::is_object));
        if !animated {
            let value = nums(k);
            return Some(Prop::constant(if value.is_empty() { default } else { &value }));
        }
        let list = k.as_array()?;
        if list.len() > MAX_KEYS {
            return None;
        }
        let mut keys: Vec<Key> = Vec::with_capacity(list.len());
        for e in list {
            let t = num(&e["t"])?;
            let mut value = nums(&e["s"]);
            if value.is_empty() {
                // Dernière clé de l'ancien format : la valeur d'arrivée de la précédente.
                value = keys.last().map(|p: &Key| p.end.clone().unwrap_or_else(|| p.v.clone())).unwrap_or_else(|| default.to_vec());
            }
            let end = Some(nums(&e["e"])).filter(|e| !e.is_empty());
            let tangent = |v: &Value| Some(nums(v)).filter(|t| t.iter().any(|x| *x != 0.0));
            keys.push(Key { t, v: value, end, hold: flag(&e["h"]), ease: ease(e), to: tangent(&e["to"]), ti: tangent(&e["ti"]) });
        }
        if keys.is_empty() {
            return Some(Prop::constant(default));
        }
        Some(Prop { keys })
    }
}

fn points(v: &Value) -> Option<Vec<(f64, f64)>> {
    let a = v.as_array()?;
    if a.len() > MAX_POINTS {
        return None;
    }
    a.iter()
        .map(|p| {
            let p = nums(p);
            Some((*p.first()?, *p.get(1)?))
        })
        .collect()
}

fn path_data(v: &Value) -> Option<PathData> {
    // Une clé range sa forme dans un tableau d'un élément.
    let v = if v.is_array() { &v[0] } else { v };
    let (pv, pi, po) = (points(&v["v"])?, points(&v["i"])?, points(&v["o"])?);
    if pi.len() != pv.len() || po.len() != pv.len() {
        return None;
    }
    Some(PathData { v: pv, i: pi, o: po, closed: flag(&v["c"]) })
}

impl PathProp {
    fn read(v: &Value) -> Option<PathProp> {
        let k = &v["k"];
        let Some(list) = k.as_array().filter(|a| a.first().is_some_and(|e| e.get("t").is_some())) else {
            let shape = path_data(k)?;
            return Some(PathProp { keys: vec![PathKey { t: 0.0, shape, end: None, hold: true, ease: [0.0, 0.0, 1.0, 1.0] }] });
        };
        if list.len() > MAX_KEYS {
            return None;
        }
        let mut keys: Vec<PathKey> = Vec::with_capacity(list.len());
        for e in list {
            let t = num(&e["t"])?;
            let shape = match path_data(&e["s"]) {
                Some(s) => s,
                None => keys.last().map(|p| p.end.clone().unwrap_or_else(|| p.shape.clone()))?,
            };
            keys.push(PathKey { t, shape, end: path_data(&e["e"]), hold: flag(&e["h"]), ease: ease(e) });
        }
        (!keys.is_empty()).then_some(PathProp { keys })
    }
}

impl Transform {
    fn read(v: &Value) -> Option<Transform> {
        let p = &v["p"];
        let pos = if flag(&p["s"]) { Position::Split(Prop::read(&p["x"], &[0.0])?, Prop::read(&p["y"], &[0.0])?) } else { Position::Joined(Prop::read(p, &[0.0, 0.0])?) };
        Some(Transform {
            anchor: Prop::read(&v["a"], &[0.0, 0.0])?,
            pos,
            scale: Prop::read(&v["s"], &[100.0, 100.0])?,
            rot: Prop::read(if v["r"].is_object() { &v["r"] } else { &v["rz"] }, &[0.0])?,
            opacity: Prop::read(&v["o"], &[100.0])?,
            skew: Prop::read(&v["sk"], &[0.0])?,
            skew_axis: Prop::read(&v["sa"], &[0.0])?,
        })
    }
}

impl Reader {
    fn node(&mut self) -> Option<()> {
        self.nodes += 1;
        (self.nodes <= MAX_NODES).then_some(())
    }

    fn line(v: &Value) -> Option<Line> {
        let mut dashes = Vec::new();
        let mut dash_offset = None;
        for d in v["d"].as_array().into_iter().flatten().take(16) {
            let p = Prop::read(&d["v"], &[0.0])?;
            if d["n"] == "o" {
                dash_offset = Some(p);
            } else {
                dashes.push(p);
            }
        }
        Some(Line { width: Prop::read(&v["w"], &[1.0])?, cap: num(&v["lc"]).unwrap_or(1.0) as u8, join: num(&v["lj"]).unwrap_or(1.0) as u8, miter: num(&v["ml"]).unwrap_or(4.0), dashes, dash_offset })
    }

    fn gradient(v: &Value) -> Option<Gradient> {
        let count = num(&v["g"]["p"])? as usize;
        if count == 0 || count > 64 {
            return None;
        }
        Some(Gradient { radial: num(&v["t"]) == Some(2.0), start: Prop::read(&v["s"], &[0.0, 0.0])?, end: Prop::read(&v["e"], &[0.0, 0.0])?, count, stops: gradient_stops(&v["g"]["k"])? })
    }

    fn items(&mut self, v: &Value, depth: usize) -> Option<Vec<Item>> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut out = Vec::new();
        for it in v.as_array().into_iter().flatten() {
            self.node()?;
            if flag(&it["hd"]) {
                continue;
            }
            let even_odd = num(&it["r"]) == Some(2.0);
            let item = match it["ty"].as_str().unwrap_or_default() {
                "gr" => {
                    let list = &it["it"];
                    let tr = list.as_array().into_iter().flatten().find(|x| x["ty"] == "tr").and_then(Transform::read);
                    Item::Group { items: self.items(list, depth + 1)?, tr }
                }
                "sh" => PathProp::read(&it["ks"]).map_or(Item::Other, Item::Path),
                "el" => Item::Ellipse { p: Prop::read(&it["p"], &[0.0, 0.0])?, s: Prop::read(&it["s"], &[0.0, 0.0])? },
                "rc" => Item::Rect { p: Prop::read(&it["p"], &[0.0, 0.0])?, s: Prop::read(&it["s"], &[0.0, 0.0])?, r: Prop::read(&it["r"], &[0.0])? },
                "sr" => Item::Star {
                    p: Prop::read(&it["p"], &[0.0, 0.0])?,
                    points: Prop::read(&it["pt"], &[5.0])?,
                    rot: Prop::read(&it["r"], &[0.0])?,
                    outer: Prop::read(&it["or"], &[0.0])?,
                    inner: Prop::read(&it["ir"], &[0.0])?,
                    polygon: num(&it["sy"]) == Some(2.0),
                },
                "fl" => Item::Fill { color: Prop::read(&it["c"], &[0.0, 0.0, 0.0, 1.0])?, opacity: Prop::read(&it["o"], &[100.0])?, even_odd },
                "st" => Item::Stroke { color: Prop::read(&it["c"], &[0.0, 0.0, 0.0, 1.0])?, opacity: Prop::read(&it["o"], &[100.0])?, line: Self::line(it)? },
                "gf" => Self::gradient(it).map_or(Item::Other, |grad| Item::GradientFill { grad, opacity: Prop::constant(&[100.0]), even_odd }),
                "gs" => match (Self::gradient(it), Self::line(it)) {
                    (Some(grad), Some(line)) => Item::GradientStroke { grad, opacity: Prop::constant(&[100.0]), line },
                    _ => Item::Other,
                },
                "tm" => Item::Trim { start: Prop::read(&it["s"], &[0.0])?, end: Prop::read(&it["e"], &[100.0])?, offset: Prop::read(&it["o"], &[0.0])?, sequential: num(&it["m"]) == Some(2.0) },
                _ => Item::Other,
            };
            // L'opacité d'un dégradé est lue à part : `Prop::read` peut échouer.
            let item = match item {
                Item::GradientFill { grad, even_odd, .. } => Item::GradientFill { grad, opacity: Prop::read(&it["o"], &[100.0])?, even_odd },
                Item::GradientStroke { grad, line, .. } => Item::GradientStroke { grad, opacity: Prop::read(&it["o"], &[100.0])?, line },
                other => other,
            };
            out.push(item);
        }
        Some(out)
    }

    fn layers(&mut self, v: &Value) -> Option<Vec<Layer>> {
        let mut out = Vec::new();
        for l in v.as_array()? {
            self.node()?;
            let kind = match num(&l["ty"]).map(|t| t as i64) {
                Some(0) => Kind::Precomp { id: l["refId"].as_str()?.to_string(), w: num(&l["w"]).unwrap_or(0.0), h: num(&l["h"]).unwrap_or(0.0) },
                Some(1) => Kind::Solid { color: hex_color(l["sc"].as_str().unwrap_or_default()), w: num(&l["sw"]).unwrap_or(0.0), h: num(&l["sh"]).unwrap_or(0.0) },
                Some(3) => Kind::Null,
                Some(4) => Kind::Shape(self.items(&l["shapes"], 0)?),
                _ => Kind::Other,
            };
            let mut masks = Vec::new();
            for m in l["masksProperties"].as_array().into_iter().flatten().take(32) {
                let mode = m["mode"].as_str().and_then(|s| s.chars().next()).unwrap_or('a');
                if mode == 'n' {
                    continue;
                }
                if let Some(path) = PathProp::read(&m["pt"]) {
                    masks.push(Mask { path, inverted: flag(&m["inv"]), mode });
                }
            }
            let matte = match num(&l["tt"]).map(|t| t as i64) {
                Some(1) => Some(Matte::Alpha),
                Some(2) => Some(Matte::AlphaInverted),
                Some(3) => Some(Matte::Luma),
                Some(4) => Some(Matte::LumaInverted),
                _ => None,
            };
            let sr = num(&l["sr"]).filter(|s| *s > 0.0).unwrap_or(1.0);
            out.push(Layer {
                kind,
                ind: num(&l["ind"]).map(|x| x as i64),
                parent: num(&l["parent"]).map(|x| x as i64),
                ip: num(&l["ip"]).unwrap_or(0.0),
                op: num(&l["op"]).unwrap_or(f64::MAX),
                st: num(&l["st"]).unwrap_or(0.0),
                sr,
                hidden: flag(&l["hd"]),
                ks: Transform::read(&l["ks"])?,
                masks,
                matte,
                is_matte: flag(&l["td"]),
                time_remap: if l["tm"].is_object() { Prop::read(&l["tm"], &[0.0]) } else { None },
            });
        }
        Some(out)
    }
}

/// Arrêts d'un dégradé : une propriété dont chaque valeur est la liste entière.
fn gradient_stops(v: &Value) -> Option<Prop> {
    Prop::read(v, &[])
}

/// `#rrggbb` en composantes de 0 à 1 ; illisible : noir.
fn hex_color(s: &str) -> [f64; 3] {
    let s = s.trim_start_matches('#');
    let c = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).map_or(0.0, |v| f64::from(v) / 255.0);
    [c(0), c(2), c(4)]
}

/// Lit une animation. `None` : pas une animation Lottie, ou hors bornes.
pub fn parse(bytes: &[u8]) -> Option<Comp> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let (w, h) = (num(&v["w"])?, num(&v["h"])?);
    let (ip, op, fr) = (num(&v["ip"]).unwrap_or(0.0), num(&v["op"])?, num(&v["fr"])?);
    if !(1.0..=8192.0).contains(&w) || !(1.0..=8192.0).contains(&h) || !(fr > 0.0 && fr <= 240.0) || op <= ip || op - ip > 100_000.0 {
        return None;
    }
    let mut r = Reader { nodes: 0 };
    let layers = r.layers(&v["layers"])?;
    let mut assets = HashMap::new();
    for a in v["assets"].as_array().into_iter().flatten().take(512) {
        if let (Some(id), true) = (a["id"].as_str(), a["layers"].is_array()) {
            assets.insert(id.to_string(), r.layers(&a["layers"])?);
        }
    }
    Some(Comp { w, h, ip, op, fr, layers, assets })
}
