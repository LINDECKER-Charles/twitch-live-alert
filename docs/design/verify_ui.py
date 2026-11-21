"""Browser checks using synthetic HTTP responses and the real production assets.

Run with Python 3.11+ and Playwright installed. No external network access is used.
"""

import argparse
import json
from datetime import datetime, timezone
import tomllib
from pathlib import Path
from urllib.parse import parse_qs, urlparse

from playwright.sync_api import expect, sync_playwright


ROOT = Path(__file__).resolve().parents[2]
ORIGIN = "http://127.0.0.1:39121"
POLICY = ("default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; "
          "connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'")
SAMPLE_TIME = datetime(2026, 11, 21, 19, 42, 16, tzinfo=timezone.utc)
ASSETS = {
    "/": ("index.html", "text/html; charset=utf-8"),
    "/app.js": ("app.js", "text/javascript; charset=utf-8"),
    "/lists.js": ("lists.js", "text/javascript; charset=utf-8"),
    "/style.css": ("style.css", "text/css; charset=utf-8"),
    "/mark.svg": ("mark.svg", "image/svg+xml"),
}


def sample_channel(index, name, title, viewers):
    return dict(id=str(index), user_name=name, title=title, viewer_count=viewers,
                started_at="2026-11-21T18:02:00Z", url="https://www.twitch.tv/" + name.lower())


class Fixture:
    def __init__(self, version):
        now = int(SAMPLE_TIME.timestamp())
        self.streams = [
            sample_channel(11, "AtelierDuSonge", "Retour à Yharnam · Chasse aux trophées", 124),
            sample_channel(12, "LanternePourpre", "Première exploration des calices", 37),
            sample_channel(13, "LeVeilleur", "La nuit de la chasse continue", 18),
        ]
        self.history = [
            dict(stream_id=str(i), user_name=channel["user_name"], title=channel["title"],
                 url=channel["url"], recorded_at=now - i * 118 - 75)
            for i, channel in enumerate((self.streams * 18)[:53])
        ]
        self.status = dict(app_id="twitch-live-alert", process_id=123, app_version=version,
                           running=True, configured=False, category="Bloodborne", language="fr",
                           poll_interval_seconds=60, notify_existing_on_first_run=False,
                           start_with_windows=False, last_checked_at=None, next_check_at=None,
                           last_error=None, notifications_available=True, live_streams=[], notifications=[])
        self.offline = False
        self.reject_credentials = False
        self.fail_history = False
        self.fail_test = False
        self.received = []

    def populate(self):
        now = int(SAMPLE_TIME.timestamp())
        self.status.update(configured=True, live_streams=self.streams, notifications=self.history[:10],
                           last_checked_at=now, next_check_at=now + 60)

    def handle(self, route):
        request = route.request
        url = urlparse(request.url)
        if url.netloc != urlparse(ORIGIN).netloc:
            route.abort()
            return
        if url.path in ASSETS:
            name, mime = ASSETS[url.path]
            route.fulfill(body=(ROOT / "web" / name).read_bytes(), content_type=mime,
                          headers={"Content-Security-Policy": POLICY})
            return
        if self.offline:
            route.abort()
            return
        endpoint = url.path.removeprefix("/api/")
        self.received.append(endpoint)
        if request.method == "POST":
            assert request.headers["x-twitch-live-alert"] == "local"
            body = request.post_data_json
            if endpoint == "credentials":
                if self.reject_credentials:
                    route.fulfill(status=422, json={"message": "Identifiants Twitch refusés."})
                    return
                assert set(body) == {"client_id", "client_secret"}
                self.status["configured"] = True
            elif endpoint == "monitoring":
                self.status["running"] = body["enabled"]
            elif endpoint == "settings":
                assert set(body) == {"category", "language", "poll_interval_seconds",
                                     "notify_existing_on_first_run", "start_with_windows"}
                self.status.update(body)
            elif endpoint == "test-notification":
                route.fulfill(status=503 if self.fail_test else 200, json={"message":
                    "Windows a refusé le test." if self.fail_test else "Notification de test transmise à Windows."})
                return
            elif endpoint == "check-now":
                route.fulfill(status=202)
                return
            else:
                raise AssertionError(endpoint)
            route.fulfill(json=self.status)
        elif endpoint == "status":
            route.fulfill(json=self.status)
        elif endpoint == "categories":
            route.fulfill(json={"items": [{"id": "1", "name": "Just Chatting"}]})
        elif endpoint == "history":
            if self.fail_history:
                route.fulfill(status=503, json={"message": "Historique temporairement indisponible."})
                return
            query = parse_qs(url.query)
            term = query.get("q", [""])[0].casefold()
            rows = [row for row in self.history if term in (row["user_name"] + row["title"]).casefold()]
            start = int(query["offset"][0])
            limit = int(query["limit"][0])
            route.fulfill(json={"items": rows[start:start + limit], "total": len(rows)})
        else:
            route.fulfill(status=404)


