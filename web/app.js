import { createCategoryPicker } from "./category-picker.js";
import { createJournal, dateLabel, emptyState, renderRows, text } from "./lists.js";

const id = name => document.getElementById(name);
const labels = { directs: "Directs", journal: "Journal", reglages: "Réglages", connexion: "Connexion Twitch" };
const disconnection = "Le service local ne répond pas. Relancez Twitch Live Alert, puis réessayez.";
const languages = new Intl.DisplayNames(["fr"], { type: "language" });
// ISO 639-1 codes. Twitch also accepts the language "other".
const codes = ("aa ab ae af ak am an ar as av ay az ba be bg bh bi bm bn bo br bs ca ce ch co cr cs cu cv cy da de dv dz ee el en eo es et eu fa ff fi fj fo fr fy ga gd gl gn gu gv ha he hi ho hr ht hu hy hz ia id ie ig ii ik io is it iu ja jv ka kg ki kj kk kl km kn ko kr ks ku kv kw ky la lb lg li ln lo lt lu lv mg mh mi mk ml mn mr ms mt my na nb nd ne ng nl nn no nr nv ny oc oj om or os pa pi pl ps pt qu rm rn ro ru rw sa sc sd se sg si sk sl sm sn so sq sr ss st su sv sw ta te tg th ti tk tl tn to tr ts tt tw ty ug uk ur uz ve vi vo wa wo xh yi yo za zh zu other").split(" ");
let status = null;
let connected = false;
let refreshing = false;
let pending = "";
let revision = 0;
let page = "";
const streamPageSize = 50;
let visibleStreams = streamPageSize;
let requestingStatus = false;
const journal = createJournal(request, id);
const categoryPicker = createCategoryPicker(id, request, settingsEdited);

function langName(code) {
  if (!code) return "Toutes les langues";
  if (code === "other") return "Autre langue";
  try {
    const name = languages.of(code) || code;
    return name.charAt(0).toLocaleUpperCase("fr") + name.slice(1);
  } catch { return code; }
}

function initLanguages() {
  const options = codes.map(code => new Option(langName(code), code))
    .sort((a, b) => a.text.localeCompare(b.text, "fr"));
  id("language").append(...options);
}

function say(element, message, tone = "") {
  element.dataset.tone = tone;
  text(element, message);
}

function currentSettings() {
  return {
    category: status && !status.configured ? status.category : id("category").value.trim(),
    language: id("language").value,
    poll_interval_seconds: id("poll-interval").valueAsNumber,
    notify_existing_on_first_run: id("notify-first").checked,
    start_with_windows: id("start-windows").checked,
  };
}

function changed(values = currentSettings(), stored = status) {
  return Boolean(stored) && Object.entries(values).some(([key, value]) => stored[key] !== value);
}

function fillSettings(data) {
  id("category").value = data.category;
  if (![...id("language").options].some(option => option.value === data.language)) {
    id("language").add(new Option(langName(data.language), data.language));
  }
  id("language").value = data.language;
  id("poll-interval").value = data.poll_interval_seconds;
  id("notify-first").checked = data.notify_existing_on_first_run;
  id("start-windows").checked = data.start_with_windows;
}

function controls() {
  const usable = connected && Boolean(status);
  const busy = Boolean(pending);
  id("check-now").hidden = Boolean(usable && (!status.configured || !status.running));
  id("check-now").disabled = !usable || !status?.configured || !status?.running || busy;
  id("check-now").querySelector("span").textContent = pending === "check" ? "Vérification…" : "Vérifier maintenant";
  const toggle = id("toggle-monitor");
  toggle.hidden = !status?.configured;
  toggle.disabled = !usable || busy;
  toggle.classList.toggle("primary", status?.running === false);
  toggle.querySelector("span").textContent = status?.running === false ? "Reprendre" : "Mettre en pause";
  toggle.querySelector("use").setAttribute("href", status?.running === false ? "#i-play" : "#i-pause");
  id("setup-link").hidden = !usable || status.configured;
  id("test-notification").disabled = !usable || busy;
  id("settings-fields").disabled = !usable || pending === "settings";
  id("save-settings").disabled = !usable || busy || !changed();
  id("credentials-fields").disabled = !usable || pending === "credentials";
  id("save-credentials").disabled = !usable || busy || !id("client-id").value.trim() || !id("client-secret").value;
}

function describeMonitor() {
  if (!connected) return ["Service hors connexion", "bad", "La surveillance ne peut pas être consultée sur ce PC."];
  if (!status.configured) return ["Twitch à configurer", "warn", "Connectez votre application Twitch pour détecter les premiers directs."];
  if (status.last_error) return ["Surveillance dégradée", "warn", status.last_error];
  if (!status.running) return ["Surveillance en pause", "", "Reprenez la surveillance pour vérifier les directs automatiquement."];
  return ["Surveillance active", "good", "Les directs sont vérifiés toutes les " + status.poll_interval_seconds + " secondes."];
}

