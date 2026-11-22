"""Headless keyboard, responsive, HTTP-contract and accessibility checks against production assets."""

import argparse
import json
import sys
import tomllib
from pathlib import Path

from playwright.sync_api import expect, sync_playwright
sys.dont_write_bytecode = True
from ui_fixture import Fixture, ORIGIN, ROOT, SAMPLE_TIME, sample_channel

def snapshot(page, folder, name):
    if folder:
        page.screenshot(path=str(folder / (name + ".png")), animations="disabled")


def panel_capture(locator, folder, name):
    if folder:
        locator.screenshot(path=str(folder / (name + ".png")), animations="disabled")


def view(page, name):
    page.locator('[data-nav="' + name + '"]').click()
    expect(page.locator("#title-" + name)).to_be_visible()


def no_overflow(page):
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), page.url


def audit(page, options):
    if options.axe:
        page.evaluate(options.axe.read_text(encoding="utf-8"))
        errors = page.evaluate("""async () => (await axe.run({
          runOnly: {type: 'tag', values: ['wcag2a', 'wcag2aa','wcag21aa','wcag22aa']}
        })).violations.map(v => [v.id, v.nodes.map(n => n.target)])""")
        assert not errors, errors


def categories(page, fixture, options):
    page.locator("#choose-category").press("Enter")
    expect(page.locator('[data-nav="regles"]')).to_have_attribute("aria-current", "page")
    expect(page.locator("#crumb-page")).to_have_text("Règles")
    expect(page.locator("#title-regles")).to_have_text("Règles")
    field = page.locator("#category-query")
    expect(field).to_be_focused()
    field.fill("Aucun résultat")
    expect(page.locator("#picker-feedback")).to_contain_text("Aucune catégorie")
    with page.expect_request(lambda req: "categories?q=Attente" in req.url):
        field.fill("Attente")
    expect(page.locator("#picker-feedback")).to_contain_text("Recherche")
    fixture.held_category.fulfill(json={"items": []})
    fixture.fail_categories = True
    field.fill("Dark Souls")
    expect(page.locator("#retry-picker")).to_be_visible()
    fixture.fail_categories = False
    page.locator("#retry-picker").press("Enter")
    tiles = page.locator(".category-tile")
    expect(tiles).to_have_count(4)
    if options.art_dir:
        expect(tiles.locator("img")).to_have_count(4)
        page.wait_for_function("Array.from(document.querySelectorAll('.category-tile img')).every(i=>i.complete && i.naturalWidth>0)")
    field.focus()
    field.press("ArrowDown")
    expect(tiles.first).to_be_focused()
    tiles.first.press("ArrowRight")
    expect(tiles.nth(1)).to_be_focused()
    audit(page, options)
    if options.category_capture:
        page.screenshot(path=str(options.category_capture), animations="disabled")
    snapshot(page, options.captures, "catalogue")
    page.keyboard.press("Escape")
    expect(page.locator("#choose-category")).to_be_focused()
    expect(page.locator("#chosen-category")).to_have_text("Choisir une catégorie")
    page.locator("#choose-category").press("Enter")
    field.fill("Dark Souls")
    expect(tiles).to_have_count(4)
    page.set_viewport_size({"width": 390, "height": 844})
    no_overflow(page)
    snapshot(page, options.captures, "mobile-catalogue")
    tiles.nth(2).tap()
    expect(page.locator("#chosen-category")).to_have_text("DARK SOULS III")
    page.set_viewport_size({"width": 1280, "height": 860})


