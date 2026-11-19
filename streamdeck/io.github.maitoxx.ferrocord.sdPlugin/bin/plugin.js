// Plugin Stream Deck de Ferrocord : micro, casque, caméra, partage d'écran,
// appui-pour-parler, tableau de sons, raccrocher, répondre et refuser un appel.
//
// Aucune dépendance : le WebSocket de Stream Deck par celui de Node 24, et
// Ferrocord par son tube nommé local (`\\.\pipe\ferrocord-commande`, ouvert
// quand le plugin « Stream Deck » est activé dans Ferrocord). Une ligne JSON
// par message, dans les deux sens :
//   vers Ferrocord : {"do":"mute"}, {"do":"talk","on":true} (touche tenue)
//   de Ferrocord   : {"v":1,"voice":true,"mute":false,"deaf":false,"camera":false,"share":false,"ringing":false,"talk":false}
// Tableau de sons : {"do":"sounds"} demande la liste ({"sounds":[{"id","guild","name"}]}),
// {"do":"sound","id":"…","guild":"…"} joue le son choisi dans les réglages de la touche.
// Rien ne sort de cet ordinateur.
import net from "node:net";

const PIPE = process.env.FERROCORD_PIPE || "\\\\.\\pipe\\ferrocord-commande";
const PREFIX = "io.github.maitoxx.ferrocord.";
const RETRY_MS = 3000;
const MAX_LINE = 1 << 20;

// Action (fin de son UUID) → champ de l'état qui allume la touche (état 1).
const LIT = { mute: "mute", deafen: "deaf", camera: "camera", share: "share", talk: "talk", leave: null, answer: null, decline: null };

const args = process.argv.slice(2);
const arg = (name) => {
  const i = args.indexOf(name);
  return i < 0 ? undefined : args[i + 1];
};

/** Touches affichées : contexte → action. */
const keys = new Map();
/** Touches « Son » : contexte → réglages ({ id, guild, name }). */
const soundKeys = new Map();
/** Dernière liste de sons reçue. */
let sounds = [];
/** Dernier état reçu ; `null` tant que Ferrocord n'est pas joignable. */
let state = null;
let pipe = null;

const deck = new WebSocket(`ws://127.0.0.1:${arg("-port")}`);
const send = (event, context, payload) => {
  if (deck.readyState === WebSocket.OPEN) deck.send(JSON.stringify({ event, context, payload }));
};

/** Dit aux réglages ouverts la liste des sons. */
function tellInspectors() {
  soundKeys.forEach((_, context) => send("sendToPropertyInspector", context, { sounds, connected: !!(pipe && state), voice: !!(state && state.voice) }));
}
const askSounds = () => {
  if (pipe) pipe.write('{"do":"sounds"}\n');
};
function soundEvent(e) {
  const context = e.context;
  switch (e.event) {
    case "willAppear":
    case "didReceiveSettings": {
      const s = (e.payload && e.payload.settings) || {};
      soundKeys.set(context, s);
      send("setTitle", context, { title: s.name || "" });
      break;
    }
    case "willDisappear":
      soundKeys.delete(context);
      break;
    case "sendToPlugin":
    case "propertyInspectorDidAppear":
      tellInspectors();
      askSounds();
      break;
    case "keyDown": {
      const s = soundKeys.get(context) || {};
      // Jouable seulement dans un salon vocal, micro ouvert (règle de Discord).
      if (pipe && state && state.voice && !state.mute && s.id) pipe.write(JSON.stringify({ do: "sound", id: s.id, guild: s.guild || null }) + "\n");
      else send("showAlert", context);
      break;
    }
  }
}

function paint(context) {
  const field = LIT[keys.get(context)];
  send("setState", context, { state: field && state && state[field] ? 1 : 0 });
}
const paintAll = () => keys.forEach((_, context) => paint(context));

deck.addEventListener("open", () => {
  deck.send(JSON.stringify({ event: arg("-registerEvent"), uuid: arg("-pluginUUID") }));
  connect();
});
deck.addEventListener("close", () => process.exit(0));
deck.addEventListener("error", () => process.exit(1));
deck.addEventListener("message", (m) => {
  let e;
  try {
    e = JSON.parse(m.data);
  } catch {
    return;
  }
  const name = typeof e.action === "string" && e.action.startsWith(PREFIX) ? e.action.slice(PREFIX.length) : null;
  if (name === "sound") return soundEvent(e);
  if (!name || !(name in LIT)) return;
  switch (e.event) {
    case "willAppear":
      keys.set(e.context, name);
      paint(e.context);
      break;
    case "willDisappear":
      keys.delete(e.context);
      // Page changée touche tenue : le micro ne reste pas ouvert.
      if (name === "talk" && pipe) pipe.write(JSON.stringify({ do: name, on: false }) + "\n");
      break;
    case "keyDown":
      // Ferrocord fermé, ou son plugin « Stream Deck » coupé : triangle d'alerte.
      if (pipe && state) pipe.write(JSON.stringify(name === "talk" ? { do: name, on: true } : { do: name }) + "\n");
      else send("showAlert", e.context);
      break;
    case "keyUp":
      if (name === "talk" && pipe) pipe.write(JSON.stringify({ do: name, on: false }) + "\n");
      // L'état vient de Ferrocord, jamais de l'appui.
      paint(e.context);
      break;
  }
});

function connect() {
  const p = net.connect(PIPE);
  let buf = "";
  let over = false;
  const lost = () => {
    if (over) return;
    over = true;
    p.destroy();
    if (pipe === p) pipe = null;
    state = null;
    paintAll();
    setTimeout(connect, RETRY_MS);
  };
  p.setEncoding("utf8");
  p.on("connect", () => {
    pipe = p;
  });
  p.on("data", (chunk) => {
    buf += chunk;
    let nl;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl);
      buf = buf.slice(nl + 1);
      try {
        const s = JSON.parse(line);
        if (s && Array.isArray(s.sounds)) {
          sounds = s.sounds;
          tellInspectors();
        } else if (s && typeof s === "object") {
          state = s;
          paintAll();
        }
      } catch {
        // Ligne illisible : ignorée.
      }
    }
    if (buf.length > MAX_LINE) lost();
  });
  p.on("error", lost);
  p.on("close", lost);
}
