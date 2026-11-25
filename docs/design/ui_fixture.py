from datetime import datetime, timedelta, timezone
from pathlib import Path
import re
from urllib.parse import parse_qs, urlparse


ROOT = Path(__file__).resolve().parents[2]
ORIGIN = "http://127.0.0.1:39121"
POLICY = ("default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: https://static-cdn.jtvnw.net; "
          "connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'")
SAMPLE_TIME = datetime(2026, 11, 21, 19, 42, 16, tzinfo=timezone.utc)
ASSETS = {
    "/": ("index.html", "text/html; charset=utf-8"),
    "/app.js": ("app.js", "text/javascript; charset=utf-8"),
    "/category-picker.js": ("category-picker.js", "text/javascript; charset=utf-8"),
    **{f"/{name}.js": (f"{name}.js", "text/javascript; charset=utf-8")
       for name in ["rules", "schedules", "connection", "mail", "analytics"]},
    "/lists.js": ("lists.js", "text/javascript; charset=utf-8"),
    "/style.css": ("style.css", "text/css; charset=utf-8"),
    "/mark.svg": ("mark.svg", "image/svg+xml"),
}


def sample_channel(index, name, title, viewers, started_at="2026-11-21T18:02:00Z"):
    return dict(id=str(index), user_name=name, title=title, viewer_count=viewers,
                started_at=started_at, url="https://www.twitch.tv/" + name.lower())


CATALOG = {460636: "Bloodborne", 512953: "ELDEN RING", 29433: "Dark Souls",
           489170: "DARK SOULS II: Scholar of the First Sin", 490292: "DARK SOULS III",
           1122982998: "Dark Souls: Remastered"}


def artwork(game_id):
    return f"https://static-cdn.jtvnw.net/ttv-boxart/{game_id}_IGDB-{{width}}x{{height}}.jpg"


def rule(key, name, game_id, email=False):
    schedule = {"timezone": "local", "windows": []}
    return dict(id=key, name=name, enabled=True, category=CATALOG[game_id], box_art_url=artwork(game_id),
                language="fr", channels={"desktop": True, "email": email}, detection=schedule,
                mail={"mode": "digest" if email else "instant", "schedule": schedule,
                      "digest_times": ["18:00"] if email else []})


