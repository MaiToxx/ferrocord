//! Comptes : ouverture et fermeture des sessions, changement de compte,
//! sélecteur de compte (au-dessus du pied utilisateur), et suivi des
//! réglages propres à chaque compte (dernier serveur et dernier salon).
//!
//! Le coffre n'est jamais appelé ici : on envoie des demandes au fil des
//! comptes (`AccountLink`) et on applique ses réponses à l'image suivante.
//! Entre les deux, l'écran de connexion dit ce qui est en cours.
//!
//! Règles :
//! - un jeton saisi n'est enregistré qu'après READY (identité connue) ;
//!   refusé avant, il n'a rien laissé derrière lui ; session fermée avant la
//!   fin de l'enregistrement : l'annulation part tout de suite (elle est
//!   faite même si l'application se ferme avant la réponse) ;
//! - à l'ajout, le coffre n'est écrit qu'une fois la liste (`settings.toml`)
//!   relue sur le disque avec ce compte ; au retrait, le coffre est vidé
//!   avant la liste : une entrée du coffre n'est jamais absente de la liste
//!   (sauf panne entre les deux, voir README). Une entrée qui vient d'être
//!   écrite remet son compte dans la liste ; une liste réécrite par une autre
//!   instance garde les comptes ajoutés ici, et les comptes qu'elle fait
//!   disparaître sont cherchés dans le coffre (remis s'ils y sont) ;
//! - un remplacement confirmé (« Oui ») va au bout même si la session se
//!   ferme pendant l'écriture ; ce qui ne peut pas être fait est dit ;
//! - le jeton de `DISCORD_TOKEN` n'est jamais enregistré ni oublié ;
//! - un refus de Discord n'efface jamais rien (ni coffre, ni fichier) :
//!   l'utilisateur retire le compte s'il le veut ;
//! - le fichier de jeton en clair n'est supprimé que par l'étape 2 de la
//!   migration (après READY, coffre relu) ou par « Supprimer le fichier de
//!   jeton en clair », confirmé ; retirer un compte ou se déconnecter ne
//!   retire que l'entrée du coffre ;
//! - changer de compte ferme la session (connexion, magasin, saisie) avant
//!   d'ouvrir l'autre : rien d'un compte n'est visible dans un autre.

use super::{
    util::{icon_button, paint_avatar, paint_initials, pixel_square},
    App,
};
use crate::account::{token::wipe, AccountKey, Act, CopyLeft, Expect, Finish, Migration, Reply, Started, Survey, Token, TokenError, WipeCheck, WipeFailure};
use crate::i18n::{
    verbatim::{f as vf, t as vt},
    Lang,
};
use crate::settings::{clean_name, Account, Place, Window, MAX_ACCOUNTS};
use crate::state::model::Avatar;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Align2, RichText, Sense, TextStyle, TextWrapMode, WidgetText};
use std::time::Instant;

/// Identifiant du champ de saisie du jeton (son état est effacé après usage).
pub(super) const TOKEN_FIELD: &str = "ferrocord_champ_jeton";

/// Où en est l'ouverture d'une session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    /// Migration de l'ancien fichier et lecture du dernier compte en cours.
    Starting,
    /// Écran de connexion.
    Login,
    /// Lecture du jeton de ce compte dans le coffre.
    Unlocking(AccountKey),
    /// Session ouverte (connexion en cours tant que READY n'est pas arrivé).
    Session,
}

/// D'où vient le jeton de la session ouverte.
pub(super) enum Origin {
    /// Compte enregistré dans le coffre.
    Stored(AccountKey),
    /// Jeton saisi, pas encore validé ; enregistré après READY si `remember`.
    Typed { token: Token, remember: bool },
    /// `DISCORD_TOKEN` : session éphémère.
    Env,
    /// Jeton du fichier de jeton en clair (migration impossible, ou essai
    /// après le refus du jeton du coffre). `remember` : enregistré après
    /// READY comme un jeton saisi (le fichier, lui, n'est pas touché).
    Legacy { remember: bool },
}

pub(super) struct Current {
    pub origin: Origin,
    /// READY reçu : l'identité du compte est connue.
    pub ready: bool,
    /// Jeton de la session (comparé à celui de la migration avant l'étape 2).
    pub token: Token,
}

/// Jeton validé en cours d'écriture dans le coffre.
pub(super) struct Storing {
    /// Numéro de l'écriture (les réponses sont reconnues par lui seul) ;
    /// `None` : pas encore demandée, la liste qui contient le compte n'est
    /// pas encore écrite sur le disque.
    pub seq: Option<u64>,
    /// Ce que l'entrée doit contenir pour que l'écriture ait lieu.
    pub expect: Expect,
    pub account: Account,
    /// Compte ajouté à la liste pour l'occasion (retiré si l'écriture échoue).
    pub added: bool,
    /// Session fermée entre-temps : l'annulation est déjà demandée (jamais
    /// pour un remplacement confirmé, qui va au bout).
    pub cancelled: bool,
    /// « Recopier le jeton d'avant » (pas une nouvelle session).
    pub recopy: bool,
    /// Jeton écrit (repris si l'entrée contient un autre jeton : question).
    pub token: Token,
    /// Remplacement confirmé (« Oui ») : refusé, la question est reposée.
    pub confirmed: bool,
    pub source: Source,
}

/// D'où vient le jeton à enregistrer (le texte de la question le dit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    /// Saisi dans le champ du jeton.
    Typed,
    /// Fichier de jeton en clair (« Essayer le jeton du fichier »).
    File,
    /// « Recopier le jeton d'avant ».
    Recopy,
}

/// Jeton saisi et validé d'un compte déjà enregistré avec un autre jeton :
/// rien n'est écrit tant que l'utilisateur n'a pas répondu « oui ».
#[derive(Clone)]
pub(super) struct Replacing {
    pub account: Account,
    pub token: Token,
    /// Ce que contient l'entrée (relu par le fil des comptes ; `None` : absente).
    pub existing: Option<Token>,
    pub source: Source,
}


/// Action irréversible en attente de confirmation (deuxième clic).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Confirm {
    /// Retirer ce compte (son jeton est effacé du coffre).
    Remove(AccountKey),
    /// Se déconnecter du compte ouvert (même effet).
    Logout,
    /// Supprimer le fichier de jeton en clair.
    WipeLegacy,
    /// Remplacer le jeton enregistré d'un compte par celui qui vient d'être saisi.
    Replace,
}

/// Pastille d'une ligne de compte.
pub(super) enum RowAvatar {
    User(Avatar),
    /// Identité pas encore connue (compte importé).
    Placeholder,
    None,
}

impl App {
    /// Premier pas après la création : session de `DISCORD_TOKEN`, ou
    /// migration de l'ancien fichier et lecture du dernier compte utilisé.
    pub(super) fn begin(&mut self, env: Option<Result<Token, TokenError>>) {
        match env {
            Some(Ok(t)) => self.env_token = Some(t),
            // Signalé à part : n'empêche pas de rouvrir le compte enregistré.
            Some(Err(e)) => {
                let m = vf::env_ignored(self.lang, e);
                self.notify(m.clone());
                self.env_warning = Some(m);
            }
            None => {}
        }
        let s = &self.prefs.now;
        let probe = s.account(&AccountKey::imported()).is_none();
        let open = match &self.env_token {
            Some(_) => None,
            None => s.last_account.clone().filter(|k| s.account(k).is_some()),
        };
        self.accounts.startup(self.legacy.clone(), open, probe, self.import_room(), s.declined_import);
        self.probe_unlisted();
        if let Some(t) = self.env_token.clone() {
            self.open_session(Origin::Env, t);
        }
    }

    pub(super) fn set_phase(&mut self, p: Phase) {
        if self.phase != p {
            self.phase = p;
            self.phase_since = Instant::now();
        }
    }

    /// Clé du compte enregistré ouvert.
    pub fn current_account(&self) -> Option<&AccountKey> {
        match self.current.as_ref().map(|c| &c.origin) {
            Some(Origin::Stored(k)) => Some(k),
            _ => None,
        }
    }

    /// Ajoute un compte depuis un jeton saisi (bouton Connexion).
    pub fn add_account(&mut self, token: Token, remember: bool) {
        let remember = remember && self.accounts.vault_name().is_some();
        self.open_session(Origin::Typed { token: token.clone(), remember }, token);
    }

    /// Passe à un compte enregistré : la session en cours est fermée, puis le
    /// jeton est lu dans le coffre (hors du fil d'interface).
    pub fn switch_account(&mut self, key: AccountKey) {
        self.menu_open = false;
        if self.current_account() == Some(&key) || self.storing.iter().any(|s| !s.cancelled && s.account.key == key) {
            return;
        }
        self.close_session();
        self.login_error = None;
        self.set_phase(Phase::Unlocking(key.clone()));
        self.accounts.load(key);
    }

    /// La liste peut recevoir le compte importé (sinon le jeton du fichier
    /// n'est pas copié dans le coffre : il y serait invisible).
    fn import_room(&self) -> bool {
        let s = &self.prefs.now;
        s.account(&AccountKey::imported()).is_some() || s.accounts.len() < MAX_ACCOUNTS
    }

    /// Retire un compte (action confirmée sur l'état `expected`, relu par
    /// [`App::ask_remove`]) : son entrée du coffre, puis la ligne de la liste
    /// (une fois l'entrée effacée). Le fil des comptes relit coffre et
    /// fichier, verrou tenu, et ne retire rien s'ils ont changé (la question
    /// est alors reposée). Jamais le fichier de jeton en clair : une étape 2
    /// programmée est d'abord annulée (le fichier reste) ; une étape 2 en
    /// cours fait refuser le retrait, avec l'explication. `false` : rien n'a
    /// été demandé.
    pub fn forget_account(&mut self, key: AccountKey, expected: Option<Survey>) -> bool {
        self.confirm = None;
        if key.is_imported() && self.finishing && !old(Old::RemoveDuringStepTwo) {
            self.notify(vt::STEP_TWO_RUNNING);
            return false;
        }
        if self.current_account() == Some(&key) {
            self.close_session();
            self.set_phase(Phase::Login);
        }
        if key.is_imported() && !old(Old::RemoveDuringStepTwo) {
            // Étape 2 annulée : le fichier reste, comme le dit la confirmation.
            self.finish_pending = None;
        }
        if !self.forgetting.contains(&key) {
            self.forgetting.push(key.clone());
            let keys = self.copy_keys();
            self.accounts.forget(key, self.legacy.clone(), keys, expected);
        }
        true
    }

