# Distribution Windows

## Fabriquer l’installeur

Utilisez un hôte Windows avec Rust MSVC, les outils C++ de Visual Studio et le SDK Windows. La CI cible Windows x64 avec Rust 1.99.1. Le script utilise la CLI Cargo Tauri installée ou télécharge la CLI 2.12.0 avec `npx` si Node.js est disponible. Pour installer la version Cargo utilisée pour cette distribution, exécutez `cargo install tauri-cli --version 2.12.0 --locked`.

```powershell
.\scripts\build-desktop.ps1
```

Le script compile `twitch-live-alert.exe` en profil release, le copie dans le répertoire de préparation Tauri avec le suffixe de la cible Rust, puis fabrique le paquet NSIS. Les deux binaires se trouvent côte à côte après installation. Rust, Node et le dépôt sont nécessaires uniquement à la construction. L’installeur télécharge le runtime Microsoft Edge WebView2 si Windows en a besoin. [Documentation Windows de Tauri](https://v2.tauri.app/distribute/windows-installer/)

L’installeur x64 se trouve dans `src-tauri/target/release/bundle/nsis/` avec la configuration Cargo par défaut. La CI conserve le fichier `*-setup.exe` dans l’artefact `Twitch-Live-Alert-Windows-x64-<commit>`. Les promotions `dev → test → main` et le téléchargement des artefacts restent indépendants des GitHub Releases. [Parcours de contribution](../CONTRIBUTING.md)

## Installer et utiliser

L’installation concerne le compte Windows courant. Par défaut, NSIS installe les exécutables sous `%LOCALAPPDATA%\Twitch Live Alert\`. La fenêtre et le navigateur accèdent au même service sur `http://127.0.0.1:37219`.

- Fermez la fenêtre pour laisser la surveillance tourner dans la zone de notification. Un clic sur l’icône ou un second lancement rouvre le tableau de bord.
- Dans le menu de l’icône, **Ouvrir dans le navigateur** ouvre le tableau de bord dans le navigateur par défaut. Les liens vers les chaînes et la console Twitch s’y ouvrent aussi.
- **Quitter** ferme le bureau et arrête le service que cette instance a lancé. Un service démarré séparément en console depuis le même exécutable reste actif. Un autre binaire déjà à l’écoute sur ce port déclenche un message avec son PID.
- Le réglage de démarrage Windows crée pour cette installation la valeur `TwitchLiveAlert` dans `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Elle lance le bureau avec `--background`.

Le lanceur vérifie dans Windows le PID à l’écoute, le chemin du binaire et la session. Il confronte ces informations à l’identité déclarée par `/api/status` avant de charger l’interface. Les pages du service accèdent à l’API HTTP locale. Le lanceur garde les commandes natives de fenêtre et de zone de notification côté Rust.

## Données du compte

| Emplacement dans `%LOCALAPPDATA%\TwitchLiveAlert\` | Contenu |
| --- | --- |
| `data/settings.json` | Réglages et filtres. |
| `data/alerts.sqlite3` | Historique des alertes. |
| `data/twitch.dpapi` | Identifiants Twitch protégés par Windows pour ce compte. |
| `webview/` | Profil du navigateur intégré. |
| `desktop-startup.log` | Dernier diagnostic d’échec au démarrage du bureau. |

La désinstallation standard retire les binaires et l’entrée de démarrage associée à ce chemin. Le profil de surveillance conserve l’historique et les réglages pour une réinstallation. Pour archiver la base, quittez d’abord le service afin que SQLite termine ses écritures.

### Reprendre une installation Bloodborne FR Live

1. Repérez le dépôt de l’ancien service, son profil `data/` et ses raccourcis `Bloodborne FR Live.lnk` dans le Bureau et le dossier Démarrage. Vérifiez leur cible dans les propriétés Windows pour identifier le lanceur qui surveille déjà le port.
2. Quittez l’ancien service avec son propre script `scripts\stop.ps1`. Conservez une copie du dossier `data/` et des anciens raccourcis. Si vous avez défini un `DATABASE_PATH` particulier, sauvegardez également cette base.
3. Avant d’ouvrir la nouvelle application, copiez la base arrêtée et les réglages souhaités dans `%LOCALAPPDATA%\TwitchLiveAlert\data\`. Pour une base SQLite avec des fichiers `-wal` ou `-shm`, conservez l’ensemble cohérent. Si le profil cible existe, sauvegardez-le séparément avant de choisir les fichiers à reprendre.
4. Au premier lancement, configurez les identifiants dans les réglages. Une migration administrée peut aussi appeler `twitch-live-alert.exe --import-credentials <profil>` depuis l’environnement de l’ancien service, sous le même compte Windows. Le profil désigne ici `%LOCALAPPDATA%\TwitchLiveAlert`. L’import valide les identifiants auprès de Twitch puis enregistre le fichier DPAPI du nouveau profil.
5. Vérifiez la catégorie, la langue et l’historique dans le nouveau tableau de bord. Déplacez l’ancien raccourci du dossier Démarrage vers votre sauvegarde, puis activez le démarrage Windows de la version installée si vous le souhaitez. Les nouveaux scripts de développement créent leurs propres raccourcis `Twitch Live Alert.lnk`.

Pour revenir à l’ancien moniteur, quittez le bureau, désactivez son démarrage automatique, puis restaurez le raccourci et démarrez l’ancien lanceur depuis son dépôt d’origine.

## Tester une candidate

Après une première construction, le binaire de service est prêt pour les contrôles natifs.

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Le smoke test installe la candidate, vérifie le refus d’un port occupé, démarre le service sans identifiants, vérifie l’accès navigateur, ouvre la fenêtre, la ferme en zone de notification, la rouvre et désinstalle le logiciel. Utilisez un compte de test disponible, sans installation Twitch Live Alert existante.

```powershell
$installer = Get-ChildItem 'src-tauri/target/release/bundle/nsis/*-setup.exe' | Select-Object -First 1
.\scripts\smoke-install.ps1 -Installer $installer.FullName -PortNumber 47219
```

Le test lance les processus dans un profil temporaire. Pour un essai manuel isolé, `TWITCH_LIVE_ALERT_DATA_DIR` accepte un chemin absolu et `TWITCH_LIVE_ALERT_PORT` un numéro de port de 1 à 65535. L’écoute reste sur `127.0.0.1`. Le test du choix de catégorie ou d’une alerte Twitch réelle nécessite ensuite des identifiants fournis par la personne qui valide la candidate.
