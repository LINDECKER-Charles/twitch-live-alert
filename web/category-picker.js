import { icon, text } from "./lists.js";

const limit = 8;

export function createCategoryPicker(id, request, onSelection) {
  const input = id("category");
  const toggle = id("category-toggle");
  const panel = id("category-popup");
  const list = id("category-options");
  const feedback = id("category-feedback");
  const help = id("category-help");
  const saved = id("category-current");
  let available = false;
  let savedName = "";
  let timer;
  let controller;
  let active = -1;
  let openingDirection = 0;

  function highlight(index) {
    const previous = list.children[active];
    if (previous) previous.setAttribute("aria-selected", "false");
    active = index;
    const option = list.children[index];
    if (option) {
      option.setAttribute("aria-selected", "true");
      input.setAttribute("aria-activedescendant", option.id);
      option.scrollIntoView({ block: "nearest" });
    } else input.removeAttribute("aria-activedescendant");
  }

  function cancelSearch() {
    clearTimeout(timer);
    controller?.abort();
    controller = null;
    openingDirection = 0;
  }

  function close() {
    cancelSearch();
    highlight(-1);
    panel.hidden = true;
    input.setAttribute("aria-expanded", "false");
    toggle.setAttribute("aria-expanded", "false");
  }

  function showSaved() {
    const different = savedName && input.value.trim() !== savedName;
    text(saved, different ? "Catégorie enregistrée · " + savedName : "");
  }

  function setContext(connected, configured, current) {
    available = connected && configured;
    savedName = current || "";
    toggle.disabled = !available;
    input.readOnly = !configured;
    if (connected && !configured && current) input.value = current;
    id("category-connect").hidden = !connected || configured;
    text(help, !connected ? "Le service local est hors connexion. Votre saisie est conservée."
      : configured ? "Recherchez une catégorie dans le catalogue Twitch ou saisissez son nom exact."
      : "Connectez Twitch pour rechercher et changer de catégorie.");
    if (!available && !panel.hidden) close();
    showSaved();
  }

  function resultOption(name, index) {
    const li = document.createElement("li");
    li.id = "category-option-" + index;
    li.setAttribute("role", "option");
    li.setAttribute("aria-selected", "false");
    li.dataset.name = name;
    const word = document.createElement("span");
    word.textContent = name;
    li.append(icon("game"), word);
    if (name === savedName) li.append(icon("check"));
    return li;
  }

  async function search(query, signal) {
    try {
      const result = await request("categories?" + new URLSearchParams({ q: query }), undefined, signal);
      if (signal.aborted) return;
      if (!Array.isArray(result?.items)) throw new Error();
      const names = [...new Set(result.items.map(item => item?.name).filter(name => typeof name === "string" && name.trim()))];
      const shown = names.slice(0, limit);
      list.replaceChildren(...shown.map(resultOption));
      const count = names.length > limit ? limit + " premiers résultats" : shown.length + (shown.length === 1 ? " résultat" : " résultats");
      text(feedback, shown.length ? count + " pour « " + query + " »" : "Aucune catégorie trouvée pour « " + query + " ».");
      id("category-keys").hidden = shown.length === 0;
      if (openingDirection && shown.length) highlight(openingDirection > 0 ? 0 : shown.length - 1);
    } catch {
      if (!signal.aborted) text(feedback, "Recherche Twitch indisponible. Rouvrez la liste pour réessayer ou saisissez le nom exact.");
    }
  }

  function open(direction = 0) {
    if (!available || input.matches(":disabled")) return;
    cancelSearch();
    highlight(-1);
    list.replaceChildren();
    panel.hidden = false;
    input.setAttribute("aria-expanded", "true");
    toggle.setAttribute("aria-expanded", "true");
    id("category-keys").hidden = true;
    const query = input.value.trim();
    if (!query) {
      text(feedback, "Saisissez quelques lettres pour rechercher une catégorie.");
      return;
    }
    text(feedback, "Recherche de « " + query + " »…");
    openingDirection = direction;
    controller = new AbortController();
    const signal = controller.signal;
    timer = setTimeout(() => void search(query, signal), 280);
  }

  function choose(option) {
    if (!option) return;
    input.value = option.dataset.name;
    close();
    input.focus({ preventScroll: true });
    showSaved();
    onSelection();
  }

  input.addEventListener("input", () => { showSaved(); open(); });
  input.addEventListener("click", () => { if (panel.hidden) open(); });
  input.addEventListener("blur", close);
  toggle.addEventListener("pointerdown", event => event.preventDefault());
  toggle.addEventListener("click", () => {
    input.focus({ preventScroll: true });
    if (panel.hidden) open(); else close();
  });
  list.addEventListener("pointerdown", event => event.preventDefault());
  list.addEventListener("click", event => choose(event.target.closest('[role="option"]')));
  document.addEventListener("pointerdown", event => {
    if (!panel.hidden && !id("category-picker").contains(event.target)) close();
  });
  input.addEventListener("keydown", event => {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      if (!panel.hidden && active >= 0) choose(list.children[active]); else close();
    } else if (event.key === "Tab" || event.key === "Escape") {
      if (event.key === "Escape" && !panel.hidden) event.preventDefault();
      close();
    } else if ((event.key === "ArrowDown" || event.key === "ArrowUp") && available) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const length = list.children.length;
      if (panel.hidden) open(event.altKey ? 0 : step);
      else if (!length) openingDirection = step;
      else if (!event.altKey) highlight(active < 0 ? (step > 0 ? 0 : length - 1) : (active + step + length) % length);
    } else if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) highlight(-1);
  });
  return { setContext, close };
}
