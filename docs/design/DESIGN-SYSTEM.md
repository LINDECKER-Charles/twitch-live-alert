# Direction graphique

Twitch Live Alert est une console de bureau locale. Sa fenêtre doit permettre de lire l’état de la surveillance, son périmètre et l’heure du dernier relevé, y compris lorsqu’aucune chaîne ne diffuse. Les surfaces graphite distinguent la navigation, le bandeau de détection et les listes. L’accent mauve relie le pictogramme, la destination courante, le focus et l’action principale disponible.

## Références examinées

Deux captures ont été prises et examinées dans Edge le 21 novembre 2026. Elles restent des références de travail. Les formes et les couleurs du produit sont originales.

| Référence | Observation visuelle | Décision pour la console |
| --- | --- | --- |
| GitHub Primer, spécimen « Items with leading visuals », 1 280 × 800 | L’élément Repos occupe une ligne grisée avec un trait bleu à gauche. Les trois icônes sont alignées avec leur libellé. Le marqueur actif se repère aussi par sa forme. | Un rail sur grand écran, quatre liens nommés, un trait et un fond sur la destination active. Sur petit écran, le trait passe sous la destination. [Spécimen Primer](https://primer.style/product/components/nav-list/#items-with-leading-visuals) |
| Grafana Play, Alert rules, 1 350 × 850 | Un fond sombre continu porte un rail, une recherche horizontale et des résultats groupés. Les états ont un signe et une couleur propres. Plusieurs colonnes donnent une place importante au filtrage. | Le journal conserve la recherche au-dessus des résultats. Le bandeau Directs rassemble état textuel, dernier et prochain relevés. Deux filtres suffisent ici et tiennent en ligne sous le titre de liste. [Console Grafana Play](https://play.grafana.org/alerting/list) · [Modèle de supervision Grafana](https://grafana.com/docs/grafana/latest/alerting/) |

L’ancien écran plaçait toutes les commandes sur une seule page. Garder une colonne unique aurait permis d’atteindre immédiatement les réglages, au prix d’un déplacement important jusqu’au journal sur mobile. Une console proche de Grafana, avec panneau de filtres permanent, aurait avantagé les recherches en série et rétréci la colonne des titres de direct. Le rail avec destinations indépendantes donne la largeur aux titres et permet d’ouvrir directement le journal ou la connexion. Cette décision serait à réexaminer si plusieurs surveillances simultanées devaient être comparées.

## Sources et fondations

Le [pictogramme](../../web/mark.svg) représente trois barres de signal obliques. Le bandeau reprend les barres en perspective sur un plateau quadrillé. L’image est décorative et fixe. Les valeurs de vérification et les états visibles proviennent du service. L’ensemble fonctionne avec les ressources embarquées et Segoe UI ou sa police système de remplacement.

Les [jetons CSS](../../web/style.css) définissent les surfaces, le texte, les états et le focus. Les titres de destination vont de 29 à 40 px, les titres de panneau de 15 à 16 px. Les compteurs et valeurs de temps utilisent des chiffres tabulaires. Les séparateurs et le rythme des rangées structurent les listes longues.

| Paire | Contraste sRGB |
| --- | --- |
| Texte principal `#f3f2f6` sur panneau `#1b1c22` | 15,25:1 |
| Texte secondaire `#a2a1af` sur panneau | 6,68:1 |
| Texte d’action `#251b42` sur mauve `#c4b2ff` | 8,49:1 |
| Lien mauve sur panneau | 9,03:1 |
| Bordure de champ `#686776` sur fond de champ `#16161c` | 3,25:1 |

Ces rapports sont obtenus par la formule de luminance relative WCAG appliquée aux deux valeurs sRGB indiquées. La [planche et les vérifications](PLANCHE.md) situent les composants et les scénarios effectivement exercés.
