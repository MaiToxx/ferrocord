//! Barre de titre intégrée sous Windows (lot V1) : la fenêtre est créée sans
//! décoration, et sa procédure de fenêtre est doublée ici pour que Windows
//! continue de tout faire lui-même.
//!
//! - `WM_NCCALCSIZE` : seule la bande du haut (titre et bord) devient zone
//!   cliente ; les bords invisibles de gauche, de droite et du bas restent ceux
//!   de Windows, avec leur redimensionnement.
//! - `WM_NCHITTEST` : la barre dessinée par l'interface répond `HTCAPTION`
//!   (déplacement, double-clic, menu système au clic droit, ancrage aux bords
//!   de l'écran : tout est fait par Windows), son bord haut `HTTOP` et ses
//!   coins, et le bouton d'agrandissement `HTMAXBUTTON`, ce qui ouvre le
//!   volet des dispositions d'ancrage de Windows 11 au survol.
//! - Le clic sur ce bouton est traité ici (il n'arrive plus à l'interface),
//!   et son survol lui est rapporté par [`state`].
//!
//! Les styles de la fenêtre ne sont pas touchés : winit garde `WS_CAPTION`,
//! `WS_SIZEBOX` et `WS_MAXIMIZEBOX` même sans décoration, donc Win+flèches et
//! l'ancrage par glisser marchent comme pour toute fenêtre.
//!
//! Tout l'`unsafe` Win32 de la barre de titre est dans ce fichier.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GetClientRect, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, IsZoomed, PostMessageW, SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, GWLP_WNDPROC, GWL_STYLE, HTCAPTION,
    HTCLIENT, HTMAXBUTTON, HTTOP, HTTOPLEFT, HTTOPRIGHT, NCCALCSIZE_PARAMS, SC_MAXIMIZE, SC_RESTORE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WINDOWPLACEMENT, WM_NCCALCSIZE, WM_NCHITTEST,
    WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSEMOVE, WM_SYSCOMMAND, WNDPROC, WS_THICKFRAME,
};

/// `WM_NCMOUSELEAVE` (absent des liaisons activées).
const WM_NCMOUSELEAVE: u32 = 0x02A2;
const WM_WINDOWPOSCHANGED: u32 = 0x0047;
const WM_ENTERSIZEMOVE: u32 = 0x0231;
const WM_EXITSIZEMOVE: u32 = 0x0232;

/// Écran de la fenêtre au dernier déplacement (0 : pas encore relevé).
static MONITOR: AtomicIsize = AtomicIsize::new(0);
/// Déplacement à la souris en cours, et changement d'écran vu pendant lui.
static MOVING: AtomicBool = AtomicBool::new(false);
static CROSSED: AtomicBool = AtomicBool::new(false);

/// Bouton de la barre des tâches : avec une barre par écran (« afficher les
/// boutons sur la barre où la fenêtre est ouverte »), le bouton restait sur
/// l'écran de départ quand la fenêtre passait sur un autre (constaté par
/// l'utilisateur, 0.1.12). Au changement d'écran, le bouton est retiré puis
/// remis (`ITaskbarList`) : l'Explorateur le range alors sur la barre du
/// nouvel écran. Pendant un déplacement à la souris, une seule fois, à la fin.
fn follow_monitor(window: HWND, msg: u32) {
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    match msg {
        WM_ENTERSIZEMOVE => MOVING.store(true, Ordering::SeqCst),
        WM_WINDOWPOSCHANGED => {
            // SAFETY: `window` est la fenêtre qui reçoit ce message ; aucun pointeur n'est passé.
            let now = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) }.0 as isize;
            let before = MONITOR.swap(now, Ordering::SeqCst);
            if before != 0 && before != now {
                if MOVING.load(Ordering::SeqCst) {
                    CROSSED.store(true, Ordering::SeqCst);
                } else {
                    refresh_taskbar_button(window);
                }
            }
        }
        WM_EXITSIZEMOVE => {
            MOVING.store(false, Ordering::SeqCst);
            if CROSSED.swap(false, Ordering::SeqCst) {
                refresh_taskbar_button(window);
            }
        }
        _ => {}
    }
}

