# Upgrade or migrate an installation

[Français](../MIGRATION.md) · [Windows distribution](DISTRIBUTION.md)

## Upgrade from Twitch Live Alert 0.2.0

1. Open **Journal** and note the existing entries, current category, broadcast language, polling interval and startup preference. Choose **Quitter** from the tray menu.
2. Make a private copy of `%LOCALAPPDATA%\TwitchLiveAlert\data\`. Keep the SQLite database and any journal files together with the other profile files. The saved Twitch credentials are already encrypted for your Windows account.
3. Run the 0.3.0 installer under that account and start the application. The previous filter becomes a Windows notification rule. Existing history and the stored Twitch connection are carried over. Configure email separately if desired.
4. Check **Règles**, **Journal** and **Connexion Twitch**. To roll back, exit 0.3.0, keep its current profile separately, reinstall 0.2.0 and restore the pre-upgrade copy. Alerts received after the upgrade belong to the newer profile.

## Migrate from Bloodborne FR Live 0.1

The original service stored its profile in the source checkout. Its default port is also 37219.

1. Record the old settings and stop the legacy service with its own `scripts/stop.ps1`. Back up its `data` folder and any separately configured `DATABASE_PATH`. Keep the original desktop and startup shortcuts.
2. With both services stopped, copy `settings.json`, `alerts.sqlite3` and any matching `-wal` and `-shm` files into a fresh `%LOCALAPPDATA%\TwitchLiveAlert\data\`. Keep existing, independently populated profiles separate.
3. Configure Twitch through **Connexion Twitch** on the new installation. If the old checkout already supplies application credentials, run the following command from that checkout under the same Windows account. It validates the credentials with Twitch and stores them under Windows DPAPI.

```powershell
& "$env:LOCALAPPDATA\Twitch Live Alert\twitch-live-alert.exe" --import-credentials "$env:LOCALAPPDATA\TwitchLiveAlert"
```

4. Check the migrated rule and search for a known history entry. If the old monitor started with Windows, move its **Bloodborne FR Live** shortcut out of the Startup folder (`shell:startup`) into your backup, then enable startup in the new dashboard as desired.

If you return to the legacy app, disable startup in the new app, exit it and restore the original shortcut and profile.