def snapshot(page, folder, name):
    if folder:
        page.screenshot(path=str(folder / (name + ".png")), animations="disabled")


def view(page, name):
    page.locator('[data-nav="' + name + '"]').click()
    expect(page.locator("#title-" + name)).to_be_visible()


def no_overflow(page):
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), page.url


def run(browser, options):
    with options.manifest.open("rb") as manifest:
        fixture = Fixture(tomllib.load(manifest)["package"]["version"])
    context = browser.new_context(viewport={"width": 1280, "height": 860}, locale="fr-FR", timezone_id="Europe/Paris")
    context.route("**/*", fixture.handle)
    page = context.new_page()
    page.clock.set_fixed_time(SAMPLE_TIME)
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(ORIGIN)
    expect(page.locator("#monitor-state")).to_have_text("Twitch à configurer")
    snapshot(page, options.captures, "premier-lancement")
    expect(page.locator("#check-now")).to_be_hidden()

    # Primary onboarding by keyboard, including the focus destination.
    page.locator("#setup-link").focus()
    page.keyboard.press("Enter")
    expect(page.locator("#title-connexion")).to_be_focused()
    page.keyboard.press("Tab")
    expect(page.get_by_role("link", name="Ouvrir la console Twitch")).to_be_focused()
    page.keyboard.press("Tab")
    page.keyboard.type("client-de-verification")
    page.keyboard.press("Tab")
    expect(page.locator("#client-secret")).to_have_attribute("type", "password")
    page.keyboard.type("valeur-fictive")
    fixture.reject_credentials = True
    page.keyboard.press("Enter")
    expect(page.locator("#credential-feedback")).to_contain_text("refusés")
    fixture.reject_credentials = False
    page.locator("#save-credentials").click()
    expect(page.locator("#credential-feedback")).to_contain_text("enregistrés")
    expect(page.locator("#client-secret")).to_have_value("")
    expect(page.locator("#client-id")).to_have_value("")
    snapshot(page, options.captures, "connexion")

    view(page, "reglages")
    page.locator("#category").fill("Just")
    expect(page.locator("#category-options option")).to_have_attribute("value", "Just Chatting")
    page.locator("#category").fill("Just Chatting")
    page.locator("#language").select_option("")
    page.locator("#poll-interval").fill("42")
    page.locator("#notify-first").focus()
    page.keyboard.press("Space")
    view(page, "journal")
    view(page, "reglages")
    expect(page.locator("#category")).to_have_value("Just Chatting")
    page.locator("#save-settings").click()
    expect(page.locator("#settings-feedback")).to_have_text("Réglages enregistrés.")
    assert fixture.status["language"] == "" and fixture.status["poll_interval_seconds"] == 42
    assert fixture.status["notify_existing_on_first_run"]
    fixture.fail_test = True
    page.locator("#test-notification").click()
    expect(page.locator("#test-feedback")).to_contain_text("refusé")
    fixture.fail_test = False
    page.locator("#test-notification").click()
    expect(page.locator("#test-feedback")).to_contain_text("transmise")

    fixture.status.update(category="Bloodborne", language="fr", poll_interval_seconds=60)
    fixture.populate()
    page.goto(ORIGIN + "/")
    expect(page.locator("#stream-list > li")).to_have_count(3)
    expect(page.locator("#recent-list > li")).to_have_count(3)
    expect(page.locator("#stream-list a").first).to_have_attribute("target", "_blank")
    snapshot(page, options.captures, "directs")
    if options.readme:
        viewport = page.viewport_size
        page.set_viewport_size({"width": viewport["width"], "height": page.evaluate("document.documentElement.scrollHeight")})
        page.screenshot(path=str(options.readme), animations="disabled")
        page.set_viewport_size(viewport)
    page.set_viewport_size({"width": 1180, "height": 820})
    no_overflow(page)
    snapshot(page, options.captures, "fenetre-1180-directs")
    page.locator("#check-now").click()
    expect(page.locator("#watch-feedback")).to_have_text("Vérification demandée.")
    page.locator("#toggle-monitor").click()
    expect(page.locator("#monitor-state")).to_have_text("Surveillance en pause")
    page.locator("#toggle-monitor").click()
    expect(page.locator("#monitor-state")).to_have_text("Surveillance active")
    fixture.status["last_error"] = "Twitch est temporairement indisponible."
    page.reload()
    expect(page.locator("#monitor-state")).to_have_text("Surveillance dégradée")
    snapshot(page, options.captures, "degrade")
    fixture.status["last_error"] = None
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#monitor-state")).to_have_text("Surveillance active")

    view(page, "journal")
    expect(page.locator("#history-list > li")).to_have_count(50)
    page.locator("#next").click()
    expect(page.locator("#history-list > li")).to_have_count(3)
    expect(page.locator("#history-range")).to_contain_text("51")
    page.locator("#history-query").fill("aucunefixture")
    expect(page.locator("#history-message")).to_contain_text("Aucun résultat")
    page.locator("#clear-search").click()
    expect(page.locator("#history-list > li")).to_have_count(50)
    fixture.fail_history = True
    page.locator("#history-query").fill("atelier")
    expect(page.locator("#history-message")).to_contain_text("Journal indisponible")
    fixture.fail_history = False
    page.get_by_role("button", name="Réessayer").click()
    expect(page.locator("#history-list > li")).to_have_count(18)
    snapshot(page, options.captures, "journal")
    page.go_back()
    expect(page.locator("#title-directs")).to_be_visible()

    for width in (390, 320):
        page.set_viewport_size({"width": width, "height": 844})
        for section in ("reglages", "connexion", "journal", "directs"):
            view(page, section)
            no_overflow(page)
            if width == 390:
                snapshot(page, options.captures, "mobile-" + section)
    fixture.offline = True
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#offline")).to_be_visible()
    expect(page.locator("#stream-list > li")).to_have_count(3)
    expect(page.locator("#check-now")).to_be_disabled()
    fixture.offline = False
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#offline")).to_be_hidden()
    expect(page.locator("#check-now")).to_be_enabled()

    fixture.status["live_streams"] = [sample_channel(i, "CanalTest" + str(i), "Direct fictif", 1) for i in range(120)]
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#stream-count")).to_have_text("120")
    expect(page.locator("#stream-list > li")).to_have_count(50)
    page.locator("#show-more").click()
    expect(page.locator("#stream-list > li")).to_have_count(100)
    page.locator("#show-more").click()
    expect(page.locator("#stream-list > li")).to_have_count(120)
    expect(page.locator("#more-streams")).to_be_hidden()
    fixture.populate()
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#stream-list > li")).to_have_count(3)

    if options.axe:
        page.set_viewport_size({"width": 1280, "height": 860})
        page.evaluate(options.axe.read_text(encoding="utf-8"))
        for section in ("reglages", "connexion", "journal", "directs"):
            view(page, section)
            violations = page.evaluate("""async () => (await axe.run({
                runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] }
            })).violations.map(v => [v.id, v.nodes.map(n => n.target)])""")
            assert not violations, (section, json.dumps(violations))
    assert not errors, errors
    context.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", help="Installed browser channel, for example msedge")
    parser.add_argument("--captures", type=Path, help="Screenshot directory (optional)")
    parser.add_argument("--manifest", default=ROOT / "Cargo.toml", type=Path, help="Manifest providing the version shown in the fixture")
    parser.add_argument("--readme", type=Path, help="Presentation screenshot path (optional)")
    parser.add_argument("--axe", type=Path, help="Local axe.min.js to run additional accessibility checks")
    args = parser.parse_args()
    if args.captures:
        args.captures.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(channel=args.channel)
        run(browser, args)
        browser.close()
    print("Interface tests passed.")
