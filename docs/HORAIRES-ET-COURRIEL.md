# Horaires de détection et de courriel

[English](en/SCHEDULES-AND-EMAIL.md) · [Guide de démarrage](../README.md)

## Quand détecter un direct

Dans **Règles**, choisissez les jours et les heures pendant lesquels cette règle doit interroger Twitch. Sans plage, elle surveille toute la journée. La langue est celle déclarée par le diffuseur. Si deux règles couvrent le même direct, chacune peut déclencher ses propres canaux.

Le fuseau local suit le PC. Choisissez un fuseau nommé, par exemple `Europe/Paris`, pour conserver des horaires parisiens lorsque le PC change de fuseau. La fin d’une plage est exclue. Une plage du vendredi 22 h au samedi 2 h se configure en sélectionnant **vendredi** et en saisissant **22:00–02:00**. Des heures de début et de fin identiques désignent toute la journée sélectionnée.

Pendant une fermeture, le PC en veille ou l’arrêt du logiciel, aucun relevé n’a lieu pour la règle concernée. À la reprise, la règle considère les directs encore en ligne. Un direct qui a entièrement eu lieu pendant l’interruption ne pourra pas être retrouvé par cette surveillance. Le choix d’alerter au premier relevé détermine le traitement initial d’un filtre neuf. Le journal et la déduplication sont persistants.

## Quand envoyer le courriel

Activez le canal courriel sur la règle et choisissez l’un des trois modes. Les horaires de détection et ceux du courriel se règlent séparément.

| Mode | Effet |
| --- | --- |
| Immédiat | Une nouvelle détection rend le message immédiatement disponible pour l’envoi. |
| Immédiat dans des plages | Hors plage, une détection attend la prochaine ouverture. Un nouvel essai après une erreur respecte aussi la plage. |
| Récapitulatif | Les directs détectés pour cette règle sont regroupés au prochain horaire choisi. Les horaires fixes doivent se trouver dans les plages autorisées. |

Par exemple, surveillez le vendredi de 18 h à 23 h avec un récapitulatif à 9 h le samedi. La règle peut préparer des messages le vendredi et les envoyer le samedi sans chercher de nouveaux directs le samedi matin.

Un récapitulatif arrivé à échéance pendant l’arrêt du PC est traité à la reprise lorsque la règle, le canal et la surveillance sont actifs. Un changement d’heure qui supprime une heure de récapitulatif reporte son échéance au premier instant représentable suivant ce jour-là. Lorsqu’une heure apparaît deux fois, sa première occurrence déclenche le récapitulatif. Une livraison mise en attente est conservée dans SQLite. Après une interruption entre l’acceptation par le serveur SMTP et la confirmation locale, une nouvelle tentative peut envoyer un doublon avec le même `Message-ID`.

## Configurer le serveur

Dans **Courriel**, renseignez l’hôte, le port, le mode de chiffrement, l’expéditeur et les destinataires. Utilisez les paramètres fournis par votre messagerie. TLS implicite utilise généralement le port 465 et STARTTLS obligatoire le port 587. Les deux modes valident le certificat du serveur. L’identifiant et le mot de passe enregistrés sont protégés par Windows DPAPI. **Envoyer un test** transmet un vrai message aux destinataires configurés. **Aperçu du message** permet de contrôler votre modèle localement.

## Écrire un modèle

Les champs **Objet** et **Corps** sont en texte brut. Insérez une valeur entre doubles accolades, par exemple `{{user_name}}`. Une variable inconnue est signalée. Une valeur manquante devient un tiret long (`—`). Pour afficher une expression littérale, utilisez `\{{user_name}}`. En récapitulatif, le corps est évalué pour chaque direct. L’objet utilise le premier direct et indique le nombre de directs regroupés.

| Variables | Valeurs |
| --- | --- |
| `{{user_name}}`, `{{user_login}}`, `{{user_id}}` | Nom affiché, identifiant public de chaîne, identifiant numérique Twitch. |
| `{{stream_id}}`, `{{title}}`, `{{type}}` | Identifiant de diffusion, titre et type de diffusion. |
| `{{category}}`, `{{game_id}}`, `{{language}}` | Catégorie, identifiant de catégorie et langue déclarée. |
| `{{started_at}}`, `{{detected_at}}` | Début du direct et détection par l’application, en ISO 8601 UTC. |
| `{{viewer_count}}`, `{{tags}}`, `{{is_mature}}` | Audience au relevé, tags séparés par des virgules et indicateur de contenu adulte. |
| `{{url}}`, `{{thumbnail_url}}` | Lien vers la chaîne et URL de l’aperçu Twitch en 640 × 360. |
| `{{rule}}` | Nom de la règle ayant détecté le direct. |

Pour l’objet, essayez `[{{rule}}] {{user_name}} est en direct`. Pour le corps, essayez `{{title}} — {{category}} ({{language}}) · {{viewer_count}} spectateurs. {{url}}`.

