import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const executable = process.env.TLA_SMOKE_BINARY || fileURLToPath(new URL(
  '../target/debug/' + (process.platform === 'win32' ? 'twitch-live-alert.exe' : 'twitch-live-alert'),
  import.meta.url,
));
// Keep the installed dashboard running while this test starts its own process.
const reservation = createServer();
const port = await new Promise((resolve, reject) => {
  reservation.once('error', reject);
  reservation.listen(0, '127.0.0.1', () => {
    const { port } = reservation.address();
    reservation.close((error) => error ? reject(error) : resolve(port));
  });
});
const origin = 'http://127.0.0.1:' + port;
const localHeaders = {
  Origin: origin,
  'Content-Type': 'application/json',
  'X-Twitch-Live-Alert': 'local',
};

function request(path, options = {}) {
  return fetch(origin + path, { signal: AbortSignal.timeout(2000), ...options });
}

function post(path, body) {
  return request(path, { method: 'POST', headers: localHeaders, body: JSON.stringify(body) });
}

async function start(t, directory) {
  const env = { ...process.env };
  for (const key of ['TWITCH_CLIENT_ID', 'TWITCH_CLIENT_SECRET', 'DATABASE_PATH',
    'TWITCH_LIVE_ALERT_DESKTOP_EXE', 'POLL_INTERVAL_SECONDS', 'NOTIFY_EXISTING_ON_FIRST_RUN']) {
    delete env[key];
  }
  const service = spawn(executable, ['--port', String(port)], {
    cwd: directory, env, stdio: 'ignore', windowsHide: true,
  });
  const exited = new Promise((resolve, reject) => {
    service.once('error', reject);
    service.once('exit', (code, signal) => resolve({ code, signal }));
  });
  // Also handle an executable that fails to start before the first HTTP request.
  const stopped = exited.then(() => true, () => true);
  t.after(async () => {
    if (service.pid && service.exitCode === null && service.signalCode === null) service.kill();
    await stopped;
  });
  for (let attempt = 0; attempt < 100; attempt++) {
    const response = await request('/api/status').catch(() => null);
    if (response) {
      const status = await response.json();
      assert.equal(status.app_id, 'twitch-live-alert');
      assert.equal(status.process_id, service.pid, 'Le port de test appartient à une autre instance.');
      return { status, exited };
    }
    if (await Promise.race([stopped, delay(100, false)])) {
      const result = await exited;
      assert.fail('Le service a quitté le démarrage (' + JSON.stringify(result) + ').');
    }
  }
  assert.fail('Le service ne répond pas après dix secondes.');
}

async function shutdown(exited) {
  assert.equal((await post('/api/shutdown', {})).status, 202);
  const result = await Promise.race([exited, delay(5000, null, { ref: false })]);
  assert.deepEqual(result, { code: 0, signal: null });
}

test('le navigateur peut préparer une surveillance sur une installation neuve et la retrouver après relance', async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'twitch-live-alert-'));
  try {
    await t.test('premier démarrage, ressources et réglages', async (t) => {
      const { status, exited } = await start(t, directory);
      assert.equal(status.configured, false);
      assert.equal(status.running, false);
      assert.equal(status.category, 'Bloodborne');
      assert.equal(status.language, 'fr');
      assert.equal(status.notify_existing_on_first_run, false);
      for (const [path, mime] of [
        ['/', 'text/html'], ['/style.css', 'text/css'], ['/app.js', 'javascript'],
        ['/lists.js', 'javascript'], ['/category-picker.js', 'javascript'], ['/rules.js', 'javascript'],
        ['/schedules.js', 'javascript'], ['/mail.js', 'javascript'], ['/connection.js', 'javascript'],
        ['/mark.svg', 'image/svg+xml'],
      ]) {
        const response = await request(path);
        assert.equal(response.status, 200, path);
        assert.ok(response.headers.get('content-type').includes(mime));
        assert.ok((await response.text()).length > 100);
      }
      const history = await request('/api/history?q=&offset=0&limit=10');
      assert.equal(history.status, 200);
      assert.deepEqual(await history.json(), { items: [], total: 0 });
      const preferences = {
        category: status.category, language: '', poll_interval_seconds: 75,
        notify_existing_on_first_run: false, start_with_windows: status.start_with_windows,
      };
      const invalid = await post('/api/settings', { ...preferences, poll_interval_seconds: 1 });
      assert.equal(invalid.status, 400);
      const unverifiedCategory = await post('/api/settings', { ...preferences, category: 'Just Chatting' });
      assert.equal(unverifiedCategory.status, 409);
      const saved = await post('/api/settings', preferences);
      assert.equal(saved.status, 200);
      assert.equal((await saved.json()).category, preferences.category);
      assert.equal((await post('/api/check-now', {})).status, 409);
      await shutdown(exited);
    });
    await t.test('relecture des choix après un arrêt propre', async (t) => {
      const { status, exited } = await start(t, directory);
      assert.equal(status.category, 'Bloodborne');
      assert.equal(status.language, '');
      assert.equal(status.poll_interval_seconds, 75);
      assert.equal(status.notify_existing_on_first_run, false);
      await shutdown(exited);
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
