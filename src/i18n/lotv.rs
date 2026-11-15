//! Textes du lot V du groupe 2 (vocal et administration) : panneau vocal,
//! pages Widget, Modèle de serveur et Élagage, alertes AutoMod. Module enfant
//! de `table` (il en reprend les macros), tenu à part pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        NOISE_ON = "Suppression du bruit activée", "Noise suppression on";
        NOISE_OFF = "Suppression du bruit coupée", "Noise suppression off";
        SOUNDS_TAB = "Sons", "Sounds";
        SOUNDS_TAB_TIP = "Ouvrir le tableau de sons", "Open the soundboard";
        ACT_WIDGET = "L'enregistrement du widget", "Saving the widget";
        ACT_TEMPLATE = "L'enregistrement du modèle", "Saving the template";
        ACT_TEMPLATE_DELETE = "La suppression du modèle", "Deleting the template";
        ACT_PRUNE = "L'élagage", "Pruning";
        WIDGET_SAVED = "Widget enregistré.", "Widget saved.";
        TEMPLATE_SAVED = "Modèle enregistré.", "Template saved.";
        TEMPLATE_DELETED = "Modèle supprimé.", "Template deleted.";
        PRUNED = "Élagage terminé.", "Prune complete.";
        COPY = "Copier", "Copy";
        COPIED = "Copié.", "Copied.";
        RETRY = "Réessayer", "Retry";
        PAGE_WIDGET = "Widget", "Widget";
        PAGE_TEMPLATE = "Modèle de serveur", "Server Template";
        PAGE_PRUNE = "Élagage", "Prune";
        WIDGET_TIP = "Le widget montre, sur un site, les membres en ligne du serveur et un bouton pour le rejoindre.", "The widget shows the server's online members on a website, with a button to join.";
        WIDGET_ENABLE = "Activer le widget du serveur", "Enable server widget";
        WIDGET_ENABLE_TIP = "Tant qu'il est coupé, l'adresse du widget ne répond pas.", "While it is off, the widget address does not answer.";
        WIDGET_CHANNEL = "Salon d'invitation", "Invite channel";
        WIDGET_CHANNEL_TIP = "Le bouton «\u{a0}Rejoindre\u{a0}» du widget crée une invitation vers ce salon.", "The widget's “Join” button creates an invite to this channel.";
        WIDGET_NO_INVITE = "Aucune invitation", "No invite";
        WIDGET_ID = "Identifiant du serveur", "Server ID";
        WIDGET_JSON = "Adresse JSON", "JSON API";
        WIDGET_JSON_TIP = "Données du widget, pour l'intégrer soi-même.", "Widget data, to build your own embed.";
        TEMPLATE_TIP = "Un modèle copie les salons, les rôles et les réglages du serveur (jamais les messages ni les membres) pour en créer d'autres à son image.", "A template copies the server's channels, roles and settings (never messages or members) so that others can be created from it.";
        TEMPLATE_NAME = "Nom du modèle", "Template name";
        TEMPLATE_NAME_HINT = "À qui s'adresse ce serveur\u{a0}?", "Who is this server for?";
        TEMPLATE_DESCRIPTION = "Description", "Description";
        TEMPLATE_DESCRIPTION_HINT = "Ce que l'on fait dans ce serveur", "What people do in this server";
        TEMPLATE_CREATE = "Créer le modèle", "Generate Template";
        TEMPLATE_LINK = "Lien du modèle", "Template link";
        TEMPLATE_DIRTY = "Le serveur a changé depuis la dernière synchronisation du modèle.", "The server has changed since the template was last synced.";
        TEMPLATE_SYNC = "Synchroniser le modèle", "Sync Template";
        TEMPLATE_DELETE = "Supprimer le modèle", "Delete Template";
        PRUNE_TIP = "L'élagage expulse les membres sans rôle qui ne sont pas venus depuis la durée choisie. Ils pourront revenir avec une invitation.", "Pruning kicks members without a role who have not been seen for the chosen time. They can come back with an invite.";
        PRUNE_DAYS = "Dernière visite il y a plus de", "Last seen more than";
        PRUNE_NONE = "Aucun membre à élaguer.", "No members to prune.";
        PRUNE_REASON = "Raison", "Reason";
        PRUNE_REASON_HINT = "Notée dans le journal d'audit (facultative)", "Written to the audit log (optional)";
        PRUNE = "Élaguer", "Prune";
        ALERT_JUMP = "Voir le message", "View message";
        ALERT_RULES = "Règles AutoMod", "AutoMod rules";
    }
}

/// Modèles.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        template_delete_question(name) = "Supprimer le modèle «\u{a0}{name}\u{a0}»\u{a0}? Son lien ne fonctionnera plus.", "Delete the template “{name}”? Its link will stop working.";
        alert_rule(rule) = "Règle\u{a0}: {rule}", "Rule: {rule}";
        alert_keyword(keyword) = "Mot-clé\u{a0}: {keyword}", "Keyword: {keyword}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        pruned = ["{n} membre élagué.", "{n} membres élagués."], ["{n} member pruned.", "{n} members pruned."];
        template_uses = ["Utilisé {n} fois", "Utilisé {n} fois"], ["Used {n} time", "Used {n} times"];
        days = ["{n} jour", "{n} jours"], ["{n} day", "{n} days"];
        prune_count = ["{n} membre serait expulsé.", "{n} membres seraient expulsés."], ["{n} member would be kicked.", "{n} members would be kicked."];
        prune_question = ["Élaguer {n} membre\u{a0}? Il sera expulsé du serveur.", "Élaguer {n} membres\u{a0}? Ils seront expulsés du serveur."], ["Prune {n} member? They will be kicked from the server.", "Prune {n} members? They will be kicked from the server."];
    }
}
