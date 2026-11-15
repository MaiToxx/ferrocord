//! Icônes dessinées, pleines, sur une grille de 24 (`docs/design.md`) : aplats
//! et traits épais aux bouts ronds, dans l'esprit de celles du client de bureau
//! sans en reprendre un tracé. La police par défaut n'a pas de pictogrammes
//! (« ⚙ », « 📢 », « 📎 » s'affichent en carrés vides), et embarquer une
//! police d'icônes coûterait plusieurs centaines de Ko. Chaque icône est
//! inscrite dans le carré qu'on lui donne (taille venant de la typographie
//! du thème) et prend la couleur demandée : aucune taille fixe ici, seulement
//! des proportions.

use eframe::egui::{
    emath::TSTransform,
    epaint::{ClippedPrimitive, ClippedShape, Primitive},
    layers::ShapeIdx,
    pos2, Color32, Context, Id, LayerId, Mesh, Order, Painter, Rect, Shape, Stroke, TextureId, Vec2,
};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// Salon textuel.
    Hash,
    /// Salon d'annonces (porte-voix).
    Megaphone,
    /// Pièce jointe (trombone).
    Paperclip,
    /// Apparence (roue dentée).
    Gear,
    /// Déconnexion (flèche qui sort d'une porte).
    Logout,
    /// Fermer (croix).
    Close,
    /// Action interdite (cadenas).
    Lock,
    /// Attente d'une échéance (sablier).
    Hourglass,
    /// Brouillon en cours (crayon).
    Pencil,
    /// Aller en bas de la liste (flèche vers le bas).
    ArrowDown,
    /// Vidéo (triangle de lecture).
    Play,
    /// Fichier (feuille au coin plié).
    File,
    /// Son (croche).
    Audio,
    /// Liste des membres (deux silhouettes).
    Members,
    /// Fil de discussion (bulle avec un trait qui en part).
    Thread,
    /// Salon forum (deux bulles).
    Forum,
    /// Salon média (cadre d'image).
    Media,
    /// Dossier de serveurs déplié (chemise à onglet).
    Folder,
    /// Ajouter (croix droite).
    Plus,
    /// Boîte de réception (bac à courrier).
    Inbox,
    /// Sondage (barres horizontales, lot T13).
    Poll,
    /// Arrivée (flèche vers la droite, lot T13).
    ArrowRight,
    /// Messages privés (bulle pleine).
    Chat,
    /// Événements (calendrier).
    Calendar,
    /// Recherche (loupe).
    Search,
    /// Chevron vers le bas (en-tête de serveur, catégorie).
    ChevronDown,
    /// Joindre (plus dans un cercle).
    PlusCircle,
    /// Salon textuel privé (dièse et cadenas).
    HashLock,
    /// Salon vocal (haut-parleur).
    Speaker,
    /// Salon vocal privé.
    SpeakerLock,
    /// Conférence (silhouette entourée d'ondes).
    Stage,
    /// Notifications (cloche).
    Bell,
    /// Notifications coupées (cloche barrée).
    BellOff,
    /// Messages épinglés (punaise).
    Pin,
    /// Aide (point d'interrogation dans un disque).
    Help,
    /// Sélecteur de GIF.
    Gif,
    /// Autocollants (carré au coin décollé).
    Sticker,
    /// Emojis (visage souriant).
    Smile,
    /// Applications (quatre carrés).
    Apps,
    /// Micro.
    Mic,
    /// Micro coupé.
    MicOff,
    /// Casque.
    Headphones,
    /// Casque coupé.
    HeadphonesOff,
    /// Appel (combiné).
    Phone,
    /// Raccrocher (combiné couché).
    HangUp,
    /// Caméra.
    Camera,
    /// Caméra coupée.
    CameraOff,
    /// Partage d'écran (moniteur).
    Screen,
    /// Partage arrêté.
    ScreenOff,
    /// Qualité de la connexion (trois barres).
    Signal,
    /// Supprimer (corbeille).
    Trash,
    /// Répondre (flèche courbe vers la gauche).
    Reply,
    /// Transférer (flèche courbe vers la droite).
    Forward,
    /// Plus d'actions (trois points en ligne).
    More,
    /// Plus d'actions (trois points en colonne).
    MoreVertical,
    /// Lien (deux maillons).
    Link,
    /// Copier (deux feuilles).
    Copy,
    /// Copier l'identifiant (carte « ID »).
    Id,
    /// Signaler (drapeau).
    Flag,
    /// Propriétaire du serveur (couronne).
    Crown,
    /// Boost du serveur (gemme).
    Boost,
    /// Ajouter un ami (silhouette et plus).
    FriendAdd,
    /// Fait, choisi (coche).
    Check,
    /// Chevrons (sous-menu, repli, retour).
    ChevronRight,
    ChevronUp,
    ChevronLeft,
    /// Découvrir (boussole).
    Compass,
    /// Télécharger (flèche vers un bac).
    Download,
    /// Favoris (étoile).
    Star,
    /// Récents (horloge).
    Clock,
    /// Voir, révéler (œil).
    Eye,
    /// Pause (deux barres).
    Pause,
    /// Plein écran (quatre coins).
    Fullscreen,
    /// Quêtes (médaillon entouré d'une couronne).
    Quest,
    /// Filtre (trois réglettes).
    Sliders,
    /// Éditeur vérifié (pastille festonnée et coche).
    Verified,
    /// Amis (silhouette qui salue de la main).
    Wave,
    /// Nitro (disque lancé et ses traits de vitesse).
    Nitro,
    /// Boutique (devanture et son auvent).
    Shop,
    /// Compte (silhouette seule).
    User,
    /// Données et confidentialité (bouclier au cadenas).
    ShieldLock,
    /// Confidentialité des messages (bulle et bouclier).
    ChatShield,
    /// Apparence (palette de peintre).
    Palette,
    /// Accessibilité (personnage dans un disque).
    Accessibility,
    /// Raccourcis clavier (clavier).
    Keyboard,
    /// Langue (idéogramme et lettre « A »).
    Language,
    /// Jeux, activité (manette).
    Gamepad,
    /// Plugins (pièce de puzzle).
    Puzzle,
    /// À propos (« i » dans un disque).
    Info,
    /// Profil du correspondant (silhouette dans un disque).
    UserCircle,
    /// Précédent (flèche vers la gauche).
    ArrowLeft,
}