function renderStreams() {
  const streams = status?.live_streams || [];
  const hasResult = Boolean(status?.last_checked_at) || streams.length > 0;
  id("stream-count").hidden = !hasResult;
  text(id("stream-count"), String(streams.length));
  id("filters").hidden = !status;
  if (status) {
    text(id("category-tag"), status.category);
    text(id("language-tag"), langName(status.language));
  }
  let heading = "", detail = "", symbol = "radio";
  if (!connected) {
    heading = streams.length ? "Dernier relevé disponible" : "Directs indisponibles";
    detail = "La connexion au service local est interrompue.";
  } else if (!status.configured) {
    symbol = "plug"; heading = "En attente de connexion Twitch";
    detail = "Les chaînes détectées apparaîtront ici après la configuration.";
  } else if (status.last_error) {
    heading = streams.length ? "Dernier relevé disponible" : "Vérification indisponible";
    detail = "La dernière vérification Twitch a échoué.";
  } else if (!status.running) {
    heading = streams.length ? "Dernier relevé avant la pause" : "Recherche en pause";
    detail = streams.length ? "" : "Reprenez la surveillance pour rechercher des chaînes.";
  } else if (!hasResult) {
    heading = "En attente du premier relevé";
    detail = "La liste se remplira après la première vérification Twitch.";
  } else if (streams.length === 0) {
    heading = "Aucune chaîne en direct";
    detail = "Aucun direct ne correspond actuellement aux filtres de surveillance.";
  }
  const message = id("stream-message");
  emptyState(message, symbol, heading, detail);
  message.classList.toggle("has-rows", streams.length > 0);
  renderRows(id("stream-list"), streams.slice(0, visibleStreams));
  id("more-streams").hidden = streams.length <= visibleStreams;
  text(id("stream-range"), Math.min(streams.length, visibleStreams) + " chaînes affichées sur " + streams.length);
  const recent = status?.notifications?.slice(0, 3) || [];
  id("recent-message").hidden = recent.length > 0;
  text(id("recent-message"), status ? "Aucune notification enregistrée pour le moment." : "Notifications indisponibles.");
  renderRows(id("recent-list"), recent, true);
}

function render() {
  const [label, tone, detail] = describeMonitor();
  id("monitor-led").dataset.tone = tone;
  text(id("monitor-state"), label);
  text(id("monitor-detail"), detail);
  id("offline").hidden = connected;
  say(id("service-state"), connected ? "Connecté" : "Hors connexion", connected ? "good" : "bad");
  text(id("version"), status?.app_version ? "v" + status.app_version : "");
  text(id("last-check"), dateLabel(status?.last_checked_at, true));
  const next = status?.next_check_at;
  const scheduled = connected && status?.configured && status.running;
  const nextLabel = next ? (next * 1000 <= Date.now() ? "Imminente" : dateLabel(next, true)) : "En attente";
  text(id("next-check"), scheduled ? nextLabel : "—");
  text(id("notifications-state"), status ? (status.notifications_available ? "Disponibles" : "Indisponibles") : "—");
  const configured = status?.configured;
  say(id("credentials-state"), !connected ? "État inconnu" : configured ? "Configurée" : "À configurer",
    !connected ? "" : configured ? "good" : "warn");
  categoryPicker.setContext(connected, Boolean(status?.configured), status?.category);
  renderStreams();
  controls();
}

async function request(path, body, signal) {
  const controller = new AbortController();
  const abort = () => controller.abort();
  if (signal?.aborted) abort(); else signal?.addEventListener("abort", abort, { once: true });
  const timeout = setTimeout(() => controller.abort(), body === undefined ? 14000 : 30000);
  const options = { cache: "no-store", signal: controller.signal, credentials: "same-origin", redirect: "error" };
  if (body !== undefined) {
    options.method = "POST";
    options.headers = { "Content-Type": "application/json", "X-Twitch-Live-Alert": "local" };
    options.body = JSON.stringify(body);
  }
  try {
    const response = await fetch("/api/" + path, options);
    if (response.status === 202 || response.status === 204) return null;
    const result = await response.json().catch(() => null);
    if (!response.ok) {
      let message = typeof result?.message === "string" ? result.message : "Le service local a refusé la demande.";
      if (path === "credentials" && Object.values(body).some(value => value && message.includes(value))) {
        message = "Impossible d’enregistrer les identifiants. Vérifiez la saisie, puis réessayez.";
      }
      throw new Error(message);
    }
    return result;
  } finally { clearTimeout(timeout); signal?.removeEventListener("abort", abort); }
}

function applyStatus(data, sync = !changed() && (!status || id("category").value === status.category)) {
  if (data?.app_id !== "twitch-live-alert" || !Array.isArray(data.live_streams)) throw Error("Réponse inattendue du service local.");
  if (data.category !== status?.category || data.language !== status?.language) visibleStreams = streamPageSize;
  status = data;
  connected = true;
  if (sync) fillSettings(data);
  render();
}

