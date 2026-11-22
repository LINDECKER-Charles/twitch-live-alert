import { dateLabel } from "./lists.js";

const states = {
  connected: ["Connecté", "good", "Twitch a accepté les identifiants. Vos règles peuvent interroger le catalogue et les directs."],
  unconfigured: ["À configurer", "warn", "Connectez votre application Twitch pour surveiller des directs."],
  unverified: ["À vérifier", "warn", "Des identifiants sont enregistrés. Vérifiez leur validité auprès de Twitch."],
  invalid: ["Identifiants refusés", "warn", "Twitch a refusé les identifiants enregistrés. Vérifiez-les ou remplacez-les."],
  unavailable: ["Twitch indisponible", "warn", "La dernière vérification n’a pas abouti. Réessayez lorsque Twitch est disponible."],
};

export function createConnection(id, request, say, refreshStatus) {
  const form = id("credentials-form");
  const feedback = id("credential-feedback");
  let state = null;
  let online = false;
  let editing = false;
  let confirming = false;
  let busy = false;
  let reading = false;
  let ticket = 0;

  function render() {
    const info = states[state?.state];
    const first = state?.state === "unconfigured";
    const fields = Boolean(info && (first || editing));
    const [label, tone, detail] = info || (reading ? ["Chargement…", "", "Lecture de l’état de connexion…"] :
      ["État inconnu", "", "Vérifiez la connexion au service local, puis réessayez."]);
    say(id("credentials-state"), label, tone);
    id("connection-detail").textContent = detail;
    id("connection-date").textContent = state?.verified_at ? "Dernière vérification le " + dateLabel(state.verified_at) : "";
    id("verify-connection").hidden = fields || confirming || (!info && reading);
    id("edit-connection").hidden = !info || fields || confirming;
    id("disconnect-twitch").hidden = !info || fields || confirming;
    id("disconnect-confirm").hidden = !confirming;
    form.hidden = !fields;
    id("cancel-connection").hidden = first;
    id("credentials-fields").disabled = !online || busy;
    id("save-credentials").disabled = !online || busy || !id("client-id").value.trim() || !id("client-secret").value;
    for (const button of id("connection-info").querySelectorAll("button")) button.disabled = !online || busy;
  }

  async function load() {
    if (busy) return;
    const version = ++ticket;
    reading = true; render();
    try {
      const data = await request("connection");
      if (version !== ticket) return;
      state = data; render();
    } catch {
      if (version !== ticket) return;
      say(feedback, "État de connexion indisponible. Réessayez depuis cette page.", "bad");
      render();
    } finally { if (version === ticket) { reading = false; render(); } }
  }

  async function run(path, body) {
    if (busy || !online) return;
    busy = true; ticket++; render();
    say(feedback, path === "disconnect" ? "Déconnexion…" : "Vérification auprès de Twitch…");
    try {
      state = await request("connection/" + path, body);
      confirming = false;
      if ((path === "replace" && state.state === "connected") || path === "disconnect") {
        editing = false; form.reset();
      }
      const ok = path === "disconnect" ? state.state === "unconfigured" : state.state === "connected";
      say(feedback, ok ? (path === "disconnect" ? "Twitch déconnecté." : "Connexion Twitch vérifiée.") :
        (states[state.state]?.[2] || "Vérification indisponible."), ok ? "good" : "bad");
      await refreshStatus();
    } catch (error) { say(feedback, error.message, "bad"); }
    finally { busy = false; render(); }
  }

  form.addEventListener("input", () => { say(feedback, ""); render(); });
  form.onsubmit = event => {
    event.preventDefault();
    void run("replace", { client_id: id("client-id").value.trim(), client_secret: id("client-secret").value });
  };
  id("verify-connection").onclick = () => state ? void run("verify", {}) : void load();
  id("edit-connection").onclick = () => { editing = true; say(feedback, ""); render(); id("client-id").focus(); };
  id("cancel-connection").onclick = () => { editing = false; form.reset(); render(); id("edit-connection").focus(); };
  id("disconnect-twitch").onclick = () => { confirming = true; render(); id("keep-connection").focus(); };
  id("keep-connection").onclick = () => { confirming = false; render(); id("disconnect-twitch").focus(); };
  id("confirm-disconnect").onclick = () => void run("disconnect", {});
  return { load, setOnline(value) { online = value; render(); },
    leave() { editing = confirming = false; form.reset(); say(feedback, ""); render(); } };
}