    /// Entrées du coffre qui ont pu être écrites (instant retenu dans
    /// `[ecritures]`) ou dont le compte vient de sortir de la liste par une
    /// relecture, absentes de la liste : cherchées dans le coffre, et remises
    /// dans la liste si elles y sont (le Gestionnaire n'est pas parcouru).
    fn probe_unlisted(&mut self) {
        let s = &self.prefs.now;
        let mut keys: Vec<AccountKey> = Vec::new();
        for k in s.written.iter().map(|(k, _)| k).chain(self.prefs.dropped.iter().map(|a| &a.key)) {
            if s.account(k).is_none() && !self.forgetting.contains(k) && !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        if !keys.is_empty() {
            self.accounts.probe(keys);
        }
    }

    /// Entrées trouvées dans le coffre alors que leur compte n'est pas listé :
    /// remises dans la liste (avec ce qu'on savait d'elles).
    fn probed(&mut self, found: Vec<AccountKey>) {
        for key in found {
            if self.prefs.now.account(&key).is_some() || self.forgetting.contains(&key) {
                continue;
            }
            let account = self.prefs.dropped.iter().find(|a| a.key == key).cloned().unwrap_or_else(|| Account {
                user_id: (!key.is_imported()).then(|| key.as_str().parse().ok()).flatten(),
                ..Account::new(key.clone())
            });
            if !self.upsert(account) {
                let name = self.account_label(&key);
                self.notify(vf::vault_still_has(self.lang, name, MAX_ACCOUNTS));
            }
        }
    }

    /// Écritures en attente dont le compte est maintenant dans la liste
    /// écrite sur le disque : demandées au fil des comptes.
    fn send_stores(&mut self) {
        for i in 0..self.storing.len() {
            let s = &self.storing[i];
            if s.seq.is_some() || !self.prefs.listed_on_disk(&s.account.key) {
                continue;
            }
            let (key, token, expect) = (s.account.key.clone(), s.token.clone(), s.expect.clone());
            self.storing[i].seq = Some(self.accounts.store(key, token, expect));
        }
    }

    /// Entrées du coffre où une copie d'un jeton peut se trouver.
    fn copy_keys(&self) -> Vec<AccountKey> {
        let mut keys = vec![AccountKey::imported()];
        keys.extend(self.prefs.now.accounts.iter().map(|a| a.key.clone()).filter(|k| !k.is_imported()));
        keys
    }

    /// Premier clic sur « retirer » : coffre et fichier sont relus, pour que
    /// la question dise s'il restera une copie du jeton.
    pub(super) fn ask_remove(&mut self, key: AccountKey) {
        self.confirm = Some(Confirm::Remove(key.clone()));
        self.start_survey(key);
    }

    fn start_survey(&mut self, key: AccountKey) {
        self.remove_survey = None;
        self.surveying = Some(key.clone());
        let keys = self.copy_keys();
        self.accounts.survey(key, self.legacy.clone(), keys);
    }

    /// État relu pour le retrait de `key` (question prête).
    pub(super) fn survey_for(&self, key: &AccountKey) -> Option<&Survey> {
        self.remove_survey.as_ref().filter(|(k, _)| k == key).map(|(_, s)| s)
    }

    /// Question de confirmation du retrait d'un compte, d'après l'état relu
    /// (coffre et fichier) ; `None` tant qu'il n'est pas relu.
    pub(super) fn remove_question(&self, key: &AccountKey) -> Option<String> {
        let name = self.twin_label(key);
        Some(removal_text(self.lang, &vf::remove_ask(self.lang, &name), &name, key, self.survey_for(key)?, self.twin_kept(key)))
    }

    /// Question de confirmation de la déconnexion (même effet qu'un retrait).
    pub(super) fn logout_question(&self) -> Option<String> {
        let key = self.current_account()?;
        let name = self.account_label(key);
        Some(removal_text(self.lang, &vf::logout_ask(self.lang, &name), &name, key, self.survey_for(key)?, self.twin_kept(key)))
    }

    /// Doublon de `key` (même compte, autre ligne) : sa clé et son nom.
    fn twin_kept(&self, key: &AccountKey) -> Option<(AccountKey, String)> {
        let other = self.twin_of(key)?;
        let name = self.twin_label(&other);
        Some((other, name))
    }

    /// Un autre compte de la liste a la même identité (même compte enregistré deux fois).
    pub(super) fn twin_of(&self, key: &AccountKey) -> Option<AccountKey> {
        let list = &self.prefs.now.accounts;
        let id = list.iter().find(|a| &a.key == key)?.user_id?;
        list.iter().find(|a| &a.key != key && a.user_id == Some(id)).map(|a| a.key.clone())
    }

    /// Nom qui distingue deux lignes du même compte (le compte importé est marqué).
    pub(super) fn twin_label(&self, key: &AccountKey) -> String {
        let name = self.account_label(key);
        if key.is_imported() && self.twin_of(key).is_some() { vf::imported_suffix(self.lang, name) } else { name }
    }

    /// Premier doublon de la liste : le compte à proposer de retirer (le
    /// compte importé de préférence, sinon celui qui n'est pas ouvert).
    pub(super) fn twin_to_remove(&self) -> Option<AccountKey> {
        let current = self.current_account();
        self.prefs.now.accounts.iter().find_map(|a| {
            let other = self.twin_of(&a.key)?;
            Some(if other.is_imported() || Some(&a.key) == current { other } else { a.key.clone() })
        })
    }

    /// « Supprimer le fichier de jeton en clair » : le coffre est lu d'abord,
    /// pour que la confirmation dise si ce jeton y a une copie.
    pub(super) fn ask_wipe(&mut self) {
        let Some(path) = self.legacy.clone() else { return };
        self.confirm = Some(Confirm::WipeLegacy);
        self.wipe_check = None;
        self.wipe_survey = None;
        self.checking_wipe = true;
        let keys = self.copy_keys();
        self.accounts.check_wipe(path, keys);
    }

    /// Question de confirmation de la suppression du fichier, selon ce que
    /// fichier et coffre contiennent ; `None` tant qu'ils ne sont pas relus.
    pub(super) fn wipe_question(&self) -> Option<String> {
        if self.checking_wipe || self.wipe_survey.is_none() {
            return None;
        }
        let (lang, ask) = (self.lang, vt::WIPE_QUESTION.get(self.lang));
        Some(match self.wipe_check.as_ref()? {
            WipeCheck::AlsoInVault => vf::wipe_also_in_vault(lang, ask, self.recent_copy_warning()),
            WipeCheck::OnlyCopy => vf::wipe_only_copy(lang, ask),
            WipeCheck::NoToken => vf::wipe_empty(lang, ask),
            WipeCheck::Unreadable(e) => vf::wipe_unreadable(lang, e, ask),
            WipeCheck::Unknown(e) => vf::wipe_vault_unknown(lang, e, ask),
            WipeCheck::Gone => vt::WIPE_GONE.get(lang).into(),
        })
    }

    /// Copie du jeton dans le coffre de moins de 24 h : la question de
    /// suppression le dit (l'action reste possible après confirmation).
    /// Quelle que soit l'entrée qui contient ce jeton ; instant inconnu pour
    /// l'une d'elles : l'avertissement est donné aussi.
    fn recent_copy_warning(&self) -> String {
        let Some(s) = &self.wipe_survey else { return String::new() };
        let (crate::account::FileState::Token(token), Ok(entries)) = (&s.file, &s.vault) else { return String::new() };
        let mut latest = Some(0u64);
        for (key, t) in entries {
            if t.as_ref() != Some(token) {
                continue;
            }
            let mut at = self.prefs.now.written.iter().find(|(k, _)| k == key).map(|(_, at)| *at);
            if key.is_imported() {
                at = at.max(self.prefs.now.vault_copy_since);
            }
            latest = latest.zip(at).map(|(a, b)| a.max(b));
        }
        match latest {
            None => vt::COPY_AGE_UNKNOWN.get(self.lang).into(),
            Some(since) if self.now_secs() < since.saturating_add(STEP_TWO_DELAY) => {
                vf::copy_recent(self.lang, local_date(self.lang, since.saturating_add(STEP_TWO_DELAY)))
            }
            Some(_) => String::new(),
        }
    }

    /// « Importer » : l'utilisateur revient sur son refus d'importer le fichier.
    pub(super) fn import_legacy(&mut self) {
        let Some(path) = self.legacy.clone() else { return };
        self.prefs.now.declined_import = None;
        self.prefs.urgent();
        self.legacy_declined = false;
        self.importing = true;
        self.accounts.import(path, self.import_room());
    }

    /// « Recopier le jeton d'avant » : réécrit la valeur gardée en mémoire.
    pub(super) fn recopy_previous(&mut self) {
        let Some((key, token)) = self.recovery.clone() else { return };
        if self.storing.iter().any(|s| s.recopy) {
            return;
        }
        let account = self.prefs.now.account(&key).cloned().unwrap_or_else(|| Account::new(key.clone()));
        // Seulement si l'entrée est vide ou contient encore ce que l'écriture ratée y a
        // laissé ; sinon rien n'est écrit et la question de remplacement est posée.
        let expect = Expect::Ours(self.recovery_left.clone());
        let seq = self.accounts.store(key, token.clone(), expect.clone());
        self.storing.push(Storing { seq: Some(seq), expect, account, added: false, cancelled: false, recopy: true, token, confirmed: false, source: Source::Recopy });
    }

    /// Le fichier de jeton en clair est (pour autant qu'on sache) encore sur le disque.
    /// Un fichier vidé (vestige sans jeton) n'en fait pas partie.
    pub(super) fn legacy_file_known(&self) -> bool {
        !self.legacy_vestige && (self.finish_pending.is_some() || self.finishing || self.legacy_file.is_some() || self.legacy_token.is_some() || self.file_token.is_some())
    }

    /// Écrase puis supprime le fichier de jeton en clair (seulement sur
    /// confirmation explicite de l'utilisateur, sur l'état relu par
    /// [`App::ask_wipe`] : rien n'est touché s'il a changé).
    fn wipe_legacy(&mut self) {
        self.confirm = None;
        let Some(expected) = self.wipe_survey.take() else { return };
        if let Some(path) = self.legacy.clone() {
            self.wiping_legacy = true;
            let keys = self.copy_keys();
            self.accounts.wipe_legacy(path, keys, expected);
        }
    }

    /// « Se déconnecter » : retire le compte ouvert du coffre et de la liste
    /// (sauf `DISCORD_TOKEN`, qui n'est jamais oublié, un jeton saisi, qui
    /// n'est pas enregistré, et le jeton du fichier, qui reste dans son
    /// fichier). Un compte enregistré : seulement sur l'état relu pour la question.
    pub(super) fn logout_current(&mut self) {
        let expected = self.current_account().and_then(|k| self.survey_for(k)).cloned();
        if self.logout_needs_confirm() && expected.is_none() {
            // Question pas encore prête : elle reste posée.
            self.confirm = Some(Confirm::Logout);
            return;
        }
        let expected = expected.filter(|_| !old(Old::Unchecked));
        self.confirm = None;
        let origin = self.current.take().map(|c| c.origin);
        self.close_session();
        self.set_phase(Phase::Login);
        match origin {
            Some(Origin::Stored(k)) => {
                self.forget_account(k, expected);
            }
            Some(Origin::Legacy { .. } | Origin::Typed { .. } | Origin::Env) | None => {}
        }
    }

    /// La déconnexion efface quelque chose : elle demande confirmation.
    pub(super) fn logout_needs_confirm(&self) -> bool {
        matches!(self.current.as_ref().map(|c| &c.origin), Some(Origin::Stored(_)))
    }

    /// Retour à l'écran de connexion pour ajouter un compte.
    pub(super) fn show_login(&mut self) {
        self.close_session();
        self.login_error = None;
        self.set_phase(Phase::Login);
    }

    /// Abandonne l'étape en cours (ouverture des comptes, lecture d'un jeton,
    /// connexion) : retour à l'écran de connexion. Une réponse tardive du
    /// fil des comptes n'ouvre alors plus rien.
    pub(super) fn cancel_busy(&mut self) {
        if self.phase == Phase::Session {
            self.close_session();
        }
        self.set_phase(Phase::Login);
    }

    pub(super) fn open_env(&mut self) {
        if let Some(t) = self.env_token.clone() {
            self.open_session(Origin::Env, t);
        }
    }

    /// Session avec le jeton du fichier de jeton en clair. Proposé après le
    /// refus du jeton du coffre, il est enregistré après READY (comme un jeton
    /// saisi) ; le fichier n'est pas touché.
    pub(super) fn open_legacy(&mut self) {
        if let Some(token) = self.legacy_token.clone() {
            let remember = self.legacy_newer && self.accounts.vault_name().is_some();
            self.open_session(Origin::Legacy { remember }, token);
        }
    }

    fn open_session(&mut self, origin: Origin, token: Token) {
        self.close_session();
        self.login_error = None;
        self.login_problem = None;
        self.current = Some(Current { origin, ready: false, token: token.clone() });
        self.store.status = crate::state::Status::Connecting;
        self.backend.connect(token);
        self.set_phase(Phase::Session);
    }

    /// Vide le champ du jeton et son état egui (dont l'historique
    /// d'annulation, qui en garde des copies).
    pub(super) fn clear_token_input(&mut self) {
        wipe(&mut self.token_input);
        self.ctx.data_mut(|d| d.remove::<egui::text_edit::TextEditState>(egui::Id::unique(TOKEN_FIELD)));
    }

    /// Ferme la session : connexion arrêtée (ses événements en attente sont
    /// jetés avec elle), magasin et historique vidés, saisie effacée. Le
    /// cache d'images, sans rien de propre au compte, est gardé. Un jeton
    /// encore en cours d'enregistrement ne sera pas retenu : l'annulation
    /// part tout de suite (derrière l'écriture, sur le fil des comptes), et
    /// le compte ajouté pour l'occasion quitte la liste.
    fn close_session(&mut self) {
        self.end_session(true);
    }

    /// Comme [`App::close_session`] ; `cancel_writes` faux : une écriture en
    /// cours va au bout et rien n'est retiré (refus de Discord).
    fn end_session(&mut self, cancel_writes: bool) {
        self.backend.disconnect();
        self.store.clear();
        // Liste virtualisée : hauteurs, positions de défilement et ancrages du compte fermé.
        self.list = Default::default();
        // Brouillons persistants (lot E3) : écrits avant que la saisie soit vidée.
        self.drafts_flush();
        self.input.clear();
        self.clear_token_input();
        self.scroll_to_bottom = false;
        self.menu_open = false;
        self.confirm = None;
        self.current = None;
        // Question de remplacement sans réponse : le coffre garde son jeton.
        self.replacing = None;
        if !cancel_writes {
            return;
        }
        // Un remplacement confirmé (« Oui ») n'est jamais défait ici : il va au bout.
        let mut dropped = Vec::new();
        let accounts = &self.accounts;
        self.storing.retain_mut(|s| {
            if s.cancelled || s.recopy || s.confirmed {
                return true;
            }
            if s.added {
                dropped.push(s.account.key.clone());
            }
            match s.seq {
                Some(seq) => {
                    s.cancelled = true;
                    accounts.undo(seq);
                    true
                }
                // Pas encore demandée : rien à défaire.
                None => false,
            }
        });
        for k in dropped {
            self.drop_from_list(&k);
        }
    }

    /// Discord a fermé la session pour de bon. Rien n'est effacé : ni le
    /// coffre, ni le fichier de jeton en clair ; l'utilisateur décide.
    pub(super) fn session_refused(&mut self, problem: crate::state::Problem, bad_token: bool) {
        let message = super::words::problem(self.lang, &problem, self.h24());
        let (origin, token) = self.current.take().map(|c| (c.origin, c.token)).unzip();
        let name = self.origin_label(origin.as_ref());
        // Jeton enregistré refusé : « Essayer le jeton du fichier » pourra le
        // remplacer sans autre question (seulement s'il est encore celui-là).
        if let (Some(Origin::Stored(k)), Some(t)) = (origin.as_ref(), token) {
            self.refused = Some((k.clone(), t));
        }
        // Une écriture en cours va au bout : un refus n'efface rien, sans exception.
        self.end_session(false);
        self.set_phase(Phase::Login);
        if !bad_token {
            self.login_error = Some(message);
            // Remis en mots si la langue change.
            self.login_problem = Some(problem);
            return;
        }
        match origin {
            Some(Origin::Stored(k)) => {
                // Pas rouvert tout seul au prochain lancement.
                if self.prefs.now.last_account.as_ref() == Some(&k) {
                    self.prefs.now.last_account = None;
                    self.prefs.urgent();
                }
                let mut text = vf::token_refused(self.lang, name, message);
                // Le fichier de jeton en clair a un autre jeton : il est proposé.
                if k.is_imported()
                    && let Some(t) = self.file_token.clone()
                {
                    self.legacy_token = Some(t);
                    self.legacy_newer = true;
                    text.push_str(vt::FILE_HAS_OTHER_TOKEN.get(self.lang));
                }
                self.login_error = Some(text);
            }
            Some(Origin::Legacy { .. }) => {
                self.login_error = Some(vf::file_token_refused(self.lang, message));
            }
            Some(Origin::Env) => {
                self.login_error = Some(vf::env_token_refused(self.lang, message));
            }
            Some(Origin::Typed { .. }) | None => {
                self.login_error = Some(message);
                self.login_problem = Some(problem);
            }
        }
    }

    /// Étape 2 après READY (B1) : l'horloge de la machine peut être en
    /// avance au lancement qui suit la copie, et rien sur la machine ne le
    /// distingue de 24 h réellement écoulées. Seule l'heure de Discord (en-tête
    /// `Date` d'une réponse HTTP) en décide : on attend une réponse dont
    /// l'heure est au moins 24 h après l'instant de la copie.
    pub(super) fn step_two_tick(&mut self) {
        if !self.finish_ready || self.finishing {
            return;
        }
        let session = self.current.as_ref().filter(|c| c.ready).map(|c| c.token.clone());
        if session.is_none() || self.finish_pending != session {
            self.finish_ready = false;
            return;
        }
        if !self.discord_confirms_delay() {
            return;
        }
        if let (Some(path), Some(t)) = (self.legacy.clone(), self.finish_pending.take()) {
            self.finish_ready = false;
            self.finishing = true;
            self.accounts.finish_migration(path, t);
        }
    }

    fn discord_confirms_delay(&self) -> bool {
        if old(Old::NoDelay) || old(Old::StepTwoAfterWrite) {
            return true;
        }
        let (Some(since), Some(ms)) = (self.prefs.now.vault_copy_since, self.store.discord_now_ms()) else { return false };
        u64::try_from(ms / 1000).is_ok_and(|now| now >= since.saturating_add(STEP_TWO_DELAY))
    }

    /// READY : l'identité du compte est connue.
    pub(super) fn on_ready(&mut self) {
        let Some(me) = self.store.me.clone() else { return };
        let Some(cur) = self.current.as_mut() else { return };
        let first = !cur.ready;
        cur.ready = true;
        let identity = Account {
            key: AccountKey::of_user(me.id),
            user_id: Some(me.id),
            name: Some(clean_name(&me.name)).filter(|n| !n.is_empty()),
            avatar: me.avatar.image.and_then(|i| match i {
                crate::state::model::ImageRef::Avatar { hash, .. } => Some(hash),
                _ => None,
            }),
            place: None,
        };
        let session_token = cur.token.clone();
        let todo = match &cur.origin {
            Origin::Stored(k) => Some(Ok(k.clone())),
            Origin::Typed { token, remember: true } if first => Some(Err((token.clone(), false))),
            Origin::Legacy { remember: true } if first => Some(Err((session_token.clone(), true))),
            _ => None,
        };
        match todo {
            Some(Ok(k)) => {
                if !self.upsert(Account { key: k.clone(), ..identity }) {
                    self.notify(vf::not_in_list(self.lang, MAX_ACCOUNTS));
                }
                self.prefs.now.last_account = Some(k.clone());
                // Le même compte enregistré deux fois n'est pas nettoyé tout seul :
                // le sélecteur le signale et propose d'en retirer un (confirmé).
                if old(Old::TwinCleanup) {
                    let twins: Vec<AccountKey> = self.prefs.now.accounts.iter().filter(|a| a.key != k && a.user_id == Some(me.id)).map(|a| a.key.clone()).collect();
                    for t in twins {
                        self.forget_account(t, None);
                    }
                }
                if first {
                    self.restore_place(&k);
                }
                // Étape 2 de la migration : READY obtenu avec le jeton copié du
                // fichier ; elle part quand l'heure de Discord confirme les 24 h.
                if first && k.is_imported() && self.finish_pending.as_ref() == Some(&session_token) {
                    self.finish_ready = true;
                    self.step_two_tick();
                }
            }
            Some(Err((token, from_file))) => self.remember_typed(identity, token, from_file),
            None => {}
        }
    }

    /// Enregistre un jeton saisi et validé : la liste d'abord (écrite tout
    /// de suite), le coffre une fois la liste sur le disque. Liste pleine : rien n'est retenu, et
    /// l'utilisateur le sait.
    fn remember_typed(&mut self, identity: Account, token: Token, from_file: bool) {
        let existing = self.prefs.now.accounts.iter().find(|a| a.user_id == identity.user_id).map(|a| a.key.clone());
        let added = existing.is_none();
        if added && self.prefs.now.accounts.len() >= MAX_ACCOUNTS {
            self.notify(vf::not_kept_add_again(self.lang, MAX_ACCOUNTS));
            return;
        }
        let key = existing.unwrap_or_else(|| identity.key.clone());
        let account = Account { key: key.clone(), ..identity };
        // Liste pleine vérifiée plus haut : l'ajout passe toujours ici.
        if !self.upsert(account.clone()) {
            return;
        }
        self.prefs.persist();
        // Jamais par-dessus un autre jeton, même celui que Discord vient de refuser
        // (« Essayer le jeton du fichier ») : l'entrée doit être vide ; sinon rien
        // n'est écrit et la question de remplacement est posée.
        let expect = match self.refused.clone() {
            _ if old(Old::SilentOverwrite) => Expect::Anything,
            Some((k, t)) if from_file && k == key && old(Old::FileOverwrite) => Expect::Holds(Some(t)),
            _ => Expect::Vacant,
        };
        let source = if from_file { Source::File } else { Source::Typed };
        self.storing.push(Storing { seq: None, expect, account, added, cancelled: false, recopy: false, token, confirmed: false, source });
        self.send_stores();
    }

    /// Ajoute ou met à jour un compte de la liste (son dernier salon est
    /// gardé). Liste pleine : un nouveau compte n'est pas ajouté (`false`).
    fn upsert(&mut self, a: Account) -> bool {
        let list = &mut self.prefs.now.accounts;
        let full = list.len() >= MAX_ACCOUNTS;
        match list.iter_mut().find(|x| x.key == a.key) {
            Some(x) => {
                if (x.user_id, &x.name, x.avatar) != (a.user_id, &a.name, a.avatar) {
                    x.user_id = a.user_id;
                    x.name = a.name;
                    x.avatar = a.avatar;
                    self.prefs.urgent();
                }
            }
            None if full => return false,
            None => {
                list.push(a);
                self.prefs.urgent();
            }
        }
        true
    }

    /// Retire un compte de la liste (son entrée du coffre est déjà partie).
    fn drop_from_list(&mut self, key: &AccountKey) {
        let s = &mut self.prefs.now;
        s.accounts.retain(|a| &a.key != key);
        if s.last_account.as_ref() == Some(key) {
            s.last_account = None;
        }
        self.prefs.urgent();
    }

    /// Rouvre le serveur et le salon où l'on était, s'ils existent encore et
    /// sont visibles (sans demander l'historique d'un autre salon).
    fn restore_place(&mut self, key: &AccountKey) {
        let Some(Place { guild, channel }) = self.prefs.now.account(key).and_then(|a| a.place) else { return };
        let s = &mut self.store;
        match guild {
            Some(g) if s.guilds.iter().any(|x| x.id == g) => {
                let in_guild = |c: u64| s.guilds.iter().any(|x| x.id == g && x.channels.iter().any(|ch| ch.id == c));
                match channel.filter(|c| in_guild(*c) && !s.is_hidden(*c)) {
                    Some(c) => {
                        s.guild = Some(g);
                        s.channel = None;
                        s.select_channel(c);
                    }
                    None => {
                        s.select_guild(g);
                    }
                }
            }
            Some(_) => return,
            None => {
                s.select_dms();
                if let Some(c) = channel.filter(|c| s.dms.iter().any(|d| d.id == *c)) {
                    s.select_channel(c);
                }
            }
        }
        self.scroll_to_bottom = true;
    }

    /// Dernier serveur et salon du compte ouvert, tenus à jour.
    pub(super) fn track_place(&mut self) {
        let Some(Current { origin: Origin::Stored(k), ready: true, .. }) = &self.current else { return };
        let place = Place { guild: self.store.guild, channel: self.store.channel };
        if let Some(a) = self.prefs.now.accounts.iter_mut().find(|a| &a.key == k)
            && a.place != Some(place)
        {
            a.place = Some(place);
        }
    }

    /// Taille, position et état maximisé de la fenêtre, en points logiques
    /// sans le zoom de l'interface (c'est ainsi que la fenêtre est rouverte).
    pub(super) fn track_window(&mut self) {
        // Points d'egui (zoom compris) -> points logiques du système.
        let seen = self.ctx.input(|i| {
            let vp = i.viewport();
            let zoom = i.pixels_per_point / vp.native_pixels_per_point.unwrap_or(i.pixels_per_point);
            let hidden = vp.minimized == Some(true) || vp.fullscreen == Some(true);
            (!hidden).then(|| (vp.maximized.unwrap_or(false), vp.inner_rect.map(|r| r.size() * zoom), vp.outer_rect.map(|r| r.min.to_vec2() * zoom)))
        });
        let Some((maximized, size, position)) = seen else { return };
        let w = &mut self.prefs.now.window;
        let mut next = Window { maximized, ..*w };
        // Maximisée : on garde la taille et la place d'avant, pour la restaurer.
        if !maximized {
            if let Some(s) = size {
                next.size = Some([s.x.round(), s.y.round()]);
            }
            if let Some(p) = position {
                next.position = Some([p.x.round(), p.y.round()]);
            }
        }
        if next != *w {
            *w = next;
        }
    }

    /// Réponses du fil des comptes.
    pub(super) fn poll_accounts(&mut self) {
        // Heure de démarrage retenue à la première image.
        self.process_start();
        // Liste relue (autre instance) : les comptes qu'elle a fait sortir sont cherchés dans le coffre.
        if std::mem::take(&mut self.prefs.reloaded) {
            self.probe_unlisted();
        }
        self.send_stores();
        // Étape 2 en attente de l'heure de Discord (B1).
        self.step_two_tick();
        for r in self.accounts.poll() {
            match r {
                Reply::Started(s) => self.started(s),
                Reply::Probed(found) => self.probed(found),
                Reply::Loaded { key, result } => {
                    if self.phase != Phase::Unlocking(key.clone()) {
                        continue;
                    }
                    let name = self.account_label(&key);
                    match result {
                        Ok(Some(t)) => self.open_session(Origin::Stored(key), t),
                        Ok(None) => {
                            self.set_phase(Phase::Login);
                            self.login_error = Some(vf::token_gone(self.lang, name));
                        }
                        Err(e) => {
                            self.set_phase(Phase::Login);
                            self.login_error = Some(vf::token_read_failed(self.lang, name, e));
                        }
                    }
                }
                Reply::Stored { seq, key, result, put_back, recover, left } => self.stored(seq, key, result, put_back, recover, left),
                Reply::Occupied { seq, key, existing } => self.occupied(seq, key, existing),
                Reply::Restored { seq, key, result } => {
                    let Some(i) = self.storing.iter().position(|s| s.seq == Some(seq)) else { continue };
                    let s = self.storing.remove(i);
                    if let Err(m) = result {
                        let name = self.account_label(&key);
                        // Visible de nouveau, pour pouvoir le retirer à la main.
                        self.upsert(s.account);
                        self.notify(vf::undo_failed(self.lang, name, m));
                    }
                }
                Reply::Forgotten { key, result, unknown } => {
                    self.forgetting.retain(|k| k != &key);
                    match result {
                        Ok(()) => {
                            // Le même compte, ajouté de nouveau entre-temps, s'écrit derrière
                            // ce retrait (même fil) : il reste dans la liste, et en sortira si
                            // cette écriture échoue.
                            let mut rewritten = false;
                            for s in self.storing.iter_mut().filter(|s| !s.cancelled && !s.recopy && s.account.key == key) {
                                s.added = true;
                                rewritten = true;
                            }
                            if rewritten {
                                self.prefs.urgent();
                            } else {
                                self.drop_from_list(&key);
                            }
                            self.prefs.now.written.retain(|(k, _)| k != &key);
                            // Compte importé retiré : l'instant de sa copie ne vaut plus rien
                            // (une copie future repartira de zéro).
                            if key.is_imported() {
                                self.prefs.now.vault_copy_since = None;
                            }
                            self.prefs.urgent();
                            if self.recovery.as_ref().is_some_and(|(k, _)| k == &key) {
                                self.recovery = None;
                            }
                            // Compte importé retiré, fichier encore là : refus retenu
                            // (taille et date du fichier, jamais son contenu) ; plus
                            // d'import tout seul, seulement l'avertissement durable.
                            if key.is_imported()
                                && self.legacy_file_known()
                                && let Some(path) = self.legacy.clone()
                            {
                                self.finish_pending = None;
                                self.legacy_token = None;
                                self.legacy_newer = false;
                                self.file_token = None;
                                self.legacy_declined = true;
                                self.legacy_file = Some(vt::FILE_REMAINS.get(self.lang).into());
                                self.accounts.mark_legacy(path);
                            }
                        }
                        // Délai dépassé : on ne sait pas si l'entrée a été retirée.
                        Err(e) if unknown => {
                            let name = self.account_label(&key);
                            self.notify(vf::remove_timeout(self.lang, name, e));
                        }
                        Err(e) => {
                            let name = self.account_label(&key);
                            self.notify(vf::remove_failed(self.lang, name, e));
                        }
                    }
                }
                Reply::Finished(f) => {
                    self.finishing = false;
                    match f {
                        Finish::Wiped => self.legacy_gone(),
                        Finish::Kept(why) => self.legacy_file = Some(why),
                        Finish::Emptied(why) => self.legacy_emptied(why),
                    }
                }
                Reply::Imported(m) => {
                    self.importing = false;
                    let open = m.token().cloned().filter(|_| m.stored());
                    self.adopt_migration(&m);
                    if self.current.is_none()
                        && matches!(self.phase, Phase::Login)
                        && let Some(t) = open
                    {
                        self.open_session(Origin::Stored(AccountKey::imported()), t);
                    }
                }
                Reply::WipeChecked(c, survey) => {
                    self.checking_wipe = false;
                    self.wipe_check = Some(c);
                    self.wipe_survey = Some(survey);
                }
                Reply::Surveyed { key, survey } => {
                    if self.surveying.as_ref() == Some(&key) {
                        self.surveying = None;
                        self.remove_survey = Some((key, survey));
                    }
                }
                Reply::Stale { act, survey } => self.stale(act, survey),
                Reply::LegacyMarked(mark) => {
                    if mark.is_none() {
                        // Plus de fichier, ou plus de jeton dedans : rien à retenir ni à importer.
                        let vestige = self.legacy_vestige.then(|| self.legacy_file.clone()).flatten();
                        self.legacy_gone();
                        if let Some(why) = vestige {
                            self.legacy_emptied(why);
                        }
                    }
                    self.prefs.now.declined_import = mark;
                    self.prefs.urgent();
                }
                Reply::LegacyWiped(result) => {
                    self.wiping_legacy = false;
                    match result {
                        Ok(()) => self.legacy_gone(),
                        Err(WipeFailure::Untouched(m)) => self.legacy_file = Some(vf::wipe_untouched(self.lang, m)),
                        Err(e @ WipeFailure::Emptied(_)) => self.legacy_emptied(e.to_string()),
                    }
                }
            }
        }
    }

    /// Le fichier de jeton en clair n'existe plus.
    fn legacy_gone(&mut self) {
        if self.prefs.now.vault_copy_since.take().is_some() {
            self.prefs.urgent();
        }
        self.legacy_vestige = false;
        self.wipe_survey = None;
        self.legacy_file = None;
        self.legacy_token = None;
        self.legacy_newer = false;
        self.file_token = None;
        self.finish_pending = None;
        self.legacy_declined = false;
        self.wipe_check = None;
        self.checking_wipe = false;
    }

    /// Le fichier a été vidé (il ne contient plus le jeton) mais n'a pas pu
    /// être supprimé : vestige signalé, jamais présenté comme une copie.
    fn legacy_emptied(&mut self, why: String) {
        self.legacy_gone();
        self.legacy_vestige = true;
        self.legacy_file = Some(why);
    }

    /// Action confirmée non faite : l'état a changé depuis la question, qui
    /// est reposée avec le texte à jour.
    fn stale(&mut self, act: Act, survey: Survey) {
        self.stale_count += 1;
        match act {
            Act::Remove(key) => {
                self.forgetting.retain(|k| k != &key);
                let name = self.twin_label(&key);
                self.confirm = Some(Confirm::Remove(key.clone()));
                self.surveying = None;
                self.remove_survey = Some((key, survey));
                self.notify(vf::not_removed_changed(self.lang, name));
            }
            Act::Wipe => {
                self.wiping_legacy = false;
                self.checking_wipe = false;
                self.confirm = Some(Confirm::WipeLegacy);
                let check = survey.wipe_check();
                if check == WipeCheck::Gone && matches!(survey.file, crate::account::FileState::Absent) {
                    self.legacy_gone();
                    self.confirm = None;
                    self.notify(vt::ALREADY_DELETED);
                    return;
                }
                self.wipe_check = Some(check);
                self.wipe_survey = Some(survey);
                self.notify(vt::STALE_WIPE);
            }
        }
    }

    /// Écriture `seq` non faite : l'entrée du compte contient un autre jeton.
    /// La session continue avec le jeton saisi (en mémoire) ; la question est
    /// posée dans le sélecteur. Sans réponse ou sur « Non », le coffre garde
    /// son jeton.
    fn occupied(&mut self, seq: u64, key: AccountKey, existing: Option<Token>) {
        let Some(i) = self.storing.iter().position(|s| s.seq == Some(seq)) else { return };
        let s = self.storing.remove(i);
        if s.cancelled {
            return;
        }
        if s.added {
            self.drop_from_list(&key);
        }
        // Remplacement confirmé dont la session est fermée depuis : le jeton n'est
        // plus en mémoire, la question ne pourrait rien écrire ; on le dit.
        if s.source != Source::Recopy && self.current.as_ref().is_none_or(|c| c.token != s.token) {
            let name = self.account_label(&key);
            self.notify(if s.confirmed {
                vf::replace_changed_closed(self.lang, name)
            } else {
                vf::already_saved_closed(self.lang, name)
            });
            return;
        }
        let again = self.replacing.is_some() || s.confirmed;
        let source = s.source;
        self.replacing = Some(Replacing { account: s.account, token: s.token, existing, source });
        self.confirm = Some(Confirm::Replace);
        self.menu_open |= self.phase == Phase::Session;
        self.notify(match source {
            _ if again => vt::SAVED_TOKEN_CHANGED,
            Source::Typed => vt::OCCUPIED_TYPED,
            Source::File => vt::OCCUPIED_FILE,
            Source::Recopy => vt::OCCUPIED_RECOPY,
        });
    }

    /// Question de remplacement du jeton enregistré (`None` : pas de question).
    pub(super) fn replace_question(&self) -> Option<String> {
        let r = self.replacing.as_ref()?;
        let name = self.account_label(&r.account.key);
        // Le refus annoncé est celui du jeton que l'entrée contient encore.
        let refused = r.existing.is_some() && self.refused.as_ref().is_some_and(|(k, t)| k == &r.account.key && Some(t) == r.existing.as_ref());
        let lang = self.lang;
        Some(match (r.source, &r.existing) {
            (Source::Typed, Some(_)) => vt::REPLACE_TYPED.get(lang).to_string(),
            (Source::Typed, None) => vf::replace_typed_gone(lang, name),
            (Source::File, Some(_)) if refused => vt::REPLACE_FILE_REFUSED.get(lang).to_string(),
            (Source::File, Some(_)) => vt::REPLACE_FILE.get(lang).to_string(),
            (Source::File, None) => vf::replace_file_gone(lang, name),
            (Source::Recopy, Some(_)) => vf::replace_recopy(lang, name),
            (Source::Recopy, None) => vf::recopy_gone(lang, name),
        })
    }

    /// Réponse à la question de remplacement. « Oui » : écrit seulement si
    /// l'entrée contient encore ce qui a été annoncé (relu verrou tenu ;
    /// sinon la question est reposée). « Non » : le coffre garde son jeton.
    pub(super) fn answer_replace(&mut self, yes: bool) {
        if self.confirm == Some(Confirm::Replace) {
            self.confirm = None;
        }
        let Some(r) = self.replacing.take() else { return };
        if !yes {
            return;
        }
        let recopy = r.source == Source::Recopy;
        if recopy {
            // Valeur d'avant plus en mémoire (recopiée, ou compte retiré) : rien à écrire.
            if self.recovery.as_ref() != Some(&(r.account.key.clone(), r.token.clone())) {
                return;
            }
        } else if self.current.as_ref().is_none_or(|c| c.token != r.token) {
            // Session fermée entre-temps : le jeton n'est plus là, rien n'est écrit.
            return;
        }
        // Compte retiré de la liste en attendant la réponse : il y revient.
        let added = self.prefs.now.account(&r.account.key).is_none();
        if added && !self.upsert(r.account.clone()) {
            self.notify(vf::not_kept_retry(self.lang, MAX_ACCOUNTS));
            return;
        }
        let expect = if old(Old::Unchecked) { Expect::Anything } else { Expect::Holds(r.existing.clone()) };
        if recopy {
            let seq = self.accounts.store(r.account.key.clone(), r.token.clone(), expect.clone());
            self.storing.push(Storing { seq: Some(seq), expect, account: r.account, added, cancelled: false, recopy, token: r.token, confirmed: true, source: r.source });
        } else {
            self.storing.push(Storing { seq: None, expect, account: r.account, added, cancelled: false, recopy, token: r.token, confirmed: true, source: r.source });
            self.send_stores();
        }
    }

    /// Question de remplacement dans le sélecteur.
    pub(super) fn replace_notice(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let Some(q) = self.replace_question() else { return };
        ui.label(RichText::new(vt::DIFFERENT_TOKEN.get(self.lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.danger));
        self.confirm_ui(ui, t, &q, Confirm::Replace);
    }

    /// Retient l'instant d'une écriture réussie dans l'entrée `key` (le
    /// compte importé : c'est aussi une copie neuve, l'étape 2 attend 24 h).
    fn note_written(&mut self, key: &AccountKey, now: u64) {
        let w = &mut self.prefs.now.written;
        match w.iter_mut().find(|(k, _)| k == key) {
            Some((_, at)) => *at = now,
            None => w.push((key.clone(), now)),
        }
        if key.is_imported() {
            self.wrote_copy = true;
            self.prefs.now.vault_copy_since = Some(now);
            // Copie neuve : une étape 2 programmée attend de nouveau 24 h.
            if self.finish_pending.take().is_some() {
                self.legacy_file = Some(waiting_text(self.lang, now.saturating_add(STEP_TWO_DELAY)));
            }
        }
        self.prefs.urgent();
    }

    /// Fin de l'écriture `seq` d'un jeton validé.
    fn stored(&mut self, seq: u64, key: AccountKey, result: Result<(), crate::account::VaultError>, put_back: Option<String>, recover: Option<Token>, left: Option<Token>) {
        let Some(i) = self.storing.iter().position(|s| s.seq == Some(seq)) else { return };
        // Instant de cette écriture, par entrée : la question de suppression du
        // fichier prévient tant qu'il date de moins de 24 h.
        if result.is_ok() {
            let now = self.now_secs();
            self.note_written(&key, now);
        }
        if self.storing[i].recopy {
            self.storing.remove(i);
            let name = self.account_label(&key);
            match result {
                Ok(()) => {
                    self.accounts.settle(seq);
                    self.recovery = None;
                    self.notify(vf::recopied(self.lang, name));
                }
                // La valeur gardée en mémoire reste proposée.
                Err(e) => self.notify(vf::recopy_failed(self.lang, name, e)),
            }
            return;
        }
        if result.is_err()
            && let Some(t) = recover
        {
            self.recovery = Some((key.clone(), t));
            self.recovery_left = left;
        }
        match result {
            // Session fermée pendant l'écriture : l'annulation est déjà en route (réponse `Restored`).
            Ok(()) if self.storing[i].cancelled => {}
            Ok(()) => {
                let s = self.storing.remove(i);
                self.accounts.settle(seq);
                // Une entrée écrite n'est jamais absente de la liste (retirée
                // entre-temps par un retrait ou une liste relue) : elle y revient.
                if self.prefs.now.account(&key).is_none() && !self.upsert(s.account.clone()) {
                    let name = self.account_label(&key);
                    self.notify(vf::in_vault_list_full(self.lang, name, MAX_ACCOUNTS));
                }
                let user = s.account.user_id;
                // La session devient celle d'un compte enregistré.
                if let Some(cur) = self.current.as_mut()
                    && matches!(cur.origin, Origin::Typed { .. } | Origin::Legacy { .. })
                    && self.store.me.as_ref().map(|m| m.id) == user
                {
                    cur.origin = Origin::Stored(key.clone());
                    self.prefs.now.last_account = Some(key.clone());
                    // Compte déjà connu : on retourne où il était, sauf si l'on a déjà bougé.
                    if self.store.guild.is_none() && self.store.channel.is_none() {
                        self.restore_place(&key);
                    }
                }
            }
            Err(e) => {
                let s = self.storing.remove(i);
                match put_back {
                    // Valeur d'avant remise (vérifiée) : proprement annulé.
                    None => {
                        if s.added && !s.cancelled {
                            self.drop_from_list(&key);
                        }
                        let open = self.current.as_ref().is_some_and(|c| c.token == s.token);
                        if !s.cancelled && open {
                            self.notify(vf::save_failed_session(self.lang, e));
                        } else if !s.cancelled {
                            // Remplacement confirmé, session fermée depuis.
                            let name = self.account_label(&key);
                            self.notify(vf::replace_failed(self.lang, name, e));
                        }
                    }
                    // Remise impossible : l'entrée est peut-être abîmée, le compte reste visible.
                    Some(m) => {
                        self.upsert(s.account);
                        self.notify(vf::save_failed_detail(self.lang, e, m));
                    }
                }
            }
        }
    }

    /// Démarrage : résultat de l'étape 1 de la migration, puis le compte à rouvrir.
    /// Applique le résultat de l'étape 1 (au démarrage ou après « Importer »).
    fn adopt_migration(&mut self, migration: &Migration) {
        self.add_imported(migration.stored());
        if let Some(why) = migration.file_left() {
            self.legacy_file = Some(why.to_string());
        }
        match migration {
            Migration::Failed { why, token } => {
                self.notify(vf::migration_failed(self.lang, why));
                self.legacy_token = Some(token.clone());
            }
            Migration::VaultNewer { file_token, .. } => self.file_token = Some(file_token.clone()),
            // Écrit à l'instant : peut-être seulement en mémoire dans le coffre
            // (panne de courant) ; l'étape 2 attend un lancement suivant.
            Migration::Written(t) => {
                // Instant de la copie retenu (heure murale, jamais le jeton) : l'étape 2 attend 24 h.
                let now = self.now_secs();
                // Enregistré tout de suite (sans attendre le regroupement des écritures).
                self.note_written(&AccountKey::imported(), now);
                self.legacy_file = Some(waiting_text(self.lang, now.saturating_add(STEP_TWO_DELAY)));
                if old(Old::StepTwoAfterWrite) {
                    self.finish_pending = Some(t.clone());
                }
            }
            // Déjà là au lancement : étape 2 après READY, mais seulement 24 h au moins
            // après la copie (instant retenu, pas dans le futur), et jamais par le
            // processus qui l'a écrite, ni après le démarrage de ce processus. Instant
            // inconnu, ou à plus de 48 h dans le futur : retenu maintenant.
            Migration::Stored(t) => {
                let now = self.now_secs();
                let start = self.process_start();
                match self.prefs.now.vault_copy_since {
                    _ if old(Old::NoDelay) => {
                        self.finish_pending = Some(t.clone());
                        self.legacy_file = Some(vt::AFTER_READY.get(self.lang).into());
                    }
                    // L'instant doit aussi être celui du disque (écrit et relu par un lancement antérieur).
                    Some(since) if since <= now && now - since >= STEP_TWO_DELAY && since < start && !self.wrote_copy && self.prefs.disk_since == Some(since) => {
                        self.finish_pending = Some(t.clone());
                        self.legacy_file = Some(vt::AFTER_READY.get(self.lang).into());
                    }
                    // Moins de 24 h, ou dans le futur de moins de 48 h (horloge qui
                    // retarde) : gardé, on attend qu'il soit dépassé de 24 h.
                    Some(since) if since <= now.saturating_add(FUTURE_KEPT) => self.legacy_file = Some(waiting_text(self.lang, since.saturating_add(STEP_TWO_DELAY))),
                    _ => {
                        self.prefs.now.vault_copy_since = Some(now);
                        self.prefs.urgent();
                        self.legacy_file = Some(waiting_text(self.lang, now.saturating_add(STEP_TWO_DELAY)));
                    }
                }
            }
            Migration::Declined => {
                self.legacy_declined = true;
                self.legacy_file = Some(vt::DECLINED.get(self.lang).into());
            }
            Migration::Vestige(Some(why)) => self.legacy_emptied(why.clone()),
            Migration::NoFile | Migration::Vestige(None) => {
                if self.prefs.now.vault_copy_since.take().is_some() {
                    self.prefs.urgent();
                }
            }
            Migration::Empty | Migration::Unreadable(_) => {}
        }
    }

    /// Le coffre a une entrée `importe` : la ligne du compte importé est ajoutée à la liste.
    fn add_imported(&mut self, found: bool) {
        let imported = AccountKey::imported();
        if found && self.prefs.now.account(&imported).is_none() {
            if self.prefs.now.accounts.len() < MAX_ACCOUNTS {
                self.prefs.now.accounts.push(Account::new(imported));
                self.prefs.urgent();
            } else {
                self.notify(vf::imported_list_full(self.lang, MAX_ACCOUNTS));
            }
        }
    }

    fn started(&mut self, s: Started) {
        let Started { migration, imported_found, opened } = s;
        let imported = AccountKey::imported();
        self.add_imported(imported_found);
        self.adopt_migration(&migration);
        if self.phase != Phase::Starting {
            return;
        }
        self.set_phase(Phase::Login);
        match opened {
            Some((key, Ok(Some(t)))) => return self.open_session(Origin::Stored(key), t),
            Some((key, Ok(None))) => {
                let name = self.account_label(&key);
                self.login_error = Some(vf::token_gone(self.lang, name));
            }
            Some((_, Err(e))) => {
                self.login_error = Some(vf::accounts_unreachable(self.lang, e));
                // Le jeton du fichier reste proposé pour cette session (le fichier n'est pas touché).
                if self.legacy_token.is_none()
                    && let Some(t) = migration.file_token()
                {
                    self.legacy_token = Some(t.clone());
                    self.legacy_newer = false;
                }
            }
            None => {}
        }
        if self.login_error.is_some() {
            return;
        }
        // Premier lancement après la migration : on rouvre le compte de l'ancien fichier, comme avant.
        match migration {
            Migration::Written(t) | Migration::Stored(t) | Migration::VaultNewer { token: t, .. } => self.open_session(Origin::Stored(imported), t),
            Migration::Failed { .. } => self.open_legacy(),
            _ => {}
        }
    }

    /// Nom affiché d'un compte enregistré.
    pub(super) fn account_label(&self, key: &AccountKey) -> String {
        match self.prefs.now.account(key).and_then(|a| a.name.clone()) {
            Some(n) => n,
            None if key.is_imported() => vt::IMPORTED_ACCOUNT.get(self.lang).into(),
            None => vf::account_number(self.lang, key),
        }
    }

    /// Explication en infobulle d'un compte dont l'identité est inconnue.
    pub(super) fn account_tip(&self, key: &AccountKey) -> Option<&'static str> {
        (key.is_imported() && self.prefs.now.account(key).is_none_or(|a| a.user_id.is_none()))
            .then_some(vt::IMPORTED_TIP.get(self.lang))
    }

    fn origin_label(&self, origin: Option<&Origin>) -> String {
        match origin {
            Some(Origin::Stored(k)) => self.account_label(k),
            Some(Origin::Typed { .. }) | None => vt::NEW_ACCOUNT.get(self.lang).into(),
            Some(Origin::Env) => "DISCORD_TOKEN".into(),
            Some(Origin::Legacy { .. }) => vt::PLAINTEXT_FILE.get(self.lang).into(),
        }
    }

    /// Nom de la session ouverte (écran « connexion en cours »).
    pub(super) fn current_label(&self) -> String {
        self.origin_label(self.current.as_ref().map(|c| &c.origin))
    }

    /// Pastille d'un compte enregistré (identité inconnue : pastille d'attente).
    pub(super) fn account_avatar(&self, key: &AccountKey) -> RowAvatar {
        match self.prefs.now.account(key) {
            Some(Account { user_id: Some(id), avatar, .. }) => RowAvatar::User(Avatar::of_user(*id, *avatar, None)),
            _ => RowAvatar::Placeholder,
        }
    }

    /// Le compte est en cours d'enregistrement ou de retrait (pas cliquable).
    pub(super) fn account_busy(&self, key: &AccountKey) -> Option<&'static str> {
        if self.forgetting.contains(key) {
            Some(vt::REMOVING.get(self.lang))
        } else if key.is_imported() && self.finishing {
            Some(vt::DELETING_PLAIN_FILE.get(self.lang))
        } else if self.storing.iter().any(|s| !s.cancelled && &s.account.key == key) {
            Some(vt::SAVING.get(self.lang))
        } else {
            None
        }
    }

