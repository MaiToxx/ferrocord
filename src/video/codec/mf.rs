//! H.264 et VP8 par Media Foundation (transformées COM `IMFTransform`) : seul
//! fichier `unsafe` des codecs. Le crate `windows` déclare `unsafe` tout appel
//! COM ; chaque bloc dit ce qui le rend correct.
//!
//! - Encodeur : transformée MATÉRIELLE seulement (AMD AMF, NVIDIA NVENC,
//!   Intel Quick Sync), asynchrone. L'encodeur logiciel de Windows n'apporte
//!   rien sur OpenH264 : sans transformée matérielle, on se replie sur lui.
//! - Décodeurs : ceux de Windows (H.264 : `CLSID_MSH264DecoderMFT` ; VP8 :
//!   celui des « Extensions vidéo VP9 », voir `vp8.rs`), en mode faible
//!   latence, sortie I420 en mémoire. Voir `mod.rs` pour les mesures qui ont
//!   guidé ces choix.
//!
//! Fils : les objets sont créés dans l'appartement multithread (MTA) ; un
//! fil déjà initialisé en STA (fil d'interface) refuse Media Foundation, et
//! l'appelant prend alors OpenH264.

use std::mem::ManuallyDrop;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{GUID, Interface};

use super::{CodecError, Decoder, Encoder, av1, h264::check_size, vp8};
use crate::video::{Codec, EncodedFrame, Frame, FrameData, convert, us_to_90k};

#[path = "mf_reader.rs"]
pub(super) mod reader;

/// 100 ns par microseconde (unité de temps de Media Foundation).
pub(super) const HNS_PER_US: i64 = 10;

pub(super) fn hr(e: windows::core::Error) -> CodecError {
    CodecError::Internal(format!("Media Foundation : {:#010x}", e.code().0))
}

/// Media Foundation démarré et fil courant dans le MTA.
pub(super) fn ready() -> bool {
    thread_local! {
        // Jamais désinitialisé : COM reste prêt pour toute la vie du fil.
        // SAFETY: appel sans pointeur ; seul le mode du fil est en jeu, et
        // RPC_E_CHANGED_MODE (fil déjà en STA) est traité comme un refus.
        static MTA: bool = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) } != RPC_E_CHANGED_MODE;
    }
    static STARTED: OnceLock<bool> = OnceLock::new();
    // SAFETY: MFStartup n'a pas de préalable ; jamais arrêté (vie du processus).
    MTA.with(|m| *m) && *STARTED.get_or_init(|| unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }.is_ok())
}

pub(super) fn video_type(subtype: &GUID) -> windows::core::Result<IMFMediaType> {
    // SAFETY: objet neuf, clés et valeurs passées par référence valide.
    unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        Ok(t)
    }
}

pub(super) fn pair(a: u32, b: u32) -> u64 {
    (u64::from(a) << 32) | u64::from(b)
}

pub(super) fn set_codec_value(api: Option<&ICodecAPI>, key: &GUID, v: VARIANT) -> bool {
    // SAFETY: `key` et `v` vivent pendant l'appel ; l'objet copie la valeur.
    api.is_some_and(|a| unsafe { a.SetValue(key, &v) }.is_ok())
}

/// Copie le contenu d'un échantillon (tampon rendu contigu).
pub(super) fn sample_bytes(s: &IMFSample, out: &mut Vec<u8>) -> windows::core::Result<()> {
    // SAFETY: entre Lock et Unlock, `p` désigne `len` octets lisibles ; ils
    // sont copiés avant Unlock et le pointeur n'est pas gardé.
    unsafe {
        let buf = s.ConvertToContiguousBuffer()?;
        let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
        buf.Lock(&mut p, None, Some(&mut len))?;
        out.clear();
        if !p.is_null() {
            out.extend_from_slice(std::slice::from_raw_parts(p, len as usize));
        }
        buf.Unlock()
    }
}

/// Échantillon d'entrée de `len` octets remplis par `fill`.
pub(super) fn new_sample(len: usize, pts_hns: i64, fill: impl FnOnce(&mut [u8])) -> windows::core::Result<IMFSample> {
    let len32 = u32::try_from(len).map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG))?;
    // SAFETY: le tampon neuf fait au moins `len` octets ; `p` n'est utilisé
    // qu'entre Lock et Unlock, sur `len` octets.
    unsafe {
        let buf = MFCreateMemoryBuffer(len32)?;
        let mut p = std::ptr::null_mut();
        buf.Lock(&mut p, None, None)?;
        if p.is_null() {
            buf.Unlock()?;
            return Err(windows::Win32::Foundation::E_POINTER.into());
        }
        fill(std::slice::from_raw_parts_mut(p, len));
        buf.Unlock()?;
        buf.SetCurrentLength(len32)?;
        let s = MFCreateSample()?;
        s.AddBuffer(&buf)?;
        s.SetSampleTime(pts_hns)?;
        Ok(s)
    }
}

pub(super) enum Out {
    Sample(IMFSample),
    NeedMore,
    StreamChange,
}

/// Un appel à `ProcessOutput`, en libérant ce que la transformée a rendu.
pub(super) fn process_output(mft: &IMFTransform, provided: Option<IMFSample>) -> windows::core::Result<Out> {
    let mut buf = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(provided), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
    let mut status = 0u32;
    // SAFETY: tableau d'un tampon valide pour le flux 0 ; échantillon et
    // collection rendus sont repris ci-dessous pour être libérés une fois.
    let r = unsafe { mft.ProcessOutput(0, &mut buf, &mut status) };
    // SAFETY: chaque champ est repris une seule fois, puis `buf` n'est plus lu.
    let (sample, _events) = unsafe { (ManuallyDrop::take(&mut buf[0].pSample), ManuallyDrop::take(&mut buf[0].pEvents)) };
    match r {
        Ok(()) => sample.map(Out::Sample).ok_or_else(|| windows::Win32::Foundation::E_POINTER.into()),
        Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(Out::NeedMore),
        Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => Ok(Out::StreamChange),
        Err(e) => Err(e),
    }
}

/// Échantillon de sortie à fournir quand la transformée ne fournit pas les siens.
pub(super) fn output_sample(mft: &IMFTransform) -> windows::core::Result<Option<IMFSample>> {
    // SAFETY: lecture d'une structure renvoyée par valeur ; objets neufs.
    unsafe {
        let info = mft.GetOutputStreamInfo(0)?;
        if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0 {
            return Ok(None);
        }
        let s = MFCreateSample()?;
        s.AddBuffer(&MFCreateMemoryBuffer(info.cbSize.max(1))?)?;
        Ok(Some(s))
    }
}

