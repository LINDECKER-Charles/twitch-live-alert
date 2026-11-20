const byId = (id) => document.getElementById(id);
const ui = {
  monitorState: byId("monitor-state"),
  watchDetail: byId("watch-detail"),
  watchError: byId("watch-error"),
  lastCheck: byId("last-check"),
  nextCheck: byId("next-check"),
  notificationState: byId("notification-state"),
  checkNow: byId("check-now"),
  toggleMonitor: byId("toggle-monitor"),
  testNotification: byId("test-notification"),
  watchFeedback: byId("watch-feedback"),
  streamCount: byId("stream-count"),
  streamMessage: byId("stream-message"),
  streamList: byId("stream-list"),
  historyMessage: byId("history-message"),
  notificationList: byId("notification-list"),
  form: byId("settings-form"),
  fields: byId("settings-fields"),
  interval: byId("poll-interval"),
  notifyFirst: byId("notify-first"),
  startWindows: byId("start-windows"),
  save: byId("save-settings"),
  settingsFeedback: byId("settings-feedback"),
  version: byId("app-version"),
};

const dateFormat = new Intl.DateTimeFormat("fr-FR", {
  dateStyle: "short",
  timeStyle: "short",
});
const numberFormat = new Intl.NumberFormat("fr-FR");
const listSnapshots = new WeakMap();
const connectionError = "Le service local ne répond pas. Si la surveillance est fermée, relancez-la depuis le raccourci Bureau.";

let status = null;
let connected = false;
let refreshing = false;
let pending = null;
let revision = 0;

function setText(element, text) {
  if (element.textContent !== text) element.textContent = text;
}

function feedback(element, text, tone = "neutral") {
  element.dataset.tone = tone;
  setText(element, text);
}

function setMessage(element, text) {
  setText(element, text);
  element.hidden = !text;
}

function formatDate(value) {
  if (value == null) return null;
  const date = new Date(typeof value === "number" ? value * 1000 : value);
  return Number.isNaN(date.getTime()) ? null : dateFormat.format(date);
}

function settingsFromForm() {
  return {
    poll_interval_seconds: ui.interval.valueAsNumber,
    notify_existing_on_first_run: ui.notifyFirst.checked,
    start_with_windows: ui.startWindows.checked,
  };
}

function formChanged() {
  if (!status) return false;
  return Object.entries(settingsFromForm()).some(([key, value]) => value !== status[key]);
}

function fillForm(data) {
  ui.interval.value = data.poll_interval_seconds;
  ui.notifyFirst.checked = data.notify_existing_on_first_run;
  ui.startWindows.checked = data.start_with_windows;
}

function setButton(button, available, busy, label) {
  button.disabled = !available;
  button.setAttribute("aria-disabled", String(busy || !available));
  setText(button, label);
}

function renderControls() {
  const canUse = connected && Boolean(status);
  const busy = pending !== null;
  setButton(ui.checkNow, Boolean(canUse && status.running && status.configured), busy,
    pending === "check" ? "Demande en cours…" : "Vérifier maintenant");
  setButton(ui.toggleMonitor, canUse, busy,
    status?.running === false ? "Reprendre" : "Mettre en pause");
  setButton(ui.testNotification, Boolean(canUse && status.notifications_available), busy,
    pending === "test" ? "Envoi en cours…" : "Tester une notification");
  ui.fields.disabled = !canUse;
  setButton(ui.save, canUse, busy || !formChanged(),
    pending === "settings" ? "Enregistrement…" : "Enregistrer");
}

function twitchUrl(value) {
  return typeof value === "string"
    && /^https:\/\/www\.twitch\.tv\/[a-z0-9_]+$/i.test(value) ? value : null;
}

function createEntry(data, meta) {
  const entry = document.createElement("li");
  entry.className = "entry";
  const copy = document.createElement("div");
  const name = document.createElement("h3");
  name.className = "entry-name";
  const href = twitchUrl(data.url);

  if (href) {
    const link = document.createElement("a");
    link.href = href;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = data.user_name;
    name.append(link);
  } else {
    name.textContent = data.user_name;
  }

  copy.append(name);
  if (data.title) {
    const title = document.createElement("p");
    title.className = "entry-title";
    title.textContent = data.title;
    copy.append(title);
  }
  entry.append(copy);

  if (meta) {
    const detail = document.createElement("p");
    detail.className = "entry-meta";
    detail.textContent = meta;
    entry.append(detail);
  }
  return entry;
}

function updateList(list, items, describe) {
  const snapshot = JSON.stringify(items);
  if (listSnapshots.get(list) === snapshot) return;
  if (list.contains(document.activeElement)) return;
  list.replaceChildren(...items.map((item) => createEntry(item, describe(item))));
  listSnapshots.set(list, snapshot);
}

