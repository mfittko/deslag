// Experimental deslag Claude Code mod (spike, not shipped). One usage
// observer on `turn.step` feeds bounded in-memory counters. The mod never
// compresses content; the PostToolUse hook and the proxy own compression.

// Bounded metadata counters: fixed keys, numbers only, no payloads or ids.
export function newStats() {
  return {
    usage: { steps: 0, unknown: 0, input: 0, output: 0, cache_read: 0, cache_creation: 0 },
  };
}

export const stats = newStats();

const USAGE_FIELDS = ['input_tokens', 'output_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens'];

// Unknown usage stays unknown: no usage, a partial usage object, or a step
// that threw or was closed early adds no zeros.
export function recordUsage(stats, result) {
  stats.usage.steps++;
  const u = result?.usage;
  if (!u || !USAGE_FIELDS.every((f) => Number.isFinite(u[f]))) {
    stats.usage.unknown++;
    return;
  }
  stats.usage.input += u.input_tokens;
  stats.usage.output += u.output_tokens;
  stats.usage.cache_read += u.cache_read_input_tokens;
  stats.usage.cache_creation += u.cache_creation_input_tokens;
}

async function* observeUsage($, e, next) {
  let result;
  try {
    // Forwards every chunk unbuffered and keeps the final result.
    result = yield* next(e);
    return result;
  } finally {
    try {
      recordUsage(stats, result);
    } catch {
      // Metrics never replace or retry the provider request.
    }
  }
}

export function register(on) {
  on('turn.step', observeUsage);
}
