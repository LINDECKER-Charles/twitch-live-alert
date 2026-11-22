# Console de surveillance Twitch

[English overview](../../en/INTERFACE.md)

### Version 0.2.0 · historique

La console s’adresse à la personne qui suit des directs Twitch sur son PC. Le geste courant consiste à vérifier la surveillance puis ouvrir une chaîne. La connexion Twitch et les préférences sont configurées à l’installation ou lors d’un changement de périmètre. Le nombre de chaînes simultanées dépend de la catégorie et de la langue choisies. Le journal se parcourt par pages de 50 notifications. Les directs s’affichent par tranches de 50 chaînes, avec le total du relevé et un bouton pour poursuivre.

| Destination | Tâche principale | Choix du conteneur |
| --- | --- | --- |
| Directs | Lire l’état et ouvrir une chaîne | Bandeau d’état avec la dernière et la prochaine vérification, liste puis aperçu des trois dernières notifications. |
| Journal | Retrouver une notification | Recherche sur l’historique complet et pagination sur le serveur local. |
| Réglages | Modifier les filtres et le comportement | Formulaire groupé avec enregistrement explicite et conservation du brouillon au rafraîchissement. |
| Connexion Twitch | Renseigner ou remplacer les identifiants | Formulaire dédié, secret masqué, état de configuration fourni par le service. |

Décision du 21 novembre 2026. L’ancien tableau réunissait l’ensemble des réglages et des résultats sur une page. L’ajout de la recherche d’historique, des filtres et de la connexion Twitch justifie quatre destinations avec URL et retour navigateur. Les gestes immédiats de vérification, de pause et de reprise restent sur Directs. Un lien de la console conduit au formulaire quand Twitch doit être configuré.

Décision du 22 novembre 2026. Le champ Catégorie Twitch ouvre les suggestions dans la page. Les flèches parcourent les résultats, Entrée copie le nom choisi et Échap ou Tab ferment la liste en conservant la saisie. Le clic ou le toucher choisit aussi une catégorie. Le bouton Enregistrer les réglages reste le seul geste qui applique le filtre. Un rejet du service conserve la catégorie enregistrée et le brouillon reste corrigeable. Sans identifiants, le champ explique comment activer la recherche et renvoie à Connexion Twitch.

Sur petit écran, les quatre destinations restent visibles et le contenu se lit sur une colonne. Au clavier, la navigation mène au titre de la destination choisie. L’application conserve le dernier relevé pendant une interruption et indique sa fraîcheur. Un premier chargement et une liste vide après vérification ont chacun un message distinct. Les retours d’échec restent à côté du geste concerné.

### Version 0.3.0

Le rail conserve Directs et Journal et donne une destination aux règles et une au courriel. Une règle possède un nom, une catégorie, une langue, des canaux et un calendrier. La liste rapproche chaque règle de sa jaquette, de son état et de ses canaux. Le formulaire occupe la page pour laisser la place à plusieurs plages. La modale native est réservée à la sélection dans le catalogue Twitch et remplace les suggestions intégrées de la 0.2.0. Le service fixe la limite du catalogue, la liste montre les huit premiers résultats.

Le calendrier de détection et celui du courriel se règlent séparément. Les jours sont ceux du début de la plage et deux horaires égaux représentent une journée entière. Pour un récapitulatif, les heures sont explicites et le choix du fuseau est visible. Les états de connexion décrivent la dernière vérification. Les champs d’identifiants apparaissent lors de la première connexion ou d’un remplacement. Le test d’envoi utilise la configuration enregistrée et l’aperçu utilise le brouillon.
