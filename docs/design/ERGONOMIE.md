# Ergonomie de la console 0.3.0

[English overview](../en/INTERFACE.md)

| Destination | Tâche et contrôles |
| --- | --- |
| Directs | Lire l’état de la surveillance, demander un relevé, mettre en pause ou reprendre. Chaque ligne nomme la règle du direct, la chaîne, le titre, l’heure de début et l’audience disponible. « Gérer les règles » ouvre la configuration. |
| Journal | Rechercher une chaîne ou un titre dans l’historique local. Chaque alerte indique sa règle et son canal. Une page contient au plus 50 entrées. |
| Règles | Créer, modifier, désactiver, réactiver ou supprimer une règle identifiée par son nom. La suppression demande confirmation sur sa ligne. Le formulaire de règle occupe la page et « Enregistrer la règle » applique le brouillon. |
| Courriel | Lire l’état des envois automatiques, paramétrer le serveur, l’expéditeur et les destinataires, préparer les modèles et les essayer. L’aperçu utilise un direct d’exemple. « Envoyer un test » contacte les destinataires enregistrés. Un échec en file indique la règle concernée et la prochaine tentative quand le service la connaît. |
| Réglages | Configurer l’intervalle de 15 à 3 600 secondes, le comportement au premier relevé et le démarrage Windows. Tester la notification Windows sur ce PC. |
| Connexion Twitch | Au premier lancement, saisir les identifiants d’une application Twitch. Après vérification, lire l’état et sa date, vérifier, remplacer les identifiants ou déconnecter. Le départ de la page efface les champs sensibles. |

La modale **Choisir une catégorie** interroge Twitch après une pause de saisie et présente au plus huit résultats illustrés. Le nom reste visible si la jaquette manque. Les flèches déplacent le focus, Entrée choisit la catégorie, Échap ferme la modale et rend le focus au bouton d’ouverture. Le chargement, la recherche vide et l’erreur du catalogue donnent un retour dans la modale.

Les plages de **détection** déterminent quand interroger Twitch pour la règle. Le canal **Courriel** a son propre calendrier et son propre fuseau. Le mode immédiat, les plages d’envoi et le récapitulatif à heures fixes sont exclusifs. Le jour coché est celui du début d’une plage nocturne. Deux heures identiques couvrent le jour civil coché. Le récapitulatif permet de choisir directement les jours d’envoi.

La liste affiche séparément le programme de détection et celui du courriel. Un horaire qui dépasse minuit porte la mention « +1 j ». Un résumé repliable donne accès à toutes les plages ou heures supplémentaires. « Activée » qualifie le choix enregistré. « Dans la plage de détection » et « Hors plage de détection » sont calculés d’après le fuseau de la règle.

Le rail garde la destination courante visible. Sur mobile, les six destinations se répartissent sur deux rangées. Le changement de page déplace le focus sur le titre. La navigation au clavier, le retour du navigateur et le lien d’évitement fonctionnent sur l’ensemble des destinations. Lors d’une interruption du service, la date et la liste du dernier relevé restent lisibles. [Fiche de parcours](fiches/surveillance.md) · [Captures et vérifications](PLANCHE.md)
