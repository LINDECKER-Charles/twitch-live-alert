const days = ["Lundi", "Mardi", "Mercredi", "Jeudi", "Vendredi", "Samedi", "Dimanche"];

function labelled(title, control) {
  const label = document.createElement("label");
  label.className = "field";
  const name = document.createElement("span");
  name.textContent = title;
  label.append(name, control);
  return label;
}

function button(text) {
  const result = document.createElement("button");
  result.type = "button";
  result.className = "small";
  result.textContent = text;
  return result;
}

function time(value) {
  const control = document.createElement("input");
  control.type = "time";
  control.required = true;
  control.value = value;
  return control;
}

export function addDigestTime(container, value = "08:00") {
  const row = document.createElement("div");
  row.className = "time-line";
  const control = time(value);
  const remove = button("Retirer");
  remove.setAttribute("aria-label", "Retirer cette heure d’envoi");
  remove.onclick = () => { row.remove(); document.getElementById("add-time").focus(); };
  row.append(labelled("Heure d’envoi", control), remove);
  container.append(row);
  return control;
}

export function scheduleEditor(host) {
  const zone = document.createElement("select");
  zone.add(new Option("Heure locale de ce PC", "local"));
  const zones = new Set(["UTC", "Europe/Paris", ...Intl.supportedValuesOf("timeZone")]);
  for (const name of [...zones].sort()) zone.add(new Option(name, name));

  const rows = document.createElement("div");
  rows.className = "window-list";
  const add = button("Ajouter une plage");
  const hint = document.createElement("small");
  const fields = document.createElement("div");
  fields.className = "stack schedule-fields";
  fields.append(labelled("Fuseau horaire", zone), rows, add, hint);
  host.append(fields);
  let digest = false;

  function explain() {
    hint.textContent = !rows.childElementCount
      ? (digest ? "Tous les jours, aux heures choisies." : "Tous les jours, à toute heure.")
      : "Une plage qui franchit minuit commence le jour coché. Deux heures identiques couvrent toute la journée cochée.";
    add.textContent = "Ajouter une plage";
    add.hidden = digest && rows.childElementCount === 1 && rows.firstElementChild.querySelector(".time-line")?.hidden;
    if (add.hidden) hint.textContent = "Le récapitulatif est envoyé les jours cochés, aux heures choisies.";
  }

  function appendWindow(window) {
    const row = document.createElement("fieldset");
    row.className = "window-row";
    const legend = document.createElement("legend");
    legend.textContent = digest ? "Jours d’envoi" : "Jours de début";
    const group = document.createElement("div");
    group.className = "day-group";
    days.forEach((name, index) => {
      const label = document.createElement("label");
      const check = document.createElement("input");
      check.type = "checkbox";
      check.value = String(index + 1);
      check.checked = window.days.includes(index + 1);
      check.setAttribute("aria-label", name);
      const mark = document.createElement("span");
      mark.textContent = name.slice(0, 3) + ".";
      label.append(check, mark);
      group.append(label);
    });
    const range = document.createElement("div");
    range.className = "time-line";
    const remove = button("Retirer la plage");
    remove.onclick = () => { row.remove(); explain(); add.focus(); };
    range.append(labelled("Début", time(window.start)), labelled("Fin", time(window.end)), remove);
    row.append(legend, group, range);
    if (digest && window.start === "00:00" && window.end === "00:00") range.hidden = true;
    rows.append(row);
    explain();
    return group.querySelector("input");
  }

  add.onclick = () => appendWindow({ days: [1, 2, 3, 4, 5, 6, 7], start: digest ? "00:00" : "09:00", end: digest ? "00:00" : "18:00" }).focus();
  return {
    setDigest(value) {
      digest = value;
      if (digest && !rows.childElementCount) appendWindow({ days: [1, 2, 3, 4, 5, 6, 7], start: "00:00", end: "00:00" });
      for (const row of rows.children) {
        row.querySelector("legend").textContent = digest ? "Jours d’envoi" : "Jours de début";
        const times = [...row.querySelectorAll("input[type=time]")];
        row.querySelector(".time-line").hidden = digest && times.every(input => input.value === "00:00");
      }
      explain();
    },
    fill(schedule = {}) {
      const timezone = schedule.timezone || "local";
      if (![...zone.options].some(option => option.value === timezone)) zone.add(new Option(timezone, timezone));
      zone.value = timezone;
      rows.replaceChildren();
      for (const window of schedule.windows || []) appendWindow(window);
      explain();
    },
    value() {
      const windows = [...rows.children].map(row => {
        const chosen = [...row.querySelectorAll("input[type=checkbox]:checked")].map(check => Number(check.value));
        const [start, end] = [...row.querySelectorAll("input[type=time]")].map(control => control.value);
        if (!chosen.length) { row.querySelector("input").focus(); throw Error("Choisissez au moins un jour par plage."); }
        if (!start || !end) throw Error("Complétez les heures de chaque plage.");
        return { days: chosen, start, end };
      });
      return { timezone: zone.value, windows };
    },
  };
}