// ---------------------------------------------------------------- encodeur

/// Encodeur matériel. Asynchrone côté Windows ; vu d'ici synchrone : une
/// image entre, et ce qui est sorti dans le délai d'une image est rendu.
pub struct MfEncoder {
    /// H.264 ; AV1 pour fabriquer les jeux d'essai.
    codec: Codec,
    /// Nom de la transformée, pour dire quelle carte graphique encode.
    name: &'static str,
    activate: IMFActivate,
    mft: IMFTransform,
    events: IMFMediaEventGenerator,
    api: Option<ICodecAPI>,
    width: u32,
    height: u32,
    fps: u32,
    /// Débit appliqué à la transformée : `asked` plafonné par [`capped`].
    bps: u32,
    /// Débit demandé par l'appelant, repris tel quel à chaque nouvelle taille.
    asked: u32,
    /// Entrées que la transformée a réclamées (METransformNeedInput) et pas encore reçues.
    credits: u32,
    ready: Vec<EncodedFrame>,
    bytes: Vec<u8>,
    /// Après une baisse de débit : (images, octets) sortis depuis, images clés à part.
    watch: Option<(u32, u64)>,
    /// La transformée ne suit pas un changement de débit en cours de route :
    /// elle est rouverte à chaque changement.
    deaf: bool,
    /// Débit changé sur une transformée sourde : à rouvrir avant la prochaine image.
    reopen: bool,
}

/// Dépassement toléré du débit après une baisse (trois demis) : au-delà,
/// mesuré sur une seconde d'images, la transformée est tenue pour sourde.
/// Constaté sur un PC à UHD 770 : l'encodeur Quick Sync accepte le nouveau
/// débit sans erreur et continue au débit d'ouverture (13 Mb/s sortis pour 3
/// demandés, quand celui de NVIDIA descend aussitôt). Le partage en 4K
/// restait à 13-18 Mb/s alors que les pertes en demandaient 4,5 : figé chez
/// ceux qui regardent (constaté par l'utilisateur, 0.1.45).
const DEAF_NUM: u64 = 3;
const DEAF_DEN: u64 = 2;

// SAFETY: les transformées sont créées dans le MTA (voir `ready`), où un
// objet COM s'utilise depuis n'importe quel fil ; `&mut self` empêche tout
// accès concurrent de notre côté.
unsafe impl Send for MfEncoder {}

/// Débit plafonné à un demi-bit par pixel et par image. Au-delà, un encodeur
/// matériel quantifie si finement (mesuré sur un PC à RTX 4090 et UHD 770 :
/// 1,5 Mb/s en 192×144, 4 Mb/s en 320×240) qu'il écrit des coefficients à
/// `level_prefix` > 15, interdits en profil Baseline : OpenH264 rejette alors
/// chaque image (`ERR_INFO_CAVLC_INVALID_LEVEL`). Le plafond ne retire rien
/// de visible : 1080p30 garde droit à 31 Mb/s.
fn capped(bps: u32, w: u32, h: u32, fps: u32) -> u32 {
    let cap = u64::from(w) * u64::from(h) * u64::from(fps) / 2;
    bps.min(u32::try_from(cap).unwrap_or(u32::MAX)).max(1)
}

/// Transformées matérielles disponibles, de la plus recommandée à la moins.
fn hardware_encoders(codec: Codec) -> Vec<IMFActivate> {
    let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    transforms(MFT_CATEGORY_VIDEO_ENCODER, flags, &MFVideoFormat_NV12, Some(&hardware_format(codec)))
}

fn hardware_format(codec: Codec) -> GUID {
    if codec == Codec::Av1 { MFVideoFormat_AV1 } else { MFVideoFormat_H264 }
}

/// Transformées logicielles synchrones (VP8) : les transformées matérielles
/// de décodage veulent un périphérique Direct3D.
const SOFTWARE: MFT_ENUM_FLAG = MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_LOCALMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);

/// Transformées de `category` qui passent de `input` à `output` (toute sortie si `None`).
pub(super) fn transforms(category: GUID, flags: MFT_ENUM_FLAG, input: &GUID, output: Option<&GUID>) -> Vec<IMFActivate> {
    let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: *input };
    let output = output.map(|o| MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: *o });
    let (mut list, mut n) = (std::ptr::null_mut::<Option<IMFActivate>>(), 0u32);
    // SAFETY: types et sorties valides ; en cas de succès, `list` désigne `n`
    // éléments alloués par COM, repris un par un (chacun une fois) avant de
    // libérer le tableau avec CoTaskMemFree.
    unsafe {
        if MFTEnumEx(category, flags, Some(&input), output.as_ref().map(std::ptr::from_ref), &mut list, &mut n).is_err() || list.is_null() {
            return Vec::new();
        }
        let out = (0..n as usize).filter_map(|i| std::ptr::read(list.add(i))).collect();
        CoTaskMemFree(Some(list as *const _));
        out
    }
}

impl MfEncoder {
    pub fn new(w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        Self::with_codec(Codec::H264, w, h, fps, bps)
    }

    /// Encodeur AV1 matériel, s'il y en a un : pour les jeux d'essai.
    #[cfg(test)]
    pub fn av1(w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        Self::with_codec(Codec::Av1, w, h, fps, bps)
    }