impl Icon {
    /// Toutes les icônes (planche de l'aperçu, tests).
    pub const ALL: &[Icon] = &[
        Icon::Hash, Icon::HashLock, Icon::Megaphone, Icon::Speaker, Icon::SpeakerLock, Icon::Stage, Icon::Forum, Icon::Media, Icon::Thread,
        Icon::Bell, Icon::BellOff, Icon::Pin, Icon::Members, Icon::Search, Icon::Inbox, Icon::Help, Icon::PlusCircle, Icon::Gif, Icon::Sticker,
        Icon::Smile, Icon::Apps, Icon::Mic, Icon::MicOff, Icon::Headphones, Icon::HeadphonesOff, Icon::Gear, Icon::Phone, Icon::HangUp,
        Icon::Camera, Icon::CameraOff, Icon::Screen, Icon::ScreenOff, Icon::Signal, Icon::Pencil, Icon::Trash, Icon::Reply, Icon::Forward,
        Icon::More, Icon::MoreVertical, Icon::Link, Icon::Copy, Icon::Id, Icon::Flag, Icon::Crown, Icon::Boost, Icon::FriendAdd, Icon::Chat,
        Icon::Close, Icon::Check, Icon::ChevronDown, Icon::ChevronRight, Icon::ChevronUp, Icon::ChevronLeft, Icon::Compass, Icon::Download,
        Icon::Folder, Icon::Calendar, Icon::Star, Icon::Clock, Icon::Eye, Icon::Lock, Icon::Hourglass, Icon::File, Icon::Play, Icon::Pause,
        Icon::Fullscreen, Icon::Paperclip, Icon::Logout, Icon::ArrowDown, Icon::ArrowRight, Icon::Audio, Icon::Plus, Icon::Poll,
        Icon::Quest, Icon::Sliders, Icon::Verified, Icon::Wave, Icon::Nitro, Icon::Shop,
        Icon::User, Icon::ShieldLock, Icon::ChatShield, Icon::Palette, Icon::Accessibility, Icon::Keyboard, Icon::Language, Icon::Gamepad,
        Icon::Puzzle, Icon::Info, Icon::UserCircle, Icon::ArrowLeft,
    ];
}

/// Dessine `icon` dans `rect` (carré conseillé), de la couleur `color`.
///
/// Lot C10 : une icône est faite de plusieurs formes (un « # » : quatre traits
/// et huit bouts ronds), et les construire puis les trianguler à chaque image
/// coûtait un tiers de la tessellation de l'écran. Le tracé n'est donc
/// construit qu'une fois par icône, couleur et emplacement exact (`Meshes`) ;
/// les images suivantes posent un seul maillage, déjà triangulé par egui
/// lui-même avec ses réglages, au même endroit : le dessin est le même, au
/// triangle près. (Réutiliser un maillage à un autre emplacement en le
/// décalant ne l'est pas : l'accroche aux pixels dépend de la position.)
/// Une liste qui défile refait donc ses icônes, comme avant.
pub fn paint(icon: Icon, painter: &Painter, rect: Rect, color: Color32) {
    let side = rect.width().min(rect.height());
    // Hors de la zone visible du peintre (ligne d'une liste sortie de la vue) :
    // rien à faire ; un tracé ne sort pas de son carré de plus d'un demi-trait.
    if !side.is_finite() || side <= 0.0 || !painter.clip_rect().intersects(rect.expand(side * 0.12 + 1.0)) {
        return;
    }
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let key = Key { icon, rect: [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(f32::to_bits), color, ppp: ppp.to_bits() };
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<std::sync::Arc<std::sync::Mutex<Meshes>>>(meshes_id()).clone());
    let found = cache.lock().ok().and_then(|mut c| {
        c.begin_pass(ctx);
        if let Some(m) = c.map.get(&key) {
            return Some(m.clone());
        }
        let m = record(ctx, icon, rect, color, ppp)?;
        if c.map.len() >= MAX_MESHES {
            c.map.clear();
        }
        c.map.insert(key, m.clone());
        Some(m)
    });
    match found {
        Some(m) => {
            painter.add(Shape::mesh(m));
        }
        // Tracé qui ne se réduit pas à un maillage simple : dessiné directement.
        None => draw(icon, painter, rect, color),
    }
}

/// Maillages gardés au plus ; au-delà, tout est refait (un survol en fondu
/// passe par une dizaine de couleurs, une liste qui défile par une position
/// par image).
const MAX_MESHES: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    icon: Icon,
    rect: [u32; 4],
    color: Color32,
    ppp: u32,
}

/// Maillages des icônes déjà tracées, par contexte egui. Les bouts ronds
/// viennent des disques de l'atlas des polices : tout est refait quand l'atlas
/// change de taille ou repart de zéro (comme `ui::text_cache`).
#[derive(Default)]
struct Meshes {
    map: std::collections::HashMap<Key, std::sync::Arc<Mesh>>,
    pass: Option<u64>,
    atlas: [usize; 2],
    fill: f32,
}

impl Meshes {
    fn begin_pass(&mut self, ctx: &Context) {
        let pass = ctx.cumulative_pass_nr();
        if self.pass == Some(pass) {
            return;
        }
        self.pass = Some(pass);
        let (atlas, fill) = ctx.fonts(|f| (f.font_image_size(), f.font_atlas_fill_ratio()));
        if atlas != self.atlas || fill < self.fill {
            self.map.clear();
        }
        (self.atlas, self.fill) = (atlas, fill);
    }
}

fn meshes_id() -> Id {
    Id::unique("ferrocord-icones-maillages")
}

/// Trace `icon` à l'écart (couche de brouillon, vidée aussitôt) et triangule
/// le résultat. `None` : le tracé contient autre chose que des formes pleines
/// (texte, image), ou ne tient pas en un maillage.
fn record(ctx: &Context, icon: Icon, rect: Rect, color: Color32, ppp: f32) -> Option<std::sync::Arc<Mesh>> {
    let layer = LayerId::new(Order::Background, Id::unique("ferrocord-icones-brouillon"));
    let scratch = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
    let from = scratch.add(Shape::Noop).0;
    draw(icon, &scratch, rect, color);
    let shapes: Vec<Shape> = ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let shapes = list.all_entries().skip(from).map(|s| s.shape.clone()).collect();
        for i in from..list.next_idx().0 {
            list.reset_shape(ShapeIdx(i));
        }
        shapes
    });
    fn plain(s: &Shape) -> bool {
        match s {
            Shape::Vec(v) => v.iter().all(plain),
            Shape::Rect(r) => r.brush.is_none(),
            Shape::Mesh(m) => m.texture_id == TextureId::default(),
            Shape::Noop | Shape::Circle(_) | Shape::Ellipse(_) | Shape::LineSegment { .. } | Shape::Path(_) | Shape::QuadraticBezier(_) | Shape::CubicBezier(_) => true,
            _ => false,
        }
    }
    if !shapes.iter().all(plain) {
        return None;
    }
    let clipped = ClippedShape { clip_rect: Rect::EVERYTHING, shape: Shape::Vec(shapes), transform_after_tessellation: TSTransform::IDENTITY };
    let mut out = ctx.tessellate(vec![clipped], ppp);
    match (out.pop(), out.is_empty()) {
        (Some(ClippedPrimitive { primitive: Primitive::Mesh(m), .. }), true) if m.texture_id == TextureId::default() => Some(std::sync::Arc::new(m)),
        _ => None,
    }
}