const formatters = new Map();
const weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export function inDetectionWindow(schedule, now = new Date()) {
  if (!Array.isArray(schedule?.windows)) return null;
  if (!schedule.windows.length) return true;
  try {
    const zone = schedule.timezone;
    if (!formatters.has(zone)) formatters.set(zone, new Intl.DateTimeFormat("en-GB", {
      timeZone: zone === "local" ? undefined : zone, weekday: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23",
    }));
    const parts = Object.fromEntries(formatters.get(zone).formatToParts(now).map(p => [p.type, p.value]));
    const day = weekdays.indexOf(parts.weekday) + 1;
    if (!day) return null;
    const previous = day === 1 ? 7 : day - 1;
    const clock = parts.hour + ":" + parts.minute;
    return schedule.windows.some(({ days: allowed, start, end }) => {
      if (start === end) return allowed.includes(day);
      if (start < end) return allowed.includes(day) && start <= clock && clock < end;
      return (allowed.includes(day) && start <= clock) || (allowed.includes(previous) && clock < end);
    });
  } catch { return null; }
}

function dayLabel(values) {
  const list = [...new Set(values)].sort((a, b) => a - b);
  if (list.length === 7) return "Tous les jours";
  const name = value => days[value - 1].slice(0, 3) + ".";
  const ranges = [];
  for (let i = 0; i < list.length; i++) {
    const first = list[i];
    while (i + 1 < list.length && list[i + 1] === list[i] + 1) i++;
    ranges.push(first === list[i] ? name(first) : name(first) + "–" + name(list[i]));
  }
  return ranges.join(", ");
}

function windowLabel(window, daysOnly) {
  const when = dayLabel(window.days);
  if (window.start === window.end) return when + (daysOnly ? "" : " toute la journée");
  return when + " " + window.start + "–" + window.end + (window.start > window.end ? " (+1 j)" : "");
}

export function scheduleSummary(schedule, mode = "detection", times = []) {
  if (mode === "instant") return { short: "Immédiat" };
  const zone = schedule.timezone === "local" ? "heure du PC" : schedule.timezone;
  const digest = mode === "digest";
  const prefix = mode === "window" ? "Plages · " : "";
  if (!schedule.windows.length && !digest) return { short: prefix + "En continu" };
  const labels = schedule.windows.map(window => windowLabel(window, digest));
  if (!labels.length) labels.push("Tous les jours");
  const sorted = [...new Set(times)].sort();
  const shortTimes = digest ? "Récap. " + sorted.slice(0, 2).join(", ") + (sorted.length > 2 ? " +" + (sorted.length - 2) : "") + " · " : "";
  const short = prefix + shortTimes + labels[0] + (labels.length > 1 ? " +" + (labels.length - 1) : "") + " · " + zone;
  if (labels.length === 1 && (!digest || sorted.length <= 2)) return { short };
  const details = digest ? ["Récapitulatif à " + sorted.join(", ") + " · " + zone, ...labels] : labels.map(label => label + " · " + zone);
  return { short, details };
}
