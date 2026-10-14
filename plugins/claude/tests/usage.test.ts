// Run with `claude plugin test plugins/claude`. The engine's own turn.step
// chain runs; the test's hooks beneath the plugin answer the environment,
// the session id and every `$.http.fetch`. No binary, network or model is
// involved. Background sends are awaited through hook signals, never a timed wait.
import { describe, expect, test } from 'claude-code/testing';

const FULL = { input_tokens: 3, output_tokens: 1, cache_read_input_tokens: 7, cache_creation_input_tokens: 0 };
const OK = { status: 204, ok: true, headers: {}, text: '' };
const OPTS = { timeoutMs: 3000 };

// Answers env, session id, fetch and the engine's turn.step beneath the
// plugin. `fetched()` resolves with the next fetch input; `reply` decides its
// response. A set `error` makes the engine's step throw after its first chunk.
function world(on: any, env: Record<string, string | undefined>) {
  const waiting: ((e: any) => void)[] = [];
  const queued: any[] = [];
  const reads: string[] = [];
  const w = {
    env,
    reads,
    result: undefined as any,
    error: undefined as Error | undefined,
    session: 'sess-1' as string | undefined,
    reply: (_e: any): Promise<any> | any => OK,
    fetched: () => new Promise<any>((r) => (queued.length ? r(queued.shift()) : waiting.push(r))),
    // Called with each env name read, before its value is returned.
    onRead: (_name: string) => {},
    // Called on each session id read, in the same segment that reads `session`.
    onSession: () => {},
    // Replies settled so far; `onSettle` runs after each one.
    settled: 0,
    onSettle: () => {},
  };
  on('env.get', async (_$: unknown, e: any) => {
    reads.push(e.name);
    w.onRead(e.name);
    return { value: w.env[e.name] };
  });
  on('session.id', async () => {
    w.onSession();
    return { value: w.session };
  });
  on('http.fetch', async (_$: unknown, e: any) => {
    const wake = waiting.shift();
    if (wake) wake(e);
    else queued.push(e);
    const value = await w.reply(e);
    w.settled++;
    w.onSettle();
    return { value };
  });
  // The engine's stream: two chunks, then the world's current result.
  on('turn.step', async function* () {
    yield { kind: 'text', index: 0, text: 'h' };
    if (w.error) throw w.error;
    yield { kind: 'text', index: 0, text: 'i' };
    return w.result;
  });
  return w;
}

// Drives one step that returns `result` through the plugin's observer.
async function step($: any, w: any, result: any) {
  w.result = result;
  const stream = $.turn.step({ turnId: result.turnId, index: result.index, model: 'fallback-model', messageCount: 1 });
  // Read by hand: on 2.1.296 `stream.result` settles undefined after a for-await.
  const steps = [await stream.next(), await stream.next(), await stream.next()];
  expect(steps.map((s: any) => s.done)).toEqual([false, false, true]);
  expect(steps.slice(0, 2).map((s: any) => s.value.text)).toEqual(['h', 'i']);
  expect(steps[2].value).toEqual(result);
}

function result(index: number, usage: any = { ...FULL, model: 'claude-haiku' }) {
  return { turnId: 't-1', index, answer: 'hi', toolUses: [], stopReason: 'end_turn', usage };
}

