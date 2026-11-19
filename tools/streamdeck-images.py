"""Fabrique le manifeste et les images du plugin Stream Deck de Ferrocord."""
import io, json, os, sys
root = sys.argv[1]
ID = "io.github.maitoxx.ferrocord"
os.makedirs(f"{root}/imgs/keys", exist_ok=True)
os.makedirs(f"{root}/imgs/actions", exist_ok=True)
os.makedirs(f"{root}/imgs/plugin", exist_ok=True)

# Tracés sur une grille de 144 ; `C` est remplacé par la couleur du trait.
MIC = '<rect x="58" y="28" width="28" height="56" rx="14" fill="C"/><path d="M44 70a28 28 0 0 0 56 0" fill="none" stroke="C" stroke-width="8" stroke-linecap="round"/><path d="M72 98v16M56 114h32" stroke="C" stroke-width="8" stroke-linecap="round"/>'
HEAD = '<path d="M36 92V76a36 36 0 0 1 72 0v16" fill="none" stroke="C" stroke-width="8" stroke-linecap="round"/><rect x="28" y="78" width="22" height="36" rx="8" fill="C"/><rect x="94" y="78" width="22" height="36" rx="8" fill="C"/>'
CAM = '<rect x="26" y="46" width="64" height="52" rx="10" fill="C"/><path d="M96 66l22-14v40l-22-14z" fill="C" stroke="C" stroke-width="4" stroke-linejoin="round"/>'
SCREEN = '<rect x="26" y="34" width="92" height="62" rx="8" fill="none" stroke="C" stroke-width="8"/><path d="M72 96v16M52 112h40" stroke="C" stroke-width="8" stroke-linecap="round"/>'
ARROW = '<path d="M72 82V52M60 62l12-12 12 12" fill="none" stroke="C" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/>'
WAVES = '<path d="M108 52a30 30 0 0 1 0 36M36 52a30 30 0 0 0 0 36" fill="none" stroke="C" stroke-width="7" stroke-linecap="round"/>'
SPEAKER = '<g transform="translate(0 -14)"><path d="M34 60h18l24-20v64l-24-20H34z" fill="C" stroke="C" stroke-width="4" stroke-linejoin="round"/><path d="M90 56a22 22 0 0 1 0 32M102 44a38 38 0 0 1 0 56" fill="none" stroke="C" stroke-width="7" stroke-linecap="round"/></g>'
PHONE = '<path d="M26 86c28-28 64-28 92 0l-10 16-20-8V80c-10-4-22-4-32 0v14l-20 8z" fill="C" stroke="C" stroke-width="4" stroke-linejoin="round"/>'
SLASH = '<path d="M34 34l76 76" stroke="BG" stroke-width="18" stroke-linecap="round"/><path d="M34 34l76 76" stroke="C" stroke-width="8" stroke-linecap="round"/>'
DARK, RED, GREEN, WHITE = "#2b2d31", "#da373c", "#248046", "#ffffff"

def svg(body, size=144, bg=None):
    back = f'<rect width="144" height="144" rx="20" fill="{bg}"/>' if bg else ""
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 144 144">{back}{body}</svg>\n'

def ink(glyph, color, bg=DARK):
    return glyph.replace("BG", bg).replace("C", color)

def up(glyph):
    return f'<g transform="rotate(-135 72 72)">{glyph}</g>'