    /// Ligne d'un compte : avatar, nom et, dessous, un détail discret.
    pub(super) fn account_row(&self, ui: &mut egui::Ui, width: f32, avatar: RowAvatar, name: &str, detail: Option<&str>, selected: bool) -> egui::Response {
        let t = &self.theme;
        let c = &t.colors;
        let height = t.account_row_height();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, t.r_row(), c.selected);
        } else if resp.hovered() {
            p.rect_filled(rect, t.r_row(), c.row_hover);
        }
        let side = t.layout.small_avatar;
        let inner = rect.shrink2(egui::vec2(t.spacing.item_x, Default::default()));
        let mut left = inner.left();
        if t.avatars_shown() && !matches!(avatar, RowAvatar::None) {
            let at = Align2::LEFT_CENTER.align_size_within_rect(egui::Vec2::splat(side), inner);
            let slot = pixel_square(ui, at.min, side);
            match &avatar {
                RowAvatar::User(a) => paint_avatar(ui, &self.images, t, slot, a, name),
                RowAvatar::Placeholder => paint_initials(ui, t, slot, t.placeholder_avatar(), name),
                RowAvatar::None => {}
            }
            left = slot.right() + t.spacing.item_x;
        }
        let text_rect = egui::Rect::from_min_max(egui::pos2(left.min(inner.right()), inner.top()), inner.max);
        let strong = if selected { c.text_strong } else { c.text };
        let title = WidgetText::from(RichText::new(name).color(strong)).into_galley(ui, Some(TextWrapMode::Truncate), text_rect.width(), TextStyle::Body);
        let sub = detail.map(|d| WidgetText::from(RichText::new(d).small().color(c.muted)).into_galley(ui, Some(TextWrapMode::Truncate), text_rect.width(), TextStyle::Small));
        // Nom et détail centrés ensemble dans la hauteur de la ligne.
        let block = egui::vec2(text_rect.width(), title.size().y + sub.as_ref().map(|s| s.size().y).unwrap_or_default());
        let top = Align2::LEFT_CENTER.align_size_within_rect(block, text_rect).top();
        ui.painter().galley(egui::pos2(text_rect.left(), top), title.clone(), strong);
        if let Some(s) = sub {
            ui.painter().galley(egui::pos2(text_rect.left(), top + title.size().y), s, c.muted);
        }
        resp
    }

    /// Fichier de jeton en clair encore présent : avertissement durable et
    /// bouton « Supprimer le fichier de jeton en clair », confirmé (avec
    /// l'étape 2 de la migration, le seul moyen de le supprimer). Écran de
    /// connexion et sélecteur.
    pub(super) fn legacy_file_notice(&mut self, ui: &mut egui::Ui) {
        let Some(why) = self.legacy_file.as_deref().map(sentence) else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let heading = if self.legacy_vestige { vt::EMPTIED_FILE_HEADING.get(lang) } else { vt::PLAIN_FILE_HEADING.get(lang) };
        ui.label(RichText::new(heading).small().family(crate::ui::fonts::demi()).strong().color(t.colors.danger));
        ui.label(RichText::new(why).small().color(t.colors.muted));
        if self.wiping_legacy {
            ui.label(RichText::new(vt::DELETING.get(lang)).small().color(t.colors.muted));
        } else if self.finishing {
            ui.label(RichText::new(vt::VERIFY_THEN_DELETE.get(lang)).small().color(t.colors.muted));
        } else if self.confirm == Some(Confirm::WipeLegacy) {
            match self.wipe_question() {
                Some(q) => self.confirm_ui(ui, &t, &q, Confirm::WipeLegacy),
                None => {
                    ui.label(RichText::new(vt::CHECKING_VAULT.get(self.lang)).small().color(t.colors.muted));
                }
            }
        } else if !self.legacy_declined {
            // Seul bouton de la notice : large comme ceux de la colonne.
            if wide_button(ui, vt::WIPE_LABEL.get(lang)).on_hover_text(vt::WIPE_TIP.get(lang)).clicked() {
                self.ask_wipe();
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                if self.legacy_declined && ui.button(vt::IMPORT_LABEL.get(lang)).on_hover_text(vt::IMPORT_TIP.get(lang)).clicked() {
                    self.import_legacy();
                }
                if ui.button(vt::WIPE_LABEL.get(lang)).on_hover_text(vt::WIPE_TIP.get(lang)).clicked() {
                    self.ask_wipe();
                }
            });
        }
    }

    /// Même compte enregistré deux fois : signalé, avec un bouton pour retirer
    /// l'un des deux (confirmé). Rien n'est retiré tout seul.
    pub(super) fn twin_notice(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let Some(key) = self.twin_to_remove() else { return };
        let Some(other) = self.twin_of(&key) else { return };
        let (name, kept) = (self.twin_label(&key), self.twin_label(&other));
        ui.label(RichText::new(vf::twin_heading(self.lang, &name, &kept)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
        if self.account_busy(&key).is_some() {
            return;
        }
        if self.confirm == Some(Confirm::Remove(key.clone())) {
            match self.remove_question(&key) {
                Some(q) => self.confirm_ui(ui, t, &q, Confirm::Remove(key)),
                None => {
                    ui.label(RichText::new(vt::CHECKING_VAULT.get(self.lang)).small().color(t.colors.muted));
                }
            }
        } else if wide_button(ui, &vf::remove_named(self.lang, &name)).on_hover_text(vt::TWIN_TIP.get(self.lang)).clicked() {
            self.ask_remove(key);
        }
    }

    /// Écriture ratée dont la valeur d'avant n'a pas pu être remise : elle
    /// est en mémoire, « Recopier le jeton d'avant » la réécrit.
    pub(super) fn recovery_notice(&mut self, ui: &mut egui::Ui) {
        let Some((key, _)) = &self.recovery else { return };
        let name = self.account_label(key);
        let t = self.theme.clone();
        ui.label(RichText::new(vf::entry_damaged(self.lang, name)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.danger));
        ui.label(RichText::new(vt::PREVIOUS_KEPT.get(self.lang)).small().color(t.colors.muted));
        if self.storing.iter().any(|s| s.recopy) {
            ui.label(RichText::new(vt::WRITING.get(self.lang)).small().color(t.colors.muted));
        } else if wide_button(ui, vt::RECOPY_LABEL.get(self.lang)).clicked() {
            self.recopy_previous();
        }
    }

    /// Nom et détail de la ligne du jeton du fichier de jeton en clair.
    pub(super) fn legacy_wording(&self) -> (&'static str, &'static str) {
        if self.legacy_newer {
            (vt::TRY_FILE_TOKEN.get(self.lang), vt::TRY_FILE_TOKEN_DETAIL.get(self.lang))
        } else {
            (vt::FILE_TOKEN.get(self.lang), vt::FILE_TOKEN_DETAIL.get(self.lang))
        }
    }

    /// Question (sur autant de lignes qu'il faut, dans la largeur donnée)
    /// puis boutons Oui / Non.
    pub(super) fn confirm_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, question: &str, what: Confirm) {
        ui.add(egui::Label::new(RichText::new(question).small().color(t.colors.text)).wrap());
        let replace = what == Confirm::Replace;
        ui.horizontal(|ui| {
            // « Oui » et « Non » écartés comme deux boutons voisins (ils se touchaient).
            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
            if ui.button(RichText::new(vt::YES.get(self.lang)).color(t.colors.danger)).clicked() {
                self.confirmed(what);
            } else if ui.button(vt::NO.get(self.lang)).clicked() {
                if replace {
                    self.answer_replace(false);
                }
                self.confirm = None;
            }
        });
    }

    /// « Oui » au retrait de `k` : seulement sur l'état relu pour la
    /// question (le fil des comptes ne retire rien s'il a changé depuis, et
    /// la question est reposée). `false` : rien n'est demandé.
    pub(super) fn confirm_remove(&mut self, k: AccountKey) -> bool {
        match self.survey_for(&k).cloned() {
            Some(s) => {
                let expected = (!old(Old::Unchecked)).then_some(s);
                self.forget_account(k, expected)
            }
            None => {
                self.confirm = Some(Confirm::Remove(k));
                false
            }
        }
    }

    /// Exécute l'action confirmée.
    pub(super) fn confirmed(&mut self, what: Confirm) {
        self.confirm = None;
        match what {
            // Seulement sur l'état relu pour la question : le fil des comptes
            // ne retire rien s'il a changé depuis (la question est reposée).
            Confirm::Remove(k) => {
                self.confirm_remove(k);
            }
            Confirm::Logout => self.logout_current(),
            // Une session ouverte avec ce jeton continue (il est en mémoire).
            // Seulement après la lecture du coffre : la question disait vrai.
            Confirm::WipeLegacy if self.checking_wipe || self.wipe_survey.is_none() => self.confirm = Some(Confirm::WipeLegacy),
            Confirm::WipeLegacy => self.wipe_legacy(),
            Confirm::Replace => self.answer_replace(true),
        }
    }

    /// Sélecteur de compte, ouvert au-dessus du pied utilisateur (`anchor` :
    /// coin haut gauche du pied ; `width` : sa largeur).
    pub(super) fn account_menu(&mut self, ui: &mut egui::Ui, anchor: egui::Pos2, width: f32, just_opened: bool) {
        if !self.menu_open {
            return;
        }
        let t = self.theme.clone();
        let mut action: Option<MenuAction> = None;
        let area = egui::Area::new(egui::Id::unique("account_menu")).order(egui::Order::Foreground).pivot(Align2::LEFT_BOTTOM).fixed_pos(anchor).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(t.colors.bg2).stroke(t.border()).corner_radius(t.r_card()).inner_margin(t.toast_margin()).show(ui, |ui| {
                // Jamais plus étroit que son bouton le plus long (« Se déconnecter de ce compte ») :
                // il tient sur une ligne, centré comme les autres.
                let need = ui.painter().layout_no_wrap(self.logout_wording().0.to_string(), egui::TextStyle::Button.resolve(ui.style()), egui::Color32::PLACEHOLDER).size().x + 2.0 * ui.spacing().button_padding.x;
                let inner = (width - t.toast_margin().sum().x).max(need.ceil());
                ui.set_width(inner);
                ui.label(RichText::new(vt::ACCOUNTS_HEADING.get(self.lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                let current = self.current_account().cloned();
                let keys: Vec<AccountKey> = self.prefs.now.accounts.iter().map(|a| a.key.clone()).collect();
                for key in keys {
                    let selected = current.as_ref() == Some(&key);
                    let busy = self.account_busy(&key);
                    let twin = (key.is_imported() && self.twin_of(&key).is_some()).then_some(vt::IMPORTED_TWICE.get(self.lang));
                    let detail = busy.or(selected.then_some(vt::ACCOUNT_OPEN.get(self.lang))).or(twin);
                    let name = self.account_label(&key);
                    let mut row = self.account_row(ui, inner, self.account_avatar(&key), &name, detail, selected);
                    if let Some(tip) = self.account_tip(&key) {
                        row = row.on_hover_text(tip);
                    }
                    if row.clicked() && busy.is_none() {
                        action = Some(MenuAction::Switch(key));
                    }
                }
                if self.env_token.is_some() {
                    let selected = matches!(self.current.as_ref().map(|c| &c.origin), Some(Origin::Env));
                    if self.account_row(ui, inner, RowAvatar::None, "DISCORD_TOKEN", Some(vt::ENV_SESSION.get(self.lang)), selected).clicked() {
                        action = Some(MenuAction::Env);
                    }
                }
                if self.legacy_token.is_some() {
                    let selected = matches!(self.current.as_ref().map(|c| &c.origin), Some(Origin::Legacy { .. }));
                    let (name, detail) = self.legacy_wording();
                    if self.account_row(ui, inner, RowAvatar::None, name, Some(detail), selected).clicked() {
                        action = Some(MenuAction::Legacy);
                    }
                }
                self.replace_notice(ui, &t);
                self.twin_notice(ui, &t);
                self.legacy_file_notice(ui);
                self.recovery_notice(ui);
                ui.separator();
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                    if ui.button(vt::ADD_ACCOUNT.get(self.lang)).clicked() {
                        action = Some(MenuAction::Add);
                    }
                    let (label, tip) = self.logout_wording();
                    if self.confirm == Some(Confirm::Logout) {
                        match self.logout_question() {
                            Some(q) => self.confirm_ui(ui, &t, &q, Confirm::Logout),
                            None => {
                                ui.label(RichText::new(tip).small().color(t.colors.muted));
                                ui.label(RichText::new(vt::CHECKING_VAULT.get(self.lang)).small().color(t.colors.muted));
                            }
                        }
                    } else if ui.button(RichText::new(label).color(t.colors.danger)).on_hover_text(tip).clicked() {
                        action = Some(MenuAction::Logout);
                    }
                });
            });
        });
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if escape || (!just_opened && area.response.clicked_elsewhere()) {
            self.menu_open = false;
            self.confirm = None;
        }
        match action {
            Some(MenuAction::Switch(k)) => self.switch_account(k),
            Some(MenuAction::Env) => {
                self.menu_open = false;
                if !matches!(self.current.as_ref().map(|c| &c.origin), Some(Origin::Env)) {
                    self.open_env();
                }
            }
            Some(MenuAction::Legacy) => {
                self.menu_open = false;
                if !matches!(self.current.as_ref().map(|c| &c.origin), Some(Origin::Legacy { .. })) {
                    self.open_legacy();
                }
            }
            Some(MenuAction::Add) => self.show_login(),
            Some(MenuAction::Logout) => self.request_logout(),
            None => {}
        }
    }

    /// Bouton de déconnexion (pied ou sélecteur) : confirmation dans le
    /// sélecteur si quelque chose est effacé, sinon fermeture directe.
    pub(super) fn request_logout(&mut self) {
        if self.logout_needs_confirm() {
            self.menu_open = true;
            self.confirm = Some(Confirm::Logout);
            if let Some(k) = self.current_account().cloned() {
                self.start_survey(k);
            }
        } else {
            self.logout_current();
        }
    }

    /// Libellé et explication de « se déconnecter », selon la session.
    pub(super) fn logout_wording(&self) -> (&'static str, &'static str) {
        let (label, tip) = match self.current.as_ref().map(|c| &c.origin) {
            Some(Origin::Env) => (vt::CLOSE_SESSION, vt::ENV_NEVER_SAVED),
            Some(Origin::Typed { token, .. }) if self.storing.iter().any(|s| s.confirmed && &s.token == token) => {
                (vt::CLOSE_SESSION, vt::CONFIRMED_REPLACE_GOES_ON)
            }
            Some(Origin::Typed { .. }) => (vt::CLOSE_SESSION, vt::TOKEN_NOT_SAVED),
            Some(Origin::Legacy { .. }) => (vt::CLOSE_SESSION, vt::FILE_NOT_TOUCHED),
            Some(Origin::Stored(k)) if k.is_imported() => (
                vt::LOG_OUT_ACCOUNT,
                vt::LOGOUT_IMPORTED_TIP,
            ),
            Some(Origin::Stored(_)) | None => (vt::LOG_OUT_ACCOUNT, vt::LOGOUT_TIP),
        };
        (label.get(self.lang), tip.get(self.lang))
    }
}

