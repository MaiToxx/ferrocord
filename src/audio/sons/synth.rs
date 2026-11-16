//! Petit synthétiseur des sons d'interface : notes à modulation de phase
//! légère (timbre de piano électrique / marimba doux), enveloppes sans clic,
//! passe-bas, réverbération courte, mise à niveau en puissance perçue.
//!
//! Tout est pur : des nombres en entrée, un tampon stéréo en sortie, aucune
//! entrée-sortie. Le tampon est en `f32`, 48 kHz, voies entrelacées (G, D).

use std::f32::consts::{PI, TAU};

/// Fréquence d'échantillonnage de tous les sons.
pub const FREQ: u32 = 48_000;
const SR: f32 = FREQ as f32;

/// Crête au plus : un cheveu sous −6 dBFS (0,5012), pour que l'arrondi en
/// 16 bits ne la dépasse jamais.
pub const CRETE_MAX: f32 = 0.5;

/// Couleur d'une note. Une seule famille : la même porteuse sinusoïdale,
/// modulée en phase par elle-même (rapport 1), plus ou moins fort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timbre {
    /// Indice de modulation à l'attaque (0 : sinus pur ; 1 : déjà brillant).
    pub indice: f32,
    /// Indice restant quand la note s'éteint (corps du son).
    pub indice_fond: f32,
    /// Temps de retombée de l'indice, en secondes (brillance de l'attaque).
    pub indice_tau: f32,
    /// Niveau de l'octave (harmonique 2), qui s'éteint deux fois plus vite.
    pub octave: f32,
    /// « Frappe » de maillet : partiel à quatre fois la note, éteint en 15 ms.
    pub frappe: f32,
    /// Durée de l'attaque, en secondes (3 à 10 ms).
    pub attaque: f32,
}

/// Timbre de base : rond, un peu de brillance à l'attaque seulement.
pub const DOUX: Timbre = Timbre { indice: 0.75, indice_fond: 0.12, indice_tau: 0.07, octave: 0.10, frappe: 0.07, attaque: 0.005 };
/// Le même, feutré (micro, casque) : presque un sinus, attaque plus lente.
pub const FEUTRE: Timbre = Timbre { indice: 0.32, indice_fond: 0.06, indice_tau: 0.04, octave: 0.05, frappe: 0.03, attaque: 0.006 };
/// Le même, un peu plus clair (mention, connexion, sonnerie).
pub const CLAIR: Timbre = Timbre { indice: 1.0, indice_fond: 0.15, indice_tau: 0.09, octave: 0.14, frappe: 0.10, attaque: 0.004 };

/// Une note d'un son.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// Début, en secondes depuis le début du son.
    pub debut: f32,
    /// Fréquence visée, en hertz.
    pub hz: f32,
    /// Départ décalé de tant de demi-tons (négatif : la note monte vers `hz`).
    pub glisse: f32,
    /// Temps de décroissance exponentielle, en secondes.
    pub tau: f32,
    /// Durée jouée avant le relâchement, en secondes.
    pub tenue: f32,
    /// Niveau relatif dans le son (1 : note principale).
    pub gain: f32,
    /// Position : −1 à gauche, 0 au centre, 1 à droite.
    pub pan: f32,
}

impl Note {
    pub const fn new(debut: f32, hz: f32, tau: f32, tenue: f32) -> Note {
        Note { debut, hz, glisse: 0.0, tau, tenue, gain: 1.0, pan: 0.0 }
    }

    pub const fn gain(mut self, gain: f32) -> Note {
        self.gain = gain;
        self
    }

    pub const fn pan(mut self, pan: f32) -> Note {
        self.pan = pan;
        self
    }

    pub const fn glisse(mut self, demi_tons: f32) -> Note {
        self.glisse = demi_tons;
        self
    }
}

/// Temps de retombée d'un glissement de hauteur, en secondes.
const GLISSE_TAU: f32 = 0.03;
/// Durée du relâchement après la tenue, en secondes.
const RELACHE: f32 = 0.045;

/// Nombre de trames pour `secondes`.
pub fn trames(secondes: f32) -> usize {
    (secondes.max(0.0) * SR).round() as usize
}

/// Demi-sinus de 0 à 1 (`x` de 0 à 1) : fondu sans angle.
fn rampe(x: f32) -> f32 {
    0.5 - 0.5 * (PI * x.clamp(0.0, 1.0)).cos()
}

