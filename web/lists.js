const svgNamespace = "http://www.w3.org/2000/svg";
const counts = new Intl.NumberFormat("fr-FR");
const dateFormat = new Intl.DateTimeFormat("fr-FR", { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });
const clockFormat = new Intl.DateTimeFormat("fr-FR", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const startTimeFormat = new Intl.DateTimeFormat("fr-FR", { hour: "2-digit", minute: "2-digit" });
const previewSnapshots = new WeakMap();

export function icon(name) {
  const svg = document.createElementNS(svgNamespace, "svg");
  svg.setAttribute("class", "icon");
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS(svgNamespace, "use");
  use.setAttribute("href", "#i-" + name);
  svg.append(use);
  return svg;
}

export function text(element, value) {
  if (element.textContent !== value) element.textContent = value;
}

export function dateLabel(epoch, short = false) {
  if (epoch == null) return "—";
  const date = new Date(typeof epoch === "number" ? epoch * 1000 : epoch);
  if (Number.isNaN(date.getTime())) return "—";
  const today = new Date();
  const isToday = date.toDateString() === today.toDateString();
  return (short && isToday ? clockFormat : dateFormat).format(date);
}

export function emptyState(element, symbol, title, detail = "", retry) {
  const key = JSON.stringify([symbol, title, detail, Boolean(retry)]);
  if (element.dataset.content === key) return;
  element.dataset.content = key;
  element.hidden = !title;
  element.replaceChildren();
  if (!title) return;
  const stamp = document.createElement("span");
  stamp.className = "empty-icon";
  stamp.append(icon(symbol));
  const heading = document.createElement("strong");
  heading.textContent = title;
  element.append(stamp, heading);
  if (detail) {
    const paragraph = document.createElement("p");
    paragraph.textContent = detail;
    element.append(paragraph);
  }
  if (retry) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "small";
    button.textContent = "Réessayer";
    button.addEventListener("click", retry);
    element.append(button);
  }
}

function channelLink(url) {
  return typeof url === "string" && /^https:\/\/www\.twitch\.tv\/[a-z0-9_]+$/i.test(url) ? url : null;
}

function createEntry(data, historical) {
  const row = document.createElement("li");
  row.dataset.key = JSON.stringify([historical ? data.stream_id : data.id, data.rule_id, data.channel, historical && data.recorded_at]);
  const marker = document.createElement("span");
  marker.className = "row-icon";
  marker.append(icon(historical ? "bell" : "stream"));
  const copy = document.createElement("div");
  copy.className = "row-copy";
  const url = channelLink(data.url);
  const name = document.createElement(url ? "a" : "span");
  name.className = "entry-name";
  const nameText = document.createElement("span");
  nameText.textContent = data.user_name || "Chaîne Twitch";
  name.append(nameText);
  if (url) {
    name.href = url;
    name.target = "_blank";
    name.rel = "noopener noreferrer";
    name.append(icon("external"));
  }
  copy.append(name);
  if (data.title) {
    const title = document.createElement("p");
    title.textContent = data.title;
    copy.append(title);
  }
  if (data.rule_name || historical && data.channel) {
    const source = document.createElement("small");
    source.className = "entry-rule";
    const names = { email: "Courriel", desktop: "Windows" };
    source.append(icon("filter"), document.createTextNode([data.rule_name, historical && names[data.channel]].filter(Boolean).join(" · ")));
    copy.append(source);
  }
  const detail = document.createElement("span");
  detail.className = "row-meta";
  if (historical) {
    const time = document.createElement("time");
    time.dateTime = new Date(data.recorded_at * 1000).toISOString();
    time.textContent = dateLabel(data.recorded_at);
    detail.append(time);
  } else {
    detail.classList.add("stream-meta");
    const start = data.started_at ? new Date(data.started_at) : null;
    if (start && Number.isFinite(start.getTime())) {
      const time = document.createElement("time");
      time.dateTime = start.toISOString();
      time.title = "Début du direct le " + dateFormat.format(start);
      time.setAttribute("aria-label", time.title);
      time.textContent = "Début " + startTimeFormat.format(start);
      detail.append(time);
    }
    const audience = document.createElement("span");
    const unit = document.createElement("span");
    unit.className = "sr-only";
    unit.textContent = data.viewer_count === 1 ? " spectateur" : " spectateurs";
    audience.title = counts.format(data.viewer_count) + unit.textContent;
    audience.append(icon("eye"), document.createTextNode(counts.format(data.viewer_count)), unit);
    detail.append(audience);
  }
  row.append(marker, copy, detail);
  return row;
}

