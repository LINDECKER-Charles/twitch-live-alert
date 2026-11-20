# Alerte Bloodborne FR Live

Service Rust qui envoie un message dans Discord quand une diffusion apparaît
dans la catégorie **Bloodborne** avec le français comme langue de diffusion
sur Twitch. Il parcourt toutes les pages de résultats accessibles via Twitch, toutes chaînes
confondues. L’alerte couvre aussi une chaîne déjà en direct qui entre dans la
catégorie. Twitch fournit la langue déclarée par le diffuseur.

Par défaut, le service interroge Twitch toutes les 60 secondes. En fonctionnement
normal, comptez jusqu’à environ une minute d’attente, plus la latence de Twitch
et de Discord. Un direct qui commence et se termine entre deux interrogations
peut passer inaperçu. Twitch peut aussi omettre temporairement un direct pendant la pagination si le classement par audience change.
Le service utilise des connexions HTTPS sortantes et
n’expose aucun port.

## Créer les accès

### Twitch

1. Vérifiez l’adresse e-mail du compte Twitch et activez l’authentification à deux facteurs.
2. Inscrivez une application dans la [console Twitch](https://dev.twitch.tv/console/apps/create).
   Choisissez un nom disponible, la catégorie **Application Integration** (ou
   son équivalent) et le type de client **Confidential**. Ajoutez
   `http://localhost` dans les URL de redirection du formulaire. Le service
   s’authentifie avec `client_credentials`, un flux sans redirection ni
   autorisation individuelle des chaînes.
3. Dans **Manage**, récupérez le **Client ID** et générez le **Client Secret** avec
   **New Secret**. Utilisez les variables `TWITCH_CLIENT_ID` et `TWITCH_CLIENT_SECRET`.

[Documentation Twitch sur l’inscription](https://dev.twitch.tv/docs/authentication/register-app) · [Flux client credentials](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/#client-credentials-grant-flow)

### Discord

Ouvrez **Paramètres du serveur › Intégrations › Webhooks**, créez un webhook pour
un salon textuel et copiez son URL complète. Pour valider l’installation avec
`--test-notification`, vous pouvez utiliser un salon privé. L’URL complète donne
le droit de publier via ce webhook. Conservez-la dans `DISCORD_WEBHOOK_URL`.

[Documentation Discord sur les webhooks](https://support.discord.com/hc/fr/articles/228383668-Intro-aux-Webhooks)

Les messages du service sont publiés sans mention. Pour recevoir une alerte à
chaque publication, sélectionnez **Tous** ou **Tous les messages** dans les
paramètres de notification du salon. Sur téléphone, activez les notifications
push pour le serveur et autorisez les notifications Discord dans les réglages
d’iOS ou d’Android.

[Notifications de salon](https://support.discord.com/hc/fr/articles/215253258-Param%C3%A8tres-de-notification-101) · [Notifications mobiles](https://support.discord.com/hc/fr/articles/218892547--Mobile-Param%C3%A8tres-de-notifications-101)

### Enregistrer les valeurs dans le coffre

Sur Windows, lancez ces commandes dans Git Bash avec la commande `coffre`
installée. Avant chaque ligne, copiez uniquement la valeur correspondante
dans le presse-papier. La valeur arrive dans le coffre via l’entrée standard.

```bash
powershell.exe -NoProfile -Command Get-Clipboard | coffre definir alerte-bloodborne-frlive .env TWITCH_CLIENT_ID
powershell.exe -NoProfile -Command Get-Clipboard | coffre definir alerte-bloodborne-frlive .env TWITCH_CLIENT_SECRET
powershell.exe -NoProfile -Command Get-Clipboard | coffre definir alerte-bloodborne-frlive .env DISCORD_WEBHOOK_URL
coffre deployer alerte-bloodborne-frlive
```

Pour déposer la configuration dans un autre checkout autorisé, ajoutez
`--dans /chemin/du/depot` à `coffre deployer alerte-bloodborne-frlive`.
Après une rotation, redéployez les valeurs et recréez le conteneur s’il tourne
déjà.

## Démarrer avec Docker Compose

Exécutez ces commandes depuis le dépôt, après avoir déposé le fichier `.env`
avec le coffre. Utilisez Docker Engine ou Docker Desktop avec Docker Compose
et des conteneurs Linux.

```sh
docker compose build
docker compose run --rm watcher --check-config
docker compose run --rm watcher --dry-run
docker compose run --rm watcher --test-notification
docker compose up -d
docker compose logs -f watcher
```

`--check-config` effectue une validation locale. `--dry-run` interroge Twitch
et liste les directs détectés sans publier ni modifier l’état. La commande
`--test-notification` publie un vrai message de test dans le salon Discord.
Ces vérifications peuvent être exécutées séparément.

Le service tourne avec l’UID non privilégié `10001`. Compose monte le volume
nommé `watch-state` dans `/app/data` et fixe le chemin de la base à
`/app/data/alerts.sqlite3`. Le volume est conservé par `docker compose down`
et lors d’une reconstruction. `docker compose down -v` efface la base et
l’historique des alertes. Exécutez une seule instance du service par volume.

Pour arrêter le service, utilisez `docker compose stop`. Lors d’un arrêt, le service
termine l’envoi Discord déjà lancé et enregistre sa confirmation éventuelle
avant de quitter. Compose lui laisse jusqu’à 60 secondes. Pour appliquer un
changement de configuration déposé par le coffre, utilisez
`docker compose up -d --force-recreate`.

## Démarrer avec Cargo

Installez Rust avec [rustup](https://rustup.rs/). Le conteneur et la CI utilisent
Rust 1.99.1. Lancez les commandes depuis le dépôt, avec le fichier `.env`
déposé à sa racine.

```sh
cargo run --locked -- --check-config
cargo run --locked -- --dry-run
cargo run --locked -- --test-notification
cargo run --locked
```

`cargo run --locked -- --once` effectue un seul cycle avec les envois et la
mise à jour SQLite habituels. Pour le faire sous Compose, arrêtez d’abord le
service, puis exécutez `docker compose run --rm watcher --once`.

## Configuration

| Variable | Valeur par défaut | Utilisation |
| --- | --- | --- |
| `TWITCH_CLIENT_ID` | Obligatoire pour Twitch | Identifiant de l’application Twitch. |
| `TWITCH_CLIENT_SECRET` | Obligatoire pour Twitch | Secret de l’application Twitch. |
| `DISCORD_WEBHOOK_URL` | Obligatoire pour Discord | URL complète du webhook Discord. Facultative pour `--dry-run`. |
| `POLL_INTERVAL_SECONDS` | `60` | Intervalle en secondes, de `15` à `3600`. |
| `DATABASE_PATH` | `data/alerts.sqlite3` | Chemin local de la base SQLite. Compose fixe un chemin dans son volume. |
| `NOTIFY_EXISTING_ON_FIRST_RUN` | `true` | Avec `false`, le premier relevé réussi sert de référence et seuls les nouveaux directs des cycles suivants sont annoncés. |
| `RUST_LOG` | `info` | Niveau de journalisation. |

Pour faire le premier relevé en mode silencieux sous Docker, définissez
`NOTIFY_EXISTING_ON_FIRST_RUN=false` avant le premier démarrage du service.
Le réglage s’applique à l’initialisation d’une base neuve. Sous Docker, les
réglages facultatifs peuvent être définis sous `environment` dans Compose.
En local, utilisez des variables d’environnement.

## Comportement des alertes

Le service résout l’identifiant Twitch de Bloodborne avec **Get Games**, puis
interroge **Get Streams** avec cet identifiant et le filtre `language=fr`. Il
suit la pagination et revalide toutes les heures le jeton d’application auprès
de Twitch. En cas d’erreur réseau ou de réponse d’API inattendue, le service
espace ses tentatives.

La base SQLite mémorise les identifiants des directs après confirmation de
l’envoi par Discord. Le même direct conserve donc son statut après un
redémarrage. Si un envoi échoue, il est retenté au cycle suivant si le direct
est toujours présent. Une interruption entre l’acceptation du message par
Discord et l’écriture dans SQLite peut exceptionnellement provoquer un doublon.
Si une opération SQLite échoue, le processus s’arrête. Compose applique alors sa
politique de redémarrage. En cas d’erreurs SQLite répétées, arrêtez le conteneur le
temps de corriger le problème de stockage.

[Référence officielle de l’API Twitch](https://dev.twitch.tv/docs/api/reference/streams#get-streams)

## Tests

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

La CI GitHub exécute ces vérifications sous Linux et Windows. Les tests
automatisés utilisent des substituts locaux aux API et ne requièrent pas de
véritables identifiants.
