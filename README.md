# Alerte Bloodborne FR Live

Recevez une notification Windows quand un direct Twitch apparaît dans la
catégorie **Bloodborne** avec le français comme langue déclarée. Le programme
surveille toutes les chaînes remontées par Twitch, y compris celles qui
changent de catégorie pendant leur direct. Cliquez sur la notification
**Bloodborne FR Live** pour ouvrir la chaîne dans votre navigateur.

La surveillance s’exécute dans votre session sur Windows 10 ou 11. Elle
s’interrompt si le PC est éteint ou en veille, ou si la session est fermée.
Le programme interroge Twitch toutes les 60 secondes par défaut. Un direct
très court peut passer entre deux relevés. Le classement par audience peut
également changer pendant la pagination et retarder la détection d’un direct.
Le programme utilise des connexions HTTPS sortantes.

## Ouvrir l’application sous Windows

Installez [Rust via rustup](https://rustup.rs/) et les [outils C++ de Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
avec la charge de travail **Développement Desktop en C++**. Depuis
PowerShell, à la racine du dépôt, compilez et installez le raccourci.

```powershell
cargo build --locked --release
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\scripts\install-desktop.ps1
```

Double-cliquez sur **Bloodborne FR Live** sur le Bureau. Le raccourci lance
le programme en arrière-plan et ouvre le tableau de bord dans votre navigateur.
Si le programme tourne déjà, le raccourci ouvre le même tableau de bord.
Vous pouvez aussi lancer `scripts/start.ps1` depuis PowerShell.

[Tableau de bord local](http://127.0.0.1:37219)

Le tableau de bord affiche l’état de la surveillance et les directs détectés,
permet de modifier les réglages locaux et propose une notification de test.
Vous pouvez l’ouvrir avant de déposer les identifiants Twitch. La surveillance
des directs commence une fois les identifiants disponibles et le programme relancé.
Fermer l’onglet du navigateur laisse la surveillance en cours.

## Configurer Twitch

1. Vérifiez l’adresse e-mail de votre compte Twitch et activez
   l’authentification à deux facteurs.
2. Créez une application depuis la [console développeur Twitch](https://dev.twitch.tv/console/apps/create).
   Choisissez un nom disponible, la catégorie **Application Integration** et
   le type de client **Confidential**. Ajoutez `http://localhost` aux URL de
   redirection du formulaire. Le programme utilise le flux `client_credentials`.
3. Ouvrez la gestion de l’application. Relevez le **Client ID** et créez un
   **Client Secret** avec **New Secret**.

[Inscription d’une application Twitch](https://dev.twitch.tv/docs/authentication/register-app) · [Flux client credentials](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/#client-credentials-grant-flow)

### Enregistrer les identifiants dans le coffre

Dans Git Bash, avec la commande `coffre` installée, copiez chaque valeur
dans le presse-papier juste avant d’exécuter la ligne correspondante.
Le coffre reçoit la valeur via l’entrée standard.

```bash
powershell.exe -NoProfile -Command Get-Clipboard | coffre definir alerte-bloodborne-frlive .env TWITCH_CLIENT_ID
powershell.exe -NoProfile -Command Get-Clipboard | coffre definir alerte-bloodborne-frlive .env TWITCH_CLIENT_SECRET
coffre deployer alerte-bloodborne-frlive
```

Le coffre dépose `.env` à la racine du dépôt. Pour viser un autre checkout,
ajoutez `--dans /chemin/du/depot` à `coffre deployer alerte-bloodborne-frlive`.
Après une rotation du secret, redéployez le fichier, puis arrêtez et relancez
la surveillance.

## Vérifier la notification Windows

Utilisez le bouton de test du tableau de bord ou lancez cette commande
depuis la racine du dépôt.

```powershell
.\target\release\alert-bloodborne-fr-live.exe --test-notification
```

Le test affiche une notification locale sans identifiants ni appel à Twitch.
Vérifiez la bannière, le centre de notifications et l’ouverture de Twitch
au clic. Si la bannière est masquée, vérifiez les notifications pour
**Bloodborne FR Live** dans **Paramètres › Système**. Sous Windows 11,
**Ne pas déranger** peut envoyer l’alerte directement au centre de
notifications. Sous Windows 10, vérifiez l’**Assistant de concentration**.
[Réglages des notifications Windows](https://support.microsoft.com/fr-fr/windows/experience/notifications-and-do-not-disturb-in-windows)

## Arrêter la surveillance et consulter les journaux

Le bouton **Mettre en pause** suspend les relevés Twitch. Le bouton
**Reprendre** les réactive. Pour fermer l’application, lancez le script
d’arrêt depuis la racine du dépôt.

```powershell
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\scripts\stop.ps1
```

Le script demande un arrêt propre au programme. Si le programme reste
bloqué, le script termine uniquement le processus qu’il identifie comme
lancé depuis ce dépôt. Pour un lancement direct dans PowerShell, vous
pouvez aussi utiliser `Ctrl+C` dans sa console.

Consultez la sortie normale dans `data/watcher.stdout.log` et les erreurs
standard dans `data/watcher.stderr.log`. Ces fichiers grossissent pendant
l’exécution et sont remplacés à chaque lancement du binaire par le script.
En cas de démarrage impossible, consultez aussi `data/watcher.startup.log`,
qui conserve la dernière erreur du lanceur. Pour archiver les journaux,
copiez-les avant une relance.

### Démarrage à l’ouverture de votre session

Activez le démarrage automatique depuis le tableau de bord ou utilisez
les scripts suivants dans votre compte Windows. Ils ajoutent ou retirent
le raccourci dans le dossier **Démarrage**. À la prochaine connexion,
Windows lance la surveillance en arrière-plan sans ouvrir le navigateur.

```powershell
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\scripts\install-startup.ps1
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\scripts\uninstall-startup.ps1
```

Retirer le raccourci laisse la surveillance en cours. Utilisez `stop.ps1`
pour l’arrêter. Si vous déplacez le dépôt, retirez le raccourci avant le
déplacement puis réinstallez-le depuis le nouvel emplacement. Supprimez
également l’ancien raccourci Bureau avant de le recréer.

## Commandes et réglages

Exécutez les commandes depuis la racine du dépôt.

```powershell
.\target\release\alert-bloodborne-fr-live.exe --check-config
.\target\release\alert-bloodborne-fr-live.exe --dry-run
.\target\release\alert-bloodborne-fr-live.exe --once
.\target\release\alert-bloodborne-fr-live.exe --no-web
```

- `--check-config` valide les paramètres sans réseau ni écriture dans SQLite.
- `--dry-run` interroge Twitch et liste les directs détectés sans notification
  ni modification de la base.
- `--once` exécute un cycle normal avec notifications et écritures SQLite, puis
  quitte. Arrêtez la surveillance régulière avant d’utiliser cette commande
  sur la même base.
- `--no-web` exécute la surveillance dans le terminal sans tableau de bord.

| Variable | Valeur par défaut | Utilisation |
| --- | --- | --- |
| `TWITCH_CLIENT_ID` | Obligatoire pour Twitch | Identifiant de l’application Twitch. |
| `TWITCH_CLIENT_SECRET` | Obligatoire pour Twitch | Secret de l’application Twitch. |
| `POLL_INTERVAL_SECONDS` | `60` | Intervalle en secondes, de `15` à `3600`. |
| `DATABASE_PATH` | `data/alerts.sqlite3` | Chemin de la base locale SQLite. |
| `NOTIFY_EXISTING_ON_FIRST_RUN` | `true` | Avec `false`, le premier relevé réussi sur une base neuve sert de référence. Les directs déjà présents restent silencieux. |
| `RUST_LOG` | `info` | Niveau de journalisation. |

Les réglages enregistrés depuis le tableau de bord se trouvent dans
`data/settings.json`. Les identifiants Twitch restent dans le fichier `.env`
déposé par le coffre.

## Historique des alertes

La base SQLite conserve l’identifiant de chaque direct dont la notification
a été acceptée par Windows. Elle évite d’annoncer le même direct après un
redémarrage. En cas d’échec de la notification, le programme réessaie si
le direct est encore présent au relevé suivant. Une interruption entre la
remise à Windows et l’écriture SQLite peut provoquer un doublon. Conservez
la base pour garder cet historique.

Le programme résout l’identifiant de Bloodborne puis interroge Twitch avec
le filtre `language=fr`. Il espace les tentatives après une erreur réseau
ou API. [Référence officielle de l’API Twitch](https://dev.twitch.tv/docs/api/reference/streams#get-streams)

## Tests

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Les tests automatisés utilisent des substituts locaux pour les API.
`--test-notification` vérifie l’affichage dans une session Windows interactive.