def schedule_boundaries(page):
    results = page.evaluate("""async () => {
      const {inDetectionWindow: active, scheduleSummary} = await import('/schedules.js');
      const window = {days:[7],start:'22:00',end:'02:00'};
      const test = (iso, zone, w=window) => active({timezone:zone,windows:[w]}, new Date(iso));
      const fold = {days:[7],start:'02:15',end:'02:45'};
      const times = scheduleSummary({timezone:'UTC', windows:[]}, 'digest', ['08:00','12:30','18:00']);
      return [test('2026-11-22T23:30Z','Europe/Paris'), test('2026-11-22T23:30Z','local'),
        test('2026-11-23T01:00Z','Europe/Paris'), test('2026-11-22T20:59Z','Europe/Paris'),
        test('2026-11-22T21:00Z','Europe/Paris'), test('2026-11-22T23:30Z','America/New_York'),
        test('2026-10-25T00:30Z','Europe/Paris',fold), test('2026-10-25T01:30Z','Europe/Paris',fold),
        test('2026-11-22T12:00Z','No/Such_Zone'), test('2026-11-22T12:00Z','UTC',{days:[7],start:'14:00',end:'14:00'}),
        times.details[0].includes('12:30') && times.details[0].includes('18:00')];
    }""")
    assert results == [True, True, False, False, True, False, True, True, None, True, True], results


def check_rules(page, fixture, options):
    view(page, "regles")
    expect(page.locator("#rules-list > li")).to_have_count(2)
    schedule_boundaries(page)
    scheduled = page.locator("#rules-list > li").filter(has_text="Entre-terre")
    expect(scheduled).to_contain_text("Dans la plage de détection")
    disclosure = scheduled.locator("details")
    disclosure.locator("summary").press("Enter")
    expect(disclosure).to_have_attribute("open", "")
    expect(disclosure).to_contain_text("Sam.–Dim. 18:00–23:30")
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(disclosure).to_have_attribute("open", "")
    disclosure.locator("summary").press("Enter")
    page.locator("#new-rule").press("Enter")
    page.locator("#rule-name").fill("Lothric le soir")
    categories(page, fixture, options)
    page.locator("#language").select_option("")
    detection = page.locator("#detection-schedule")
    detection.get_by_role("button", name="Ajouter une plage").click()
    detection.get_by_label("Début", exact=True).fill("22:00")
    detection.get_by_label("Fin", exact=True).fill("02:00")
    detection.get_by_role("checkbox", name="Dimanche").uncheck()
    detection.get_by_role("combobox").select_option("Europe/Paris")
    if options.captures:
        page.screenshot(path=str(options.captures / "formulaire-regle.png"), animations="disabled", full_page=True)
    page.locator("#rule-email").check()
    policy = page.locator("#mail-policy").locator("xpath=ancestor::section[1]")
    panel_capture(policy, options.captures, "envoi-immediat")
    page.locator("#mail-mode").select_option("window")
    delivery = page.locator("#delivery-schedule")
    delivery.get_by_role("button", name="Ajouter une plage").click()
    delivery.get_by_label("Début", exact=True).fill("10:00")
    delivery.get_by_label("Fin", exact=True).fill("18:00")
    panel_capture(policy, options.captures, "envoi-plages")
    delivery.get_by_role("button", name="Retirer la plage").click()
    expect(page.locator("#policy-help")).to_contain_text("prochaine plage")
    page.locator("#mail-mode").select_option("digest")
    page.locator("#digest-times input").fill("00:00")
    page.locator("#add-time").click()
    page.locator("#digest-times input").last.fill("18:00")
    delivery = page.locator("#delivery-schedule")
    expect(delivery.get_by_text("Jours d’envoi", exact=True)).to_be_visible()
    delivery.get_by_role("checkbox", name="Samedi").uncheck()
    delivery.get_by_role("combobox").select_option("Europe/Paris")
    panel_capture(policy, options.captures, "envoi-recapitulatif")
    audit(page, options)
    page.locator("#save-rule").click()
    expect(page.locator("#rules-feedback")).to_have_text("Règle enregistrée.")
    saved = fixture.last_body
    assert saved["category"] == "DARK SOULS III" and saved["language"] == ""
    assert saved["detection"] == {"timezone": "Europe/Paris", "windows": [
        {"days": [1, 2, 3, 4, 5, 6], "start": "22:00", "end": "02:00"}]}
    assert saved["mail"]["digest_times"] == ["00:00", "18:00"]
    assert saved["mail"]["schedule"]["windows"][0]["days"] == [1, 2, 3, 4, 5, 7]
    expect(page.locator("#rules-list > li")).to_have_count(3)
    page.locator("#title-regles").scroll_into_view_if_needed()
    summary = page.locator("#rules-list > li").filter(has_text="Lothric le soir")
    expect(summary).to_contain_text("Hors plage de détection")
    expect(summary).to_contain_text("22:00–02:00 (+1 j)")
    expect(summary).to_contain_text("Récap. 00:00, 18:00")
    snapshot(page, options.captures, "regles")
    page.set_viewport_size({"width": 390, "height": 844})
    no_overflow(page)
    if options.captures:
        page.screenshot(path=str(options.captures / "mobile-regles-calendriers.png"), full_page=True)
    page.set_viewport_size({"width": 1280, "height": 860})
    page.get_by_role("button", name="Désactiver la règle Lothric le soir").click()
    expect(page.locator("#rules-feedback")).to_contain_text("désactivée")
    page.get_by_role("button", name="Activer la règle Lothric le soir", exact=True).click()
    expect(page.locator("#rules-feedback")).to_contain_text("activée")
    page.get_by_role("button", name="Modifier la règle Lothric le soir").click()
    expect(page.locator("#digest-times input").first).to_have_value("00:00")
    expect(page.locator("#rule-form-title")).to_have_text("Modifier la règle")
    expect(page.locator("#rule-email")).to_be_checked()
    expect(page.locator("#mail-mode")).to_have_value("digest")
    page.locator("#rule-name").blur()
    if options.captures:
        page.screenshot(path=str(options.captures / "modifier-regle.png"), full_page=True)
    for width in [390, 320]:
        page.set_viewport_size({"width": width, "height": 844})
        no_overflow(page)
        if options.captures and width == 390:
            page.screenshot(path=str(options.captures / "mobile-modifier-regle.png"), full_page=True)
    page.set_viewport_size({"width": 1280, "height": 860})
    page.locator("#cancel-rule").press("Enter")
    page.get_by_role("button", name="Supprimer la règle Lothric le soir").click()
    page.get_by_role("button", name="Confirmer", exact=True).click()
    expect(page.locator("#rules-feedback")).to_contain_text("supprimée")
    expect(page.locator("#rules-list > li")).to_have_count(2)