# Action : (nom, infobulle, tracé de l'icône de liste, [images des états]).
ACTIONS = [
    ("mute", "Micro", "Coupe ou rétablit le micro. La touche montre son état.", MIC,
        [ink(MIC, WHITE), ink(MIC, RED) + ink(SLASH, RED)]),
    ("deafen", "Casque", "Coupe ou rétablit le son du casque (et le micro avec lui).", HEAD,
        [ink(HEAD, WHITE), ink(HEAD, RED) + ink(SLASH, RED)]),
    ("leave", "Raccrocher", "Quitte le salon vocal ou l'appel en cours.", PHONE,
        [(ink(PHONE, WHITE, RED), RED)]),
    ("camera", "Caméra", "Allume ou éteint la caméra pendant un appel.", CAM,
        [ink(CAM, WHITE) + ink(SLASH, WHITE), (ink(CAM, WHITE, GREEN), GREEN)]),
    ("share", "Partage d'écran", "Arrête le partage d'écran, ou ouvre le choix de ce qui sera partagé.", SCREEN,
        [ink(SCREEN + ARROW, WHITE), (ink(SCREEN + ARROW, WHITE, GREEN), GREEN)]),
    ("talk", "Appui-pour-parler", "À maintenir pour parler : vaut la touche d'appui-pour-parler, ou ouvre un micro coupé le temps de l'appui.", MIC + WAVES,
        [ink(MIC + WAVES, "#949ba4"), (ink(MIC + WAVES, WHITE, GREEN), GREEN)]),
    ("sound", "Son", "Joue un son du tableau de sons dans le salon vocal. Le son se choisit dans les réglages de la touche.", SPEAKER,
        [ink(SPEAKER, WHITE)]),
    ("volume-out", "Volume du casque", "Molette : tourner règle le volume de sortie, appuyer coupe ou rétablit le casque.", HEAD,
        [ink(HEAD, WHITE)]),
    ("volume-in", "Volume du micro", "Molette : tourner règle le volume d'entrée, appuyer coupe ou rétablit le micro.", MIC,
        [ink(MIC, WHITE)]),
    ("answer", "Répondre", "Répond à l'appel qui sonne.", up(PHONE),
        [(up(ink(PHONE, WHITE, GREEN)), GREEN)]),
    ("decline", "Refuser", "Refuse l'appel qui sonne.", PHONE,
        [ink(PHONE, RED) + ink(SLASH, RED)]),
]

def write(path, text):
    io.open(path, "w", encoding="utf-8", newline="\n").write(text)

actions = []
for key, name, tip, glyph, states in ACTIONS:
    write(f"{root}/imgs/actions/{key}.svg", svg(ink(glyph, WHITE), 40))
    st = []
    for i, s in enumerate(states):
        body, bg = s if isinstance(s, tuple) else (s, DARK)
        file = key if i == 0 else f"{key}-on"
        write(f"{root}/imgs/keys/{file}.svg", svg(body, 144, bg))
        st.append({"Image": f"imgs/keys/{file}", "TitleAlignment": "bottom", "FontSize": 9} if key == "sound" else {"Image": f"imgs/keys/{file}", "ShowTitle": False})
    extra = {"PropertyInspectorPath": "ui/sound.html", "UserTitleEnabled": False} if key == "sound" else {"DisableAutomaticStates": True}
    dial = key.startswith("volume-")
    if dial:
        extra = {"SupportedInMultiActions": False, "UserTitleEnabled": False, "Encoder": {"layout": "$B1", "Icon": f"imgs/actions/{key}", "TriggerDescription": {"Rotate": "Volume", "Push": "Couper / rétablir", "Touch": "Couper / rétablir"}}}
    actions.append({**extra,
        "Name": name,
        "UUID": f"{ID}.{key}",
        "Icon": f"imgs/actions/{key}",
        "Tooltip": tip,
        "Controllers": ["Encoder"] if dial else ["Keypad"],
        "States": st,
    })

LOGO = '<path d="M72 18l46 20v34c0 28-19 46-46 54-27-8-46-26-46-54V38z" fill="none" stroke="C" stroke-width="9" stroke-linejoin="round"/><path d="M58 56h30M58 56v40M58 76h22" fill="none" stroke="C" stroke-width="9" stroke-linecap="round" stroke-linejoin="round"/>'
write(f"{root}/imgs/plugin/category.svg", svg(ink(LOGO, WHITE), 56))

manifest = {
    "$schema": "https://schemas.elgato.com/streamdeck/plugins/manifest.json",
    "Name": "Ferrocord",
    "Version": sys.argv[2],
    "Author": "Ferrocord",
    "Description": "Pilote Ferrocord depuis le Stream Deck : micro, casque, appui-pour-parler, tableau de sons, volume du casque et du micro à la molette, caméra, partage d'écran, raccrocher, répondre ou refuser un appel. Les touches montrent l'état réel. Demande le plugin « Stream Deck » activé dans Ferrocord.",
    "UUID": ID,
    "Category": "Ferrocord",
    "CategoryIcon": "imgs/plugin/category",
    "Icon": "imgs/plugin/marketplace",
    "URL": "https://github.com/MaiToxx/ferrocord",
    "CodePath": "bin/plugin.js",
    "SDKVersion": 3,
    "Software": {"MinimumVersion": "7.1"},
    "OS": [{"Platform": "windows", "MinimumVersion": "10"}],
    "Nodejs": {"Version": "24"},
    "Actions": actions,
}
write(f"{root}/manifest.json", json.dumps(manifest, ensure_ascii=False, indent="\t") + "\n")
write(f"{root}/bin/package.json", '{ "type": "module" }\n')
print("ok", len(actions))
