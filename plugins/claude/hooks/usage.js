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

// Unknown usage stays unknown: no usage or a partial usage object is null,
// never zero.
export function usageOf(result) {
  const u = result?.usage;
  if (!u || !USAGE_FIELDS.every((f) => Number.isSafeInteger(u[f]) && u[f] >= 0)) return null;
  return Object.fromEntries(USAGE_FIELDS.map((f) => [f, u[f]]));
}

// Same order as deslag_telemetry::state_dir; empty or relative values are ignored.
async function socketPath($) {
  const abs = (v) => (typeof v === 'string' && v.startsWith('/') ? v : undefined);
  const dir =
    abs(await $.env.get('DESLAG_STATE_DIR')) ??
    abs(await $.env.get('XDG_STATE_HOME'))?.concat('/deslag') ??
    abs(await $.env.get('HOME'))?.concat('/.local/state/deslag');
  return dir && `${dir}/local-http.sock`;
}

async function send($, e, result) {
  if ((await $.env.get('DESLAG_USAGE_STATS')) !== '1') return;
  if (inFlight >= MAX_IN_FLIGHT) {
    dropped++;
    return;
  }
  inFlight++;
  // Swap, not read: concurrent sends never carry the same drops twice.
  const carried = dropped;
  dropped = 0;
  try {
    const socket = await socketPath($);
    const body = JSON.stringify({
      kind: 'usage',
      session: await $.session.id(),
      turn: e.turnId,
      index: result?.index ?? e.index,
      model: result?.usage?.model ?? e.model,
      usage: usageOf(result),
      dropped: carried,
    });
    const res = socket
      ? await $.http.fetch('http://deslag/usage', {
          method: 'POST',
          headers: { 'content-type': 'application/json', 'content-length': String(new TextEncoder().encode(body).length) },
          body,
          socketPath: socket,
        })
      : undefined;
    if (!res?.ok) dropped += carried + 1;
  } catch {
    dropped += carried + 1;
  } finally {
    inFlight--;
  }
}

async function* observeUsage($, e, next) {
  let result;
  try {
    // Forwards every chunk unbuffered and keeps the final result.
    result = yield* next(e);
    return result;
  } finally {
    // Not awaited: usage never delays, replaces or retries the provider request.
    send($, e, result).catch(() => {});
  }
}

export function register(on) {
  on('turn.step', observeUsage);
}