def analytics_sessions():
    now = int(SAMPLE_TIME.timestamp())
    groups = (
        ("AtelierDuSonge", "Retour à Yharnam · Chasse aux trophées", 31, 0),
        ("LanternePourpre", "Première exploration des calices", 8, 2),
        ("LeVeilleur", "Exploration nocturne de l’Entre-terre", 5, 5),
        ("ChasseursDesAnciensCalices", "Le dernier voyage dans les calices profonds", 2, 38),
        ("MarcheuseDeBrume", "Retour au faubourg de la cathédrale", 1, 110),
    )
    sessions = []
    for name, title, count, first_day in groups:
        for index in range(count):
            occurred_at = now - 6 * 3600 - (first_day + index * 5 // 7) * 86400 - index % 7 * 2400
            duration = None if index % 9 == 1 else 2700 + (index * 37 + count * 17) % 195 * 60
            sessions.append(dict(
                stream_id=f"{name.casefold()}-{index + 1}", user_name=name,
                title=f"{title} · Session {index + 1}", occurred_at=occurred_at,
                started_at=occurred_at if duration is not None else None,
                last_seen_at=occurred_at + duration if duration is not None else None,
                duration_seconds=duration, peak_viewers=20 + index * 3 if duration is not None else None,
            ))
    return sorted(sessions, key=lambda session: session["occurred_at"], reverse=True)


def analytics_payload(sessions, days, channel, offset):
    # Fixture sessions occur around noon in Paris, on the same calendar day in UTC.
    def session_day(session):
        return datetime.fromtimestamp(session["occurred_at"], tz=timezone.utc).date()

    today = SAMPLE_TIME.date()
    cutoff = today - timedelta(days=max(days - 1, 0))
    in_period = [session for session in sessions if not days or session_day(session) >= cutoff]
    groups = {}
    for session in in_period:
        groups.setdefault(session["user_name"], []).append(session)

    def summarize(rows):
        durations = [row["duration_seconds"] for row in rows if row["duration_seconds"] is not None]
        return dict(live_count=len(rows), measured_count=len(durations), total_seconds=sum(durations))

    channels = [dict(key="login:" + name.casefold(), name=name, last_at=rows[0]["occurred_at"], **summarize(rows))
                for name, rows in groups.items()]
    channels.sort(key=lambda item: (-item["live_count"], item["name"]))
    rows = [session for session in in_period if not channel or "login:" + session["user_name"].casefold() == channel]
    weekdays = [0] * 7
    for row in rows:
        weekday = session_day(row).weekday()
        weekdays[weekday] += 1
    totals = dict(**summarize(rows), desktop_alerts=len(rows), email_alerts=len(rows) // 3)
    earliest = session_day(in_period[-1]) if in_period else today
    return dict(period_days=days or (today - earliest).days + 1, channels=channels, totals=totals, weekdays=weekdays,
                sessions=rows[offset:offset + 25], offset=offset)


class Fixture:
    def __init__(self, version, art_dir=None):
        now = int(SAMPLE_TIME.timestamp())
        self.art_dir = art_dir
        self.rules = [rule("yharnam", "Soirées à Yharnam", 460636), rule("elden", "Entre-terre", 512953, True)]
        self.rules[0]["channels"]["email"] = True
        self.rules[1]["detection"] = {"timezone": "Europe/Paris", "windows": [
            {"days": [1,2,3,4,5], "start": "17:00", "end": "21:00"},
            {"days": [6,7], "start": "18:00", "end": "23:30"}]}
        self.rules[1]["mail"] = {"mode": "window", "schedule": {"timezone": "local", "windows": [
            {"days": [1,2,3,4,5], "start": "09:00", "end": "18:00"}]}, "digest_times": []}
        self.connection = {"state": "unconfigured", "verified_at": None}
        self.last_body = None
        self.tests_sent = 0
        self.delivery = dict(available=True, pending_count=0, scheduled_count=0, failed_count=0,
                             last_error=None, rule_name=None, next_attempt_at=None, error_code=None)
        self.mail = dict(host="", security="tls", port=465, sender="", recipients=[], credentials_configured=False,
                         subject_template="{{user_name}} en direct", body_template="{{title}}\n{{url}}",
                         default_subject_template="{{user_name}} en direct", default_body_template="{{title}}\n{{url}}",
                         variables=["user_name", "title", "url", "tags", "rule"])
        self.streams = [
            sample_channel(11, "AtelierDuSonge", "Retour à Yharnam · Chasse aux trophées", 124),
            sample_channel(12, "LanternePourpre", "Première exploration des calices", 37, "2026-11-21T17:38:00Z"),
            sample_channel(13, "LeVeilleur", "Exploration nocturne de l’Entre-terre", 18, "2026-11-21T19:17:00Z"),
        ]
        for stream in self.streams:
            stream.update(rule_id="yharnam", rule_name="Soirées à Yharnam")
        self.streams[-1].update(rule_id="elden", rule_name="Entre-terre")
        self.history = [
            dict(stream_id=str(i), user_name=channel["user_name"], title=channel["title"],
                 url=channel["url"], rule_id=channel["rule_id"], rule_name=channel["rule_name"],
                 channel="email" if channel["rule_id"] == "elden" else "desktop", recorded_at=now - i * 118 - 75)
            for i, channel in enumerate((self.streams * 18)[:53])
        ]
        self.status = dict(app_id="twitch-live-alert", process_id=123, app_version=version,
                           running=False, enabled=True, active_rule_count=2, configured=False, category="Bloodborne", language="fr",
                           poll_interval_seconds=60, notify_existing_on_first_run=False,
                           start_with_windows=False, last_checked_at=None, next_check_at=None,
                           last_error=None, notifications_available=True, live_streams=[], notifications=[])
        self.offline = False
        self.reject_credentials = False
        self.fail_categories = False
        self.held_category = None
        self.fail_history = False
        self.analytics_sessions = analytics_sessions()
        self.analytics_requests = []
        self.fail_analytics = False
        self.hold_analytics = False
        self.held_analytics = None
        self.fail_test = False
        self.received = []

    def populate(self):
        now = int(SAMPLE_TIME.timestamp())
        self.status.update(configured=True, running=True, live_streams=self.streams, notifications=self.history[:10],
                           last_checked_at=now, next_check_at=now + 60)

    def handle(self, route):
        request = route.request
        url = urlparse(request.url)
        if url.hostname == "static-cdn.jtvnw.net":
            game_id = url.path.split("/")[-1].split("_")[0]
            art = self.art_dir / (game_id + ".jpg") if self.art_dir else None
            if art and art.is_file():
                route.fulfill(path=str(art), content_type="image/jpeg")
            else:
                route.abort()
            return
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
            if endpoint.startswith("connection/"):
                if endpoint.endswith("replace"):
                    assert set(body) == {"client_id", "client_secret"}
                    if self.reject_credentials:
                        route.fulfill(status=422, json={"message": "Identifiants Twitch refusés."})
                        return
                self.connection = dict(state="unconfigured" if endpoint.endswith("disconnect") else "connected",
                                       verified_at=int(SAMPLE_TIME.timestamp()))
                self.status["configured"] = self.connection["state"] == "connected"
                self.status["running"] = self.status["enabled"] and self.status["configured"]
                route.fulfill(json=self.connection)
                return
            elif endpoint.startswith("rules"):
                if endpoint.endswith("/delete"):
                    self.rules = [r for r in self.rules if r["id"] != endpoint.split("/")[1]]
                else:
                    assert set(body) <= {"id","name","enabled","category","language","box_art_url","channels","detection","mail"}
                    body["id"] = body.get("id") or "nouvelle"
                    self.rules = [r for r in self.rules if r["id"] != body["id"]] + [body]
                    self.last_body = body
                self.status["active_rule_count"] = sum(r["enabled"] for r in self.rules)
                route.fulfill(json={"rules": self.rules})
                return
            elif endpoint == "mail/preview":
                assert set(body) == {"subject_template", "body_template"}
                values = {"user_name": "AtelierDuSonge", "title": "Retour à Yharnam", "rule": "Soirées à Yharnam",
                          "tags": "—", "url": "https://www.twitch.tv/atelierdusonge"}
                def render(template):
                    return re.sub(r"\{\{(\w+)\}\}", lambda match: values.get(match[1], "—"), template)
                route.fulfill(json={"subject": render(body["subject_template"]), "body": render(body["body_template"])})
                return
            elif endpoint == "mail/test":
                self.tests_sent += 1
                route.fulfill(status=503 if self.fail_test else 200, json={"message":
                    "Serveur de courriel indisponible." if self.fail_test else "Courriel de test transmis."})
                return
            elif endpoint == "mail":
                if "username" in body:
                    self.mail["credentials_configured"] = bool(body.pop("username") and body.get("password"))
                    body.pop("password")
                self.mail.update(body)
                route.fulfill(json=self.mail)
                return
            elif endpoint == "monitoring":
                self.status["enabled"] = self.status["running"] = body["enabled"]
            elif endpoint == "settings":
                assert set(body) == {"poll_interval_seconds", "notify_existing_on_first_run", "start_with_windows"}
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
        elif endpoint == "connection":
            route.fulfill(json=self.connection)
        elif endpoint == "rules":
            route.fulfill(json={"rules": self.rules})
        elif endpoint == "mail":
            route.fulfill(json=self.mail)
        elif endpoint == "mail/delivery":
            route.fulfill(json=self.delivery)
        elif endpoint == "categories":
            term = parse_qs(url.query)["q"][0].casefold()
            if term == "attente":
                self.held_category = route
            elif self.fail_categories:
                route.fulfill(status=503, json={"message": "Recherche Twitch indisponible."})
            else:
                route.fulfill(json={"items": [{"id": str(i), "name": name, "box_art_url": artwork(i)} for i, name in CATALOG.items() if term in name.casefold()]})
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
        elif endpoint == "analytics":
            query = parse_qs(url.query)
            days = int(query.get("days", ["30"])[0])
            channel = query.get("channel", [""])[0]
            offset = int(query.get("offset", ["0"])[0])
            self.analytics_requests.append((days, channel, offset))
            payload = analytics_payload(self.analytics_sessions, days, channel, offset)
            if self.fail_analytics:
                route.fulfill(status=503, json={"message": "Statistiques temporairement indisponibles."})
            elif self.hold_analytics:
                self.held_analytics = (route, payload)
            else:
                route.fulfill(json=payload)
        else:
            route.fulfill(status=404)