    fn with_codec(codec: Codec, w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        check_size(w, h)?;
        if !ready() {
            return Err(CodecError::Internal("Media Foundation indisponible sur ce fil".into()));
        }
        let (w, h, fps, asked) = (w & !1, h & !1, fps.clamp(1, 240), bps.max(1));
        let bps = capped(asked, w, h, fps);
        let mut last = CodecError::Internal("aucun encodeur matériel".into());
        for a in hardware_encoders(codec) {
            match Self::open(a, codec, w, h, fps, bps) {
                Ok(mut e) => {
                    e.asked = asked;
                    return Ok(e);
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn open(activate: IMFActivate, codec: Codec, w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        // SAFETY: appels COM sur des objets valides, avec des arguments
        // possédés qui vivent pendant chaque appel.
        let built = unsafe {
            (|| -> windows::core::Result<(IMFTransform, IMFMediaEventGenerator, Option<ICodecAPI>)> {
                let mft: IMFTransform = activate.ActivateObject()?;
                let attrs = mft.GetAttributes()?;
                if attrs.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0 {
                    attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
                }
                let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
                let api = mft.cast::<ICodecAPI>().ok();
                let a = api.as_ref();
                // CBR : mesuré sur AMD, le VBR plafonné rend chaque image ~22 ms
                // plus tard. Le remplissage du CBR est retiré (`strip_filler`).
                set_codec_value(a, &CODECAPI_AVEncCommonRateControlMode, VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32));
                set_codec_value(a, &CODECAPI_AVEncCommonMeanBitRate, VARIANT::from(bps));
                set_codec_value(a, &CODECAPI_AVEncCommonMaxBitRate, VARIANT::from(bps));
                set_codec_value(a, &CODECAPI_AVLowLatencyMode, VARIANT::from(true));
                set_codec_value(a, &CODECAPI_AVEncMPVDefaultBPictureCount, VARIANT::from(0u32));
                // Images clés à la demande (perte, nouveau spectateur) plutôt que périodiques.
                set_codec_value(a, &CODECAPI_AVEncMPVGOPSize, VARIANT::from(fps.saturating_mul(60)));

                let out = video_type(&hardware_format(codec))?;
                out.SetUINT32(&MF_MT_AVG_BITRATE, bps)?;
                out.SetUINT64(&MF_MT_FRAME_SIZE, pair(w, h))?;
                out.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1))?;
                out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
                out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                if codec == Codec::H264 {
                    out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
                }
                mft.SetOutputType(0, &out, 0)?;
                let inp = video_type(&MFVideoFormat_NV12)?;
                inp.SetUINT64(&MF_MT_FRAME_SIZE, pair(w, h))?;
                inp.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1))?;
                inp.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
                inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                mft.SetInputType(0, &inp, 0)?;
                let events = mft.cast::<IMFMediaEventGenerator>()?;
                mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
                mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
                Ok((mft, events, api))
            })()
        };
        let name = friendly_name(&activate);
        let (mft, events, api) = match built {
            Ok(b) => b,
            Err(e) => {
                // SAFETY: objet valide ; libère la transformée à moitié configurée.
                let _ = unsafe { activate.ShutdownObject() };
                return Err(hr(e));
            }
        };
        Ok(Self {
            codec,
            name,
            activate,
            mft,
            events,
            api,
            width: w,
            height: h,
            fps,
            bps,
            asked: bps,
            credits: 0,
            ready: Vec::new(),
            bytes: Vec::new(),
            watch: None,
            deaf: false,
            reopen: false,
        })
    }

    /// Traite les événements en attente ; `false` s'il n'y en avait aucun.
    fn pump(&mut self) -> Result<bool, CodecError> {
        // SAFETY: lecture non bloquante d'un événement sur un objet valide.
        let ev = match unsafe { self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
            Ok(ev) => ev,
            Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => return Ok(false),
            Err(e) => return Err(hr(e)),
        };
        // SAFETY: événement valide, lu par valeur.
        let (kind, status) = unsafe { (ev.GetType().map_err(hr)?, ev.GetStatus().map_err(hr)?) };
        status.ok().map_err(hr)?;
        if kind == METransformNeedInput.0 as u32 {
            self.credits += 1;
        } else if kind == METransformHaveOutput.0 as u32 {
            self.collect()?;
        }
        Ok(true)
    }

    fn collect(&mut self) -> Result<(), CodecError> {
        let out = match process_output(&self.mft, output_sample(&self.mft).map_err(hr)?) {
            Ok(o) => o,
            // Sortie demandée sans qu'elle soit annoncée : rien à prendre pour l'instant.
            Err(e) if e.code() == windows::Win32::Foundation::E_UNEXPECTED => return Ok(()),
            Err(e) => return Err(hr(e)),
        };
        match out {
            Out::NeedMore => {}
            Out::StreamChange => {
                // SAFETY: le type proposé par la transformée lui est rendu tel quel.
                unsafe {
                    let t = self.mft.GetOutputAvailableType(0, 0).map_err(hr)?;
                    self.mft.SetOutputType(0, &t, 0).map_err(hr)?;
                }
                // Transformée asynchrone : une sortie par annonce (METransformHaveOutput).
                // En redemander une tout de suite rendait 0x8000ffff sur Intel Quick
                // Sync ; l'encodeur était jeté, rouvert, et échouait de nouveau à chaque
                // définition, jusqu'à l'encodeur logiciel en 720p (constaté par
                // l'utilisateur, 0.1.32). La sortie viendra avec l'annonce suivante.
            }
            Out::Sample(s) => {
                sample_bytes(&s, &mut self.bytes).map_err(hr)?;
                if self.codec == Codec::H264 {
                    strip_filler(&mut self.bytes);
                }
                // SAFETY: lectures d'attributs d'un échantillon valide.
                let (key, t) = unsafe { (s.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0, s.GetSampleTime().unwrap_or(0)) };
                if !self.bytes.is_empty() {
                    let pts_us = u64::try_from(t / HNS_PER_US).unwrap_or(0);
                    self.ready.push(EncodedFrame { codec: self.codec, keyframe: key, ts90k: us_to_90k(pts_us), data: std::mem::take(&mut self.bytes) });
                }
            }
        }
        Ok(())
    }
}

/// Nom donné par Windows à une transformée (« NVIDIA H.264 Encoder MFT »),
/// gardé pour toute la vie du processus (il n'y en a qu'une poignée).
fn friendly_name(a: &IMFActivate) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let (mut p, mut n) = (windows::core::PWSTR::null(), 0u32);
    // SAFETY: en cas de succès, `p` désigne une chaîne allouée par COM,
    // copiée puis libérée une fois.
    let name = unsafe {
        if a.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut p, &mut n).is_err() || p.is_null() {
            return "";
        }
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    };
    let mut names = NAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(known) = names.iter().find(|k| **k == name) {
        return known;
    }
    let kept: &'static str = Box::leak(name.into_boxed_str());
    names.push(kept);
    kept
}

#[cfg(test)]
impl MfEncoder {
    /// Un encodeur par transformée matérielle de la machine, avec son nom.
    pub(super) fn each(w: u32, h: u32, fps: u32, bps: u32) -> Vec<(&'static str, Result<Self, CodecError>)> {
        if !ready() {
            return Vec::new();
        }
        hardware_encoders(Codec::H264).into_iter().map(|a| (friendly_name(&a), Self::open(a, Codec::H264, w, h, fps, bps))).collect()
    }
}

impl MfEncoder {
    /// Le débit sorti depuis la dernière baisse la dépasse-t-il trop ? Jugé
    /// sur une seconde d'images, images clés à part (elles pèsent dix fois
    /// plus, quel que soit le débit).
    fn judge(&mut self, out: &[EncodedFrame]) {
        let Some((frames, bytes)) = &mut self.watch else { return };
        for e in out.iter().filter(|e| !e.keyframe) {
            *frames += 1;
            *bytes += e.data.len() as u64;
        }
        if *frames < self.fps {
            return;
        }
        let rate = *bytes * 8 * u64::from(self.fps) / u64::from(*frames);
        self.watch = None;
        if rate * DEAF_DEN > u64::from(self.bps) * DEAF_NUM {
            self.deaf = true;
            self.reopen = true;
            crate::sharelog::line(format_args!("encodeur : {} ne suit pas le débit ({} kb/s sortis pour {} demandés), rouvert à chaque changement", self.name, rate / 1000, self.bps / 1000));
        }
    }
}

