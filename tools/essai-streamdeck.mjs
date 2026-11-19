// Essai du plugin : faux Stream Deck (WebSocket écrit à la main) et faux Ferrocord (tube).
import http from "node:http";
import net from "node:net";
import crypto from "node:crypto";
import { spawn } from "node:child_process";

const plugin = process.argv[2];
const PIPE = `\\\\.\\pipe\\ferrocord-essai-${process.pid}`;
const got = []; // messages du plugin vers Stream Deck
const asked = []; // lignes du plugin vers Ferrocord
let sock = null;
let ferro = null;

function frame(text) {
  const b = Buffer.from(text);
  const head = b.length < 126 ? Buffer.from([0x81, b.length]) : Buffer.from([0x81, 126, b.length >> 8, b.length & 255]);
  return Buffer.concat([head, b]);
}
function decode(buf, out) {
  while (buf.length >= 2) {
    let len = buf[1] & 127, off = 2;
    if (len === 126) { if (buf.length < 4) break; len = buf.readUInt16BE(2); off = 4; }
    const masked = buf[1] & 128;
    const need = off + (masked ? 4 : 0) + len;
    if (buf.length < need) break;
    const mask = masked ? buf.subarray(off, off + 4) : null;
    const data = Buffer.from(buf.subarray(off + (masked ? 4 : 0), need));
    if (mask) for (let i = 0; i < data.length; i++) data[i] ^= mask[i % 4];
    if ((buf[0] & 15) === 1) out(data.toString());
    buf = buf.subarray(need);
  }
  return buf;
}
const server = http.createServer();
server.on("upgrade", (req, s) => {
  const key = crypto.createHash("sha1").update(req.headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest("base64");
  s.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${key}\r\n\r\n`);
  sock = s; s.on("error", () => {});
  let buf = Buffer.alloc(0);
  s.on("data", (d) => { buf = decode(Buffer.concat([buf, d]), (t) => got.push(JSON.parse(t))); });
});
const pipe = net.createServer((c) => {
  ferro = c;
  c.setEncoding("utf8");
  c.on("data", (d) => asked.push(...d.split("\n").filter(Boolean)));
  c.on("error", () => {});
});
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(what, f) {
  for (let i = 0; i < 300; i++) { if (f()) return; await wait(20); }
  throw new Error(`attendu : ${what}\nreçu : ${JSON.stringify(got)}\ndemandé : ${JSON.stringify(asked)}`);
}
const ev = (event, action, context) => sock.write(frame(JSON.stringify({ event, action: "io.github.maitoxx.ferrocord." + action, context, device: "d", payload: {} })));
const last = (context) => got.filter((m) => m.event === "setState" && m.context === context).at(-1)?.payload.state;

await new Promise((r) => server.listen(0, "127.0.0.1", r));
const child = spawn(process.execPath, [plugin, "-port", String(server.address().port), "-pluginUUID", "UUID-1", "-registerEvent", "registerPlugin", "-info", "{}"], { env: { ...process.env, FERROCORD_PIPE: PIPE }, stdio: "inherit" });
try {
  await until("inscription", () => got.some((m) => m.event === "registerPlugin" && m.uuid === "UUID-1"));
  // Ferrocord absent : la touche reste éteinte, l'appui montre l'alerte.
  ev("willAppear", "mute", "A");
  ev("willAppear", "leave", "B");
  await until("touche éteinte", () => last("A") === 0);
  ev("keyDown", "mute", "A");
  await until("alerte sans Ferrocord", () => got.some((m) => m.event === "showAlert" && m.context === "A"));
  // Ferrocord arrive (le plugin réessaie toutes les 3 s).
  await new Promise((r) => pipe.listen(PIPE, r));
  await until("branchement au tube", () => ferro);
  ferro.write(JSON.stringify({ v: 1, voice: true, mute: true, deaf: false, camera: false, share: false, ringing: false }) + "\n");
  await until("touche allumée", () => last("A") === 1);
  ev("keyDown", "mute", "A");
  ev("keyDown", "leave", "B");
  await until("demandes", () => asked.length === 2);
  if (asked[0] !== '{"do":"mute"}' || asked[1] !== '{"do":"leave"}') throw new Error("demandes : " + asked);
  ferro.write(JSON.stringify({ v: 1, voice: true, mute: false }) + "\n");
  await until("touche éteinte par l'état", () => last("A") === 0);
  // Appui-pour-parler : enfoncé puis relâché.
  ev("willAppear", "talk", "C");
  ev("keyDown", "talk", "C");
  ev("keyUp", "talk", "C");
  await until("appui-pour-parler", () => asked.length === 4);
  if (asked[2] !== '{"do":"talk","on":true}' || asked[3] !== '{"do":"talk","on":false}') throw new Error("appui : " + asked);
  // Tableau de sons : réglages ouverts, liste demandée puis transmise, son joué.
  sock.write(frame(JSON.stringify({ event: "willAppear", action: "io.github.maitoxx.ferrocord.sound", context: "S", payload: { settings: { id: "1234567890123456789", guild: "42", name: "Clairon" } } })));
  await until("titre du son", () => got.some((m) => m.event === "setTitle" && m.context === "S" && m.payload.title === "Clairon"));
  ev("sendToPlugin", "sound", "S");
  await until("liste demandée", () => asked.includes('{"do":"sounds"}'));
  ferro.write(JSON.stringify({ sounds: [{ id: "7", guild: null, name: "Tada" }] }) + "\n");
  await until("liste transmise aux réglages", () => got.some((m) => m.event === "sendToPropertyInspector" && m.context === "S" && m.payload.sounds.length === 1 && m.payload.sounds[0].name === "Tada"));
  ev("keyDown", "sound", "S");
  await until("son joué", () => asked.includes('{"do":"sound","id":"1234567890123456789","guild":"42"}'));
  if (last("A") !== 0) throw new Error("la liste des sons a changé l'état des touches");
  // Ferrocord fermé : tout s'éteint.
  ferro.write(JSON.stringify({ v: 1, voice: true, mute: true }) + "\n");
  await until("rallumée", () => last("A") === 1);
  ferro.destroy();
  await until("éteinte à la fermeture", () => last("A") === 0);
  console.log("ESSAI RÉUSSI", got.length, "messages");
} catch (e) {
  console.error("ESSAI ÉCHOUÉ\n" + e.message);
  process.exitCode = 1;
} finally {
  child.kill();
  server.close();
  pipe.close();
  setTimeout(() => process.exit(), 100);
}