/// Retire puis remet le bouton de `window` dans la barre des tâches.
fn refresh_taskbar_button(window: HWND) {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
    // SAFETY: appelé sur le fil de la fenêtre, où winit a initialisé COM
    // (OleInitialize) ; l'objet est local et libéré en sortie ; `window` est
    // vivante. Un échec (pas d'Explorateur, COM absent) est sans effet.
    unsafe {
        let Ok(list) = CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER) else { return };
        if list.HrInit().is_ok() && list.DeleteTab(window).is_ok() {
            let _ = list.AddTab(window);
        }
    }
}

/// Rectangle en pixels de la zone cliente : gauche, haut, droite, bas.
pub type Px = [i32; 4];

/// Ce que l'interface a dessiné à cette image, en pixels de la zone cliente.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Regions {
    /// Hauteur de la barre de titre ; 0 : aucune (plein écran).
    pub height: i32,
    /// Bande du haut qui redimensionne la fenêtre, et largeur de ses coins.
    pub resize: i32,
    /// Bouton d'agrandissement.
    pub maximize: Px,
    /// Boutons de la barre gérés par l'interface (boîte, aide, réduire,
    /// fermer) : ils ne déplacent pas la fenêtre.
    pub holes: Vec<Px>,
}

/// Survol et appui du bouton d'agrandissement, vus par Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaxButton {
    pub hovered: bool,
    pub pressed: bool,
}

/// Fenêtre doublée (0 : aucune) et sa procédure d'origine.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);
/// La barre intégrée est en service (sinon tout est laissé à la procédure d'origine).
static ACTIVE: AtomicBool = AtomicBool::new(false);
static HOVERED: AtomicBool = AtomicBool::new(false);
static PRESSED: AtomicBool = AtomicBool::new(false);
static TRACKING: AtomicBool = AtomicBool::new(false);
static REGIONS: Mutex<Option<Regions>> = Mutex::new(None);
static WAKE: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

fn wake() {
    let wake = WAKE.lock().ok().and_then(|w| w.clone());
    if let Some(w) = wake {
        w();
    }
}

fn contains(r: &Px, x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

/// Zone de la barre touchée par un point de la zone cliente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Client,
    Caption,
    Maximize,
    Top,
    TopLeft,
    TopRight,
}

/// Décision de `WM_NCHITTEST` pour le point (`x`, `y`) d'une zone cliente
/// large de `width`. `sizable` : la fenêtre se redimensionne (ni agrandie, ni
/// en plein écran).
pub fn hit(r: &Regions, x: i32, y: i32, width: i32, sizable: bool) -> Hit {
    if r.height <= 0 || y < 0 || y >= r.height || x < 0 || x >= width {
        return Hit::Client;
    }
    if sizable && y < r.resize {
        let corner = 3 * r.resize;
        return if x < corner {
            Hit::TopLeft
        } else if x >= width - corner {
            Hit::TopRight
        } else {
            Hit::Top
        };
    }
    if contains(&r.maximize, x, y) {
        return Hit::Maximize;
    }
    if r.holes.iter().any(|h| contains(h, x, y)) { Hit::Client } else { Hit::Caption }
}

/// Double la procédure de la fenêtre `hwnd` (une seule fenêtre : la
/// principale ; un second appel ne fait rien). Si la fenêtre a été créée sans
/// décoration (sa zone cliente la couvre toute), la barre est en service tout
/// de suite. `wake` : redessiner (le survol du bouton d'agrandissement a
/// changé). À appeler sur le fil de la fenêtre.
pub fn install(hwnd: isize, wake: Arc<dyn Fn() + Send + Sync>) -> bool {
    if hwnd == 0 || WINDOW.compare_exchange(0, hwnd, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return false;
    }
    if let Ok(mut w) = WAKE.lock() {
        *w = Some(wake);
    }
    let window = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: `hwnd` est la fenêtre principale, vivante, créée par ce fil
    // (l'appelant le garantit) ; `proc` a la signature d'une procédure de
    // fenêtre et vit autant que le programme. La procédure d'origine est
    // gardée pour lui passer tout ce qui n'est pas traité ici.
    let previous = unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, proc as *const () as isize) };
    if previous == 0 {
        WINDOW.store(0, Ordering::SeqCst);
        return false;
    }
    PREVIOUS.store(previous, Ordering::SeqCst);
    if frameless(window) {
        ACTIVE.store(true, Ordering::SeqCst);
        keep_client_size(window);
    }
    true
}

