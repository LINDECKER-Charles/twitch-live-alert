# Distribution Windows

[English](en/DISTRIBUTION.md)

## Télécharger la version 0.3.0

[Télécharger l’installateur pour Windows 10 et 11 (x64)](https://github.com/LINDECKER-Charles/twitch-live-alert/releases/download/v0.3.0/Twitch-Live-Alert_0.3.0_x64-setup.exe)

L’installateur est non signé. Pour vérifier le fichier, ouvrez PowerShell dans le dossier de téléchargement et comparez le résultat de la commande ci-dessous à l’empreinte SHA-256 publiée sur la [page de la version 0.3.0](https://github.com/LINDECKER-Charles/twitch-live-alert/releases/tag/v0.3.0).

```powershell
Get-FileHash -Algorithm SHA256 .\Twitch-Live-Alert_0.3.0_x64-setup.exe
```

## Fabriquer l’installeur

Utilisez un hôte Windows avec Rust MSVC, les outils C++ de Visual Studio et le SDK Windows. La CI cible Windows x64 avec Rust 1.99.1. Le script utilise la CLI Cargo Tauri installée ou télécharge la CLI 2.12.0 avec `npx` si Node.js est disponible. Pour installer la version Cargo utilisée pour cette distribution, exécutez `cargo install tauri-cli --version 2.12.0 --locked`.

```powershell
.\scripts\build-desktop.ps1 -PublicRelease
```

Le script compile `twitch-live-alert.exe` en profil release, le copie dans le répertoire de préparation Tauri avec le suffixe de la cible Rust, puis fabrique le paquet NSIS. L’option de publication remplace les chemins locaux de compilation et retire le répertoire du fichier PDB dans les exécutables. Les deux binaires se trouvent côte à côte après installation. Rust, Node et le dépôt sont nécessaires uniquement à la construction. L’installeur télécharge le runtime Microsoft Edge WebView2 si Windows en a besoin. [Documentation Windows de Tauri](https://v2.tauri.app/distribute/windows-installer/)

Après une construction locale, l’installeur x64 se trouve dans `src-tauri/target/release/bundle/nsis/` avec la configuration Cargo par défaut. La CI du dépôt public teste le paquet dans l’espace temporaire du runner. [Parcours de contribution](../CONTRIBUTING.md)

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
| `data/alerts.sqlite3` | Journal et état des livraisons, y compris les courriels en attente. |
| `data/twitch.dpapi` | Identifiants Twitch protégés par Windows pour ce compte. |
| `data/mail.json` | Configuration SMTP, modèles et authentification protégée par DPAPI. |
| `webview/` | Profil du navigateur intégré. |
| `desktop-startup.log` | Dernier diagnostic d’échec au démarrage du bureau. |

La désinstallation standard retire les binaires et l’entrée de démarrage associée à ce chemin. Le profil de surveillance conserve l’historique et les réglages pour une réinstallation. Pour archiver la base, quittez d’abord le service afin que SQLite termine ses écritures.

### Mettre à niveau une installation

La 0.3.0 convertit le filtre de la 0.2.0 en règle. Le guide de migration couvre aussi la reprise du journal du moniteur Bloodborne FR Live, les sauvegardes cohérentes et le retour à une version antérieure. [Migrer le profil Windows](MIGRATION.md)

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
