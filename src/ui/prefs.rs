//! Réglages persistants vus par l'interface : la copie en mémoire de
//! `settings.toml`. L'interface la modifie librement ; l'enregistrement est
//! confié au fil des réglages, qui le diffère, le regroupe et ne touche
//! jamais à un fichier refusé.
//!
//! Deux instances peuvent écrire le même fichier : quand il est relu
//! (modifié au dehors), un compte ajouté ici et pas encore écrit sur le
//! disque est gardé (sinon son entrée du coffre, écrite ensuite, serait
//! absente de la liste).

use crate::settings::{Account, Settings, SettingsLink, SettingsUpdate, MAX_ACCOUNTS};

pub(super) struct Prefs {
    link: SettingsLink,
    /// Réglages en vigueur (lus, puis modifiés par l'interface).
    pub now: Settings,
    /// Derniers réglages confiés au fil d'écriture (ou lus sur disque).
    saved: Settings,
    /// Derniers réglages connus sur le disque (lus, ou écrits et `sync` fait).
    disk: Settings,
    /// `settings.toml` est refusé : rien n'y est enregistré.
    pub file_error: Option<String>,
    /// La dernière écriture a échoué (rien n'est relu ni confirmé depuis).
    write_failed: bool,
    /// Un enregistrement immédiat est demandé (liste des comptes changée).
    urgent: bool,
    /// Instant de la copie tel qu'il est sur le disque (lu, ou écrit et `sync` fait).
    pub disk_since: Option<u64>,
    /// Le fichier a été relu depuis la dernière fois que l'interface l'a demandé.
    pub reloaded: bool,
    /// Comptes que des relectures ont fait sortir de la liste (pour les
    /// remettre tels quels si leur entrée est encore dans le coffre).
    pub dropped: Vec<Account>,
}

impl Prefs {
    /// `first` : première lecture, faite avant la fenêtre. Renvoie aussi le
    /// message à afficher si le fichier est refusé.
    pub fn new(link: SettingsLink, first: Option<SettingsUpdate>) -> (Self, Option<String>) {
        let mut p = Prefs {
            link,
            now: Settings::default(),
            saved: Settings::default(),
            disk: Settings::default(),
            file_error: None,
            write_failed: false,
            urgent: false,
            disk_since: None,
            reloaded: false,
            dropped: Vec::new(),
        };
        let notice = first.and_then(|u| p.handle(u));
        p.reloaded = false;
        (p, notice)
    }

    pub fn set_wake(&self, f: impl Fn() + Send + Sync + 'static) {
        self.link.set_wake(f);
    }

    /// Nouvelles du fil ; renvoie un message à afficher et `true` si les
    /// réglages ont été remplacés par le fichier (modifié au dehors).
    pub fn poll(&mut self) -> (Option<String>, bool) {
        let mut notice = None;
        let mut reloaded = false;
        for u in self.link.poll() {
            reloaded |= matches!(u, SettingsUpdate::Loaded(_));
            if let Some(m) = self.handle(u) {
                notice = Some(m);
            }
        }
        (notice, reloaded)
    }

