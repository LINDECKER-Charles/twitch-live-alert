import { dateLabel, text } from "./lists.js";

const variableLabels = {
  user_name: "Nom de la chaîne", user_login: "Identifiant public de la chaîne", user_id: "ID de la chaîne",
  stream_id: "ID du direct", title: "Titre du direct", category: "Catégorie", game_id: "ID de catégorie",
  language: "Langue", started_at: "Début du direct", viewer_count: "Spectateurs", url: "Lien du direct",
  tags: "Tags Twitch", is_mature: "Contenu adulte", thumbnail_url: "Lien de l’aperçu", type: "Type de diffusion",
  rule: "Nom de la règle", detected_at: "Heure de détection",
};

export function createMail(id, request, say) {
  const form = id("mail-form");
  let saved = null;
  let online = false;
  let busy = false;
  let loading = false;
  let checkingDelivery = false;
  let modelField = id("mail-body");
  let version = 0;

  function controls() {
    id("mail-fields").disabled = !online || !saved || busy;
    const edited = dirty();
    id("test-mail").disabled = !online || busy || !saved?.host || !saved?.recipients?.length || edited;
    id("mail-test-help").textContent = edited ? "Enregistrez vos modifications pour tester cette configuration." :
      saved?.host ? "Envoyer un message aux destinataires enregistrés." : "Enregistrez d’abord le serveur, l’expéditeur et les destinataires.";
  }

  function draft() {
    const body = {
      host: id("mail-host").value.trim(), port: Number(id("mail-port").value),
      security: id("mail-security").value, sender: id("mail-sender").value.trim(),
      recipients: id("mail-recipients").value.split(/\r?\n/).map(line => line.trim()).filter(Boolean),
      subject_template: id("mail-subject").value, body_template: id("mail-body").value,
    };
    if (id("mail-auth").value === "replace") {
      body.username = id("smtp-user").value;
      body.password = id("smtp-password").value;
    } else if (id("mail-auth").value === "clear" && saved?.credentials_configured) {
      body.username = body.password = "";
    }
    return body;
  }

  function dirty() {
    if (!saved) return false;
    return Object.entries(draft()).some(([name, value]) => JSON.stringify(value) !== JSON.stringify(saved[name]));
  }

  function resetSecrets() {
    id("smtp-user").value = id("smtp-password").value = "";
    id("mail-auth").value = "keep";
    auth();
  }

  function auth() {
    const replace = id("mail-auth").value === "replace";
    id("smtp-auth-fields").hidden = !replace;
    id("smtp-user").required = id("smtp-password").required = replace;
    if (!replace) id("smtp-user").value = id("smtp-password").value = "";
    controls();
  }

  function variables(names) {
    const container = id("mail-variables");
    container.replaceChildren();
    for (const name of names) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "variable";
      const token = "{{" + name + "}}";
      button.textContent = token;
      if (variableLabels[name]) {
        const label = document.createElement("span");
        label.textContent = variableLabels[name];
        button.append(label);
      }
      button.onclick = () => {
        modelField.setRangeText(token, modelField.selectionStart, modelField.selectionEnd, "end");
        modelField.focus();
        modelField.dispatchEvent(new Event("input", { bubbles: true }));
      };
      container.append(button);
    }
  }

  function fill(data) {
    saved = data;
    id("mail-host").value = data.host;
    id("mail-port").value = data.port;
    id("mail-security").value = data.security;
    id("mail-sender").value = data.sender;
    id("mail-recipients").value = data.recipients.join("\n");
    id("mail-subject").value = data.subject_template;
    id("mail-body").value = data.body_template;
    id("mail-auth").options[0].textContent = data.credentials_configured ? "Conserver les identifiants enregistrés" : "Aucun identifiant enregistré";
    resetSecrets();
    variables(data.variables);
    controls();
  }

  async function delivery() {
    if (checkingDelivery || id("title-courriel").closest("[data-page]").hidden) return;
    checkingDelivery = true;
    const status = id("delivery-state");
    const summary = id("delivery-summary");
    const error = id("delivery-error");
    const next = id("delivery-next");
    try {
      const data = await request("mail/delivery");
      if (!data?.available) throw Error();
      const { pending_count: pending, failed_count: failed, scheduled_count: scheduled } = data;
      say(status, failed ? "Envoi en échec" : pending ? "En attente" : "À jour", failed ? "warn" : pending ? "" : "good");
      const count = (n, one, plural) => n + " " + (n === 1 ? one : plural);
      const parts = [count(pending, "message en attente", "messages en attente")];
      if (scheduled) parts.push(count(scheduled, "programmé", "programmés"));
      if (failed) parts.push(count(failed, "en échec", "en échec"));
      text(summary, pending ? parts.join(" · ") : "Aucun courriel en attente.");
      error.hidden = !failed;
      text(error, failed ? [data.rule_name, data.last_error || "Le serveur SMTP a refusé un envoi."].filter(Boolean).join(" — ") : "");
      next.hidden = !data.next_attempt_at && !failed;
      text(next, data.next_attempt_at ? (failed ? "Prochaine tentative automatique le " : "Prochain envoi prévu le ") +
        dateLabel(data.next_attempt_at) + "." : failed ? "La file reprendra automatiquement après correction de la configuration." : "");
    } catch {
      say(status, "Indisponible", "warn");
      text(summary, "La file de courriels est inaccessible. Son état sera vérifié à nouveau.");
      error.hidden = next.hidden = true;
    } finally { checkingDelivery = false; }
  }

  async function load() {
    void delivery();
    if (loading || busy || dirty()) return;
    loading = true;
    const ticket = version;
    if (!saved) say(id("mail-load"), "Chargement de la configuration…");
    id("retry-mail").hidden = true;
    try {
      const data = await request("mail");
      if (ticket !== version || dirty()) return;
      fill(data); say(id("mail-load"), "");
    } catch {
      say(id("mail-load"), "Configuration du courriel indisponible. Vérifiez le service local.", "bad");
      id("retry-mail").hidden = false;
    } finally { loading = false; controls(); }
  }

  async function run(target, waiting, task) {
    if (busy || !online) return;
    version++;
    busy = true; controls(); say(target, waiting);
    try { say(target, await task(), "good"); }
    catch (error) { say(target, error.message, "bad"); }
    finally { busy = false; controls(); }
  }

  id("mail-auth").onchange = auth;
  id("mail-body").onfocus = id("mail-subject").onfocus = event => { modelField = event.target; };
  form.addEventListener("input", event => {
    version++;
    say(id("mail-feedback"), "");
    say(id("mail-test-feedback"), "");
    if ([id("mail-body"), id("mail-subject")].includes(event.target)) {
      id("mail-preview").hidden = true; say(id("preview-feedback"), "");
    }
    controls();
  });
  id("mail-security").onchange = () => {
    const port = id("mail-port");
    if (port.value === "465" && id("mail-security").value === "starttls") port.value = "587";
    else if (port.value === "587" && id("mail-security").value === "tls") port.value = "465";
  };
  form.onsubmit = event => {
    event.preventDefault();
    const body = draft();
    if (saved?.credentials_configured && id("mail-auth").value === "keep" &&
      (body.host !== saved.host || body.port !== saved.port || body.security !== saved.security)) {
      say(id("mail-feedback"), "Pour changer de serveur, renseignez ses identifiants ou choisissez « Sans authentification ».", "bad");
      id("mail-auth").focus(); return;
    }
    void run(id("mail-feedback"), "Enregistrement…", async () => {
      fill(await request("mail", body));
      void delivery();
      return "Configuration du courriel enregistrée.";
    });
  };
  id("preview-mail").onclick = () => void run(id("preview-feedback"), "Préparation de l’aperçu…", async () => {
    const data = await request("mail/preview", { subject_template: id("mail-subject").value, body_template: id("mail-body").value });
    id("preview-subject").textContent = data.subject;
    id("preview-body").textContent = data.body;
    id("mail-preview").hidden = false;
    return "Aperçu avec un direct d’exemple.";
  });
  id("default-template").onclick = () => {
    id("mail-subject").value = saved.default_subject_template;
    id("mail-body").value = saved.default_body_template;
    id("mail-body").dispatchEvent(new Event("input", { bubbles: true }));
    say(id("mail-feedback"), "Modèle par défaut rétabli dans le brouillon.");
  };
  id("test-mail").onclick = () => void run(id("mail-test-feedback"), "Envoi du courriel de test…", async () =>
    (await request("mail/test", {})).message);
  id("retry-mail").onclick = load;
  return { load, delivery, clearSecrets: resetSecrets, setOnline(value) { online = value; controls(); } };
}