describe('turn.step usage observer', () => {
  test('module off: chunks and result pass through, nothing is sent', OPTS, async ($, on) => {
    const w = world(on, {});
    const offRead = new Promise<void>((r) => (w.onRead = (n) => n === 'DESLAG_USAGE_STATS' && r()));
    await step($, w, result(0));
    // Turned on for the second step only, after step 0 read it as off: exactly one record arrives.
    await offRead;
    w.onRead = () => {};
    w.env.DESLAG_USAGE_STATS = '1';
    w.env.DESLAG_STATE_DIR = '/state';
    const sent = w.fetched();
    await step($, w, result(1));
    const first = JSON.parse((await sent).init.body);
    expect([first.index, first.dropped]).toEqual([1, 0]);
    expect(w.reads.filter((n) => n === 'DESLAG_STATE_DIR')).toEqual(['DESLAG_STATE_DIR']);
  });

  test('module on: the record reaches the service socket', OPTS, async ($, on) => {
    const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: '/state' });
    let sent = w.fetched();
    await step($, w, result(0));
    const e = await sent;
    expect(e.url).toBe('http://deslag/usage');
    expect(e.init.method).toBe('POST');
    expect(e.init.socketPath).toBe('/state/local-http.sock');
    expect(JSON.parse(e.init.body)).toEqual({
      kind: 'usage', session: 'sess-1', turn: 't-1', index: 0, model: 'claude-haiku', usage: FULL, dropped: 0,
    });

    // No usage, or a partial one, is null, and the model falls back to the step's.
    for (const usage of [null, { input_tokens: 5 }]) {
      sent = w.fetched();
      await step($, w, result(1, usage));
      const body = JSON.parse((await sent).init.body);
      expect([body.usage, body.model]).toEqual([null, 'fallback-model']);
    }
  });

  test('a relative state dir falls back to XDG_STATE_HOME', OPTS, async ($, on) => {
    const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: 'rel', XDG_STATE_HOME: '/xdg' });
    const sent = w.fetched();
    await step($, w, result(0));
    expect((await sent).init.socketPath).toBe('/xdg/deslag/local-http.sock');
  });

  test('service absent or busy: the stream is unchanged and drops are counted', OPTS, async ($, on) => {
    const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: '/state' });
    let calls = 0;
    w.reply = () => {
      calls++;
      if (calls === 1) throw new Error('ENOENT');
      if (calls === 2) return { status: 503, ok: false, headers: {}, text: '' };
      return OK;
    };
    for (const index of [0, 1]) {
      const sent = w.fetched();
      await step($, w, result(index));
      await sent;
    }
    // The failures settle after their fetch hook returns, so a later record carries them.
    let carried = 0;
    for (let index = 2; index < 6 && carried < 2; index++) {
      const sent = w.fetched();
      await step($, w, result(index));
      carried += JSON.parse((await sent).init.body).dropped;
    }
    expect(carried).toBe(2);
  });

  test('a step over four sends in flight is dropped and counted', OPTS, async ($, on) => {
    const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: '/state' });
    const release: (() => void)[] = [];
    w.reply = () => new Promise((r) => release.push(() => r(OK)));
    for (const index of [0, 1, 2, 3]) {
      const sent = w.fetched();
      await step($, w, result(index));
      await sent;
    }
    // The cap is checked when the step settles, so releasing right after it cannot race.
    await step($, w, result(4));
    w.reply = () => OK;
    // Step 5 waits until the four released sends settled, so it is never capped.
    const allSettled = new Promise<void>((r) => (w.onSettle = () => w.settled === 4 && r()));
    for (const r of release) r();
    await allSettled;
    // The fetch hook signals before usage.js runs its `finally`. One macrotask
    // drains every microtask hop back into it, so inFlight is 0 afterwards.
    await new Promise((r) => setTimeout(r, 0));
    let carried = 0;
    for (let index = 5; index < 9 && carried < 1; index++) {
      const sent = w.fetched();
      await step($, w, result(index));
      carried += JSON.parse((await sent).init.body).dropped;
    }
    expect(carried).toBe(1);
  });

  test('session and turn of 1-64 bytes [A-Za-z0-9_-] are sent', OPTS, async ($, on) => {
    const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: '/state' });
    w.session = 'aZ09_-'.repeat(10) + 'abcd';
    for (const turn of ['t', 'Az-_9'.repeat(12) + 'abcd']) {
      const sent = w.fetched();
      await step($, w, { ...result(0), turnId: turn });
      const body = JSON.parse((await sent).init.body);
      expect([body.session, body.turn]).toEqual([w.session, turn]);
    }
  });

  test('model of 1-64 printable ASCII bytes without quote or backslash is sent', OPTS, async ($, on) => {
    const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: '/state' });
    for (const model of ['m', ' !#[]~ claude/haiku-4.5 (x)'.padEnd(64, '~')]) {
      const sent = w.fetched();
      await step($, w, result(0, { ...FULL, model }));
      expect(JSON.parse((await sent).init.body).model).toBe(model);
    }
  });

  // Each invalid record is a local drop that the next valid record carries.
  const invalid: [string, (w: any, r: any) => void][] = [
    ['undefined session id', (w) => (w.session = undefined)],
    ['turn id over 64 bytes', (_w, r) => (r.turnId = 't'.repeat(65))],
    ["turn id containing ':'", (_w, r) => (r.turnId = 't:1')],
    ['empty model', (_w, r) => (r.usage.model = '')],
    ['model containing \'"\'', (_w, r) => (r.usage.model = 'claude"haiku')],
    ["model containing '\\'", (_w, r) => (r.usage.model = 'claude\\haiku')],
    ['missing index', (_w, r) => (r.index = undefined)],
  ];
  for (const [form, spoil] of invalid) {
    test(`${form}: no send, counted as a drop`, OPTS, async ($, on) => {
      const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: '/state' });
      const bad = result(0);
      spoil(w, bad);
      // Step 0's send reads the session after the step returns. Reset it only after that read.
      const sessionRead = new Promise<void>((r) => (w.onSession = r));
      await step($, w, bad);
      await sessionRead;
      w.onSession = () => {};
      w.session = 'sess-1';
      let carried = 0;
      for (let index = 1; index < 5 && carried < 1; index++) {
        const sent = w.fetched();
        await step($, w, result(index));
        const body = JSON.parse((await sent).init.body);
        expect(body.index).toBe(index);
        carried += body.dropped;
      }
      expect(carried).toBe(1);
    });
  }

  test('a failing step rejects unchanged and still sends a record', OPTS, async ($, on) => {
    const w = world(on, { DESLAG_USAGE_STATS: '1', DESLAG_STATE_DIR: '/state' });
    w.error = new Error('provider failed');
    const sent = w.fetched();
    const stream = $.turn.step({ turnId: 't-1', index: 3, model: 'fallback-model', messageCount: 1 });
    expect((await stream.next()).value.text).toBe('h');
    // The engine turns a throwing hook into a skip and fails the step with its own
    // HooksError (2.1.299); that error reaches the caller through the observer unchanged.
    const failure = await stream.next().then(() => undefined, (e: any) => e);
    expect([failure?.name, failure?.message]).toEqual(['HooksError', 'no implementation for turn.step']);
    const body = JSON.parse((await sent).init.body);
    expect([body.index, body.model, body.usage]).toEqual([3, 'fallback-model', null]);
  });
});
