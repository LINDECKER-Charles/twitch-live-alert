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
      : "Une plage qui franchit minuit commence le jour coché. Deux heures identiques couvrent 24 heures.";
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
