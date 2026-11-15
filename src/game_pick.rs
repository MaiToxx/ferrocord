//! Programme ajouté à la main comme jeu (lot C3, page Activités) : boîte
//! « Ouvrir » de Windows dans un fil à part ; l'interface reprend le chemin
//! choisi par [`GamePicker::poll`] quand elle est réveillée. Rien n'est lu
//! dans le fichier : seul son chemin compte.

use crate::attach::Wake;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub struct GamePicker {
    // Hors Windows, pas de boîte « Ouvrir » : rien n'est jamais envoyé.
    #[cfg_attr(not(windows), allow(dead_code))]
    tx: mpsc::Sender<String>,
    rx: mpsc::Receiver<String>,
    #[cfg_attr(not(windows), allow(dead_code))]
    wake: Wake,
    busy: Arc<AtomicBool>,
}

impl GamePicker {
    pub fn new(wake: Wake) -> Self {
        let (tx, rx) = mpsc::channel();
        GamePicker { tx, rx, wake, busy: Arc::default() }
    }

    /// La boîte de dialogue existe sur cette plateforme.
    pub const CAN_PICK: bool = cfg!(windows);

    /// Boîte « Ouvrir » (un programme) ; annulée : rien.
    #[cfg(windows)]
    pub fn pick(&self, title: &str, filter: &str) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let (title, filter) = (title.to_string(), filter.to_string());
        let (tx, wake, busy) = (self.tx.clone(), self.wake.clone(), self.busy.clone());
        let spawned = std::thread::Builder::new().name("programme-jeu".into()).spawn(move || {
            if let Some(path) = rfd::FileDialog::new().set_title(title).add_filter(filter, &["exe"]).pick_file() {
                let _ = tx.send(path.to_string_lossy().into_owned());
            }
            busy.store(false, Ordering::SeqCst);
            wake();
        });
        if spawned.is_err() {
            self.busy.store(false, Ordering::SeqCst);
        }
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn poll(&self) -> Option<String> {
        self.rx.try_iter().last()
    }
}
