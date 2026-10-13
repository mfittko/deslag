// Experimental deslag Claude Code mod (spike, not shipped). One entrypoint with
// two internal modules: a compression adapter on `session.append` and a usage
// observer on `turn.step`. Both feed bounded in-memory counters.
//
// Configuration: plugin options `mode` (off | observe | safe, default off) and
// `binary` (absolute path to a deslag build with `codec --protocol 1`). An
// unset option falls back to DESLAG_MOD_MODE / DESLAG_MOD_BINARY. Without a
// valid mode and binary, every row passes through unchanged.

const MODES = new Set(['off', 'observe', 'safe']);
// ponytail: one fixed bridge budget; make it an option once live numbers exist.
const BRIDGE_TIMEOUT_MS = 2000;
const POLICY_VERSION = 'spike-1';
const utf8 = new TextEncoder();

export function category(origin) {
  if (origin?.kind !== 'tool') return null;
  if (origin.tool === 'Bash') return 'bash';
  if (typeof origin.tool === 'string' && origin.tool.startsWith('mcp__')) return 'mcp';
  return null;
}

// Approved text slots of a tool-result row: a Bash result's whole string
// content, or the text blocks of an MCP result. Error results, other tools and
// any other shape yield no slots, so the row passes through.
export function slots(message, tool) {
  const found = [];
  if (!Array.isArray(message?.content)) return [];
  for (const [b, block] of message.content.entries()) {
    if (block?.type !== 'tool_result') continue;
    if (block.is_error === true) return [];
    if (tool === 'bash' && typeof block.content === 'string') {
      found.push({ slot: found.length, block: b, inner: null, text: block.content });
    } else if (tool === 'mcp' && Array.isArray(block.content)) {
      for (const [i, inner] of block.content.entries()) {
        if (inner?.type === 'text' && typeof inner.text === 'string') {
          found.push({ slot: found.length, block: b, inner: i, text: inner.text });
        }
      }
    } else {
      return [];
    }
  }
  return found;
}

// Validates one codec response against the request's slots. Returns the
// per-slot items in slot order, or null when anything is off: nonzero exit,
// truncated or extra output, wrong version, missing or extra slots, or a
// replacement whose UTF-8 length disagrees with the declared one.
export function parseResponse(result, want) {
  if (result?.exitCode !== 0 || typeof result.stdout !== 'string') return null;
  let response;
  try {
    response = JSON.parse(result.stdout);
  } catch {
    return null;
  }
  if (response?.schema_version !== 1 || response.complete !== true) return null;
  if (response.policy_version !== POLICY_VERSION) return null;
  if (!Array.isArray(response.items) || response.items.length !== want.length) return null;
  const items = [];
  for (const s of want) {
    const item = response.items.find((it) => it?.slot === s.slot);
    if (!item) return null;
    if (item.outcome === 'changed') {
      if (typeof item.replacement !== 'string') return null;
      if (utf8.encode(item.replacement).length !== item.replacement_utf8_bytes) return null;
    } else if (!['would_change', 'skipped', 'failed'].includes(item.outcome)) {
      return null;
    }
    items.push(item);
  }
  return items;
}

// A copy of the row with every changed slot replaced; the frozen input is untouched.
export function rewrite(message, want, items) {
  const content = message.content.map((block) =>
    Array.isArray(block.content) ? { ...block, content: [...block.content] } : { ...block },
  );
  for (const [k, s] of want.entries()) {
    if (items[k].outcome !== 'changed') continue;
    const block = content[s.block];
    if (s.inner === null) block.content = items[k].replacement;
    else block.content[s.inner] = { ...block.content[s.inner], text: items[k].replacement };
  }
  return { ...message, content };
}

function storedText(message, s) {
  const block = message?.content?.[s.block];
  return s.inner === null ? block?.content : block?.content?.[s.inner]?.text;
}

// Bounded metadata counters: fixed keys, numbers only, no payloads or ids.
// `slots` counts approved text slots by outcome; a bridge error or a failed
// slot counts every slot of its row, because the whole row stayed unchanged.
export function newStats() {
  return {
    slots: { changed: 0, would_change: 0, not_applied: 0, skipped: 0, failed: 0, overridden: 0, bridge_error: 0 },
    bytes: { original: 0, candidate: 0, stored: 0 },
    usage: { steps: 0, unknown: 0, input: 0, output: 0, cache_read: 0, cache_creation: 0 },
  };
}

// Per-load state. The engine requires hooks to be named functions or
// literals, so they read configuration from here rather than a closure.
const config = { mode: undefined, binary: undefined };
export const stats = newStats();

async function compressToolResult($, e, next) {
  let outgoing = e;
  let want = [];
  let items = null;
  let mode = 'off';
  try {
    const tool = category(e.origin);
    mode = pickMode(config.mode || (await $.env.get('DESLAG_MOD_MODE')));
    const binary = pickBinary(config.binary || (await $.env.get('DESLAG_MOD_BINARY')));
    want = tool && mode !== 'off' && binary ? slots(e.message, tool) : [];
    if (want.length > 0) {
      const request = {
        schema_version: 1,
        policy_version: POLICY_VERSION,
        mode,
        tool_category: tool,
        items: want.map(({ slot, text }) => ({ slot, text })),
      };
      const result = await $.process.run([binary, 'codec', '--protocol', '1'], {
        stdin: JSON.stringify(request),
        timeoutMs: BRIDGE_TIMEOUT_MS,
      });
      items = parseResponse(result, want);
    }
    if (items && mode === 'safe' && !next.signal?.aborted && !items.some(failed)) {
      outgoing = { ...e, message: rewrite(e.message, want, items) };
    }
  } catch {
    // Any preparation or bridge failure stores the original row.
    outgoing = e;
    items = null;
  }
  // Outside any catch: next runs exactly once, and its failure propagates.
  const stored = await next(outgoing);
  try {
    if (want.length > 0) record(stats, want, items, stored, mode, outgoing !== e);
  } catch {
    // Accounting never changes the stored row.
  }
  return stored;
}

function failed(item) {
  return item.outcome === 'failed';
}

// Counts a replacement as changed only when the stored row holds it.
export function record(stats, want, items, stored, mode, applied) {
  for (const s of want) stats.bytes.original += utf8.encode(s.text).length;
  if (items === null) {
    stats.slots.bridge_error += want.length;
    return;
  }
  if (items.some(failed)) {
    stats.slots.failed += want.length;
    return;
  }
  for (const [k, s] of want.entries()) {
    const item = items[k];
    const kept = storedText(stored?.message, s);
    if (typeof kept === 'string') stats.bytes.stored += utf8.encode(kept).length;
    if (item.outcome === 'changed' && applied) {
      stats.bytes.candidate += item.replacement_utf8_bytes;
      stats.slots[kept === item.replacement ? 'changed' : 'overridden']++;
    } else if (item.outcome === 'changed' && mode === 'safe') {
      // Proposed in safe mode but not applied (the append was aborted).
      stats.slots.not_applied++;
    } else if (item.outcome === 'changed' || item.outcome === 'would_change') {
      stats.slots.would_change++;
    } else {
      stats.slots.skipped++;
    }
  }
}

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

export function register(on, options) {
  config.mode = options?.mode;
  config.binary = options?.binary;
  on('session.append', { door: 'tool-result' }, compressToolResult);
  on('turn.step', observeUsage);
}

export function pickMode(value) {
  return MODES.has(value) ? value : 'off';
}

export function pickBinary(value) {
  return typeof value === 'string' && value.startsWith('/') ? value : null;
}