enum MenuAction {
    Switch(AccountKey),
    Env,
    Legacy,
    Add,
    Logout,
}


/// Bouton d'une notice, large comme la colonne (« Ajouter un compte » et
/// « Se déconnecter » le sont : un bouton plus étroit au-dessus d'eux jurait).
fn wide_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized([ui.available_width(), ui.spacing().interact_size.y], egui::Button::new(text))
}

/// Raison affichée seule : majuscule en tête, point final.
fn sentence(why: &str) -> String {
    let mut c = why.chars();
    let mut out: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// Bouton « retirer » d'une ligne de compte (écran de connexion).
pub(super) fn remove_button(ui: &mut egui::Ui, t: &Tokens, tip: &str) -> egui::Response {
    icon_button(ui, t, Icon::Close, tip)
}

/// Anciens comportements, remis seulement par les tests (la table d'états
/// doit les prendre en défaut). Hors tests : jamais.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Old {
    /// Étape 2 dès la session qui a écrit l'entrée du coffre.
    StepTwoAfterWrite,
    /// Retrait du compte importé sans annuler ni attendre l'étape 2.
    RemoveDuringStepTwo,
    /// Doublon retiré tout seul après READY.
    TwinCleanup,
    /// Étape 2 dès le lancement suivant, sans attendre 24 h.
    NoDelay,
    /// Retrait confirmé exécuté sans relire coffre et fichier.
    Unchecked,
    /// Jeton saisi écrit par-dessus le jeton enregistré du même compte, sans question.
    SilentOverwrite,
    /// « Essayer le jeton du fichier » écrit par-dessus le jeton refusé, sans question.
    FileOverwrite,
}

