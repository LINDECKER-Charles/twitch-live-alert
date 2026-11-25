# Analytics

La personne qui surveille Twitch sur son PC vient ici comparer la fréquence des directs des chaînes détectées, puis retrouver les sessions qui expliquent une moyenne. La destination `#analytics` conserve le rail, le titre de page et les panneaux graphite de Directs et du Journal.

## Choix du 25 novembre 2026

- Les filtres **Période** et **Chaîne** utilisent des listes natives. La période initiale couvre 30 jours. Les autres choix couvrent 7 jours, 90 jours et tout l’historique.
- La vue d’ensemble rapproche le nombre de directs, leur fréquence hebdomadaire et leurs durées moyenne et cumulée observées. Le comparatif des chaînes ouvre le même écran filtré. Il présente d’abord les dix chaînes ayant le plus de directs, puis « Afficher la suite » en ajoute dix. Le filtre Chaîne donne accès à toute la période. « Toutes les chaînes » permet de retrouver le comparatif.
- Sept colonnes donnent le nombre de directs par jour de la semaine, à l’heure du PC. Le jour est celui du début Twitch ou, pour les anciennes entrées sans date de début, celui de la première trace. Chaque valeur reste lisible en chiffres. Le panneau voisin compte séparément les notifications Windows et courriel associées aux directs retenus.
- L’historique se lit par pages de 25 directs, taille fixée par le service local. Il donne la date, la chaîne, le titre, la durée observée et le pic d’audience relevé quand les mesures existent.

## États et accessibilité

Un chargement lié à une nouvelle sélection masque les chiffres de la sélection précédente. Une actualisation de la même sélection conserve les chiffres disponibles et le focus. Une erreur ou un refus du service propose « Réessayer ». Une sélection vide propose d’élargir la période, et un historique entièrement vide explique que les relevés enrichiront les statistiques.

La navigation au clavier suit les deux filtres, les boutons des chaînes, puis la pagination. Ouvrir une chaîne place le focus sur le filtre qui porte son nom. Revenir depuis le détail restitue le focus à cette chaîne dans le comparatif. Sur petit écran, les panneaux s’empilent et les longues lignes de données se déploient verticalement.

## Vocabulaire

| Libellé | Calcul ou donnée |
| --- | --- |
| Directs détectés | Sessions distinctes enregistrées par la surveillance ou retrouvées dans le journal local. |
| Fréquence | Nombre de directs ramené à sept jours sur la période sélectionnée. « Tout l’historique » utilise une période commune à toutes les chaînes, du premier direct connu à aujourd’hui. |
| Durée moyenne observée | Moyenne des durées mesurées du début Twitch au dernier relevé en direct. |
| Durée cumulée observée | Somme de ces intervalles pour les directs mesurés de la sélection. |
| Durée inconnue | Les traces disponibles ne permettent pas de mesurer une durée. Ces directs restent exclus de la moyenne. |
| Première trace | Date locale de rattachement lorsque Twitch n’a pas fourni de date de début. |
| Pic observé | Plus grand nombre de spectateurs relevé pendant le direct. |
