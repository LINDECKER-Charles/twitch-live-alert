# Detection schedules and email delivery

[Français](../HORAIRES-ET-COURRIEL.md) · [Getting started](../../README.en.md)

## Choose when to watch

Open **Règles** and set the weekdays and times when this rule should poll Twitch. With no windows it watches throughout the day. Language means the language selected by the broadcaster. Each matching rule may notify through its own channels.

You can create up to 64 rules. Matching filters share a poll. The service limits Helix traffic to 120 requests per minute per session with one request in flight at a time. Many filters or pages can make a polling cycle take longer.

Local time follows the PC. Select a named time zone such as `Europe/Paris` to keep the schedule on Paris time while travelling. The end of a window is exclusive. To watch from Friday 22:00 until Saturday 02:00, select **Friday** as the starting day and enter **22:00–02:00**. Equal start and end times cover the entire selected calendar day.

While the window is closed, the PC is asleep or the program is stopped, the rule does not poll. On return it processes streams still live. A stream that starts and ends entirely during the gap cannot be recovered by polling. The initial-poll setting controls first observations for a new filter. Alert history and deduplication survive restarts.

## Choose when to send

Enable email for the rule and choose a delivery policy. Detection hours and delivery hours are independent.

| Mode | Behaviour |
| --- | --- |
| Immediate | A new detection becomes eligible for email immediately. |
| Within windows | Detections outside a delivery window wait until it opens. Retries also wait for an allowed window. |
| Scheduled digest | Detections from this rule are grouped for the next selected time. Fixed digest times must fall within permitted windows. |

For example, watch from 18:00 to 23:00 on Friday and send a summary at 09:00 on Saturday. The application can send Friday’s detections on Saturday without polling for new streams on Saturday morning.

An overdue digest is processed after restart when monitoring, the rule and email delivery are active. If the clock skips a scheduled digest time, the application uses the next valid time on that day. If the clock repeats the scheduled time, it uses the first occurrence. SQLite keeps pending deliveries. SMTP can accept a message just before the client loses its connection or the program stops. Retrying that message can produce a duplicate with the same `Message-ID`.

Pending messages keep the delivery mode and schedule in effect at detection. At send time the service uses the current SMTP settings, recipients and templates. Disabling a rule pauses its pending deliveries. Deleting the rule cancels its queued messages. A delivery already in progress may complete. Accepted deliveries remain in the history.

## Configure your mail server

Open **Courriel**. Enter the SMTP host and port, encryption mode, sender and recipients provided by your mail service. Implicit TLS generally uses port 465, mandatory STARTTLS generally uses 587. Both verify the server certificate. Windows DPAPI protects the saved login and password for your account. **Envoyer un test** sends an actual message to the configured recipients. **Aperçu du message** renders a local preview. Delivery status shows queued mail, failures and the next retry. After you correct the SMTP settings, the service retries pending messages.

## Write a template

The subject and body are plain text. Insert values with double braces, for example `{{user_name}}`. Unknown variables produce an error. A missing value is displayed as an em dash (`—`). To show the expression itself, enter `\{{user_name}}`. For a digest, the body is rendered separately for each stream. The subject uses the first stream and includes the number of streams in the digest.

| Variables | Values |
| --- | --- |
| `{{user_name}}`, `{{user_login}}`, `{{user_id}}` | Display name, channel login and numeric Twitch user ID. |
| `{{stream_id}}`, `{{title}}`, `{{type}}` | Stream ID, title and stream type. |
| `{{category}}`, `{{game_id}}`, `{{language}}` | Category, category ID and broadcast language. |
| `{{started_at}}`, `{{detected_at}}` | Stream start and local detection, as ISO 8601 timestamps in UTC. |
| `{{viewer_count}}`, `{{tags}}`, `{{is_mature}}` | Viewers at detection, comma-separated tags and the mature-content flag. |
| `{{url}}`, `{{thumbnail_url}}` | Channel link and 640 × 360 Twitch preview URL. |
| `{{rule}}` | Name of the rule that detected the stream. |

For the subject, try `[{{rule}}] {{user_name}} is live`. For the body, try `{{title}} — {{category}} ({{language}}) · {{viewer_count}} viewers. {{url}}`.