def check_mail(page, fixture, options):
    view(page, "courriel")
    expect(page.locator("#delivery-summary")).to_have_text("Aucun courriel en attente.")
    fixture.delivery.update(pending_count=3, scheduled_count=1, failed_count=2, next_attempt_at=int(SAMPLE_TIME.timestamp()) + 300,
                            last_error="La connexion au serveur SMTP a échoué.", rule_name="Entre-terre", error_code="connection")
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#delivery-state")).to_have_text("Envoi en échec")
    expect(page.locator("#delivery-error")).to_contain_text("Entre-terre")
    expect(page.locator("#delivery-summary")).to_contain_text("2 en échec")
    expect(page.locator("#delivery-next")).to_contain_text("Prochaine tentative automatique")
    panel_capture(page.locator(".delivery-panel"), options.captures, "file-echec")
    fixture.delivery.update(pending_count=0, scheduled_count=0, failed_count=0, last_error=None, next_attempt_at=None)
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#delivery-state")).to_have_text("À jour")
    page.locator("#mail-host").fill("smtp.example.test")
    page.locator("#mail-security").select_option("starttls")
    expect(page.locator("#mail-port")).to_have_value("587")
    page.locator("#mail-sender").fill("Alertes Twitch <alertes@example.test>")
    page.locator("#mail-recipients").fill("lecteur@example.test\nautre@example.test")
    page.locator("#mail-auth").select_option("replace")
    page.locator("#smtp-user").fill("tla-test")
    page.locator("#smtp-password").fill("fictif")
    page.locator("#mail-subject").fill("Direct de {{user_name}}")
    page.locator("#mail-body").fill("<b>Hello</b> · {{tags}}")
    page.locator("#preview-mail").click()
    expect(page.locator("#preview-subject")).to_have_text("Direct de AtelierDuSonge")
    expect(page.locator("#preview-body")).to_have_text("<b>Hello</b> · —")
    expect(page.locator("#preview-body b")).to_have_count(0)
    assert not fixture.tests_sent
    page.locator("#mail-body").fill("{{title}}\n{{url}}")
    page.locator("#mail-variables").locator("..").locator("summary").click()
    page.get_by_role("button", name="{{rule}} Nom de la règle").click()
    assert "{{rule}}" in page.locator("#mail-body").input_value()
    page.locator("#save-mail").click()
    expect(page.locator("#mail-feedback")).to_contain_text("enregistrée")
    expect(page.locator("#smtp-password")).to_have_value("")
    assert fixture.mail["credentials_configured"]
    assert fixture.mail["recipients"] == ["lecteur@example.test", "autre@example.test"]
    fixture.fail_test = True
    page.locator("#test-mail").click()
    expect(page.locator("#mail-test-feedback")).to_contain_text("indisponible")
    fixture.fail_test = False
    page.locator("#test-mail").click()
    expect(page.locator("#mail-test-feedback")).to_contain_text("transmis")
    page.locator("#mail-security").select_option("tls")
    page.locator("#save-mail").click()
    expect(page.locator("#mail-feedback")).to_contain_text("Pour changer de serveur")
    page.locator("#mail-security").select_option("starttls")
    page.locator("#mail-body").fill("{{title}}\n{{url}}\nRègle suivie · {{rule}}")
    page.locator("#mail-variables").locator("..").locator("summary").click()
    page.locator("#save-mail").click()
    expect(page.locator("#mail-feedback")).to_contain_text("enregistrée")
    page.locator("#preview-mail").click()
    expect(page.locator("#preview-feedback")).to_contain_text("exemple")
    panel_capture(page.locator("#mail-preview").locator("xpath=ancestor::section[1]"), options.captures, "modele-courriel")
    page.locator("#title-courriel").focus()
    page.locator("#title-courriel").scroll_into_view_if_needed()
    if options.captures:
        page.screenshot(path=str(options.captures / "courriel.png"), animations="disabled", full_page=True)
    page.set_viewport_size({"width": 390, "height": 844})
    no_overflow(page)
    if options.captures:
        page.screenshot(path=str(options.captures / "mobile-courriel-configure.png"), full_page=True)
    page.set_viewport_size({"width": 1280, "height": 860})
    audit(page, options)


