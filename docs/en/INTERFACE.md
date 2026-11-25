# Dashboard layout and browser checks

[Français](../design/PLANCHE.md) · [User guide](../../README.en.md)

Twitch Live Alert uses the same local dashboard in WebView2 and in a browser. The sidebar groups monitoring (**Directs**, **Journal**, **Analytics**) and configuration (**Règles**, **Courriel**, **Réglages**, **Connexion Twitch**). On a narrow screen those links move above the content. The heading, current navigation item and breadcrumb name the same destination.

| View | Layout and interaction |
| --- | --- |
| Current streams and history | Stream rows show the channel, title, source rule, available Twitch details and a link to the channel. The history supports search and pagination. |
| Analytics | Choose a period and a channel to see how often it streams, which weekdays it started, and the duration measured for each stream. Older entries can have an unknown duration. |
| Rules | Each row shows its enabled state, category, language, notification channels, detection window and email schedule. A multi-window summary expands for details. Editing keeps the detection schedule separate from the email policy. |
| Category search | A dialog shows the search field, category names and Twitch artwork. Use the arrow keys to move through results, Enter to select or Escape to return. Loading, empty, failed and missing-image states have a text equivalent. |
| Email | Connection and delivery settings are separate from the plain-text template editor and its preview. The queue status can explain a rejected delivery and show the next attempt. |
| Twitch connection | A connected application displays its validation state, last check time and actions. Choosing to replace credentials opens the entry form. |

The screenshots use demonstration channels and addresses. Category covers in the illustrated examples come from Twitch’s CDN. [Current streams](../design/apercu-directs.png) · [Rules](../design/regles.png) · [Category search](../design/recherche-categories.png) · [Mobile category search](../design/catalogue-mobile.png) · [Email](../design/courriel.png)

## Reproduce the browser checks

The script serves deterministic local responses for Twitch and SMTP. It covers the keyboard, rule editing and schedules, connection states, SMTP previews and retry diagnostics, and widths down to 320 px. With no cover files supplied, it exercises the missing-image fallback.

```sh
python -m pip install playwright==1.60.0
python -m playwright install chromium
python docs/design/verify_ui.py
```

On Windows, append `--channel msedge` to use an installed Edge. Append `--axe path/to/axe.min.js` to check the WCAG A and AA rules exercised by the scenario. Browser fixtures test the interface independently of an SMTP provider or Twitch account. [Browser checks](../design/verify_ui.py)