impl Encoder for MfEncoder {
    fn name(&self) -> &'static str {
        self.name
    }

    fn encode(&mut self, f: &Frame, force_keyframe: bool) -> Result<Vec<EncodedFrame>, CodecError> {
        if self.reopen {
            // Transformée sourde : une neuve au débit voulu (sa première image est une image clé).
            // La même transformée, pas la première de la liste : une autre carte encoderait.
            let (asked, bps) = (self.asked, capped(self.asked, self.width, self.height, self.fps));
            let same = hardware_encoders(self.codec).into_iter().find(|a| friendly_name(a) == self.name);
            *self = match same {
                Some(a) => Self::open(a, self.codec, self.width, self.height, self.fps, bps)?,
                None => Self::with_codec(self.codec, self.width, self.height, self.fps, asked)?,
            };
            self.asked = asked;
            self.deaf = true;
        }
        let deaf = self.deaf;
        let out = self.encode_now(f, force_keyframe)?;
        // Nouvelle taille : la transformée neuve est la même, sourde ou non.
        self.deaf |= deaf;
        self.judge(&out);
        Ok(out)
    }

    fn set_bitrate(&mut self, bps: u32) {
        self.asked = bps.max(1);
        let bps = capped(self.asked, self.width, self.height, self.fps);
        if self.deaf {
            self.reopen |= bps != self.bps;
            return;
        }
        let a = self.api.as_ref();
        // Plafond d'abord quand le débit baisse (moyenne ≤ plafond à tout instant).
        let mut keys = [CODECAPI_AVEncCommonMaxBitRate, CODECAPI_AVEncCommonMeanBitRate];
        if bps > self.bps {
            keys.reverse();
        }
        let set = keys.map(|k| set_codec_value(a, &k, VARIANT::from(bps)));
        // Le plafond est facultatif en CBR ; la moyenne, non.
        if set[usize::from(bps <= self.bps)] {
            // Baisse : la sortie dira si elle est suivie.
            if bps < self.bps {
                self.watch = Some((0, 0));
            }
            self.bps = bps;
        }
    }
}

impl MfEncoder {
    fn encode_now(&mut self, f: &Frame, force_keyframe: bool) -> Result<Vec<EncodedFrame>, CodecError> {
        if !f.is_consistent() {
            return Err(CodecError::BadFrame("taille des plans incohérente"));
        }
        check_size(f.width, f.height)?;
        if (f.width & !1, f.height & !1) != (self.width, self.height) {
            // Nouvelle taille : nouvelle transformée (la première image est une image clé).
            *self = Self::with_codec(self.codec, f.width, f.height, self.fps, self.asked)?;
        }
        let (w, h) = (f.width as usize, f.height as usize);
        // NV12 aux dimensions paires : dernière colonne ou ligne impaire rognée.
        let (ew, eh, cw) = (self.width as usize, self.height as usize, w.div_ceil(2));
        let pts = i64::try_from(f.pts_us).unwrap_or(i64::MAX / HNS_PER_US) * HNS_PER_US;
        let mut bad = false;
        let sample = new_sample(ew * eh * 3 / 2, pts, |dst| match &f.data {
            // Image d'écran : convertie droit dans le tampon de la transformée.
            FrameData::Bgra(b) => bad = b.len() != w * h * 4 || convert::bgra_to_nv12(w, b, ew, eh, dst).is_err(),
            FrameData::I420 { y, u, v } => {
                let (dy, duv) = dst.split_at_mut(ew * eh);
                for (row, d) in dy.chunks_exact_mut(ew).enumerate() {
                    d.copy_from_slice(&y[row * w..row * w + ew]);
                }
                for (row, d) in duv.chunks_exact_mut(ew).enumerate() {
                    let (us, vs) = (&u[row * cw..row * cw + ew / 2], &v[row * cw..row * cw + ew / 2]);
                    for ((p, &a), &b) in d.as_chunks_mut::<2>().0.iter_mut().zip(us).zip(vs) {
                        *p = [a, b];
                    }
                }
            }
        })
        .map_err(hr)?;
        if bad {
            return Err(CodecError::BadFrame("BGRA incohérent"));
        }
        // SAFETY: échantillon valide ; durée en unités de 100 ns.
        let _ = unsafe { sample.SetSampleDuration(10_000_000 / i64::from(self.fps)) };
        if force_keyframe {
            set_codec_value(self.api.as_ref(), &CODECAPI_AVEncVideoForceKeyFrame, VARIANT::from(1u32));
        }

        // Attente d'une demande d'entrée (normalement déjà là), puis de la sortie
        // pendant au plus une période d'image : au-delà, elle partira avec la suivante.
        let period = Duration::from_secs(1) / self.fps;
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.credits == 0 {
            if !self.pump()? {
                if Instant::now() > deadline {
                    return Err(CodecError::Internal("l'encodeur matériel ne réclame plus d'images".into()));
                }
                std::thread::sleep(Duration::from_micros(250));
            }
        }
        // SAFETY: échantillon valide, demandé par la transformée (crédit).
        unsafe { self.mft.ProcessInput(0, &sample, 0) }.map_err(hr)?;
        self.credits -= 1;
        let deadline = Instant::now() + period;
        while self.ready.is_empty() && Instant::now() < deadline {
            if !self.pump()? {
                std::thread::sleep(Duration::from_micros(250));
            }
        }
        while self.pump()? {}
        Ok(std::mem::take(&mut self.ready))
    }
}

impl Drop for MfEncoder {
    fn drop(&mut self) {
        // SAFETY: objets valides ; ShutdownObject rompt les références
        // circulaires de la file d'événements des transformées asynchrones.
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.activate.ShutdownObject();
        }
    }
}

// ---------------------------------------------------------- encodeur VP8