function renderLists() {
  if (!status) {
    if (!connected) {
      setMessage(ui.streamMessage, "Directs indisponibles tant que le service local est inaccessible.");
      setMessage(ui.historyMessage, "Historique indisponible tant que le service local est inaccessible.");
    }
    return;
  }

  const streams = status.live_streams;
  let streamMessage = "";
  if (!connected) {
    streamMessage = "Dernier relevé disponible. La connexion au service local est interrompue.";
  } else if (!status.configured) {
    streamMessage = "Configurez Twitch pour détecter les directs.";
  } else if (!status.running) {
    streamMessage = streams.length
      ? "Dernier relevé avant la mise en pause."
      : "Reprenez la surveillance pour rechercher les directs.";
  } else if (status.last_error) {
    streamMessage = streams.length ? "Dernier relevé disponible." : "La dernière vérification a échoué.";
  } else if (!status.last_checked_at) {
    streamMessage = "En attente de la première vérification Twitch.";
  } else if (streams.length === 0) {
    streamMessage = "Aucun direct Bloodborne en français détecté.";
  }

  setMessage(ui.streamMessage, streamMessage);
  ui.streamCount.hidden = !status.last_checked_at && streams.length === 0;
  setText(ui.streamCount, numberFormat.format(streams.length)
    + (streams.length === 1 ? " direct" : " directs"));
  updateList(ui.streamList, streams, (stream) =>
    numberFormat.format(stream.viewer_count) + (stream.viewer_count === 1 ? " spectateur" : " spectateurs"));

  const notifications = status.notifications;
  setMessage(ui.historyMessage, notifications.length ? "" : "Aucune notification de direct enregistrée.");
  updateList(ui.notificationList, notifications, (notification) => formatDate(notification.recorded_at));
}

function describeStatus() {
  if (!connected) return ["Hors connexion", "error", connectionError];
  if (!status.configured) {
    return ["Twitch à configurer", "attention",
      "Ajoutez les identifiants Twitch via le coffre, puis relancez la surveillance."];
  }
  if (!status.running) {
    return ["En pause", "quiet", "La recherche automatique des directs est suspendue."];
  }
  if (status.last_error) {
    return ["À vérifier", "attention", "La surveillance continue. La dernière vérification a rencontré un problème."];
  }
  return ["Active", "live", "Les nouveaux directs détectés déclenchent une notification Windows."];
}

function render() {
  const [label, tone, detail] = describeStatus();
  ui.monitorState.dataset.tone = tone;
  setText(ui.monitorState, label);
  setText(ui.watchDetail, detail);
  setMessage(ui.watchError, connected ? status?.last_error || "" : "");
  setText(ui.lastCheck, formatDate(status?.last_checked_at) || "—");
  setText(ui.nextCheck, connected && status?.running && status?.configured
    ? formatDate(status.next_check_at) || "En attente"
    : "—");
  setText(ui.notificationState, status
    ? (status.notifications_available ? "Disponibles" : "Indisponibles")
    : "—");
  setText(ui.version, status?.app_version ? "Version " + status.app_version : "");
  renderLists();
  renderControls();
}

async function request(path, body) {
  const controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), 12000);
  try {
    const options = { cache: "no-store", signal: controller.signal };
    if (body !== undefined) {
      Object.assign(options, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Bloodborne-Request": "local",
        },
        body: JSON.stringify(body),
      });
    }
    const response = await fetch("/api/" + path, options);
    const result = await response.json().catch(() => null);
    if (!response.ok) {
      throw new Error(result?.message || "Le service local a refusé la demande.");
    }
    return result;
  } finally {
    clearTimeout(deadline);
  }
}

function applyStatus(data, syncForm = !formChanged()) {
  if (data?.app_id !== "bloodborne-fr-live") throw new Error("Réponse inattendue du service local.");
  status = data;
  connected = true;
  if (syncForm) fillForm(status);
  render();
}

async function refresh() {
  if (refreshing || pending) return;
  refreshing = true;
  const requestRevision = revision;
  try {
    const data = await request("status");
    if (revision === requestRevision) applyStatus(data);
  } catch {
    if (revision === requestRevision) {
      connected = false;
      render();
    }
  } finally {
    refreshing = false;
  }
}

async function runAction(action, message, task) {
  if (pending || !connected) return;
  const target = action === "settings" ? ui.settingsFeedback : ui.watchFeedback;
  pending = action;
  revision += 1;
  feedback(target, message);
  renderControls();
  try {
    feedback(target, await task(), "success");
  } catch (error) {
    const unavailable = error instanceof TypeError || error.name === "AbortError";
    feedback(target, unavailable ? connectionError : error.message, "error");
  } finally {
    pending = null;
    renderControls();
    void refresh();
  }
}

ui.form.addEventListener("input", () => {
  if (pending !== "settings") feedback(ui.settingsFeedback, "");
  renderControls();
});

ui.form.addEventListener("submit", (event) => {
  event.preventDefault();
  if (!formChanged()) return;
  const submitted = settingsFromForm();
  void runAction("settings", "Enregistrement des réglages…", async () => {
    const data = await request("settings", submitted);
    const editedDuringSave = Object.entries(settingsFromForm())
      .some(([key, value]) => submitted[key] !== value);
    applyStatus(data, !editedDuringSave);
    return editedDuringSave ? "Réglages enregistrés. Des modifications restent à enregistrer." : "Réglages enregistrés.";
  });
});

ui.checkNow.addEventListener("click", () => {
  void runAction("check", "Demande de vérification en cours…", async () => {
    await request("check-now", {});
    return "Vérification demandée.";
  });
});

ui.toggleMonitor.addEventListener("click", () => {
  void runAction("monitoring", "Mise à jour de la surveillance…", async () => {
    applyStatus(await request("monitoring", { enabled: !status.running }));
    return status.running ? "Surveillance reprise." : "Surveillance en pause.";
  });
});

ui.testNotification.addEventListener("click", () => {
  void runAction("test", "Envoi de la notification de test…", async () => {
    const result = await request("test-notification", {});
    return result?.message || "Notification de test transmise à Windows.";
  });
});

void refresh();
setInterval(refresh, 5000);
