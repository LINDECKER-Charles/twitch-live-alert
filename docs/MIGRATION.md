# Reprendre une installation Bloodborne FR Live

La version 0.1 conservait les réglages et le journal dans le dossier `data` du dépôt. La version installée de Twitch Live Alert utilise un profil propre à la session Windows. Les deux programmes utilisent le port local 37219. Prévoyez une courte interruption pour effectuer la bascule.

## Sauvegarder le journal

1. Notez l’intervalle, l’état de la surveillance et le choix de démarrage automatique dans l’ancien tableau de bord.
2. Quittez l’ancien service avec son script `scripts/stop.ps1`, depuis l’ancienne version du dépôt. Fermez également Twitch Live Alert s’il a déjà été lancé.
3. Copiez le dossier `data` d’origine dans un dossier de sauvegarde. Conservez notamment `alerts.sqlite3`, ses éventuels fichiers `-wal` et `-shm`, ainsi que `settings.json`. Ces fichiers forment un instantané cohérent une fois le service arrêté.
4. Dans un **profil neuf** de Twitch Live Alert, créez `%LOCALAPPDATA%\TwitchLiveAlert\data` et copiez-y ces fichiers SQLite et `settings.json`. Si le profil contient déjà un journal, conservez les deux profils séparément. Écraser une base supprimerait ses propres alertes.

À la première ouverture, le service met à niveau **la copie** de la base. Le filtre de l’ancien moniteur reste Bloodborne en français. Les identifiants de directs déjà enregistrés restent disponibles pour la déduplication.

## Reprendre la connexion Twitch

Sur une installation neuve, ouvrez **Connexion Twitch** pour saisir votre paire d’identifiants. Si vous avez conservé la configuration de la version 0.1, vous pouvez également l’importer depuis une console ouverte dans **l’ancien dépôt**. Pour une installation à l’emplacement proposé par défaut, utilisez la commande suivante.

```powershell
& "$env:LOCALAPPDATA\Twitch Live Alert\twitch-live-alert.exe" --import-credentials "$env:LOCALAPPDATA\TwitchLiveAlert"
```

Le service lit les identifiants depuis l’environnement ou le fichier `.env` du dossier courant, les vérifie auprès de Twitch, puis les enregistre pour l’utilisateur Windows dans le profil cible. Conservez le fichier de développement dans son emplacement d’origine. Pour ce dépôt, la commande `coffre deployer alerte-bloodborne-frlive` sous Git Bash permet de le rétablir depuis le coffre chiffré.

## Vérifier la reprise

1. Ouvrez Twitch Live Alert et contrôlez le filtre, le délai et l’état de la surveillance.
2. Ouvrez **Journal** et recherchez une chaîne de votre ancien historique. Patientez jusqu’au premier relevé réussi ou lancez une vérification.
3. Si l’ancienne application démarrait avec Windows, activez ce réglage dans Twitch Live Alert. Déplacez ensuite le raccourci **Bloodborne FR Live** du dossier Démarrage (`shell:startup`) dans votre dossier de sauvegarde. Le raccourci Bureau de l’ancienne version peut lui aussi être déplacé après vérification de la nouvelle installation.

Pour revenir temporairement à la version 0.1, désactivez le démarrage automatique dans Twitch Live Alert, puis quittez l’application depuis la zone de notification. Restaurez l’ancien raccourci et relancez l’ancienne version depuis son dossier d’origine. Les alertes reçues uniquement après la bascule restent dans le nouveau journal.