/// Encodeur VP8 logiciel des « Extensions vidéo VP9 », synchrone, entrée
/// IYUV (l'agencement I420 : aucune conversion pour la caméra).
pub struct MfVp8Encoder {
    /// VP8 ; VP9 pour fabriquer les jeux d'essai.
    codec: Codec,
    activate: IMFActivate,
    mft: IMFTransform,
    api: Option<ICodecAPI>,
    width: u32,
    height: u32,
    fps: u32,
    bps: u32,
    /// Nouveau débit, appliqué en recréant la transformée à la prochaine image
    /// clé demandée : mesuré, elle accepte un débit à chaud sans l'appliquer.
    pending_bps: Option<u32>,
    bytes: Vec<u8>,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

// SAFETY: comme `MfEncoder` (créé dans le MTA, accès exclusif par `&mut self`).
unsafe impl Send for MfVp8Encoder {}

impl MfVp8Encoder {
    pub fn new(w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        Self::with_codec(Codec::Vp8, w, h, fps, bps)
    }

    /// Encodeur VP9 des mêmes extensions : pour les jeux d'essai.
    #[cfg(test)]
    pub fn vp9(w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        Self::with_codec(Codec::Vp9, w, h, fps, bps)
    }

    fn with_codec(codec: Codec, w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        check_size(w, h)?;
        if !ready() {
            return Err(CodecError::Internal("Media Foundation indisponible sur ce fil".into()));
        }
        let (w, h, fps, bps) = (w & !1, h & !1, fps.clamp(1, 240), bps.max(1));
        let mut last = CodecError::Internal("aucun encodeur VP8 (Extensions vidéo VP9 absentes)".into());
        for a in transforms(MFT_CATEGORY_VIDEO_ENCODER, SOFTWARE, &MFVideoFormat_IYUV, Some(&vpx_format(codec))) {
            match Self::open(a, codec, w, h, fps, bps) {
                Ok(e) => return Ok(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn open(activate: IMFActivate, codec: Codec, w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        // SAFETY: appels COM sur des objets valides, avec des arguments
        // possédés qui vivent pendant chaque appel.
        let built = unsafe {
            (|| -> windows::core::Result<(IMFTransform, Option<ICodecAPI>)> {
                let mft: IMFTransform = activate.ActivateObject()?;
                let _ = mft.GetAttributes().and_then(|a| a.SetUINT32(&MF_LOW_LATENCY, 1));
                let api = mft.cast::<ICodecAPI>().ok();
                let a = api.as_ref();
                set_codec_value(a, &CODECAPI_AVLowLatencyMode, VARIANT::from(true));
                set_codec_value(a, &CODECAPI_AVEncCommonRateControlMode, VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32));
                set_codec_value(a, &CODECAPI_AVEncCommonMeanBitRate, VARIANT::from(bps));
                // Images clés à la demande plutôt que périodiques, comme pour H.264.
                set_codec_value(a, &CODECAPI_AVEncMPVGOPSize, VARIANT::from(fps.saturating_mul(60)));

                let out = video_type(&vpx_format(codec))?;
                out.SetUINT32(&MF_MT_AVG_BITRATE, bps)?;
                out.SetUINT64(&MF_MT_FRAME_SIZE, pair(w, h))?;
                out.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1))?;
                out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
                out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                mft.SetOutputType(0, &out, 0)?;
                let inp = video_type(&MFVideoFormat_IYUV)?;
                inp.SetUINT64(&MF_MT_FRAME_SIZE, pair(w, h))?;
                inp.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1))?;
                inp.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
                inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                mft.SetInputType(0, &inp, 0)?;
                mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
                mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
                Ok((mft, api))
            })()
        };
        let (mft, api) = match built {
            Ok(b) => b,
            Err(e) => {
                // SAFETY: objet valide ; libère la transformée à moitié configurée.
                let _ = unsafe { activate.ShutdownObject() };
                return Err(hr(e));
            }
        };
        Ok(Self { codec, activate, mft, api, width: w, height: h, fps, bps, pending_bps: None, bytes: Vec::new(), y: Vec::new(), u: Vec::new(), v: Vec::new() })
    }

    /// Tout ce que la transformée a de prêt.
    fn collect(&mut self, out: &mut Vec<EncodedFrame>, ts90k: u32) -> Result<(), CodecError> {
        loop {
            match process_output(&self.mft, output_sample(&self.mft).map_err(hr)?).map_err(hr)? {
                Out::NeedMore => return Ok(()),
                Out::StreamChange => {
                    // SAFETY: le type proposé par la transformée lui est rendu tel quel.
                    unsafe {
                        let t = self.mft.GetOutputAvailableType(0, 0).map_err(hr)?;
                        self.mft.SetOutputType(0, &t, 0).map_err(hr)?;
                    }
                }
                Out::Sample(s) => {
                    sample_bytes(&s, &mut self.bytes).map_err(hr)?;
                    if !self.bytes.is_empty() {
                        // En-tête de l'image : plus sûr que l'attribut de l'échantillon.
                        let keyframe = if self.codec == Codec::Vp9 { vp8::vp9_keyframe_size(&self.bytes).is_some() } else { self.bytes[0] & 1 == 0 };
                        out.push(EncodedFrame { codec: self.codec, keyframe, ts90k, data: std::mem::take(&mut self.bytes) });
                    }
                }
            }
        }
    }
}

impl Encoder for MfVp8Encoder {
    fn encode(&mut self, f: &Frame, force_keyframe: bool) -> Result<Vec<EncodedFrame>, CodecError> {
        if !f.is_consistent() {
            return Err(CodecError::BadFrame("taille des plans incohérente"));
        }
        check_size(f.width, f.height)?;
        let rebuild_bps = self.pending_bps.filter(|_| force_keyframe);
        let mut rebuild = (f.width & !1, f.height & !1) != (self.width, self.height) || rebuild_bps.is_some();
        // Sans image clé à la demande, une transformée neuve en commence une.
        rebuild |= force_keyframe && !rebuild && !set_codec_value(self.api.as_ref(), &CODECAPI_AVEncVideoForceKeyFrame, VARIANT::from(1u32));
        if rebuild {
            // Nouvelle taille, nouveau débit ou image clé : nouvelle transformée.
            *self = Self::with_codec(self.codec, f.width, f.height, self.fps, rebuild_bps.unwrap_or(self.bps))?;
        }
        let (w, h) = (f.width as usize, f.height as usize);
        let (y, u, v): (&[u8], &[u8], &[u8]) = match &f.data {
            FrameData::I420 { y, u, v } => (y, u, v),
            FrameData::Bgra(b) => {
                convert::bgra_to_i420(w, h, b, &mut self.y, &mut self.u, &mut self.v).map_err(|_| CodecError::BadFrame("BGRA incohérent"))?;
                (&self.y, &self.u, &self.v)
            }
        };
        // IYUV aux dimensions paires : dernière colonne ou ligne impaire rognée.
        let (ew, eh, cw) = (self.width as usize, self.height as usize, w.div_ceil(2));
        let pts = i64::try_from(f.pts_us).unwrap_or(i64::MAX / HNS_PER_US) * HNS_PER_US;
        let sample = new_sample(ew * eh * 3 / 2, pts, |dst| {
            let (dy, dc) = dst.split_at_mut(ew * eh);
            let (du, dv) = dc.split_at_mut(ew * eh / 4);
            for (row, d) in dy.chunks_exact_mut(ew).enumerate() {
                d.copy_from_slice(&y[row * w..row * w + ew]);
            }
            for (row, (du, dv)) in du.chunks_exact_mut(ew / 2).zip(dv.chunks_exact_mut(ew / 2)).enumerate() {
                du.copy_from_slice(&u[row * cw..row * cw + ew / 2]);
                dv.copy_from_slice(&v[row * cw..row * cw + ew / 2]);
            }
        })
        .map_err(hr)?;
        // SAFETY: échantillon valide ; durée en unités de 100 ns.
        let _ = unsafe { sample.SetSampleDuration(10_000_000 / i64::from(self.fps)) };
        let ts90k = us_to_90k(f.pts_us);
        let mut out = Vec::new();
        // SAFETY: échantillon valide pour le flux 0.
        let mut r = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        if matches!(&r, Err(e) if e.code() == MF_E_NOTACCEPTING) {
            self.collect(&mut out, ts90k)?;
            // SAFETY: idem, après avoir vidé la sortie.
            r = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        }
        r.map_err(hr)?;
        self.collect(&mut out, ts90k)?;
        Ok(out)
    }