/// Ajoute `note` au tampon stéréo `buf` (ce qui dépasse la fin est coupé).
pub fn jouer(buf: &mut [f32], note: &Note, t: &Timbre) {
    let start = trames(note.debut);
    let len = trames(note.tenue + RELACHE);
    let angle = (note.pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    let (gl, gr) = (angle.cos() * std::f32::consts::SQRT_2, angle.sin() * std::f32::consts::SQRT_2);
    let mut phase = 0.0f32;
    for i in 0..len {
        let Some(frame) = buf.get_mut(2 * (start + i)..2 * (start + i) + 2) else { break };
        let s = i as f32 / SR;
        let hz = if note.glisse == 0.0 { note.hz } else { note.hz * 2f32.powf(note.glisse / 12.0 * (-s / GLISSE_TAU).exp()) };
        let env = rampe(s / t.attaque) * (-s / note.tau).exp() * rampe((note.tenue + RELACHE - s) / RELACHE);
        let indice = t.indice_fond + (t.indice - t.indice_fond) * (-s / t.indice_tau).exp();
        let corps = (phase + indice * phase.sin()).sin();
        let octave = t.octave * (-s / note.tau).exp() * (2.0 * phase).sin();
        let frappe = t.frappe * (-s / 0.015).exp() * (4.0 * phase).sin();
        let x = note.gain * env * (corps + octave + frappe);
        frame[0] += gl * x;
        frame[1] += gr * x;
        phase += TAU * hz / SR;
        if phase > TAU {
            phase -= TAU;
        }
    }
}

/// Passe-bas doux (deux pôles simples à la suite, 12 dB par octave).
pub fn passe_bas(buf: &mut [f32], hz: f32) {
    let a = 1.0 - (-TAU * hz / SR).exp();
    let mut z = [[0.0f32; 2]; 2];
    for frame in buf.as_chunks_mut::<2>().0 {
        for (c, x) in frame.iter_mut().enumerate() {
            z[0][c] += a * (*x - z[0][c]);
            z[1][c] += a * (z[0][c] - z[1][c]);
            *x = z[1][c];
        }
    }
}

/// Retire la composante continue (passe-haut à un pôle, vers 30 Hz).
pub fn sans_continu(buf: &mut [f32]) {
    let r = 1.0 - TAU * 30.0 / SR;
    let (mut x1, mut y1) = ([0.0f32; 2], [0.0f32; 2]);
    for frame in buf.as_chunks_mut::<2>().0 {
        for (c, x) in frame.iter_mut().enumerate() {
            let y = *x - x1[c] + r * y1[c];
            x1[c] = *x;
            y1[c] = y;
            *x = y;
        }
    }
}

/// Peigne à rétroaction amortie (une ligne à retard de la réverbération).
struct Peigne {
    ligne: Vec<f32>,
    at: usize,
    retour: f32,
    amorti: f32,
}

impl Peigne {
    fn new(ms: f32, rt60: f32) -> Peigne {
        let n = trames(ms / 1000.0).max(1);
        // Le son a perdu 60 dB après `rt60` secondes de tours dans la ligne.
        let retour = 10f32.powf(-3.0 * (n as f32 / SR) / rt60.max(0.01));
        Peigne { ligne: vec![0.0; n], at: 0, retour, amorti: 0.0 }
    }

    fn passe(&mut self, x: f32) -> f32 {
        let y = self.ligne[self.at];
        // Les aigus s'éteignent plus vite que les graves, comme dans une pièce.
        self.amorti += 0.45 * (y - self.amorti);
        self.ligne[self.at] = x + self.retour * self.amorti;
        self.at = (self.at + 1) % self.ligne.len();
        y
    }
}

/// Passe-tout (diffuse les échos du peigne).
struct PasseTout {
    ligne: Vec<f32>,
    at: usize,
}

impl PasseTout {
    fn new(ms: f32) -> PasseTout {
        PasseTout { ligne: vec![0.0; trames(ms / 1000.0).max(1)], at: 0 }
    }

    fn passe(&mut self, x: f32) -> f32 {
        let d = self.ligne[self.at];
        let y = d - 0.5 * x;
        self.ligne[self.at] = x + 0.5 * y;
        self.at = (self.at + 1) % self.ligne.len();
        y
    }
}

/// Retards des peignes (ms), premiers entre eux ; la voie droite est un peu
/// plus longue : la queue est large sans que la note quitte le centre.
const PEIGNES: [f32; 5] = [19.3, 22.7, 25.9, 28.1, 31.7];
const ECART_DROIT: f32 = 0.53;
const PASSE_TOUT: [f32; 2] = [5.1, 1.7];

/// Petite réverbération (Schroeder : peignes en parallèle, puis passe-tout)
/// ajoutée au son : `dose` de 0 à 1, `rt60` en secondes.
pub fn reverberer(buf: &mut [f32], dose: f32, rt60: f32) {
    if dose <= 0.0 {
        return;
    }
    let mut voies: [(Vec<Peigne>, Vec<PasseTout>); 2] = [0.0, ECART_DROIT].map(|ecart| {
        (PEIGNES.iter().map(|ms| Peigne::new(ms + ecart, rt60)).collect(), PASSE_TOUT.iter().map(|ms| PasseTout::new(ms + ecart / 4.0)).collect())
    });
    let k = dose / PEIGNES.len() as f32;
    for frame in buf.as_chunks_mut::<2>().0 {
        let entree = 0.5 * (frame[0] + frame[1]);
        for (c, (peignes, diffuseurs)) in voies.iter_mut().enumerate() {
            let mut y: f32 = peignes.iter_mut().map(|p| p.passe(entree)).sum();
            for d in diffuseurs.iter_mut() {
                y = d.passe(y);
            }
            frame[c] += k * y;
        }
    }
}

/// Fondu de fin (demi-sinus) sur les `secondes` dernières ; la première et
/// la dernière trame sont mises à zéro exactement : ni clic, ni saut en boucle.
pub fn fondre(buf: &mut [f32], secondes: f32) {
    let frames = buf.len() / 2;
    let n = trames(secondes).min(frames);
    for i in 0..n {
        let g = rampe(i as f32 / n as f32);
        let f = frames - 1 - i;
        buf[2 * f] *= g;
        buf[2 * f + 1] *= g;
    }
    if frames > 0 {
        buf[0] = 0.0;
        buf[1] = 0.0;
    }
}

/// Filtre à deux pôles, forme directe transposée (pondération K).
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn passe(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// Pondération K à 48 kHz (UIT-R BS.1770 : relèvement des aigus, coupure
/// des graves), proche de la sensibilité de l'oreille.
fn ponderation_k() -> [Biquad; 2] {
    [
        Biquad { b: [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85], a: [-1.690_659_293_182_41, 0.732_480_774_215_85], z: [0.0; 2] },
        Biquad { b: [1.0, -2.0, 1.0], a: [-1.990_047_454_833_98, 0.990_072_250_366_21], z: [0.0; 2] },
    ]
}

/// Fenêtre de mesure de la puissance perçue, en secondes : un son plus bref
/// qu'elle paraît moins fort, comme à l'oreille.
const FENETRE: f32 = 0.2;

/// Puissance perçue du passage le plus fort, en décibels (pondération K,
/// moyenne sur une fenêtre glissante de 200 ms). `-inf` pour du silence.
pub fn sonie(buf: &[f32]) -> f32 {
    let frames = buf.len() / 2;
    let mut carres = Vec::with_capacity(frames);
    let mut filtres = [ponderation_k(), ponderation_k()];
    for frame in buf.as_chunks::<2>().0 {
        let mut p = 0.0f64;
        for (c, x) in frame.iter().enumerate() {
            let y = filtres[c].iter_mut().fold(f64::from(*x), |v, f| f.passe(v));
            p += y * y;
        }
        carres.push(p / 2.0);
    }
    let w = trames(FENETRE);
    let (mut somme, mut max) = (0.0f64, 0.0f64);
    for i in 0..frames {
        somme += carres[i];
        if i >= w {
            somme -= carres[i - w];
        }
        max = max.max(somme);
    }
    (10.0 * (max / w as f64).log10()) as f32
}

/// Plus grande valeur absolue du tampon.
pub fn crete(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// Amène le son à la puissance perçue `cible` (dB), puis le baisse s'il le
/// faut pour que sa crête ne dépasse pas [`CRETE_MAX`].
pub fn niveler(buf: &mut [f32], cible: f32) {
    let l = sonie(buf);
    if !l.is_finite() {
        return;
    }
    let mut g = 10f32.powf((cible - l) / 20.0);
    let c = crete(buf) * g;
    if c > CRETE_MAX {
        g *= CRETE_MAX / c;
    }
    for x in buf.iter_mut() {
        *x *= g;
    }
}

/// Convertit en 16 bits (arrondi au plus proche).
pub fn en_i16(buf: &[f32]) -> Vec<i16> {
    buf.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect()
}

/// Fichier WAV (PCM 16 bits, stéréo, 48 kHz) de ces échantillons entrelacés.
pub fn wav(pcm: &[i16]) -> Vec<u8> {
    let data = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&FREQ.to_le_bytes());
    out.extend_from_slice(&(FREQ * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}
