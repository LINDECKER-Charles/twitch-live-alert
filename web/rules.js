import { cover, createCategoryPicker } from "./category-picker.js";
import { emptyState, icon } from "./lists.js";
import { addDigestTime, scheduleEditor } from "./schedules.js";

const modeNames = { instant: "Immédiat", window: "Plages autorisées", digest: "Récapitulatif" };
const freshSchedule = () => ({ timezone: "local", windows: [] });
function freshRule() {
  return { name: "", category: "", language: "fr", enabled: true, channels: { desktop: true, email: false },
    detection: freshSchedule(), mail: { mode: "instant", schedule: freshSchedule(), digest_times: [] } };
}

function action(label, task, name) {
  const button = document.createElement("button");
  button.className = "small";
  button.type = "button";
  button.textContent = label;
  if (name) button.setAttribute("aria-label", label + " la règle " + name);
  button.addEventListener("click", task);
  return button;
}

export function createRules(id, request, say, langName, refreshStatus) {
  const form = id("rule-form");
  const list = id("rules-list");
  const detection = scheduleEditor(id("detection-schedule"));
  const delivery = scheduleEditor(id("delivery-schedule"));
  const picker = createCategoryPicker(id, request, selectCategory);
  let rules = null;
  let selected = null;
  let category = null;
  let available = false;
  let configured = false;
  let busy = false;
  let generation = 0;
  let confirmId = null;
  const listHint = id("rules-feedback");
  const editHint = id("rule-feedback");

  function controls() {
    id("new-rule").disabled = !available || busy || !rules;
    id("rule-fields").disabled = !available || busy;
    for (const button of list.querySelectorAll("button")) button.disabled = !available || busy;
    id("choose-category").disabled = !configured || !available;
    id("catalog-connect").hidden = configured || !available;
    id("catalog-help").textContent = !available ? "Service local hors connexion." : !configured ? "Connectez Twitch pour rechercher une catégorie." : "";
  }

  function selectCategory(value) {
    category = value;
    id("chosen-category").textContent = value?.name || "Choisir une catégorie";
    id("selected-art").replaceChildren(cover(value));
  }

  function showPolicy() {
    const enabled = id("rule-email").checked;
    const mode = id("mail-mode").value;
    id("mail-policy").hidden = id("mail-policy").disabled = !enabled;
    id("delivery-schedule").hidden = mode === "instant";
    for (const input of id("delivery-schedule").querySelectorAll("input, select, button")) input.disabled = mode === "instant";
    id("digest-fields").hidden = id("digest-fields").disabled = mode !== "digest";
    if (mode === "digest" && !id("digest-times").childElementCount) addDigestTime(id("digest-times"));
    delivery.setDigest(mode === "digest");
    id("policy-help").textContent = {
      instant: "Envoyer un courriel dès la détection du direct.",
      window: "Conserver les alertes en attente jusqu’à la prochaine plage d’envoi.",
      digest: "Regrouper les directs en attente dans un même courriel à chaque échéance.",
    }[mode];
  }

  function edit(rule) {
    selected = rule;
    form.reset();
    confirmId = null;
    id("rules-overview").hidden = id("new-rule").hidden = true;
    form.hidden = false;
    say(listHint, ""); say(editHint, "");
    id("rule-form-title").textContent = selected.id ? "Modifier la règle" : "Nouvelle règle";
    id("rule-name").value = rule.name;
    if (![...id("language").options].some(option => option.value === rule.language)) id("language").add(new Option(langName(rule.language), rule.language));
    id("language").value = rule.language;
    selectCategory(rule.category ? { name: rule.category, box_art_url: rule.box_art_url } : null);
    id("rule-enabled").checked = rule.enabled;
    id("rule-desktop").checked = rule.channels.desktop;
    id("rule-email").checked = rule.channels.email;
    id("mail-mode").value = rule.mail.mode;
    detection.fill(rule.detection);
    delivery.fill(rule.mail.schedule);
    id("digest-times").replaceChildren();
    for (const value of rule.mail.digest_times) addDigestTime(id("digest-times"), value);
    showPolicy();
    controls();
    id("rule-name").focus();
  }

  function finish() {
    selected = null;
    form.hidden = true;
    id("rules-overview").hidden = id("new-rule").hidden = false;
    draw();
    id("new-rule").focus();
  }

  function row(rule) {
    const item = document.createElement("li");
    item.className = "rule-row";
    const main = document.createElement("div");
    const name = document.createElement("h3");
    name.textContent = rule.name;
    const line = document.createElement("p");
    line.textContent = rule.category + " · " + langName(rule.language);
    const details = document.createElement("div");
    details.className = "rule-details";
    const tags = [
      [rule.enabled ? "check" : "pause", rule.enabled ? "Active" : "En pause", rule.enabled ? "good" : ""],
      ...(rule.channels.desktop ? [["screen", "Windows"]] : []),
      ...(rule.channels.email ? [["mail", modeNames[rule.mail.mode]]] : []),
    ];
    for (const [symbol, label, tone] of tags) {
      const tag = document.createElement("span");
      tag.className = "rule-detail";
      if (tone) tag.dataset.tone = tone;
      tag.append(icon(symbol), document.createTextNode(label));
      details.append(tag);
    }
    main.append(name, line, details);
    const actions = document.createElement("div");
    actions.className = "inline-actions row-actions";
    if (confirmId === rule.id) {
      const prompt = document.createElement("strong");
      prompt.textContent = "Supprimer cette règle ?";
      const cancel = action("Annuler", () => { confirmId = null; draw(rule.id); });
      const yes = action("Confirmer", () => void mutate("rules/" + encodeURIComponent(rule.id) + "/delete", {}, "Règle supprimée."));
      yes.classList.add("danger");
      actions.append(prompt, cancel, yes);
    } else {
      actions.append(action("Modifier", () => edit(rule), rule.name),
        action(rule.enabled ? "Désactiver" : "Activer", () => void mutate("rules", { ...rule, enabled: !rule.enabled }, rule.enabled ? "Règle désactivée." : "Règle activée.", rule.id), rule.name),
        action("Supprimer", () => { confirmId = rule.id; draw(rule.id); }, rule.name));
    }
    item.dataset.rule = rule.id;
    item.append(cover(rule), main, actions);
    return item;
  }

  function draw(focusId) {
    if (!rules) return;
    list.replaceChildren(...rules.map(row));
    const count = rules.filter(rule => rule.enabled).length;
    id("rules-count").textContent = count + (count === 1 ? " active sur " : " actives sur ") + rules.length;
    emptyState(id("rules-empty"), "filter", rules.length ? "" : "Aucune règle de surveillance", "Créez une règle pour choisir les directs qui vous intéressent.");
    controls();
    if (focusId) [...list.children].find(node => node.dataset.rule === focusId)?.querySelector("button")?.focus();
  }

  async function load() {
    const ticket = ++generation;
    if (!rules) emptyState(id("rules-empty"), "filter", "Chargement des règles…");
    try {
      const data = await request("rules");
      if (ticket !== generation) return;
      if (!Array.isArray(data?.rules)) throw Error();
      rules = data.rules;
      draw();
    } catch {
      if (ticket !== generation) return;
      emptyState(id("rules-empty"), "filter", "Règles indisponibles", "Vérifiez le service local, puis réessayez.", load);
    }
  }

  async function mutate(path, body, message, focusId) {
    if (!available || busy) return;
    const hint = selected ? editHint : listHint;
    busy = true;
    generation++;
    controls();
    say(hint, "Enregistrement…");
    try {
      const data = await request(path, body);
      rules = data.rules;
      confirmId = null;
      if (selected) finish(); else draw(focusId);
      say(listHint, message, "good");
      await refreshStatus();
    } catch (error) { say(hint, error.message, "bad"); }
    finally {
      busy = false; controls();
      if (document.activeElement === document.body && !selected) {
        const item = [...list.children].find(node => node.dataset.rule === focusId);
        (item?.querySelector("button") || id("new-rule")).focus({ preventScroll: true });
      }
    }
  }

  form.addEventListener("submit", event => {
    event.preventDefault();
    try {
      const name = id("rule-name").value.trim();
      if (!name) { id("rule-name").focus(); throw Error("Donnez un nom à la règle."); }
      if (!category) { id("choose-category").focus(); throw Error("Choisissez une catégorie Twitch."); }
      const channels = { desktop: id("rule-desktop").checked, email: id("rule-email").checked };
      if (!channels.desktop && !channels.email) { id("rule-desktop").focus(); throw Error("Choisissez au moins un canal de notification."); }
      const mode = id("mail-mode").value;
      const times = [...id("digest-times").querySelectorAll("input")].map(input => input.value).filter(Boolean);
      if (channels.email && mode === "digest" && !times.length) throw Error("Ajoutez une heure pour le récapitulatif.");
      const body = { ...selected, name, category: category.name, language: id("language").value,
        box_art_url: category.box_art_url || "", enabled: id("rule-enabled").checked, channels,
        detection: detection.value(), mail: { mode, schedule: mode === "instant" ? selected.mail.schedule : delivery.value(),
          digest_times: [...new Set(times)].sort() } };
      void mutate("rules", body, "Règle enregistrée.");
    } catch (error) { say(editHint, error.message, "bad"); }
  });
  id("new-rule").onclick = () => edit(freshRule());
  id("cancel-rule").onclick = finish;
  id("choose-category").onclick = () => picker.open(category?.name);
  id("rule-email").onchange = showPolicy;
  id("mail-mode").onchange = showPolicy;
  id("add-time").onclick = () => addDigestTime(id("digest-times")).focus();
  return { load, closePicker: picker.close,
    setContext(online, hasTwitch) { available = online; configured = hasTwitch; controls(); } };
}
