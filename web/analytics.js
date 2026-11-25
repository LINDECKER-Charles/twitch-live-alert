import { dateLabel, emptyState, icon, text } from "./lists.js";

const counts = new Intl.NumberFormat("fr-FR");
const rates = new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 1 });
const weekdays = ["Lundi", "Mardi", "Mercredi", "Jeudi", "Vendredi", "Samedi", "Dimanche"];
const pageSize = 25;
const channelBatch = 10;

function node(tag, content, className) {
  const element = document.createElement(tag);
  if (content != null) element.textContent = content;
  if (className) element.className = className;
  return element;
}

function frequency(count, days) {
  const rate = count * 7 / days;
  return rate > 0 && rate < 0.05 ? "< 0,1" : rates.format(rate);
}

function duration(seconds) {
  if (seconds == null) return "Durée inconnue";
  if (seconds < 60) return Math.floor(seconds) + " s";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes + " min";
  const remainder = minutes % 60;
  return counts.format(Math.floor(minutes / 60)) + " h" + (remainder ? " " + String(remainder).padStart(2, "0") : "");
}

function average(stats) {
  return stats.measured_count ? duration(stats.total_seconds / stats.measured_count) : "Durée inconnue";
}

function weekdayChart(values) {
  const max = Math.max(1, ...values);
  return values.map((value, index) => {
    const item = node("li");
    const shortDay = node("abbr", weekdays[index].slice(0, 3) + ".");
    shortDay.title = weekdays[index];
    const column = node("div", null, "weekday-column");
    // SVG height follows the data while keeping inline CSS outside the page policy.
    const bar = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    bar.setAttribute("height", (value / max * 72) + "%");
    bar.setAttribute("width", "26");
    bar.setAttribute("aria-hidden", "true");
    const rect = document.createElementNS(bar.namespaceURI, "rect");
    rect.setAttribute("width", "100%");
    rect.setAttribute("height", "100%");
    rect.setAttribute("rx", "2");
    bar.append(rect);
    column.append(node("strong", counts.format(value)), bar);
    item.append(node("span", weekdays[index] + " ", "sr-only"), column, shortDay);
    shortDay.setAttribute("aria-hidden", "true");
    return item;
  });
}

function channelRow(channel, days) {
  const row = node("tr");
  const name = node("th");
  name.scope = "row";
  const button = node("button", channel.name, "channel-link");
  button.type = "button";
  button.dataset.channel = channel.key;
  button.append(icon("chevron"));
  name.append(button);
  row.append(name);
  for (const [label, value] of [
    ["Directs", counts.format(channel.live_count)],
    ["Directs / sem.", frequency(channel.live_count, days)],
    ["Durée moyenne", average(channel)],
    ["Durée cumulée", duration(channel.measured_count ? channel.total_seconds : null)],
  ]) {
    const cell = node("td", value);
    cell.dataset.label = label;
    row.append(cell);
  }
  return row;
}

function sessionRow(session) {
  const item = node("li");
  const copy = node("div", null, "row-copy");
  copy.append(node("strong", session.title || "Direct sans titre"), node("p", session.user_name || "Chaîne inconnue"));
  const date = node("div", null, "session-measure");
  date.append(node("span", session.started_at == null ? "Première trace" : "Début"), node("span", dateLabel(session.occurred_at)));
  const measure = node("div", null, "session-measure");
  measure.append(node("span", "Durée observée"), node("span", duration(session.duration_seconds)));
  const peak = node("div", null, "session-measure");
  peak.append(node("span", "Pic observé"), node("span", session.peak_viewers == null ? "Inconnu" : counts.format(session.peak_viewers) + (session.peak_viewers > 1 ? " spectateurs" : " spectateur")));
  item.append(copy, date, measure, peak);
  return item;
}