/// Le tracé lui-même, forme par forme.
fn draw(icon: Icon, painter: &Painter, rect: Rect, color: Color32) {
    if filled(icon, painter, rect, color) {
        return;
    }
    let side = rect.width().min(rect.height());
    let c = rect.center();
    // Repère : unité = moitié du côté, origine au centre.
    let u = side * 0.5;
    let p = |x: f32, y: f32| pos2(c.x + x * u, c.y + y * u);
    let line = |w: f32| Stroke::new((side * w).max(1.0), color);
    match icon {
        Icon::Paperclip => {
            // Trombone vertical d'un seul trait (trois branches), penché à 45°.
            let mut pts = vec![(-0.3, -0.45), (-0.3, 0.5)];
            pts.extend(arc_points((0.0, 0.5), 0.3, 0.5, 0.0));
            pts.push((0.3, -0.55));
            pts.extend(arc_points((0.08, -0.55), 0.22, 0.0, -0.5));
            pts.push((-0.14, 0.4));
            let (sin, cos) = (TAU / 8.0).sin_cos();
            let pts = pts.into_iter().map(|(x, y)| p(x * cos - y * sin, x * sin + y * cos)).collect();
            painter.add(Shape::line(pts, line(0.09)));
        }
        Icon::Gear => {
            let r = u * 0.84;
            painter.circle_stroke(c, r * 0.55, Stroke::new(r * 0.3, color));
            for i in 0..8 {
                let dir = Vec2::angled(i as f32 * TAU / 8.0);
                painter.line_segment([c + dir * (r * 0.6), c + dir * r], Stroke::new(r * 0.34, color));
            }
        }
        Icon::Pencil => {
            // Corps penché à 45°, pointe en bas à gauche.
            let body = vec![p(0.45, -0.75), p(0.75, -0.45), p(-0.35, 0.65), p(-0.65, 0.35)];
            painter.add(Shape::convex_polygon(body, color, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![p(-0.65, 0.35), p(-0.35, 0.65), p(-0.8, 0.8)], color, Stroke::NONE));
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(vec![p(-0.45, -0.7), p(0.7, 0.0), p(-0.45, 0.7)], color, Stroke::NONE));
        }
        Icon::Audio => {
            painter.line_segment([p(0.3, -0.75), p(0.3, 0.45)], line(0.1));
            painter.line_segment([p(0.3, -0.75), p(0.7, -0.45)], line(0.1));
            painter.circle_filled(p(0.0, 0.5), u * 0.32, color);
        }
        Icon::Folder => {
            painter.add(Shape::convex_polygon(vec![p(-0.8, -0.6), p(-0.25, -0.6), p(-0.05, -0.38), p(-0.8, -0.38)], color, Stroke::NONE));
            painter.rect_filled(Rect::from_min_max(p(-0.8, -0.4), p(0.8, 0.65)), side * 0.06, color);
        }
        Icon::Poll => {
            for (y, len) in [(-0.55, 1.3), (0.0, 0.8), (0.55, 1.05)] {
                painter.rect_filled(Rect::from_min_max(p(-0.75, y - 0.17), p(-0.75 + len, y + 0.17)), side * 0.04, color);
            }
        }
        Icon::Chat => {
            painter.rect_filled(Rect::from_min_max(p(-0.85, -0.7), p(0.85, 0.42)), side * 0.2, color);
            painter.add(Shape::convex_polygon(vec![p(-0.5, 0.3), p(-0.05, 0.3), p(-0.55, 0.88)], color, Stroke::NONE));
        }
        Icon::PlusCircle => {
            // Disque plein, croix dans le ton opposé : lisible sur tous les fonds.
            painter.circle_filled(p(0.0, 0.0), u * 1.15, color);
            let light = u16::from(color.r()) + u16::from(color.g()) + u16::from(color.b()) > 384;
            let cross = Stroke::new((side * 0.13).max(1.5), if light { Color32::from_black_alpha(200) } else { Color32::from_white_alpha(230) });
            painter.line_segment([p(-0.55, 0.0), p(0.55, 0.0)], cross);
            painter.line_segment([p(0.0, -0.55), p(0.0, 0.55)], cross);
        }
        // Tracé sur la grille de 24 : `filled`.
        _ => {}
    }
}

/// Points d'un arc (repère de l'icône) : `from` et `to` en tours, 0 = droite,
/// 0,25 = bas (l'axe vertical de l'écran descend).
fn arc_points(center: (f32, f32), radius: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    let steps = 12;
    (0..=steps)
        .map(|i| {
            let v = Vec2::angled((from + (to - from) * i as f32 / steps as f32) * TAU) * radius;
            (center.0 + v.x, center.1 + v.y)
        })
        .collect()
}

