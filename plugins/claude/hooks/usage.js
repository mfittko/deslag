// deslag usage module. One observer on `turn.step` sends the provider usage
// of each model request to the local stats service (`deslag local`). It never
// compresses content; the PostToolUse hook and the proxy own compression.
//
// Opt-in: it sends only when DESLAG_USAGE_STATS=1. The stream path never
// awaits I/O: every chunk and the final result pass through unchanged, and
// the send starts in the background after the step settles.

const USAGE_FIELDS = ['input_tokens', 'output_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens'];

// Sends in flight. A step over the cap is dropped and counted.
const MAX_IN_FLIGHT = 4;
let inFlight = 0;
// Records not delivered yet, carried in the next record's `dropped` field.
let dropped = 0;
// Settles when the send that carries drops knows its outcome. Later sends wait
// for it, so no record overtakes the carrier and leaves its drops uncarried.
let carrier = null;

// Unknown usage stays unknown: no usage or a partial usage object is null,
// never zero.
export function usageOf(result) {
  const u = result?.usage;
  if (!u || !USAGE_FIELDS.every((f) => Number.isSafeInteger(u[f]) && u[f] >= 0)) return null;
  return Object.fromEntries(USAGE_FIELDS.map((f) => [f, u[f]]));
}

// deslag_telemetry::valid_id: 1 to 64 bytes of [A-Za-z0-9_-].
const validId = (s) => typeof s === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(s);
// The service's valid_model: 1 to 64 bytes of printable ASCII without '"' and '\'.
const validModel = (s) => typeof s === 'string' && /^[ -~]{1,64}$/.test(s) && !/["\\]/.test(s);
// The service reads index as a u64.
const validIndex = (n) => Number.isSafeInteger(n) && n >= 0;

// Same order as deslag_telemetry::state_dir; empty or relative values are ignored.
async function socketPath($) {
  const abs = (v) => (typeof v === 'string' && v.startsWith('/') ? v : undefined);
  const dir =
    abs(await $.env.get('DESLAG_STATE_DIR')) ??
    abs(await $.env.get('XDG_STATE_HOME'))?.concat('/deslag') ??
    abs(await $.env.get('HOME'))?.concat('/.local/state/deslag');
  return dir && `${dir}/local-http.sock`;
}

// Resolves to `{ session, socket }` when the record was delivered, `false`
// when its delivery failed, `null` when it was dropped without a send, and
// undefined when the module is off.
async function send($, e, result) {
  // Decided before the first await: the cap applies when the step settles.
  const over = inFlight >= MAX_IN_FLIGHT;
  if (!over) inFlight++;
  try {
    if ((await $.env.get('DESLAG_USAGE_STATS')) !== '1') return;
    if (over) {
      dropped++;
      return null;
    }
    while (carrier) await carrier;
    // Swap, not read: concurrent sends never carry the same drops twice.
    const carried = dropped;
    dropped = 0;
    let settle;
    if (carried) carrier = new Promise((r) => (settle = r));
    try {
      const session = await $.session.id();
      const model = result?.usage?.model ?? e.model;
      const index = result?.index ?? e.index;
      // The service rejects a record that fails these rules after the bridge
      // answered 204, so the record and its carried drops would go uncounted.
      if (!validId(session) || !validId(e.turnId) || !validModel(model) || !validIndex(index)) {
        dropped += carried + 1;
        return null;
      }
      const socket = await socketPath($);
      if (!socket) {
        dropped += carried + 1;
        return null;
      }
      const body = JSON.stringify({
        kind: 'usage',
        session,
        turn: e.turnId,
        index,
        model,
        usage: usageOf(result),
        dropped: carried,
      });
      const res = await $.http.fetch('http://deslag/usage', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'content-length': String(new TextEncoder().encode(body).length) },
        body,
        socketPath: socket,
      });
      if (res?.ok) return { session, socket };
      dropped += carried + 1;
      return false;
    } catch {
      dropped += carried + 1;
      return false;
    } finally {
      if (settle) {
        carrier = null;
        settle();
      }
    }
  } finally {
    if (!over) inFlight--;
  }
}

// Session reads not settled yet. A refresh over the cap is skipped.
// ponytail: $.http.fetch has no timeout or abort, so a hung host fetch holds
// its slot until it settles; at the cap the line stops refreshing. Abort the
// fetch instead once the host offers it.
const MAX_READS = 2;
let reads = 0;
const READ_TIMEOUT_MS = 1000;

const count = (n) => (Number.isSafeInteger(n) ? n.toLocaleString('en-US') : 'unknown');
const estimate = (n) => (Number.isSafeInteger(n) ? `~${count(n)}` : 'unknown');

// The widget line for a `deslag stats --json --session <id>` document: the
// last request's estimated removed tokens next to its provider input tokens,
// and the session total. Throws on a document without requests.
export function savingsLine(doc) {
  const last = doc.requests.at(-1);
  if (doc.schema_version !== 1 || !last) throw new Error('no request');
  const u = last.usage;
  const input = u && u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens;
  return (
    `deslag: ${estimate(last.estimated_removed_tokens)} tokens removed (estimate) of ${count(input)} input tokens` +
    ` · session ${estimate(doc.totals?.estimated_removed_tokens)} removed (estimate)`
  );
}

async function readLine($, { session, socket }) {
  const res = await $.http.fetch(`http://deslag/session/${session}`, { socketPath: socket });
  if (!res?.ok) throw new Error(`status ${res?.status}`);
  return savingsLine(JSON.parse(res.text));
}

// Bumped by each refresh that sets the line; only the latest one writes it.
let generation = 0;

// Opt-in with DESLAG_SAVINGS_WIDGET=1. Shows the line after a delivered send,
// and clears it on a failed delivery, a failed read or a read slower than
// READ_TIMEOUT_MS. A record dropped without a send (send cap, invalid ids, no
// state dir) or a refresh over the read cap leaves the line unchanged.
async function refresh($, sent) {
  if (sent == null || (await $.env.get('DESLAG_SAVINGS_WIDGET')) !== '1') return;
  if (sent && reads >= MAX_READS) return;
  const mine = ++generation;
  let text;
  if (sent) {
    reads++;
    const read = readLine($, sent).finally(() => reads--);
    const timeout = $.clock.sleep(READ_TIMEOUT_MS).then(() => undefined);
    text = await Promise.race([read, timeout]).catch(() => undefined);
  }
  // A newer refresh owns the line: this read's late text or timeout is ignored.
  if (mine === generation) await $.ui.status(text);
}

async function* observeUsage($, e, next) {
  let result;
  try {
    // Forwards every chunk unbuffered and keeps the final result.
    result = yield* next(e);
    return result;
  } finally {
    // Not awaited: usage and the widget never delay, replace or retry the
    // provider request.
    send($, e, result)
      .then((sent) => refresh($, sent))
      .catch(() => {});
  }
}

export function register(on) {
  on('turn.step', observeUsage);
}