    fn set_bitrate(&mut self, bps: u32) {
        let bps = bps.max(1);
        self.pending_bps = (bps != self.bps).then_some(bps);
    }
}

impl Drop for MfVp8Encoder {
    fn drop(&mut self) {
        // SAFETY: objets valides.
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.activate.ShutdownObject();
        }
    }
}

// ---------------------------------------------------------------- décodeur

/// Décodeur de Windows, synchrone : H.264 (`CLSID_MSH264DecoderMFT`) ou
/// VP8 (transformée logicielle des « Extensions vidéo VP9 », livrées avec
/// Windows 10 et 11, qui décode aussi le VP8).
pub struct MfDecoder {
    codec: Codec,
    mft: IMFTransform,
    /// Transformée obtenue par énumération (VP8) : arrêtée en fin de vie.
    activate: Option<IMFActivate>,
    /// Dimensions affichées, et pas de ligne et hauteur des plans décodés (alignées sur 16).
    shape: Option<Shape>,
    out: Option<IMFSample>,
    /// Rien n'est confié au décodeur avant une image clé (début ou erreur).
    need_key: bool,
    /// VP8 : taille de la dernière image clé, posée sur le type d'entrée
    /// (`None` : type d'entrée pas encore posé).
    key_size: Option<(u32, u32)>,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Shape {
    width: usize,
    height: usize,
    stride: usize,
    rows: usize,
    /// Sortie NV12 (chrominance entrelacée) plutôt que I420.
    nv12: bool,
}

// SAFETY: comme `MfEncoder` (créé dans le MTA, accès exclusif par `&mut self`).
unsafe impl Send for MfDecoder {}

/// Sous-type Media Foundation d'un flux VP8, VP9 ou AV1.
fn vpx_format(codec: Codec) -> GUID {
    match codec {
        Codec::Vp9 => MFVideoFormat_VP90,
        Codec::Av1 => MFVideoFormat_AV1,
        _ => MFVideoFormat_VP80,
    }
}

/// VP9 et AV1 décodables sur cette machine (« Extensions vidéo VP9 » et
/// « Extension vidéo AV1 »). Appelé sur un fil dans le MTA.
pub(super) fn extra_support() -> (bool, bool) {
    let has = |c| ready() && !transforms(MFT_CATEGORY_VIDEO_DECODER, SOFTWARE, &vpx_format(c), None).is_empty();
    (has(Codec::Vp9), has(Codec::Av1))
}

/// Vrai si l'unité d'accès (Annex B) contient une tranche IDR.
fn has_idr(data: &[u8]) -> bool {
    data.windows(4).any(|w| w[..3] == [0, 0, 1] && w[3] & 0x1f == 5)
}

/// Unités NAL d'un flux Annex B : (début du code de départ, fin, type).
fn nal_units(d: &[u8]) -> Vec<(usize, usize, u8)> {
    let starts: Vec<usize> = (0..d.len().saturating_sub(3)).filter(|&i| d[i..i + 3] == [0, 0, 1]).collect();
    let begin = |p: usize| if p > 0 && d[p - 1] == 0 { p - 1 } else { p };
    starts.iter().enumerate().map(|(k, &p)| (begin(p), starts.get(k + 1).map_or(d.len(), |&n| begin(n)), d[p + 3] & 0x1f)).collect()
}

/// Retire les NAL de remplissage (type 12) : inutiles sur le réseau, et le
/// décodeur de Windows attend l'unité d'accès suivante quand une image en
/// finit par une (une image de retard).
fn strip_filler(d: &mut Vec<u8>) {
    if !d.windows(4).any(|w| w[..3] == [0, 0, 1] && w[3] & 0x1f == 12) {
        return;
    }
    let mut out = Vec::with_capacity(d.len());
    for (a, b, t) in nal_units(d) {
        if t != 12 {
            out.extend_from_slice(&d[a..b]);
        }
    }
    *d = out;
}

impl MfDecoder {
    /// Décodeur H.264.
    pub fn new() -> Result<Self, CodecError> {
        if !ready() {
            return Err(CodecError::Internal("Media Foundation indisponible sur ce fil".into()));
        }
        // SAFETY: création et configuration d'un objet COM neuf, arguments possédés.
        let mft = unsafe {
            (|| -> windows::core::Result<IMFTransform> {
                let mft: IMFTransform = CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)?;
                let _ = mft.GetAttributes()?.SetUINT32(&MF_LOW_LATENCY, 1);
                mft.SetInputType(0, &video_type(&MFVideoFormat_H264)?, 0)?;
                Ok(mft)
            })()
        }
        .map_err(hr)?;
        let mut d = Self::with(Codec::H264, mft, None);
        d.choose_output().map_err(hr)?;
        // SAFETY: objet valide.
        unsafe { d.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) }.map_err(hr)?;
        Ok(d)
    }

    /// Décodeur VP8 logiciel, s'il est installé. Le type d'entrée attend la
    /// première image clé, qui donne la taille.
    pub fn vp8() -> Result<Self, CodecError> {
        Self::vpx(Codec::Vp8)
    }

    /// Décodeur VP9 des mêmes extensions.
    pub fn vp9() -> Result<Self, CodecError> {
        Self::vpx(Codec::Vp9)
    }

    /// Décodeur AV1 logiciel de l'« Extension vidéo AV1 », s'il est installé.
    pub fn av1() -> Result<Self, CodecError> {
        Self::vpx(Codec::Av1)
    }

    fn vpx(codec: Codec) -> Result<Self, CodecError> {
        if !ready() {
            return Err(CodecError::Internal("Media Foundation indisponible sur ce fil".into()));
        }
        let mut last = CodecError::Internal("aucun décodeur de Windows pour ce codec (extension vidéo absente)".into());
        for a in transforms(MFT_CATEGORY_VIDEO_DECODER, SOFTWARE, &vpx_format(codec), None) {
            // SAFETY: activation d'un objet énuméré, valide ; en cas d'échec il est arrêté.
            match unsafe { a.ActivateObject::<IMFTransform>() } {
                Ok(mft) => {
                    // SAFETY: objet neuf, valeur passée par valeur.
                    let _ = unsafe { mft.GetAttributes().and_then(|at| at.SetUINT32(&MF_LOW_LATENCY, 1)) };
                    return Ok(Self::with(codec, mft, Some(a)));
                }
                Err(e) => {
                    last = hr(e);
                    // SAFETY: objet valide.
                    let _ = unsafe { a.ShutdownObject() };
                }
            }
        }
        Err(last)
    }

    fn with(codec: Codec, mft: IMFTransform, activate: Option<IMFActivate>) -> Self {
        Self { codec, mft, activate, shape: None, out: None, need_key: true, key_size: None, bytes: Vec::new() }
    }

    /// VP8 : type d'entrée à la taille de l'image clé (posé au premier
    /// passage, reposé quand la taille change).
    fn vp8_input(&mut self, size: (u32, u32)) -> Result<(), CodecError> {
        if self.key_size == Some(size) {
            return Ok(());
        }
        if self.key_size.is_some() {
            // Le type ne change plus en cours de flux (mesuré : 0xc00d6d74) :
            // nouvelle transformée.
            *self = Self::vpx(self.codec)?;
        }
        // SAFETY: objet valide, type neuf passé par référence.
        let set = unsafe {
            (|| -> windows::core::Result<()> {
                let t = video_type(&vpx_format(self.codec))?;
                t.SetUINT64(&MF_MT_FRAME_SIZE, pair(size.0, size.1))?;
                self.mft.SetInputType(0, &t, 0)
            })()
        };
        set.and_then(|()| self.choose_output()).map_err(hr)?;
        // SAFETY: objet valide.
        unsafe { self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) }.map_err(hr)?;
        self.key_size = Some(size);
        self.need_key = false;
        Ok(())
    }

    /// Choisit la sortie I420 (ou IYUV, le même agencement), à défaut NV12,
    /// et relève sa géométrie.
    fn choose_output(&mut self) -> windows::core::Result<()> {
        // SAFETY: types énumérés par la transformée et rendus tels quels ;
        // l'ouverture minimale est lue dans une structure de la bonne taille.
        unsafe {
            let mut nv12 = None;
            let mut chosen = None;
            for i in 0.. {
                let Ok(t) = self.mft.GetOutputAvailableType(0, i) else { break };
                let sub = t.GetGUID(&MF_MT_SUBTYPE)?;
                // AV1 : NV12 seulement. Mesuré : le décodeur propose IYUV, l'accepte,
                // puis refuse de sortir la première image (0xc00d36b4).
                if self.codec != Codec::Av1 && (sub == MFVideoFormat_I420 || sub == MFVideoFormat_IYUV) {
                    chosen = Some((t, false));
                    break;
                }
                if sub == MFVideoFormat_NV12 && nv12.is_none() {
                    nv12 = Some(t);
                }
            }
            let Some((t, is_nv12)) = chosen.or(nv12.map(|t| (t, true))) else { return Err(MF_E_INVALIDMEDIATYPE.into()) };
            self.mft.SetOutputType(0, &t, 0)?;
            self.shape = shape_of(&t, is_nv12);
            self.out = None;
            Ok(())
        }
    }

    fn flush(&mut self) {
        // SAFETY: objet valide.
        let _ = unsafe { self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) };
        self.need_key = true;
    }

    /// Plans I420 contigus aux dimensions affichées, depuis `self.bytes`.
    fn frame(&self, s: Shape, pts_us: u64) -> Option<Frame> {
        i420_frame(&self.bytes, s, pts_us)
    }
}