/// Fenêtre sans décoration : winit lui donne une zone cliente qui la couvre toute.
fn frameless(window: HWND) -> bool {
    let (mut outer, mut inner) = (RECT::default(), RECT::default());
    // SAFETY: `window` est vivante ; les deux RECT sont sur la pile, vivants pendant les appels.
    let ok = unsafe { GetWindowRect(window, &mut outer).is_ok() && GetClientRect(window, &mut inner).is_ok() };
    ok && inner.right - inner.left == outer.right - outer.left && inner.bottom - inner.top == outer.bottom - outer.top
}

/// Une fenêtre est doublée : la barre intégrée peut être mise en service.
pub fn installed() -> bool {
    WINDOW.load(Ordering::SeqCst) != 0
}

/// Met la barre intégrée en service ou hors service (réglage changé pendant
/// que l'application tourne) ; le cadre est recalculé.
pub fn set_active(on: bool) {
    if ACTIVE.swap(on, Ordering::SeqCst) == on {
        return;
    }
    if !on {
        HOVERED.store(false, Ordering::SeqCst);
        PRESSED.store(false, Ordering::SeqCst);
    }
    let hwnd = WINDOW.load(Ordering::SeqCst);
    if hwnd != 0 {
        // SAFETY: fenêtre doublée par `install`, encore vivante (sinon l'appel
        // échoue sans effet) ; aucun pointeur n'est passé.
        unsafe {
            let _ = SetWindowPos(HWND(hwnd as *mut core::ffi::c_void), None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// La barre intégrée est en service sur une fenêtre doublée.
pub fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst) && WINDOW.load(Ordering::SeqCst) != 0
}

/// Zones de la barre dessinée à cette image.
pub fn set_regions(r: Regions) {
    if let Ok(mut cur) = REGIONS.lock()
        && cur.as_ref() != Some(&r)
    {
        *cur = Some(r);
    }
}

/// Survol et appui du bouton d'agrandissement.
pub fn state() -> MaxButton {
    MaxButton { hovered: HOVERED.load(Ordering::SeqCst), pressed: PRESSED.load(Ordering::SeqCst) }
}

/// winit a donné à la fenêtre sans décoration la taille de sa zone cliente ;
/// les bords invisibles de Windows, rendus ici, la réduiraient d'autant à
/// chaque lancement. La fenêtre (ou sa place d'avant l'agrandissement) est
/// donc élargie de leur épaisseur, une fois.
fn keep_client_size(window: HWND) {
    let mut probe = RECT { left: 0, top: 0, right: 1000, bottom: 1000 };
    // SAFETY: `window` est vivante ; avec `wParam` nul, `lParam` désigne un
    // RECT valide (sur la pile, vivant pendant l'appel) que Windows remplace
    // par la zone cliente qu'aurait une fenêtre de ce rectangle.
    unsafe {
        DefWindowProcW(window, WM_NCCALCSIZE, WPARAM(0), LPARAM(&mut probe as *mut RECT as isize));
    }
    let (dx, dy) = (probe.left + (1000 - probe.right), 1000 - probe.bottom);
    if !(0..200).contains(&dx) || !(0..200).contains(&dy) {
        return;
    }
    // SAFETY: `window` est vivante ; les structures passées sont sur la pile,
    // de la bonne taille (`length` renseigné pour WINDOWPLACEMENT), et vivent
    // pendant les appels.
    unsafe {
        if IsZoomed(window).as_bool() {
            let mut place = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            if GetWindowPlacement(window, &mut place).is_ok() {
                place.rcNormalPosition.right += dx;
                place.rcNormalPosition.bottom += dy;
                let _ = SetWindowPlacement(window, &place);
            }
            let _ = SetWindowPos(window, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        } else {
            let mut r = RECT::default();
            if GetWindowRect(window, &mut r).is_ok() {
                let _ = SetWindowPos(window, None, 0, 0, r.right - r.left + dx, r.bottom - r.top + dy, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }
}

/// Coordonnées d'écran d'un message de souris hors zone cliente.
fn screen_point(lparam: LPARAM) -> POINT {
    POINT { x: i32::from(lparam.0 as u16 as i16), y: i32::from((lparam.0 >> 16) as u16 as i16) }
}

/// Procédure de fenêtre : la barre de titre, puis celle d'origine (winit).
unsafe extern "system" fn proc(window: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let previous = PREVIOUS.load(Ordering::SeqCst);
    // SAFETY: `previous` est la procédure rendue par SetWindowLongPtrW dans
    // `install` (non nulle : sinon cette procédure n'est pas restée en place) ;
    // l'entier a la taille d'un pointeur de fonction, et `WNDPROC` est une
    // option de pointeur de fonction de cette signature.
    let next = |msg, wparam, lparam| unsafe { CallWindowProcW(std::mem::transmute::<isize, WNDPROC>(previous), window, msg, wparam, lparam) };
    follow_monitor(window, msg);
    if !ACTIVE.load(Ordering::SeqCst) {
        return next(msg, wparam, lparam);
    }
    match msg {
        // Fenêtre agrandie : winit ramène déjà la zone cliente à la zone de travail de l'écran.
        // SAFETY: `window` est la fenêtre qui reçoit ce message.
        WM_NCCALCSIZE if wparam.0 != 0 && !unsafe { IsZoomed(window) }.as_bool() => {
            // SAFETY: avec `wParam` non nul, Windows garantit que `lParam`
            // désigne un NCCALCSIZE_PARAMS valide pendant le message.
            let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
            let top = params.rgrc[0].top;
            // SAFETY: mêmes arguments que ceux reçus, pour la même fenêtre.
            unsafe { DefWindowProcW(window, msg, wparam, lparam) };
            // Bords de gauche, de droite et du bas : ceux de Windows. En haut : rien, la barre est dessinée.
            params.rgrc[0].top = top;
            LRESULT(0)
        }
        WM_NCHITTEST => {
            // SAFETY: mêmes arguments que ceux reçus, pour la même fenêtre.
            let default = unsafe { DefWindowProcW(window, msg, wparam, lparam) };
            if default.0 as u32 != HTCLIENT {
                return default;
            }
            let mut pt = screen_point(lparam);
            let mut client = RECT::default();
            // SAFETY: `window` est vivante ; `pt` et `client` sont sur la pile, vivants pendant les appels.
            let (ok, zoomed, style) = unsafe { (ScreenToClient(window, &mut pt).as_bool() && GetClientRect(window, &mut client).is_ok(), IsZoomed(window).as_bool(), GetWindowLongPtrW(window, GWL_STYLE) as u32) };
            if !ok {
                return default;
            }
            let regions = REGIONS.lock().ok().and_then(|r| r.clone()).unwrap_or_default();
            let sizable = !zoomed && style & WS_THICKFRAME.0 != 0;
            let code = match hit(&regions, pt.x, pt.y, client.right - client.left, sizable) {
                Hit::Client => HTCLIENT,
                Hit::Caption => HTCAPTION,
                Hit::Maximize => HTMAXBUTTON,
                Hit::Top => HTTOP,
                Hit::TopLeft => HTTOPLEFT,
                Hit::TopRight => HTTOPRIGHT,
            };
            LRESULT(code as isize)
        }
        // Bouton d'agrandissement : survol, appui et clic, sans laisser Windows dessiner le sien.
        WM_NCMOUSEMOVE => {
            let over = wparam.0 as u32 == HTMAXBUTTON;
            if HOVERED.swap(over, Ordering::SeqCst) != over {
                wake();
            }
            if !over {
                if PRESSED.swap(false, Ordering::SeqCst) {
                    wake();
                }
                return next(msg, wparam, lparam);
            }
            if !TRACKING.swap(true, Ordering::SeqCst) {
                let mut track = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: window, dwHoverTime: 0 };
                // SAFETY: structure valide sur la pile, de la taille annoncée, pour une fenêtre vivante.
                unsafe {
                    let _ = TrackMouseEvent(&mut track);
                }
            }
            LRESULT(0)
        }
        WM_NCMOUSELEAVE => {
            TRACKING.store(false, Ordering::SeqCst);
            if HOVERED.swap(false, Ordering::SeqCst) | PRESSED.swap(false, Ordering::SeqCst) {
                wake();
            }
            next(msg, wparam, lparam)
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wparam.0 as u32 == HTMAXBUTTON => {
            PRESSED.store(true, Ordering::SeqCst);
            wake();
            LRESULT(0)
        }
        WM_NCLBUTTONUP if wparam.0 as u32 == HTMAXBUTTON => {
            if PRESSED.swap(false, Ordering::SeqCst) {
                wake();
                // SAFETY: `window` est la fenêtre qui reçoit ce message ; la commande système est postée, sans pointeur.
                unsafe {
                    let command = if IsZoomed(window).as_bool() { SC_RESTORE } else { SC_MAXIMIZE };
                    let _ = PostMessageW(Some(window), WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        _ => next(msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regions() -> Regions {
        Regions { height: 40, resize: 6, maximize: [900, 0, 946, 40], holes: vec![[760, 4, 792, 36], [854, 0, 900, 40], [946, 0, 992, 40]] }
    }

    #[test]
    fn hit_test_follows_the_drawn_bar() {
        let r = regions();
        let w = 992;
        // Sous la barre et hors de la fenêtre : zone cliente.
        assert_eq!(hit(&r, 300, 40, w, true), Hit::Client);
        assert_eq!(hit(&r, 300, 500, w, true), Hit::Client);
        assert_eq!(hit(&r, -1, 10, w, true), Hit::Client);
        // Titre : déplacement par Windows.
        assert_eq!(hit(&r, 300, 20, w, true), Hit::Caption);
        // Boutons de l'interface : zone cliente ; agrandissement : bouton de Windows.
        assert_eq!(hit(&r, 770, 20, w, true), Hit::Client);
        assert_eq!(hit(&r, 860, 20, w, true), Hit::Client);
        assert_eq!(hit(&r, 980, 30, w, true), Hit::Client);
        assert_eq!(hit(&r, 920, 20, w, true), Hit::Maximize);
        // Bord du haut et coins, par-dessus les boutons.
        assert_eq!(hit(&r, 300, 2, w, true), Hit::Top);
        assert_eq!(hit(&r, 5, 2, w, true), Hit::TopLeft);
        assert_eq!(hit(&r, 985, 2, w, true), Hit::TopRight);
        assert_eq!(hit(&r, 920, 2, w, true), Hit::Top);
        // Fenêtre agrandie : plus de bord, les boutons vont jusqu'en haut.
        assert_eq!(hit(&r, 300, 2, w, false), Hit::Caption);
        assert_eq!(hit(&r, 920, 0, w, false), Hit::Maximize);
        assert_eq!(hit(&r, 991, 0, w, false), Hit::Client);
        // Plein écran : pas de barre.
        assert_eq!(hit(&Regions::default(), 300, 2, w, false), Hit::Client);
    }

    #[test]
    fn screen_point_is_signed() {
        // Écran à gauche de l'écran principal : coordonnées négatives.
        let l = LPARAM(((0xFFF0u32 << 16) | 0xFF9C) as isize);
        let p = screen_point(l);
        assert_eq!((p.x, p.y), (-100, -16));
    }
}