/// Pinceau des icônes pleines : grille de 24 unités dans le carré de l'icône
/// (les tracés tiennent entre 2 et 22), aplats et traits épais aux bouts ronds.
struct Pen<'a> {
    painter: &'a Painter,
    origin: eframe::egui::Pos2,
    /// Une unité de la grille, en points.
    g: f32,
    color: Color32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> eframe::egui::Pos2 {
        pos2(self.origin.x + x * self.g, self.origin.y + y * self.g)
    }
    fn stroke(&self, w: f32) -> Stroke {
        Stroke::new((w * self.g).max(1.0), self.color)
    }
    /// Rectangle plein aux coins arrondis.
    fn rr(&self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) {
        self.painter.rect_filled(Rect::from_min_max(self.p(x0, y0), self.p(x1, y1)), r * self.g, self.color);
    }
    /// Contour de rectangle arrondi, tracé vers l'intérieur.
    fn frame(&self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, w: f32) {
        self.painter.rect_stroke(Rect::from_min_max(self.p(x0, y0), self.p(x1, y1)), r * self.g, self.stroke(w), eframe::egui::StrokeKind::Inside);
    }
    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_filled(self.p(x, y), r * self.g, self.color);
    }
    fn ring(&self, x: f32, y: f32, r: f32, w: f32) {
        self.painter.circle_stroke(self.p(x, y), r * self.g, self.stroke(w));
    }
    /// Polygone plein, convexe.
    fn poly(&self, pts: &[(f32, f32)]) {
        self.painter.add(Shape::convex_polygon(pts.iter().map(|&(x, y)| self.p(x, y)).collect(), self.color, Stroke::NONE));
    }
    /// Ligne brisée épaisse, bouts et angles arrondis.
    fn line(&self, pts: &[(f32, f32)], w: f32) {
        let s = self.stroke(w);
        self.painter.add(Shape::line(pts.iter().map(|&(x, y)| self.p(x, y)).collect(), s));
        for &(x, y) in pts {
            self.painter.circle_filled(self.p(x, y), s.width / 2.0, self.color);
        }
    }
    /// Arc épais aux bouts ronds (`from`, `to` en tours ; 0 = droite, 0,25 = bas).
    fn arc(&self, c: (f32, f32), r: f32, from: f32, to: f32, w: f32) {
        let pts = arc_points(c, r, from, to);
        let s = self.stroke(w);
        self.painter.add(Shape::line(pts.iter().map(|&(x, y)| self.p(x, y)).collect(), s));
        for &(x, y) in [pts[0], pts[pts.len() - 1]].iter() {
            self.painter.circle_filled(self.p(x, y), s.width / 2.0, self.color);
        }
    }
    /// Secteur plein (convexe : un demi-tour au plus), fermé par sa corde.
    fn cap(&self, c: (f32, f32), r: f32, from: f32, to: f32) {
        self.poly(&arc_points(c, r, from, to));
    }
    /// Le même pinceau, d'une autre couleur.
    fn with(&self, color: Color32) -> Pen<'_> {
        Pen { painter: self.painter, origin: self.origin, g: self.g, color }
    }
    /// Encre d'un détail posé sur un aplat de la couleur de l'icône : le ton
    /// opposé, lisible sur tous les fonds (comme la croix de `PlusCircle`).
    fn ink(&self) -> Pen<'_> {
        let c = self.color;
        let light = u16::from(c.r()) + u16::from(c.g()) + u16::from(c.b()) > 384;
        self.with(if light { Color32::from_black_alpha(215) } else { Color32::from_white_alpha(235) })
    }
    /// Barre d'une icône « coupée ».
    fn slash(&self) {
        self.line(&[(4.0, 3.5), (20.0, 20.5)], 2.2);
    }
    /// Dièse inscrit dans le carré de coin (`x`, `y`) et de côté `s`.
    fn hash(&self, x: f32, y: f32, s: f32) {
        let q = |u: f32, v: f32| (x + u * s, y + v * s);
        let w = 2.1 * (s / 18.0).max(0.8);
        self.line(&[q(0.40, 0.06), q(0.28, 0.94)], w);
        self.line(&[q(0.72, 0.06), q(0.60, 0.94)], w);
        self.line(&[q(0.08, 0.34), q(0.94, 0.34)], w);
        self.line(&[q(0.06, 0.66), q(0.92, 0.66)], w);
    }
    /// Petit cadenas (coin haut droit des salons privés).
    fn padlock(&self, x: f32, y: f32) {
        self.arc((x + 3.0, y + 3.4), 2.0, 0.5, 1.0, 1.5);
        self.rr(x, y + 3.2, x + 6.0, y + 8.2, 1.2);
    }
    fn speaker(&self) {
        self.rr(2.5, 9.0, 7.0, 15.0, 1.0);
        self.poly(&[(6.5, 9.0), (12.5, 4.0), (12.5, 20.0), (6.5, 15.0)]);
    }
    fn bell(&self) {
        let mut dome = arc_points((12.0, 10.0), 6.0, 0.5, 1.0);
        dome.extend([(18.0, 16.0), (6.0, 16.0)]);
        self.poly(&dome);
        self.poly(&[(6.0, 13.5), (18.0, 13.5), (20.5, 18.0), (3.5, 18.0)]);
        self.dot(12.0, 3.6, 1.3);
        self.cap((12.0, 19.2), 2.3, 0.0, 0.5);
    }
    fn mic(&self) {
        self.rr(8.5, 2.5, 15.5, 14.0, 3.5);
        self.arc((12.0, 11.0), 6.3, 0.03, 0.47, 2.0);
        self.line(&[(12.0, 17.4), (12.0, 21.0)], 2.0);
        self.line(&[(8.5, 21.0), (15.5, 21.0)], 2.0);
    }
    fn headphones(&self) {
        self.arc((12.0, 13.0), 8.0, 0.5, 1.0, 2.2);
        self.rr(2.8, 13.0, 7.6, 20.5, 2.0);
        self.rr(16.4, 13.0, 21.2, 20.5, 2.0);
    }
    fn camera(&self) {
        self.rr(2.0, 6.5, 15.5, 17.5, 2.5);
        self.poly(&[(16.5, 10.6), (22.0, 7.2), (22.0, 16.8), (16.5, 13.4)]);
    }
    fn screen(&self) {
        self.rr(2.0, 4.0, 22.0, 16.5, 2.0);
        self.rr(10.6, 16.0, 13.4, 19.2, 0.0);
        self.rr(7.5, 18.6, 16.5, 20.6, 1.0);
    }
    /// Silhouette : tête et épaules.
    fn person(&self, x: f32, head: f32, r: f32) {
        self.dot(x, head, r * 0.56);
        self.cap((x, head + r * 2.05), r, 0.5, 1.0);
    }
    fn chevron(&self, turns: f32) {
        let (s, c) = (turns * TAU).sin_cos();
        let rot = |x: f32, y: f32| (12.0 + x * c - y * s, 12.0 + x * s + y * c);
        self.line(&[rot(-6.0, -3.0), rot(0.0, 3.2), rot(6.0, -3.0)], 2.4);
    }
    /// Flèche courbe de « répondre » ; `flip` : vers la droite (transférer).
    fn turn_arrow(&self, flip: bool) {
        let m = |x: f32, y: f32| (if flip { 24.0 - x } else { x }, y);
        self.poly(&[m(2.2, 10.0), m(10.0, 3.6), m(10.0, 16.4)]);
        self.line(&[m(9.5, 10.0), m(14.0, 10.0), m(17.2, 11.0), m(19.4, 13.3), m(20.3, 16.5), m(20.4, 19.6)], 3.0);
    }
}

