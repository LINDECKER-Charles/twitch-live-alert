import { icon } from "./lists.js";

export function cover(category) {
  const frame = document.createElement("span");
  frame.className = "cover";
  const fallback = icon("game");
  frame.append(fallback);
  if (!category?.box_art_url) return frame;
  try {
    const sized = category.box_art_url.replaceAll("{width}", "144").replaceAll("{height}", "192");
    const url = new URL(sized);
    if (url.origin !== "https://static-cdn.jtvnw.net" || !url.pathname.startsWith("/ttv-boxart/") || url.username || url.password) return frame;
    // Search returns a small fixed-size cover. Games may return a size template instead.
    url.pathname = url.pathname.replace(/-\d+x\d+(?=\.(?:jpg|jpeg|png)$)/i, "-144x192");
    const image = new Image();
    image.alt = "";
    image.referrerPolicy = "no-referrer";
    image.addEventListener("error", () => image.remove());
    image.src = url.href;
    frame.append(image);
  } catch { /* Keep the category name and pictogram when artwork is unavailable. */ }
  return frame;
}

export function createCategoryPicker(id, request, choose) {
  const dialog = id("category-dialog");
  const field = id("category-query");
  const list = id("category-results");
  const hint = id("picker-feedback");
  const retry = id("retry-picker");
  let timer;
  let generation = 0;
  let controller;
  let currentName = "";

  function reset() {
    clearTimeout(timer);
    controller?.abort();
    generation++;
    retry.hidden = true;
    list.replaceChildren();
  }

  async function search(immediate = false) {
    reset();
    const term = field.value.trim();
    if (!term) { hint.textContent = "Saisissez un nom pour chercher une catégorie."; return; }
    const ticket = generation;
    hint.textContent = "Recherche dans le catalogue Twitch…";
    const run = async () => {
      controller = new AbortController();
      try {
        const data = await request("categories?q=" + encodeURIComponent(term), undefined, controller.signal);
        if (ticket !== generation) return;
        const items = data.items.filter(item => typeof item.name === "string" && item.name.trim()).slice(0, 8);
        hint.textContent = items.length === 0 ? "Aucune catégorie trouvée. Essayez un autre nom." :
          items.length === 1 ? "1 catégorie trouvée" : items.length + " catégories trouvées";
        for (const item of items) {
          const li = document.createElement("li");
          const tile = document.createElement("button");
          tile.type = "button";
          tile.className = "category-tile";
          const label = document.createElement("strong");
          label.textContent = item.name;
          tile.append(cover(item), label);
          if (currentName === item.name) {
            tile.setAttribute("aria-current", "true");
            const selected = document.createElement("span");
            selected.className = "tile-current";
            selected.append(icon("check"), document.createTextNode("Choisie"));
            tile.append(selected);
          }
          tile.addEventListener("click", () => { choose(item); dialog.close(); });
          li.append(tile);
          list.append(li);
        }
      } catch {
        if (ticket !== generation) return;
        hint.textContent = "Catalogue Twitch indisponible. Vérifiez la connexion et réessayez.";
        retry.hidden = false;
      }
    };
    if (immediate) await run(); else timer = setTimeout(run, 260);
  }

  field.addEventListener("input", () => void search());
  retry.addEventListener("click", () => void search(true));
  dialog.addEventListener("keydown", event => {
    const buttons = [...list.querySelectorAll("button")];
    if (!buttons.length) return;
    const index = buttons.indexOf(document.activeElement);
    if (index === -1 && document.activeElement !== field) return;
    const columns = getComputedStyle(list).gridTemplateColumns.split(" ").length;
    let next;
    if (event.key === "ArrowDown") next = index < 0 ? 0 : index + columns;
    else if (index >= 0 && event.key === "ArrowUp") next = index - columns;
    else if (index >= 0 && event.key === "ArrowLeft") next = index - 1;
    else if (index >= 0 && event.key === "ArrowRight") next = index + 1;
    else return;
    event.preventDefault();
    if (next < 0) field.focus();
    else buttons[Math.min(next, buttons.length - 1)].focus();
  });
  id("close-picker").addEventListener("click", () => dialog.close());
  dialog.addEventListener("close", reset);
  return {
    open(name) {
      currentName = name || "";
      field.value = currentName;
      dialog.showModal();
      field.focus();
      field.select();
      void search(true);
    },
    close() { if (dialog.open) dialog.close(); },
  };
}