    fn handle(&mut self, u: SettingsUpdate) -> Option<String> {
        match u {
            SettingsUpdate::Loaded(s) => {
                // L'instant de la copie dans le coffre ne recule jamais (autre
                // instance, fichier restauré) : le plus récent des deux est gardé.
                let since = self.now.vault_copy_since.max(s.vault_copy_since);
                // Instants d'écriture par compte : le plus récent aussi.
                let mut written = s.written.clone();
                for (k, at) in &self.now.written {
                    match written.iter_mut().find(|(x, _)| x == k) {
                        Some((_, w)) if *w < *at => *w = *at,
                        Some(_) => {}
                        None => written.push((k.clone(), *at)),
                    }
                }
                // Ajoutés ici et jamais vus sur le disque : l'autre instance ne
                // les connaissait pas, elle ne les a pas retirés.
                let ours: Vec<Account> = self.now.accounts.iter().filter(|a| s.account(&a.key).is_none() && self.disk.account(&a.key).is_none()).cloned().collect();
                for a in self.now.accounts.iter().filter(|a| s.account(&a.key).is_none() && !ours.iter().any(|o| o.key == a.key)) {
                    self.dropped.retain(|d| d.key != a.key);
                    self.dropped.push(a.clone());
                }
                let excess = self.dropped.len().saturating_sub(MAX_ACCOUNTS);
                self.dropped.drain(..excess);
                self.disk_since = s.vault_copy_since;
                self.disk = s.clone();
                self.now = s.clone();
                self.saved = s;
                let mut changed = false;
                for a in ours {
                    if self.now.accounts.len() < MAX_ACCOUNTS {
                        self.now.accounts.push(a);
                        changed = true;
                    }
                }
                if self.now.vault_copy_since != since || self.now.written != written {
                    self.now.vault_copy_since = since;
                    self.now.written = written;
                    changed = true;
                }
                self.urgent |= changed;
                self.file_error = None;
                self.write_failed = false;
                self.reloaded = true;
                None
            }
            SettingsUpdate::FileError(e) => {
                self.file_error = Some(e.clone());
                Some(e)
            }
            SettingsUpdate::Error(e) => {
                self.write_failed = true;
                Some(e)
            }
            SettingsUpdate::Saved(s) => {
                self.disk_since = s.vault_copy_since;
                self.disk = s;
                self.write_failed = false;
                None
            }
            SettingsUpdate::Imported(_) | SettingsUpdate::Info(_) => None,
        }
    }

    /// La prochaine écriture part sans attendre.
    pub fn urgent(&mut self) {
        self.urgent = true;
    }

    /// Le compte `key` est dans la liste écrite sur le disque, ou la liste
    /// ne peut pas y être écrite (pas de fichier, fichier refusé, écriture
    /// en échec) : rien ne sert alors d'attendre.
    pub fn listed_on_disk(&self, key: &crate::account::AccountKey) -> bool {
        !self.link.writable() || self.file_error.is_some() || self.write_failed || self.disk.account(key).is_some()
    }

    /// Confie au fil tout changement pas encore enregistré.
    pub fn persist(&mut self) {
        if self.now == self.saved || self.file_error.is_some() {
            return;
        }
        let s = self.now.clone().sanitized();
        if std::mem::take(&mut self.urgent) {
            self.link.save_now(&s);
        } else {
            self.link.save(&s);
        }
        self.now = s.clone();
        self.saved = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountKey;
    use crate::theme::WatchConfig;

    fn prefs(first: Settings) -> Prefs {
        let link = SettingsLink::open(None, false, WatchConfig::default()).0;
        Prefs::new(link, Some(SettingsUpdate::Loaded(first))).0
    }

    /// Un compte ajouté ici, pas encore écrit, survit à la relecture d'un
    /// fichier réécrit par une autre instance (qui ne le connaissait pas) ;
    /// un compte que le disque avait et que l'autre instance a retiré part.
    #[test]
    fn reload_keeps_accounts_added_here_only() {
        let (x, y, z) = (AccountKey::of_user(1), AccountKey::of_user(2), AccountKey::of_user(3));
        let mut start = Settings::default();
        start.accounts.push(Account::new(y.clone()));
        let mut p = prefs(start);
        p.now.accounts.push(Account { name: Some("Nouveau".into()), ..Account::new(x.clone()) });
        // L'autre instance : Y retiré, Z ajouté, X inconnu.
        let mut other = Settings::default();
        other.accounts.push(Account::new(z.clone()));
        p.handle(SettingsUpdate::Loaded(other));
        let keys: Vec<&AccountKey> = p.now.accounts.iter().map(|a| &a.key).collect();
        assert_eq!(keys, vec![&z, &x], "X gardé, Y retiré ailleurs, Z ajouté ailleurs");
        assert_eq!(p.now.account(&x).and_then(|a| a.name.as_deref()), Some("Nouveau"));
        assert!(p.urgent && p.reloaded);
        assert!(p.dropped.iter().any(|a| a.key == y), "Y retenu pour une recherche dans le coffre");
        // Une fois sur le disque, X suit le fichier comme les autres.
        let mut saved = p.now.clone();
        p.handle(SettingsUpdate::Saved(saved.clone()));
        saved.accounts.retain(|a| a.key != x);
        p.handle(SettingsUpdate::Loaded(saved));
        assert!(p.now.account(&x).is_none());
    }
}