def run(browser, options):
    with options.manifest.open("rb") as manifest:
        fixture = Fixture(options.fixture_version or tomllib.load(manifest)["package"]["version"], options.art_dir)
    context = browser.new_context(viewport={"width": 1280, "height": 860}, locale="fr-FR", timezone_id="Europe/Paris", has_touch=True)
    context.route("**/*", fixture.handle)
    page = context.new_page()
    page.clock.set_fixed_time(SAMPLE_TIME)
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(ORIGIN)
    expect(page.locator("#monitor-state")).to_have_text("Twitch à configurer")
    snapshot(page, options.captures, "premier-lancement")
    expect(page.locator("#check-now")).to_be_hidden()

    view(page, "regles")
    page.locator("#new-rule").click()
    expect(page.locator("#catalog-connect")).to_be_visible()
    expect(page.locator("#choose-category")).to_be_disabled()
    page.locator("#cancel-rule").click()
    view(page, "directs")

    # Primary onboarding by keyboard, including the focus destination.
    page.locator("#setup-link").focus()
    page.keyboard.press("Enter")
    expect(page.locator("#title-connexion")).to_be_focused()
    expect(page.locator("#credentials-state")).to_have_text("À configurer")
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
    expect(page.locator("#credential-feedback")).to_contain_text("vérifiée")
    expect(page.locator("#client-secret")).to_have_value("")
    expect(page.locator("#client-id")).to_have_value("")
    snapshot(page, options.captures, "connexion")

    expect(page.locator("#credentials-form")).to_be_hidden()
    page.locator("#verify-connection").click()
    expect(page.locator("#credential-feedback")).to_contain_text("vérifiée")
    page.locator("#edit-connection").click()
    expect(page.locator("#client-secret")).to_have_value("")
    page.locator("#cancel-connection").click()
    expect(page.locator("#credentials-form")).to_be_hidden()
    page.locator("#edit-connection").click()
    page.locator("#client-secret").fill("brouillon-fictif")
    view(page, "reglages")
    view(page, "connexion")
    expect(page.locator("#credentials-form")).to_be_hidden()
    expect(page.locator("#client-secret")).to_have_value("")
    page.locator("#disconnect-twitch").click()
    page.locator("#keep-connection").press("Enter")
    expect(page.locator("#credentials-state")).to_have_text("Connecté")
    page.locator("#disconnect-twitch").click()
    page.locator("#confirm-disconnect").click()
    expect(page.locator("#credentials-state")).to_contain_text("À configurer")
    page.locator("#client-id").fill("client-fictif")
    page.locator("#client-secret").fill("valeur-fictive")
    page.locator("#save-credentials").click()
    expect(page.locator("#credentials-state")).to_have_text("Connecté")
    check_rules(page, fixture, options)
    check_mail(page, fixture, options)
    view(page, "reglages")
    page.locator("#poll-interval").fill("42")
    page.locator("#notify-first").focus()
    page.keyboard.press("Space")
    view(page, "journal")
    view(page, "reglages")
    expect(page.locator("#poll-interval")).to_have_value("42")
    page.locator("#save-settings").click()
    expect(page.locator("#settings-feedback")).to_have_text("Réglages enregistrés.")
    assert fixture.status["poll_interval_seconds"] == 42
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
    expect(page.locator("#stream-list time").first).to_have_text("Début 19:02")
    expect(page.locator("#stream-list time").first).to_have_attribute("datetime", "2026-11-21T18:02:00.000Z")
    expect(page.locator("#stream-list time").first).to_have_attribute("title", "Début du direct le 21/11/2026 19:02")
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
        for section in ("regles", "courriel", "reglages", "connexion", "journal", "directs"):
            view(page, section)
            no_overflow(page)
            if width == 390:
                snapshot(page, options.captures, "mobile-" + section)
    fixture.offline = True
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#offline")).to_be_visible()
    expect(page.locator("#stream-list > li")).to_have_count(3)
    expect(page.locator("#check-now")).to_be_disabled()
    view(page, "reglages")
    expect(page.locator("#poll-interval")).to_be_disabled()
    view(page, "directs")
    fixture.offline = False
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#offline")).to_be_hidden()
    expect(page.locator("#check-now")).to_be_enabled()

    fixture.status["live_streams"] = [sample_channel(71, "SansDate", "Heure inconnue", 5, None),
                                      sample_channel(72, "DateInvalide", "Heure illisible", 4, "invalide")]
    page.evaluate("document.dispatchEvent(new Event('visibilitychange'))")
    expect(page.locator("#stream-list > li")).to_have_count(2)
    expect(page.locator("#stream-list time")).to_have_count(0)
    expect(page.locator("#stream-list .stream-meta").first).to_contain_text("5 spectateurs")
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
        for section in ("regles", "courriel", "reglages", "connexion", "journal", "directs"):
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
    parser.add_argument("--category-capture", type=Path, help="Illustrated Twitch category dialog")
    parser.add_argument("--readme", type=Path, help="Presentation screenshot path (optional)")
    parser.add_argument("--fixture-version", help="Version label for release documentation fixtures")
    parser.add_argument("--art-dir", type=Path, help="Optional real Twitch CDN covers named by game ID, e.g. 460636.jpg")
    parser.add_argument("--axe", type=Path, help="Local axe.min.js to run additional accessibility checks")
    args = parser.parse_args()
    if args.category_capture and not args.art_dir:
        parser.error("--category-capture requires real covers via --art-dir")
    if args.captures:
        args.captures.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(channel=args.channel)
        run(browser, args)
        browser.close()
    print("Interface tests passed.")
