# Planche de l’interface locale

Les spécimens se trouvent dans la page servie à l’adresse `http://127.0.0.1:37219/` avec les données du service local.

| Spécimen | Voir dans la page | Source |
|---|---|---|
| Fondations | En-tête serif, surface de surveillance, accents | [Jetons et base CSS](../../web/style.css) |
| État | Connexion, actif, pause, Twitch à configurer, incident, hors connexion | [`describeStatus`](../../web/app.js) |
| Commandes | Groupe de boutons sous les dates et bouton d’enregistrement | [HTML](../../web/index.html), [`renderControls`](../../web/app.js) |
| Formulaire | Champ numérique, cases à cocher, validation et retours | [HTML](../../web/index.html), [gestion du formulaire](../../web/app.js) |
| Résultats | Chaîne, titre, détail et messages de liste | [`createEntry` et `renderLists`](../../web/app.js) |
| Focus | Bordure laiton visible au clavier | [`:focus-visible`](../../web/style.css) |

Voir la [fiche Surveillance](fiches/surveillance.md) et le [design system](DESIGN-SYSTEM.md).