/// Géométrie d'un type de sortie décodé ; `None` tant que la taille n'est pas
/// connue (avant le premier SPS ou la première image clé).
fn shape_of(t: &IMFMediaType, nv12: bool) -> Option<Shape> {
    // SAFETY: type valide ; l'ouverture minimale est lue dans une structure
    // de la bonne taille, que personne d'autre ne voit pendant l'appel.
    unsafe {
        let size = t.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        let (pw, ph) = ((size >> 32) as usize, (size & 0xffff_ffff) as usize);
        let stride = t.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(pw, |s| (s as i32).unsigned_abs() as usize).max(pw);
        let mut area = MFVideoArea::default();
        let bytes = std::slice::from_raw_parts_mut((&raw mut area).cast::<u8>(), size_of::<MFVideoArea>());
        let (w, h) = match t.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, None) {
            Ok(()) if area.Area.cx > 0 && area.Area.cy > 0 => (area.Area.cx as usize, area.Area.cy as usize),
            _ => (pw, ph),
        };
        Some(Shape { width: w.min(pw), height: h.min(ph), stride, rows: ph, nv12 })
    }
}

/// Plans I420 contigus aux dimensions affichées, depuis une image décodée `b`.
fn i420_frame(b: &[u8], s: Shape, pts_us: u64) -> Option<Frame> {
    {
        let (w, h, st) = (s.width, s.height, s.stride);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let plane = |off: usize, stride: usize, pw: usize, ph: usize| -> Option<Vec<u8>> {
            let mut out = Vec::with_capacity(pw * ph);
            for r in 0..ph {
                out.extend_from_slice(b.get(off + r * stride..off + r * stride + pw)?);
            }
            Some(out)
        };
        let y = plane(0, st, w, h)?;
        let (u, v) = if s.nv12 {
            let uv = plane(st * s.rows, st, cw * 2, ch)?;
            uv.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).unzip()
        } else {
            // Côtés impairs : la chrominance arrondit au-dessus (comme
            // `MFCalculateImageSize`) ; sans effet sur des côtés pairs.
            let (cst, crows) = (st.div_ceil(2), s.rows.div_ceil(2));
            (plane(st * s.rows, cst, cw, ch)?, plane(st * s.rows + cst * crows, cst, cw, ch)?)
        };
        Some(Frame { width: u32::try_from(w).ok()?, height: u32::try_from(h).ok()?, pts_us, data: FrameData::I420 { y, u, v } })
    }
}