/// Icônes pleines sur la grille de 24 ; `false` : l'icône garde son ancien tracé.
fn filled(icon: Icon, painter: &Painter, rect: Rect, color: Color32) -> bool {
    let side = rect.width().min(rect.height());
    let pen = Pen { painter, origin: rect.center() - Vec2::splat(side / 2.0), g: side / 24.0, color };
    let k = &pen;
    match icon {
        Icon::Hash => k.hash(2.5, 2.5, 19.0),
        Icon::HashLock => {
            k.hash(2.0, 5.5, 16.5);
            k.padlock(16.0, 1.5);
        }
        Icon::Speaker => {
            k.speaker();
            k.arc((12.0, 12.0), 4.2, -0.13, 0.13, 2.0);
            k.arc((12.0, 12.0), 8.2, -0.14, 0.14, 2.0);
        }
        Icon::SpeakerLock => {
            k.speaker();
            k.arc((12.0, 13.0), 4.2, 0.0, 0.13, 2.0);
            k.padlock(15.5, 2.0);
        }
        Icon::Stage => {
            k.person(12.0, 10.0, 4.6);
            for (a, b) in [(0.36, 0.64), (-0.14, 0.14)] {
                k.arc((12.0, 10.0), 6.0, a, b, 1.8);
                k.arc((12.0, 10.0), 9.4, a + 0.02, b - 0.02, 1.8);
            }
        }
        Icon::Bell => k.bell(),
        Icon::BellOff => {
            k.bell();
            k.slash();
        }
        Icon::Pin => {
            // Punaise penchée à 45°, pointe en bas à gauche.
            let (s, c) = (TAU / 8.0).sin_cos();
            let r = |x: f32, y: f32| (12.0 + (x - 12.0) * c - (y - 12.0) * s, 12.0 + (x - 12.0) * s + (y - 12.0) * c);
            k.poly(&[r(7.5, 1.5), r(16.5, 1.5), r(16.5, 4.0), r(7.5, 4.0)]);
            k.poly(&[r(9.3, 4.0), r(14.7, 4.0), r(15.6, 11.0), r(8.4, 11.0)]);
            k.poly(&[r(5.5, 13.6), r(8.4, 10.6), r(15.6, 10.6), r(18.5, 13.6)]);
            k.poly(&[r(11.0, 13.6), r(13.0, 13.6), r(12.0, 22.5)]);
        }
        Icon::Help => {
            k.dot(12.0, 12.0, 10.0);
            let i = k.ink();
            i.arc((12.0, 9.6), 3.1, 0.52, 1.17, 2.0);
            i.line(&[(13.5, 12.3), (12.0, 13.6), (12.0, 14.4)], 2.0);
            i.dot(12.0, 17.6, 1.3);
        }
        Icon::Gif => {
            k.rr(1.5, 4.5, 22.5, 19.5, 3.5);
            let i = k.ink();
            i.line(&[(9.2, 9.6), (6.2, 9.6), (6.2, 14.4), (9.2, 14.4), (9.2, 12.2), (8.0, 12.2)], 1.6);
            i.line(&[(12.0, 9.5), (12.0, 14.5)], 1.6);
            i.line(&[(17.8, 9.6), (14.8, 9.6), (14.8, 14.5)], 1.6);
            i.line(&[(14.8, 12.1), (17.0, 12.1)], 1.6);
        }
        Icon::Sticker => {
            k.poly(&[(5.5, 2.5), (18.5, 2.5), (21.5, 5.5), (21.5, 13.5), (13.5, 21.5), (5.5, 21.5), (2.5, 18.5), (2.5, 5.5)]);
            // Coin décollé.
            k.with(color.gamma_multiply(0.5)).poly(&[(15.0, 21.6), (21.6, 15.0), (21.6, 19.0), (19.0, 21.6)]);
            let i = k.ink();
            i.dot(8.6, 9.2, 1.3);
            i.dot(14.6, 9.2, 1.3);
            i.arc((11.6, 11.0), 4.2, 0.1, 0.4, 1.6);
        }
        Icon::Smile => {
            k.dot(12.0, 12.0, 10.0);
            let i = k.ink();
            i.dot(8.5, 9.8, 1.5);
            i.dot(15.5, 9.8, 1.5);
            i.arc((12.0, 12.2), 5.0, 0.08, 0.42, 1.9);
        }
        Icon::Apps => {
            for (x, y) in [(3.0, 3.0), (13.2, 3.0), (3.0, 13.2), (13.2, 13.2)] {
                k.rr(x, y, x + 7.8, y + 7.8, 2.2);
            }
        }
        Icon::Mic => k.mic(),
        Icon::MicOff => {
            k.mic();
            k.slash();
        }
        Icon::Headphones => k.headphones(),
        Icon::HeadphonesOff => {
            k.headphones();
            k.slash();
        }
        Icon::Phone => {
            // Combiné : bande courbe, écouteur et micro plus larges.
            let band = arc_points((20.0, 4.0), 13.0, 0.275, 0.475);
            k.arc((20.0, 4.0), 13.0, 0.275, 0.475, 4.2);
            for &(x, y) in [band[0], band[band.len() - 1]].iter() {
                k.dot(x, y, 3.0);
            }
        }
        Icon::HangUp => {
            k.arc((12.0, 22.0), 10.5, 0.60, 0.90, 4.0);
            k.rr(1.8, 12.4, 7.4, 17.4, 1.6);
            k.rr(16.6, 12.4, 22.2, 17.4, 1.6);
        }
        Icon::Camera => k.camera(),
        Icon::CameraOff => {
            k.camera();
            k.slash();
        }
        Icon::Screen => k.screen(),
        Icon::ScreenOff => {
            k.screen();
            k.slash();
        }
        Icon::Signal => {
            k.rr(3.5, 14.0, 8.0, 20.5, 1.2);
            k.rr(9.8, 9.0, 14.3, 20.5, 1.2);
            k.rr(16.0, 3.5, 20.5, 20.5, 1.2);
        }
        Icon::Trash => {
            k.rr(9.2, 2.5, 14.8, 5.5, 1.2);
            k.rr(3.5, 4.8, 20.5, 7.4, 1.2);
            k.poly(&[(5.4, 8.8), (18.6, 8.8), (17.4, 21.5), (6.6, 21.5)]);
        }
        Icon::Reply => k.turn_arrow(false),
        Icon::Forward => k.turn_arrow(true),
        Icon::More => {
            for x in [4.5, 12.0, 19.5] {
                k.dot(x, 12.0, 2.2);
            }
        }
        Icon::MoreVertical => {
            for y in [4.5, 12.0, 19.5] {
                k.dot(12.0, y, 2.2);
            }
        }
        Icon::Link => {
            // Deux maillons allongés, à 45°, qui se recouvrent au milieu.
            let d = std::f32::consts::FRAC_1_SQRT_2;
            for (cx, cy) in [(8.4, 15.6), (15.6, 8.4)] {
                let (a, b) = ((cx + 2.2 * d, cy - 2.2 * d), (cx - 2.2 * d, cy + 2.2 * d));
                let mut pts = arc_points(a, 3.4, 0.625, 1.125);
                pts.extend(arc_points(b, 3.4, 0.125, 0.625));
                pts.push(pts[0]);
                k.painter.add(Shape::line(pts.iter().map(|&(x, y)| k.p(x, y)).collect(), k.stroke(2.1)));
            }
        }
        Icon::Copy => {
            k.frame(8.5, 2.5, 21.0, 16.5, 2.2, 2.0);
            k.rr(3.0, 7.5, 15.5, 21.5, 2.2);
        }
        Icon::Id => {
            k.rr(1.5, 4.5, 22.5, 19.5, 3.0);
            let i = k.ink();
            i.line(&[(7.6, 9.0), (7.6, 15.0)], 1.9);
            i.line(&[(11.4, 9.0), (13.8, 9.0), (16.0, 10.3), (16.7, 12.0), (16.0, 13.7), (13.8, 15.0), (11.4, 15.0), (11.4, 9.0)], 1.9);
        }
        Icon::Flag => {
            k.line(&[(5.0, 3.0), (5.0, 21.0)], 2.2);
            k.poly(&[(5.5, 3.5), (20.0, 3.5), (16.6, 8.5), (5.5, 8.5)]);
            k.poly(&[(5.5, 8.5), (16.6, 8.5), (20.0, 13.5), (5.5, 13.5)]);
        }
        Icon::Crown => {
            k.poly(&[(2.5, 6.5), (10.0, 16.5), (4.5, 16.5)]);
            k.poly(&[(12.0, 4.5), (17.5, 16.5), (6.5, 16.5)]);
            k.poly(&[(21.5, 6.5), (19.5, 16.5), (14.0, 16.5)]);
            k.rr(4.5, 14.0, 19.5, 17.0, 0.0);
            k.rr(4.5, 18.2, 19.5, 20.6, 1.0);
        }
        Icon::Boost => {
            k.poly(&[(12.0, 2.0), (18.5, 7.5), (18.5, 16.5), (12.0, 22.0), (5.5, 16.5), (5.5, 7.5)]);
            k.ink().poly(&[(12.0, 7.0), (15.0, 9.8), (15.0, 14.2), (12.0, 17.0), (9.0, 14.2), (9.0, 9.8)]);
        }
        Icon::FriendAdd => {
            k.person(9.0, 8.0, 6.4);
            k.line(&[(18.5, 5.5), (18.5, 11.5)], 2.0);
            k.line(&[(15.5, 8.5), (21.5, 8.5)], 2.0);
        }
        Icon::Check => k.line(&[(4.5, 12.5), (9.5, 17.5), (19.5, 6.5)], 2.6),
        Icon::ChevronDown => k.chevron(0.0),
        Icon::ChevronLeft => k.chevron(0.25),
        Icon::ChevronUp => k.chevron(0.5),
        Icon::ChevronRight => k.chevron(0.75),
        Icon::Compass => {
            k.dot(12.0, 12.0, 10.0);
            let i = k.ink();
            i.poly(&[(16.6, 7.4), (13.6, 13.6), (7.4, 16.6), (10.4, 10.4)]);
        }
        Icon::Download => {
            k.line(&[(12.0, 3.5), (12.0, 14.5)], 2.4);
            k.line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)], 2.4);
            k.rr(4.0, 18.6, 20.0, 21.0, 1.2);
        }
        Icon::Star => {
            let (c, outer, inner) = ((12.0, 12.8), 10.2, 4.4);
            let at = |i: usize, r: f32| {
                let v = Vec2::angled((i as f32 / 10.0 - 0.25) * TAU) * r;
                (c.0 + v.x, c.1 + v.y)
            };
            for i in 0..5 {
                k.poly(&[c, at(2 * i + 9, inner), at(2 * i, outer), at(2 * i + 1, inner)]);
            }
        }
        Icon::Clock => {
            k.dot(12.0, 12.0, 10.0);
            k.ink().line(&[(12.0, 6.5), (12.0, 12.0), (15.8, 14.4)], 2.0);
        }
        Icon::Eye => {
            let mut lens = arc_points((12.0, 20.0), 12.5, 0.615, 0.885);
            lens.extend(arc_points((12.0, 4.0), 12.5, 0.115, 0.385));
            k.poly(&lens);
            k.ink().dot(12.0, 12.0, 3.2);
        }
        Icon::Pause => {
            k.rr(5.5, 4.0, 10.0, 20.0, 1.2);
            k.rr(14.0, 4.0, 18.5, 20.0, 1.2);
        }
        Icon::Quest => {
            // Médaillon, et sa couronne de feuilles ouverte en haut.
            k.dot(12.0, 12.0, 4.8);
            for i in 0..10 {
                let v = Vec2::angled((0.87 + 0.76 * i as f32 / 9.0) * TAU) * 9.0;
                k.dot(12.0 + v.x, 12.0 + v.y, 1.75);
            }
        }
        Icon::Wave => {
            // Tête, épaules, et le bras levé terminé par la main.
            k.dot(9.6, 7.4, 3.9);
            k.cap((9.6, 21.6), 7.6, 0.5, 1.0);
            k.line(&[(15.6, 15.4), (19.4, 8.6)], 2.8);
            k.dot(19.9, 5.6, 2.4);
        }
        Icon::Nitro => {
            k.ring(14.4, 12.0, 6.3, 2.7);
            k.dot(14.4, 12.0, 2.3);
            k.line(&[(2.6, 6.6), (8.6, 6.6)], 2.3);
            k.line(&[(1.6, 12.0), (4.8, 12.0)], 2.3);
            k.line(&[(3.6, 17.4), (8.6, 17.4)], 2.3);
        }
        Icon::Shop => {
            // Auvent et ses festons, puis la devanture et sa porte.
            k.poly(&[(4.6, 3.2), (19.4, 3.2), (21.6, 8.4), (2.4, 8.4)]);
            for x in [5.6, 12.0, 18.4] {
                k.dot(x, 8.6, 3.15);
            }
            k.rr(4.4, 13.4, 19.6, 21.2, 1.4);
            k.ink().rr(9.8, 16.0, 14.2, 21.2, 0.9);
        }
        Icon::Sliders => {
            for (y, knob) in [(6.0, 15.5), (12.0, 8.0), (18.0, 16.5)] {
                k.line(&[(3.5, y), (20.5, y)], 1.8);
                k.dot(knob, y, 3.0);
                k.ink().dot(knob, y, 1.2);
            }
        }
        Icon::Verified => {
            k.dot(12.0, 12.0, 8.4);
            for i in 0..8 {
                let v = Vec2::angled(i as f32 / 8.0 * TAU) * 7.6;
                k.dot(12.0 + v.x, 12.0 + v.y, 3.2);
            }
            k.ink().line(&[(7.8, 12.4), (10.8, 15.3), (16.4, 9.2)], 2.3);
        }
        Icon::Fullscreen => {
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                let q = |x: f32, y: f32| (12.0 + sx * x, 12.0 + sy * y);
                k.line(&[q(-8.0, -3.0), q(-8.0, -8.0), q(-3.0, -8.0)], 2.2);
            }
        }
        Icon::Close => {
            k.line(&[(5.0, 5.0), (19.0, 19.0)], 2.4);
            k.line(&[(19.0, 5.0), (5.0, 19.0)], 2.4);
        }
        Icon::Plus => {
            k.line(&[(12.0, 4.0), (12.0, 20.0)], 2.6);
            k.line(&[(4.0, 12.0), (20.0, 12.0)], 2.6);
        }
        Icon::ArrowDown => {
            k.line(&[(12.0, 3.5), (12.0, 20.0)], 2.4);
            k.line(&[(5.5, 13.5), (12.0, 20.0), (18.5, 13.5)], 2.4);
        }
        Icon::ArrowRight => {
            k.line(&[(3.5, 12.0), (20.0, 12.0)], 2.4);
            k.line(&[(13.5, 5.5), (20.0, 12.0), (13.5, 18.5)], 2.4);
        }
        Icon::ArrowLeft => {
            k.line(&[(20.5, 12.0), (4.0, 12.0)], 2.4);
            k.line(&[(10.5, 5.5), (4.0, 12.0), (10.5, 18.5)], 2.4);
        }
        Icon::UserCircle => {
            k.dot(12.0, 12.0, 10.0);
            let i = k.ink();
            i.dot(12.0, 8.8, 3.1);
            // Épaules : un disque coupé par le bord du grand.
            i.cap((12.0, 19.0), 5.6, 0.5, 1.0);
        }
        Icon::Search => {
            k.ring(10.2, 10.2, 6.6, 2.6);
            k.line(&[(15.4, 15.4), (20.6, 20.6)], 2.8);
        }
        Icon::Lock => {
            k.arc((12.0, 9.5), 4.6, 0.5, 1.0, 2.4);
            k.rr(4.5, 9.5, 19.5, 21.5, 2.5);
        }
        Icon::File => {
            k.poly(&[(4.5, 2.0), (14.0, 2.0), (19.5, 7.5), (19.5, 22.0), (4.5, 22.0)]);
            k.ink().line(&[(13.4, 3.4), (13.4, 8.2), (18.2, 8.2)], 1.4);
        }
        Icon::Members => {
            k.person(15.8, 8.4, 5.4);
            k.person(8.6, 7.4, 6.6);
        }
        Icon::Thread => {
            k.line(&[(3.5, 3.5), (3.5, 12.0), (7.0, 12.0)], 2.2);
            k.rr(8.5, 7.5, 21.5, 17.0, 2.5);
            k.poly(&[(11.0, 16.5), (15.5, 16.5), (10.5, 21.5)]);
        }
        Icon::Forum => {
            k.rr(8.0, 9.0, 22.0, 18.5, 2.5);
            k.poly(&[(15.5, 18.0), (19.5, 18.0), (19.5, 22.0)]);
            let back = k.with(color.gamma_multiply(0.55));
            back.rr(2.0, 2.5, 15.5, 8.2, 2.5);
            back.rr(2.0, 5.0, 7.2, 12.0, 2.0);
        }
        Icon::Inbox => {
            k.frame(2.5, 3.5, 21.5, 20.5, 3.0, 2.2);
            k.poly(&[(3.5, 13.0), (8.6, 13.0), (9.8, 15.6), (9.8, 19.0), (3.5, 19.0)]);
            k.poly(&[(20.5, 13.0), (20.5, 19.0), (14.2, 19.0), (14.2, 15.6), (15.4, 13.0)]);
            k.rr(9.4, 15.6, 14.6, 19.0, 0.0);
        }
        Icon::Calendar => {
            k.rr(3.0, 5.0, 21.0, 21.0, 2.6);
            k.line(&[(7.8, 2.8), (7.8, 6.0)], 2.2);
            k.line(&[(16.2, 2.8), (16.2, 6.0)], 2.2);
            let i = k.ink();
            i.line(&[(6.2, 10.4), (17.8, 10.4)], 1.5);
            for (x, y) in [(8.2, 14.2), (12.0, 14.2), (15.8, 14.2), (8.2, 17.6), (12.0, 17.6)] {
                i.dot(x, y, 1.0);
            }
        }
        Icon::Media => {
            k.rr(2.0, 4.0, 22.0, 20.0, 2.6);
            let i = k.ink();
            i.poly(&[(5.0, 17.0), (10.2, 10.2), (15.4, 17.0)]);
            i.poly(&[(12.6, 17.0), (16.2, 12.6), (19.4, 17.0)]);
            i.dot(16.8, 8.4, 1.5);
        }
        Icon::Logout => {
            k.poly(&[(3.5, 3.0), (12.0, 3.0), (12.0, 21.0), (3.5, 21.0)]);
            k.line(&[(15.0, 12.0), (21.0, 12.0)], 2.2);
            k.line(&[(18.0, 8.6), (21.4, 12.0), (18.0, 15.4)], 2.2);
        }
        Icon::Megaphone => {
            k.rr(2.0, 8.5, 6.5, 14.5, 1.2);
            k.poly(&[(6.0, 8.5), (19.0, 3.5), (19.0, 19.5), (6.0, 14.5)]);
            k.line(&[(5.4, 15.5), (7.4, 20.5)], 2.6);
            k.arc((19.0, 11.5), 2.6, -0.2, 0.2, 1.8);
        }
        Icon::Hourglass => {
            k.rr(4.5, 2.0, 19.5, 4.4, 1.0);
            k.rr(4.5, 19.6, 19.5, 22.0, 1.0);
            k.poly(&[(6.5, 4.4), (17.5, 4.4), (12.0, 12.6)]);
            k.poly(&[(12.0, 11.4), (17.5, 19.6), (6.5, 19.6)]);
        }
        Icon::User => k.person(12.0, 7.0, 7.2),
        Icon::ShieldLock => {
            k.poly(&[(12.0, 2.0), (20.0, 5.0), (20.0, 12.0), (17.5, 17.5), (12.0, 22.0), (6.5, 17.5), (4.0, 12.0), (4.0, 5.0)]);
            let i = k.ink();
            i.arc((12.0, 10.6), 2.1, 0.5, 1.0, 1.4);
            i.rr(8.6, 10.6, 15.4, 15.8, 1.0);
        }
        Icon::ChatShield => {
            k.rr(2.0, 2.5, 15.5, 11.0, 3.0);
            k.poly(&[(4.5, 10.5), (9.0, 10.5), (4.5, 15.0)]);
            k.poly(&[(17.5, 11.5), (22.0, 13.0), (22.0, 17.0), (20.5, 20.0), (17.5, 22.0), (14.5, 20.0), (13.0, 17.0), (13.0, 13.0)]);
        }
        Icon::Palette => {
            k.dot(12.0, 12.0, 10.0);
            let i = k.ink();
            i.dot(7.6, 10.0, 1.6);
            i.dot(11.6, 6.8, 1.6);
            i.dot(16.2, 8.8, 1.6);
            // Trou du pouce.
            i.dot(15.6, 15.8, 2.7);
        }
        Icon::Accessibility => {
            k.dot(12.0, 12.0, 10.0);
            let i = k.ink();
            i.dot(12.0, 6.8, 1.6);
            i.line(&[(7.0, 10.2), (17.0, 10.2)], 1.7);
            i.line(&[(9.4, 18.4), (12.0, 13.8), (14.6, 18.4)], 1.7);
            i.line(&[(12.0, 10.2), (12.0, 13.8)], 1.7);
        }
        Icon::Keyboard => {
            k.rr(1.5, 5.0, 22.5, 19.0, 3.0);
            let i = k.ink();
            for y in [9.2, 12.4] {
                for x in [6.0, 10.0, 14.0, 18.0] {
                    i.dot(x, y, 1.05);
                }
            }
            i.line(&[(8.0, 15.8), (16.0, 15.8)], 1.6);
        }
        Icon::Language => {
            // Un idéogramme simplifié à gauche, la lettre « A » à droite.
            k.line(&[(2.5, 6.0), (12.5, 6.0)], 1.8);
            k.line(&[(7.5, 3.2), (7.5, 6.0)], 1.8);
            k.line(&[(10.6, 6.0), (8.4, 10.4), (3.0, 14.0)], 1.8);
            k.line(&[(5.2, 9.0), (8.4, 12.2), (11.4, 13.6)], 1.8);
            k.line(&[(12.8, 21.0), (17.0, 10.5), (21.2, 21.0)], 1.9);
            k.line(&[(14.4, 17.6), (19.6, 17.6)], 1.7);
        }
        Icon::Gamepad => {
            k.rr(2.0, 6.5, 22.0, 16.5, 5.0);
            k.dot(5.6, 15.8, 3.6);
            k.dot(18.4, 15.8, 3.6);
            let i = k.ink();
            i.line(&[(5.6, 11.4), (9.6, 11.4)], 1.5);
            i.line(&[(7.6, 9.4), (7.6, 13.4)], 1.5);
            i.dot(15.2, 12.6, 1.15);
            i.dot(18.0, 10.2, 1.15);
        }
        Icon::Puzzle => {
            k.rr(3.0, 8.0, 17.0, 21.5, 2.0);
            k.dot(10.0, 6.6, 2.7);
            k.dot(18.4, 14.8, 2.7);
        }
        Icon::Info => {
            k.dot(12.0, 12.0, 10.0);
            let i = k.ink();
            i.dot(12.0, 7.4, 1.4);
            i.line(&[(12.0, 11.2), (12.0, 17.0)], 2.2);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chaque icône se dessine dans son carré, à toutes les tailles utiles,
    /// sans en sortir de plus d'un demi-trait ; la liste `ALL` est complète.
    #[test]
    fn every_icon_paints_inside_its_square() {
        let ctx = eframe::egui::Context::default();
        let mut seen = std::collections::HashSet::new();
        let mut out = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
            for &icon in Icon::ALL {
                assert!(seen.insert(format!("{icon:?}")), "{icon:?} en double");
                for side in [12.0, 16.0, 20.0, 24.0, 48.0] {
                    let rect = Rect::from_min_size(pos2(100.0, 100.0), Vec2::splat(side));
                    let layer = eframe::egui::LayerId::new(eframe::egui::Order::Middle, eframe::egui::Id::unique((format!("{icon:?}"), side as u32)));
                    let painter = ui.ctx().layer_painter(layer);
                    paint(icon, &painter, rect, Color32::WHITE);
                    let used = ui.ctx().graphics(|g| g.get(layer).map(|l| l.all_entries().fold(Rect::NOTHING, |r, s| r.union(s.shape.visual_bounding_rect()))) ).unwrap_or(Rect::NOTHING);
                    assert!(used.is_positive(), "{icon:?} ne dessine rien à {side}");
                    assert!(rect.expand(side * 0.12 + 1.0).contains_rect(used), "{icon:?} à {side} : {used:?} hors de {rect:?}");
                }
            }
        });
        out.textures_delta.clear();
        assert_eq!(seen.len(), 91);
    }

    /// Triangule ce qu'une couche a reçu depuis `from`, comme le fera l'image.
    fn mesh_of(ctx: &Context, layer: LayerId, from: usize) -> Mesh {
        let shapes: Vec<Shape> = ctx.graphics(|g| g.get(layer).map(|l| l.all_entries().skip(from).map(|s| s.shape.clone()).collect()).unwrap_or_default());
        let clipped = ClippedShape { clip_rect: Rect::EVERYTHING, shape: Shape::Vec(shapes), transform_after_tessellation: TSTransform::IDENTITY };
        let mut out = ctx.tessellate(vec![clipped], ctx.pixels_per_point());
        assert_eq!(out.len(), 1, "un seul maillage");
        match out.pop().map(|p| p.primitive) {
            Some(Primitive::Mesh(m)) => m,
            _ => panic!("maillage attendu"),
        }
    }

    fn count(ctx: &Context, layer: LayerId) -> usize {
        ctx.graphics(|g| g.get(layer).map_or(0, |l| l.all_entries().count()))
    }

    /// Lot C10 : le maillage gardé donne les mêmes triangles que le tracé
    /// direct, au premier dessin comme aux suivants, et rien n'est posé hors
    /// du découpage.
    #[test]
    fn cached_mesh_matches_direct_drawing() {
        let ctx = Context::default();
        let direct = LayerId::new(Order::Middle, Id::unique("direct"));
        let cached = LayerId::new(Order::Middle, Id::unique("garde"));
        for ppp in [1.0, 1.25] {
            let input = eframe::egui::RawInput { viewport_id: eframe::egui::ViewportId::ROOT, ..Default::default() };
            ctx.set_pixels_per_point(ppp);
            let mut out = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                let (a, b) = (ctx.layer_painter(direct), ctx.layer_painter(cached));
                for &icon in Icon::ALL {
                    for (side, at) in [(16.0, pos2(100.0, 100.0)), (20.0, pos2(37.3, 81.7)), (14.0, pos2(8.5, 300.25))] {
                        // Deux poses au même endroit (la seconde vient du cache), puis ailleurs.
                        for shift in [Vec2::ZERO, Vec2::ZERO, eframe::egui::vec2(40.0, 24.0)] {
                            let rect = Rect::from_min_size(at + shift, Vec2::splat(side));
                            let (fa, fb) = (count(&ctx, direct), count(&ctx, cached));
                            draw(icon, &a, rect, Color32::from_rgb(200, 210, 220));
                            paint(icon, &b, rect, Color32::from_rgb(200, 210, 220));
                            assert_eq!(count(&ctx, cached), fb + 1, "{icon:?} : une seule forme posée");
                            let (ma, mb) = (mesh_of(&ctx, direct, fa), mesh_of(&ctx, cached, fb));
                            assert_eq!(ma.indices, mb.indices, "{icon:?} à {side}, {rect:?}, {ppp}");
                            assert_eq!(ma.vertices.len(), mb.vertices.len());
                            assert!(ma.vertices == mb.vertices, "{icon:?} à {side}, {rect:?}, {ppp}");
                        }
                    }
                }
                // Hors du découpage du peintre : rien.
                let before = count(&ctx, cached);
                let clipped = b.with_clip_rect(Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(50.0)));
                paint(Icon::ALL[0], &clipped, Rect::from_min_size(pos2(200.0, 200.0), Vec2::splat(16.0)), Color32::WHITE);
                assert_eq!(count(&ctx, cached), before);
            });
            out.textures_delta.clear();
        }
        // Le brouillon ne laisse rien à l'écran et le cache reste borné.
        let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<std::sync::Arc<std::sync::Mutex<Meshes>>>(meshes_id()).clone());
        let n = cache.lock().map(|c| c.map.len()).unwrap_or(usize::MAX);
        assert!(n > 0 && n <= MAX_MESHES, "{n}");
    }
}
