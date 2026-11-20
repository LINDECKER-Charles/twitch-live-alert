# Surveillance Twitch

**Route** `/` · **Sources** `web/` · **Surface** locale sur le PC qui exécute la surveillance.

Le joueur consulte les directs, vérifie la réception d’une notification Windows et modifie les paramètres à l’installation ou selon ses besoins. Le nombre de chaînes simultanées reste inconnu avant une première mesure et les listes acceptent plusieurs lignes sans limite visuelle arbitraire. L’interface utilise le thème sombre du projet.

| Zone | Choix |
|---|---|
| Surveillance | Bloc d’état en haut de page, avec la dernière vérification, la prochaine et des commandes immédiates. |
| Directs | Liste de résultats, avec un lien par chaîne valide. |
| Réglages | Formulaire intégré à la page. Le champ numérique reçoit un intervalle en secondes. Deux cases représentent les options indépendantes. Le bouton Enregistrer applique les changements ensemble. |
| Notifications récentes | Historique en bas de page, avec les mêmes libellés de chaîne et des dates. |

| Situation | Réponse |
|---|---|
| Chargement initial | État de connexion et commandes indisponibles. |
| Service local inaccessible | Message de connexion et nouvelle tentative automatique, avec conservation de la dernière vue. |
| Twitch à configurer | Indication dans l’état et dans la liste des directs. |
| Surveillance en pause | Commande Reprendre et dernière liste datée si disponible. |
| Liste vide | Message selon la disponibilité et la fraîcheur des données. Aucun filtre sur cette page. |
| Échec d’un geste | Retour visible près des commandes ou du formulaire. |
| Enregistrement | Le rafraîchissement préserve les brouillons. Le bouton signale la progression puis la confirmation. |
| 390 px | Une colonne et des commandes de 44 px minimum. |
| Clavier | Ordre du document, focus visible, contrôles natifs et retours annoncés. |

Décision du 20 novembre 2026. La page combine l’état et les réglages dans une seule vue pour permettre de vérifier l’effet d’une action sans naviguer.