export function createAnalytics(request, id) {
  const period = id("analytics-period");
  const channel = id("analytics-channel");
  const content = id("analytics-content");
  const message = id("analytics-message");
  const channelList = id("analytics-channels");
  const page = content.closest("[data-page]");
  let offset = 0;
  let visibleChannels = channelBatch;
  let current = null;
  let loadedKey = "";
  let active = null;
  let nextRefresh = 0;
  let restoreChannel = "";
  let focusSessions = false;
  let snapshot = "";

  function channelButton(key) {
    return [...channelList.querySelectorAll("button")].find(button => button.dataset.channel === key);
  }

  function updateOptions(channels, key) {
    const entries = channels.map(item => [item.name, item.key]).sort((a, b) => a[0].localeCompare(b[0], "fr"));
    if (key && !channels.some(item => item.key === key)) {
      entries.push([channel.selectedOptions[0].text, key]);
    }
    const options = JSON.stringify(entries);
    if (channel.dataset.options === options) return;
    channel.replaceChildren(new Option("Toutes les chaînes", ""), ...entries.map(([label, value]) => new Option(label, value)));
    channel.value = key;
    channel.dataset.options = options;
  }

  function renderChannels() {
    const { channels, period_days } = current;
    const oldFocus = channelList.contains(document.activeElement) ? document.activeElement.dataset.channel : null;
    const sorted = [...channels].sort((a, b) => b.live_count - a.live_count || a.name.localeCompare(b.name, "fr"));
    const targetIndex = restoreChannel ? sorted.findIndex(item => item.key === restoreChannel) : -1;
    visibleChannels = Math.max(visibleChannels, targetIndex + 1);
    const shown = sorted.slice(0, visibleChannels);
    channelList.replaceChildren(...shown.map(item => channelRow(item, period_days)));
    id("analytics-more-channels").hidden = shown.length === sorted.length;
    text(id("analytics-channel-range"), counts.format(shown.length) + " chaînes sur " + counts.format(sorted.length));
    if (oldFocus) channelButton(oldFocus)?.focus({ preventScroll: true });
  }

  function render(data, key) {
    current = data;
    const { totals, channels, period_days } = data;
    updateOptions(channels, channel.value);
    const selected = channel.value;
    if (!totals.live_count) {
      content.hidden = true;
      const canBroaden = Boolean(selected) || period.value !== "0";
      emptyState(message, "chart", "Aucun direct enregistré" + (canBroaden ? " pour cette sélection" : ""),
        canBroaden ? "Essayez une autre chaîne ou une période plus longue." : "Les directs détectés par vos règles apparaîtront ici.");
      return;
    }
    emptyState(message, "chart", "");
    content.hidden = false;
    const serial = JSON.stringify(data);
    if (key === loadedKey && snapshot === serial) return;
    snapshot = serial;
    const scope = channel.selectedOptions[0].text;
    text(id("analytics-scope"), scope);
    id("analytics-back").hidden = !selected;
    text(id("analytics-live-count"), counts.format(totals.live_count));
    const latest = selected && channels.find(item => item.key === selected)?.last_at;
    text(id("analytics-live-note"), selected ? (latest != null ? "Dernier direct le " + dateLabel(latest) : "") :
      counts.format(channels.length) + (channels.length > 1 ? " chaînes" : " chaîne"));
    text(id("analytics-frequency"), frequency(totals.live_count, period_days));
    text(id("analytics-frequency-unit"), period.value === "0" ? "directs par semaine sur " + counts.format(period_days) + (period_days > 1 ? " jours" : " jour") : "directs par semaine");
    text(id("analytics-average"), average(totals));
    text(id("analytics-duration-total"), duration(totals.measured_count ? totals.total_seconds : null));
    text(id("analytics-coverage"), counts.format(totals.measured_count) + (totals.measured_count > 1 ? " directs mesurés sur " : " direct mesuré sur ") + counts.format(totals.live_count));
    text(id("analytics-desktop"), counts.format(totals.desktop_alerts));
    text(id("analytics-email"), counts.format(totals.email_alerts));
    id("analytics-weekdays").replaceChildren(...weekdayChart(data.weekdays));
    id("analytics-comparison").hidden = Boolean(selected);
    if (!selected) renderChannels();
    id("analytics-sessions").replaceChildren(...data.sessions.map(sessionRow));
    text(id("analytics-range"), counts.format(offset + 1) + "–" + counts.format(offset + data.sessions.length) + " sur " + counts.format(totals.live_count));
    id("analytics-pagination").hidden = totals.live_count <= pageSize;
    id("analytics-previous").disabled = offset === 0;
    id("analytics-next").disabled = offset + pageSize >= totals.live_count;
    text(id("analytics-page-number"), "Page " + (1 + Math.floor(offset / pageSize)) + " sur " + Math.ceil(totals.live_count / pageSize));
  }

  function leave() {
    active?.controller.abort();
    active = null;
    restoreChannel = "";
    focusSessions = false;
  }

  async function load() {
    if (page.hidden) return;
    const query = new URLSearchParams({ days: period.value, offset: String(offset) });
    if (channel.value) query.set("channel", channel.value);
    const key = query.toString();
    if (active?.key === key) return;
    active?.controller.abort();
    const controller = new AbortController();
    active = { key, controller };
    if (loadedKey !== key) {
      content.hidden = true;
      emptyState(message, "chart", "Chargement des statistiques…");
      text(id("analytics-feedback"), "Chargement des statistiques…");
    }
    try {
      const data = await request("analytics?" + key, undefined, controller.signal);
      if (controller.signal.aborted) return;
      if (!(data?.period_days > 0) || !data.totals || !Array.isArray(data.channels) || !Array.isArray(data.sessions) || data.weekdays?.length !== 7) {
        throw Error("Réponse incomplète");
      }
      if (offset && offset >= data.totals.live_count) {
        offset = Math.max(0, Math.floor((data.totals.live_count - 1) / pageSize) * pageSize);
        return void load();
      }
      const announce = loadedKey !== key || !message.hidden;
      render(data, key);
      loadedKey = key;
      if (announce) text(id("analytics-feedback"), data.totals.live_count ? counts.format(data.totals.live_count) + " directs, " + channel.selectedOptions[0].text + "." : message.textContent);
      if (restoreChannel) (channelButton(restoreChannel) || channel).focus();
      if (focusSessions && !content.hidden) id("analytics-sessions-title").focus();
      restoreChannel = "";
      focusSessions = false;
    } catch {
      if (controller.signal.aborted) return;
      const stale = key === loadedKey && !content.hidden;
      if (!stale) content.hidden = true;
      emptyState(message, "chart", "Statistiques indisponibles", stale ? "Le dernier relevé de cette sélection reste affiché." : "Vérifiez la connexion au service local, puis réessayez.", load);
      text(id("analytics-feedback"), message.textContent);
    } finally {
      if (active?.controller === controller) {
        active = null;
        nextRefresh = Date.now() + 30000;
      }
    }
  }

  function filterChanged() {
    offset = 0;
    visibleChannels = channelBatch;
    restoreChannel = "";
    focusSessions = false;
    void load();
  }

  function changePage(delta) {
    offset += delta * pageSize;
    focusSessions = true;
    id("analytics-sessions-title").focus();
    void load();
  }

  period.addEventListener("change", filterChanged);
  channel.addEventListener("change", filterChanged);
  channelList.addEventListener("click", event => {
    const button = event.target.closest("button[data-channel]");
    if (!button) return;
    channel.value = button.dataset.channel;
    channel.focus();
    filterChanged();
  });
  id("analytics-back").addEventListener("click", () => {
    restoreChannel = channel.value;
    channel.value = "";
    channel.focus();
    offset = 0;
    void load();
  });
  id("analytics-show-channels").addEventListener("click", () => {
    const firstNewIndex = visibleChannels;
    visibleChannels += channelBatch;
    renderChannels();
    channelList.rows[firstNewIndex]?.querySelector("button").focus();
  });
  id("analytics-previous").addEventListener("click", () => changePage(-1));
  id("analytics-next").addEventListener("click", () => changePage(1));
  return { load, leave, refreshIfDue() { if (Date.now() >= nextRefresh) void load(); } };
}