impl MfDecoder {
    fn drain(&mut self, pts_us: u64) -> Result<Option<Frame>, CodecError> {
        let mut last = None;
        loop {
            if self.out.is_none() {
                self.out = output_sample(&self.mft).map_err(hr)?;
            }
            match process_output(&self.mft, self.out.clone()) {
                Ok(Out::NeedMore) => return Ok(last),
                Ok(Out::StreamChange) => self.choose_output().map_err(hr)?,
                Ok(Out::Sample(s)) => {
                    sample_bytes(&s, &mut self.bytes).map_err(hr)?;
                    self.out = None;
                    let shape = self.shape.ok_or_else(|| CodecError::Internal("taille décodée inconnue".into()))?;
                    last = Some(self.frame(shape, pts_us).ok_or_else(|| CodecError::Internal("image décodée plus courte qu'annoncé".into()))?);
                }
                Err(e) => {
                    self.flush();
                    return Err(CodecError::Corrupt(format!("sortie {:#010x}", e.code().0)));
                }
            }
        }
    }
}

impl Decoder for MfDecoder {
    fn decode(&mut self, f: &EncodedFrame) -> Result<Option<Frame>, CodecError> {
        if f.codec != self.codec {
            return Err(CodecError::WrongCodec(f.codec));
        }
        if f.data.is_empty() {
            return Err(CodecError::Corrupt("image vide".into()));
        }
        // Un seul décodage VP8 ou VP9 à la fois dans le processus (voir `VPX_ONE`).
        let _one = matches!(self.codec, Codec::Vp8 | Codec::Vp9).then(|| VPX_ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let vp8_key = match self.codec {
            Codec::Vp8 => vp8::keyframe_size(&f.data),
            Codec::Vp9 => vp8::vp9_keyframe_size(&f.data),
            Codec::Av1 => av1::sequence_size(&f.data),
            Codec::H264 => None,
        };
        if self.need_key {
            let key = match self.codec {
                Codec::Vp8 | Codec::Vp9 | Codec::Av1 => vp8_key.is_some(),
                Codec::H264 => has_idr(&f.data),
            };
            if !key {
                return Ok(None);
            }
            self.need_key = false;
        }
        if let Some(size) = vp8_key
            && let Err(e) = self.vp8_input(size)
        {
            self.need_key = true;
            return Err(e);
        }
        let pts_us = crate::video::ts90k_to_us(f.ts90k);
        let mut data = std::borrow::Cow::Borrowed(&f.data);
        if self.codec == Codec::H264 && f.data.windows(4).any(|w| w[..3] == [0, 0, 1] && w[3] & 0x1f == 12) {
            strip_filler(data.to_mut());
        }
        if self.codec == Codec::Av1 {
            // Image reçue du réseau : délimiteur temporel et tailles d'OBU remis.
            let Some(unit) = av1::temporal_unit(&f.data) else {
                self.need_key = true;
                return Err(CodecError::Corrupt("OBU illisibles".into()));
            };
            data = std::borrow::Cow::Owned(unit);
        }
        let sample = new_sample(data.len(), i64::try_from(pts_us).unwrap_or(0) * HNS_PER_US, |d| d.copy_from_slice(&data)).map_err(hr)?;
        // SAFETY: échantillon valide pour le flux 0.
        let mut r = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        let mut first = None;
        if matches!(&r, Err(e) if e.code() == MF_E_NOTACCEPTING) {
            first = self.drain(pts_us)?;
            // SAFETY: idem, après avoir vidé la sortie.
            r = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        }
        if let Err(e) = r {
            self.flush();
            return Err(CodecError::Corrupt(format!("entrée {:#010x}", e.code().0)));
        }
        let mut got = self.drain(pts_us)?.or(first);
        // Le décodeur de Windows décode sur ses propres fils : machine chargée,
        // l'image d'une tranche déjà acceptée peut sortir un peu plus tard. En
        // mode faible latence, chaque image en donne une : on l'attend (fil du
        // décodeur, jamais celui de l'interface), au plus `LATE_OUTPUT`.
        let picture = match self.codec {
            Codec::Vp8 => vp8::is_shown(&data),
            // VP9 : une image cachée voyage avec celle qui la montre (super-image) ;
            // AV1 : de même dans l'unité temporelle.
            Codec::Vp9 | Codec::Av1 => true,
            _ => has_picture(&data),
        };
        if got.is_none() && picture {
            let until = std::time::Instant::now() + LATE_OUTPUT;
            while got.is_none() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(1));
                got = self.drain(pts_us)?;
            }
        }
        Ok(got)
    }
}

impl Drop for MfDecoder {
    fn drop(&mut self) {
        if let Some(a) = &self.activate {
            // SAFETY: objet valide ; libère la transformée obtenue par activation.
            let _ = unsafe { a.ShutdownObject() };
        }
    }
}

/// Les décodeurs VP8 et VP9 des « Extensions vidéo VP9 » de Windows ne supportent
/// pas d'être appelés en même temps depuis plusieurs fils, même par des
/// transformées distinctes : de temps en temps, une image décodée n'était jamais
/// rendue (ni tout de suite, ni après 3 s d'attente, ni avec l'image suivante).
/// Mesuré : 3 échecs sur 72 passes des tests VP8 en parallèle, 0 sur 120 avec ce
/// verrou ; jamais d'échec avec un seul décodeur. Le décodage d'une image dure
/// bien moins qu'une milliseconde à ces tailles : l'attente est sans effet visible.
static VPX_ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Attente au plus d'une image décodée en retard sur son entrée.
const LATE_OUTPUT: std::time::Duration = std::time::Duration::from_millis(100);

/// Vrai si l'unité d'accès contient une tranche d'image (NAL 1 à 5).
fn has_picture(data: &[u8]) -> bool {
    data.windows(4).any(|w| w[..3] == [0, 0, 1] && (1..=5).contains(&(w[3] & 0x1f)))
}

/// VP8 : (encodeur, décodeur) présents. Appelé sur un fil neuf (voir `mod.rs`).
pub(super) fn vp8_support() -> (bool, bool) {
    if !ready() {
        return (false, false);
    }
    let enc = !transforms(MFT_CATEGORY_VIDEO_ENCODER, SOFTWARE, &MFVideoFormat_IYUV, Some(&MFVideoFormat_VP80)).is_empty();
    let dec = !transforms(MFT_CATEGORY_VIDEO_DECODER, SOFTWARE, &MFVideoFormat_VP80, None).is_empty();
    (enc, dec)
}