async function refresh() {
  if (pending) return;
  if (refreshing) { requestingStatus = true; return; }
  refreshing = true;
  const ticket = revision;
  try {
    const data = await request("status");
    if (ticket === revision) applyStatus(data);
  } catch {
    if (ticket === revision) { connected = false; render(); }
  } finally {
    refreshing = false;
    if (requestingStatus) { requestingStatus = false; void refresh(); }
  }
}

async function act(key, target, waiting, task) {
  if (pending || !connected) return;
  const previousFocus = document.activeElement;
  const previousPage = page;
  pending = key;
  revision++;
  controls();
  say(target, waiting);
  try {
    say(target, await task(), "good");
  } catch (error) {
    const network = error instanceof TypeError || error.name === "AbortError";
    say(target, network ? disconnection : error.message, "bad");
  } finally {
    pending = "";
    controls();
    if (previousPage === page && document.activeElement === document.body && previousFocus !== document.body) {
      if (previousFocus.matches(":enabled") && previousFocus.offsetParent) previousFocus.focus({ preventScroll: true });
      else { target.tabIndex = -1; target.focus({ preventScroll: true }); }
    }
    void refresh();
  }
}

function route(focus = true) {
  const hash = location.hash.slice(1);
  const next = Object.hasOwn(labels, hash) ? hash : "directs";
  if (page === next) return;
  if (page === "connexion") id("credentials-form").reset();
  if (page === "reglages") categoryPicker.close();
  page = next;
  for (const section of document.querySelectorAll("[data-page]")) section.hidden = section.dataset.page !== page;
  for (const link of document.querySelectorAll("[data-nav]")) {
    if (link.dataset.nav === page) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  text(id("crumb-page"), labels[page]);
  document.title = labels[page] + " · Twitch Live Alert";
  if (focus) {
    id("title-" + page).focus({ preventScroll: true });
    window.scrollTo(0, 0);
  }
  controls();
  if (page === "journal") void journal.load();
}

function settingsEdited() {
  say(id("settings-feedback"), "");
  controls();
}

id("settings-form").addEventListener("input", settingsEdited);
id("settings-form").addEventListener("submit", event => {
  event.preventDefault();
  if (!changed()) return;
  const values = currentSettings();
  if (!values.category) {
    say(id("settings-feedback"), "Saisissez le nom d’une catégorie Twitch.", "bad"); id("category").focus(); return;
  }
  void act("settings", id("settings-feedback"), "Enregistrement en cours…", async () => {
    applyStatus(await request("settings", values), true);
    return "Réglages enregistrés.";
  });
});
id("credentials-form").addEventListener("input", () => { say(id("credential-feedback"), ""); controls(); });
id("credentials-form").addEventListener("submit", event => {
  event.preventDefault();
  const clientId = id("client-id").value.trim();
  const secret = id("client-secret").value;
  if (!clientId || !secret) return;
  void act("credentials", id("credential-feedback"), "Enregistrement en cours…", async () => {
    applyStatus(await request("credentials", { client_id: clientId, client_secret: secret }));
    id("credentials-form").reset();
    return "Identifiants Twitch enregistrés.";
  });
});
id("show-more").addEventListener("click", event => {
  const firstNew = visibleStreams;
  visibleStreams += streamPageSize;
  renderStreams();
  if (event.detail === 0 || id("more-streams").hidden) {
    id("stream-list").children[firstNew]?.querySelector("a")?.focus();
  }
});
id("check-now").addEventListener("click", () => void act("check", id("watch-feedback"), "Demande de vérification…", async () => {
  await request("check-now", {});
  return "Vérification demandée.";
}));
id("toggle-monitor").addEventListener("click", () => void act("monitoring", id("watch-feedback"), "Mise à jour de la surveillance…", async () => {
  applyStatus(await request("monitoring", { enabled: !status.running }));
  return status.running ? "Surveillance reprise." : "Surveillance mise en pause.";
}));
id("test-notification").addEventListener("click", () => void act("test", id("test-feedback"), "Envoi à Windows…", async () => {
  const data = await request("test-notification", {});
  return data?.message || "Notification de test transmise à Windows.";
}));

document.querySelector(".skip").addEventListener("click", event => { event.preventDefault(); id("main").focus(); });
window.addEventListener("hashchange", () => route());
window.addEventListener("pagehide", () => id("credentials-form").reset());
initLanguages();
emptyState(id("stream-message"), "radio", "Chargement des directs…");
route(false);
void refresh();
setInterval(() => {
  if (document.hidden) return;
  void refresh();
  if (page === "journal") void journal.load();
}, 5000);
document.addEventListener("visibilitychange", () => { if (!document.hidden) { void refresh(); if (page === "journal") void journal.load(); } });