export function renderRows(list, items, historical = false) {
  const snapshot = JSON.stringify(items);
  if (previewSnapshots.get(list) === snapshot) return;
  const focused = list.contains(document.activeElement) ? document.activeElement.closest("a") : null;
  const rows = items.map(item => createEntry(item, historical));
  if (focused) {
    const matching = rows.find(row => row.dataset.key === focused.closest("li").dataset.key)?.querySelector("a");
    if (matching) {
      // Keep keyboard focus on the same real link when viewer counts refresh.
      matching.replaceWith(focused);
      focused.replaceChildren(...matching.childNodes);
    }
  }
  list.replaceChildren(...rows);
  if (focused?.isConnected && document.activeElement !== focused) focused.focus({ preventScroll: true });
  list.classList.toggle("history-rows", historical);
  previewSnapshots.set(list, snapshot);
}

const pageSize = 50;

export function createJournal(request, id) {
  const query = id("history-query");
  const list = id("history-list");
  const message = id("history-message");
  const hint = id("history-feedback");
  let offset = 0;
  let generation = 0;
  let loadedKey = "";
  let busyKey = "";
  let timer = null;
  let total = 0;

  function paginate() {
    id("pagination").hidden = total <= pageSize;
    id("previous").disabled = offset === 0;
    id("next").disabled = offset + pageSize >= total;
    text(id("history-range"), total ? (offset + 1) + "–" + Math.min(offset + pageSize, total) + " sur " + counts.format(total) : "");
  }

  async function load() {
    if (id("title-journal").closest("[data-page]").hidden) return;
    const key = JSON.stringify([query.value.trim(), offset]);
    if (busyKey === key) return;
    busyKey = key;
    const ticket = ++generation;
    text(hint, loadedKey !== key ? "Chargement du journal…" : "");
    const parameters = new URLSearchParams({ q: query.value.trim(), offset: String(offset), limit: String(pageSize) });
    try {
      const data = await request("history?" + parameters);
      if (generation !== ticket) return;
      if (!Array.isArray(data?.items) || !Number.isInteger(data.total)) throw Error("Historique indisponible.");
      total = data.total;
      if (offset && offset >= total) {
        offset = Math.max(0, Math.floor((total - 1) / pageSize) * pageSize);
        return void load();
      }
      loadedKey = key;
      text(hint, "");
      text(id("history-total"), counts.format(total) + (total === 1 ? " notification" : " notifications"));
      const search = Boolean(query.value.trim());
      emptyState(message, search ? "search" : "history", data.items.length ? "" : search ? "Aucun résultat" : "Aucune notification enregistrée",
        search ? "Essayez un autre nom de chaîne ou de titre." : "Les prochaines notifications de direct apparaîtront ici.");
      renderRows(list, data.items, true);
      paginate();
    } catch {
      if (generation !== ticket) return;
      text(hint, "");
      if (loadedKey !== key) {
        renderRows(list, [], true);
        total = 0;
        paginate();
        text(id("history-total"), "");
      }
      emptyState(message, "history", "Journal indisponible", "Vérifiez la connexion au service local, puis réessayez.", load);
    } finally {
      if (generation === ticket) busyKey = "";
    }
  }

  function searchChanged() {
    offset = 0;
    generation++;
    busyKey = "";
    id("clear-search").hidden = !query.value;
    text(hint, "Recherche en cours…");
    clearTimeout(timer);
    timer = setTimeout(load, 240);
  }
  query.addEventListener("input", searchChanged);
  id("clear-search").addEventListener("click", () => { query.value = ""; query.focus(); searchChanged(); });
  id("previous").addEventListener("click", () => { offset = Math.max(0, offset - pageSize); void load(); });
  id("next").addEventListener("click", () => { if (offset + pageSize < total) { offset += pageSize; void load(); } });
  return { load };
}
