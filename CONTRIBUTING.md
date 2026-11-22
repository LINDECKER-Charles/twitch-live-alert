# Contribuer à Twitch Live Alert

[English](docs/en/CONTRIBUTING.md)

## Branches

| Branche | Usage |
| --- | --- |
| `dev` | Intégration des fonctionnalités et corrections. Les branches de travail partent de `dev` et leurs pull requests la ciblent. |
| `test` | Candidate à installer. Une pull request de `dev` vers `test` rassemble les changements à valider ensemble. |
| `main` | Version installable. Une candidate vérifiée sur Windows est promue de `test` vers `main`. |

Décrivez le comportement attendu dans la pull request et indiquez les vérifications exécutées. Pour une modification visuelle, joignez une capture du parcours concerné. Les identifiants Twitch personnels, bases de données et fichiers de profil restent sur votre machine.

## Organisation du projet

| Répertoire | Responsabilité |
| --- | --- |
| `src/` et `tests/` | Client Twitch, surveillance, notifications et API HTTP locale en Rust. |
| `web/` | Tableau de bord HTML, CSS et JavaScript embarqué dans le service. |
| `src-tauri/` | Fenêtre Windows, zone de notification, cycle de vie du service et configuration NSIS. |
| `scripts/` | Démarrage depuis les sources, construction et test de l’installeur. |
| `docs/` | Documentation technique et références d’interface. |

Les deux manifestes Rust ont chacun leur `Cargo.lock`. Respectez les scopes de `AGENTS.md` pour les changements par composant. Utilisez `desktop` pour le lanceur natif.

## Vérifications

Depuis la racine, avec Rust 1.99.1 et Node.js 24, le service et son API se vérifient ainsi.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
node --test tests/smoke-dashboard.mjs
```

Les parcours du tableau de bord se vérifient dans Chromium avec des réponses Twitch simulées.

```sh
python -m pip install playwright==1.60.0
python -m playwright install chromium
python docs/design/verify_ui.py
```

Sur Linux, `python -m playwright install --with-deps chromium` installe également les bibliothèques système du navigateur. Sous Windows, `python docs/design/verify_ui.py --channel msedge` utilise Microsoft Edge.

Pour valider la candidate Windows, construisez l’installeur, lancez les tests natifs et vérifiez son premier lancement sur un profil de test. Les commandes et les contrôles sont décrits dans le [guide de distribution](docs/DISTRIBUTION.md).