#[cfg(test)]
thread_local! {
    pub(super) static OLD: std::cell::Cell<Option<Old>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn old(o: Old) -> bool {
    OLD.with(|c| c.get() == Some(o))
}

#[cfg(not(test))]
fn old(_: Old) -> bool {
    false
}

/// Question de retrait (ou de déconnexion) d'après l'état relu : dit
/// toujours s'il restera une copie du jeton, ou qu'on ne le sait pas.
/// `twin` : l'autre ligne du même compte (doublon), avec son nom : si son
/// entrée (relue) a un autre jeton, le compte reste accessible par elle.
fn removal_text(lang: Lang, ask: &str, name: &str, key: &AccountKey, s: &Survey, twin: Option<(AccountKey, String)>) -> String {
    match s.entry(key) {
        Err(e) => vf::removal_unknown(lang, ask, e),
        Ok(None) => vf::removal_list_only(lang, name),
        Ok(Some(token)) => match s.copy_remains(key) {
            Some(CopyLeft::File) => vf::removal_file_copy(lang, ask),
            Some(CopyLeft::Entry) => vf::removal_entry_copy(lang, ask),
            Some(CopyLeft::Nothing) => match twin.filter(|(k, _)| s.entry(k).is_ok_and(|t| t.is_some_and(|t| t != token))) {
                Some((_, kept)) => vf::removal_twin(lang, ask, kept),
                None => vf::removal_last(lang, ask),
            },
            None => vf::removal_file_unknown(lang, ask),
        },
    }
}

/// Instant de la copie dans le futur gardé jusqu'à 48 h (horloge qui retarde) ;
/// au-delà, il est remis à maintenant.
pub(super) const FUTURE_KEPT: u64 = 48 * 3600;

/// Une journée : délai minimal entre la copie dans le coffre et l'étape 2.
pub(super) const STEP_TWO_DELAY: u64 = 24 * 3600;

/// Date lisible (heure locale) d'un instant en secondes depuis 1970.
fn local_date(lang: Lang, secs: u64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(i64::try_from(secs).unwrap_or(i64::MAX), 0).single() {
        Some(d) => d.format(vt::LOCAL_DATE_FORMAT.get(lang)).to_string(),
        None => vt::IN_24_HOURS.get(lang).into(),
    }
}

/// Avertissement entre les deux étapes : date la plus proche de l'étape 2.
fn waiting_text(lang: Lang, not_before: u64) -> String {
    vf::waiting(lang, local_date(lang, not_before))
}
