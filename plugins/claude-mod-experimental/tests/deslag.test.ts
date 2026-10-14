// Run with `claude plugin test plugins/claude-mod-experimental`. The engine's
// own session.append and turn.step chains run; process.run is answered
// beneath the mod, so no binary, network or model is involved.
import { describe, expect, mock, test } from 'claude-code/testing';
import { newStats, parseResponse, record, recordUsage, slots } from '../hooks/deslag.js';

const PRETTY_MCP = '[\n  {\n    "id": 1,\n    "name": "äöü"\n  }\n]';
// Captured from `deslag codec --protocol 1` (release build of this branch,
// safe mode, owner mod) for the request this file's first test sends.
const REAL_RESPONSE =
  '{"complete":true,"items":[{"outcome":"changed","reason":null,"replacement":"[{\\"id\\":1,\\"name\\":\\"äöü\\"}]","replacement_utf8_bytes":26,"report":{"candidate_bytes":26,"candidate_tokens":null,"compression_duration":20958,"emitted_bytes":26,"emitted_tokens":null,"format_version":null,"mode":"safe","original_bytes":45,"original_tokens":null,"outcome":"changed","policy_version":1,"schema_version":1,"skip_or_failure_reason":null,"transform_id":"json-minify","transform_version":1,"validation_outcome":"passed"},"slot":0},{"outcome":"skipped","reason":"unsupported_format","replacement":null,"replacement_utf8_bytes":null,"report":{"candidate_bytes":null,"candidate_tokens":null,"compression_duration":125,"emitted_bytes":5,"emitted_tokens":null,"format_version":null,"mode":"safe","original_bytes":5,"original_tokens":null,"outcome":"skipped","policy_version":1,"schema_version":1,"skip_or_failure_reason":"unsupported_format","transform_id":"json-minify","transform_version":1,"validation_outcome":"not_run"},"slot":1}],"policy_version":"spike-1","schema_version":1}\n';

const changed = (slot: number, replacement: string) => ({
  slot,
  outcome: 'changed',
  replacement,
  replacement_utf8_bytes: new TextEncoder().encode(replacement).length,
});
const envelope = (items: unknown[]) => JSON.stringify({ schema_version: 1, policy_version: 'spike-1', complete: true, items });

function bash(content: string, extra = {}) {
  return {
    door: 'tool-result',
    origin: { kind: 'tool', tool: 'Bash' },
    uuid: 'row-1',
    message: { type: 'user', role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_1', content, ...extra }] },
  };
}

function mcp(content: unknown[]) {
  return {
    door: 'tool-result',
    origin: { kind: 'tool', tool: 'mcp__srv__fetch' },
    uuid: 'row-2',
    message: { type: 'user', role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_2', content }] },
  };
}

// Answers process.run beneath the mod and records every call.
function bridge(on: any, answer: (e: any) => unknown) {
  const calls: any[] = [];
  on('process.run', (_$: unknown, e: any) => {
    calls.push(e);
    return answer(e);
  });
  return calls;
}

describe('session.append compression', () => {
  test('safe mode stores the codec replacement and keeps the tool_use_id', async ($, on) => {
    mock.env(on, { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' });
    const calls = bridge(on, () => ({ value: { exitCode: 0, stdout: REAL_RESPONSE, stderr: '' } }));
    const stored = await $.session.append(mcp([{ type: 'text', text: PRETTY_MCP }, { type: 'text', text: 'plain' }]));
    expect(stored.message.content[0].tool_use_id).toBe('toolu_2');
    expect(stored.message.content[0].content).toEqual([
      { type: 'text', text: '[{"id":1,"name":"äöü"}]' },
      { type: 'text', text: 'plain' },
    ]);
    expect(calls).toHaveLength(1);
    // The payload crosses on stdin only; argv carries no content.
    expect(calls[0].argv).toEqual(['/opt/deslag', 'codec', '--protocol', '1']);
    expect(JSON.parse(calls[0].init.stdin).items).toEqual([
      { slot: 0, text: PRETTY_MCP },
      { slot: 1, text: 'plain' },
    ]);
  });

  test('media blocks keep their place beside rewritten text', async ($, on) => {
    mock.env(on, { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' });
    bridge(on, () => ({ value: { exitCode: 0, stdout: envelope([changed(0, '{}')]), stderr: '' } }));
    const image = { type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'AAAA' } };
    const stored = await $.session.append(mcp([image, { type: 'text', text: '{ }' }]));
    expect(stored.message.content[0].content).toEqual([image, { type: 'text', text: '{}' }]);
  });

  test('observe mode asks the codec but stores the original', async ($, on) => {
    mock.env(on, { DESLAG_MOD_MODE: 'observe', DESLAG_MOD_BINARY: '/opt/deslag' });
    const calls = bridge(on, () => ({
      value: { exitCode: 0, stdout: envelope([{ slot: 0, outcome: 'would_change', replacement: null }]), stderr: '' },
    }));
    const stored = await $.session.append(bash('{\n "a": 1\n}'));
    expect(stored.message.content[0].content).toBe('{\n "a": 1\n}');
    expect(JSON.parse(calls[0].init.stdin).mode).toBe('observe');
  });

  const neverBridged: Array<[string, Record<string, string>, any]> = [
    ['off mode', { DESLAG_MOD_MODE: 'off', DESLAG_MOD_BINARY: '/opt/deslag' }, bash('{ }')],
    ['a relative binary', { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: 'deslag' }, bash('{ }')],
    ['a Read row', { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' }, { ...bash('{ }'), origin: { kind: 'tool', tool: 'Read' } }],
    ['a model row', { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' }, { ...bash('{ }'), origin: { kind: 'model', model: 'm' } }],
    ['an is_error row', { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' }, bash('{ }', { is_error: true })],
  ];
  for (const [name, env, row] of neverBridged) {
    test(`${name} never calls the bridge`, async ($, on) => {
      const calls = bridge(on, () => ({ value: { exitCode: 0, stdout: envelope([changed(0, 'X')]), stderr: '' } }));
      mock.env(on, env);
      const stored = await $.session.append(row);
      expect(stored.message.content[0].content).toBe('{ }');
      expect(calls).toHaveLength(0);
    });
  }

  const broken: Array<[string, (e: any) => unknown]> = [
    ['nonzero exit', () => ({ value: { exitCode: 65, stdout: '', stderr: 'bad' } })],
    ['truncated stdout', () => ({ value: { exitCode: 0, stdout: envelope([changed(0, 'X')]).slice(0, 40), stderr: '' } })],
    ['wrong byte count', () => ({ value: { exitCode: 0, stdout: envelope([{ ...changed(0, 'ä'), replacement_utf8_bytes: 1 }]), stderr: '' } })],
    ['missing slot', () => ({ value: { exitCode: 0, stdout: envelope([]), stderr: '' } })],
    ['extra slot', () => ({ value: { exitCode: 0, stdout: envelope([changed(0, 'X'), changed(1, 'Y')]), stderr: '' } })],
    ['incomplete', () => ({ value: { exitCode: 0, stdout: JSON.stringify({ schema_version: 1, items: [changed(0, 'X')] }), stderr: '' } })],
    ['denied or timed out', () => ({ deny: 'timeout' })],
    ['policy mismatch', () => ({ value: { exitCode: 0, stdout: envelope([changed(0, 'X')]).replace('spike-1', 'other'), stderr: '' } })],
  ];
  for (const [name, answer] of broken) {
    test(`${name}: the original is stored once`, async ($, on) => {
      mock.env(on, { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' });
      bridge(on, answer);
      let appends = 0;
      on('session.append', (_$: unknown, e: any, next: any) => {
        appends++;
        return next(e);
      });
      const stored = await $.session.append(bash('{\n "a": 1\n}'));
      expect(stored.message.content[0].content).toBe('{\n "a": 1\n}');
      expect(appends).toBe(1);
    });
  }

  test('unexpected row shapes are stored once without a bridge call', async ($, on) => {
    mock.env(on, { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' });
    const calls = bridge(on, () => ({ value: { exitCode: 0, stdout: envelope([changed(0, 'X')]), stderr: '' } }));
    let appends = 0;
    on('session.append', (_$: unknown, e: any, next: any) => {
      appends++;
      return next(e);
    });
    const rows = [mcp('{ }' as any), mcp([null, { type: 'text' }] as any), bash(['{ }'] as any)];
    for (const row of rows) {
      const stored = await $.session.append(row);
      expect(stored.message.content[0].content).toEqual(row.message.content[0].content);
    }
    expect(appends).toBe(rows.length);
    expect(calls).toHaveLength(0);
  });

  test('one failed slot leaves every slot of the row unchanged', async ($, on) => {
    mock.env(on, { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' });
    bridge(on, () => ({
      value: { exitCode: 0, stdout: envelope([changed(0, '{}'), { slot: 1, outcome: 'failed', replacement: null }]), stderr: '' },
    }));
    const stored = await $.session.append(mcp([{ type: 'text', text: '{ }' }, { type: 'text', text: '{ }' }]));
    expect(stored.message.content[0].content).toEqual([{ type: 'text', text: '{ }' }, { type: 'text', text: '{ }' }]);
  });
});

describe('turn.step observer', () => {
  test('forwards every chunk and the final result unchanged', async ($, on) => {
    const result = {
      turnId: 't', index: 0, answer: 'hi', toolUses: [], stopReason: 'end_turn',
      usage: { input_tokens: 3, output_tokens: 1, cache_read_input_tokens: 0, cache_creation_input_tokens: 0, model: 'm' },
    };
    on('turn.step', async function* (_$: unknown, e: any) {
      expect(e.turnId).toBe('t');
      yield { kind: 'text', index: 0, text: 'h' };
      yield { kind: 'text', index: 0, text: 'i' };
      return result;
    });
    const stream = $.turn.step({ turnId: 't', index: 0, model: 'm', messageCount: 1 });
    // Read by hand: on 2.1.296 `stream.result` settles undefined after a for-await.
    const steps = [await stream.next(), await stream.next(), await stream.next()];
    expect(steps.map((s: any) => s.done)).toEqual([false, false, true]);
    expect(steps.slice(0, 2).map((s: any) => s.value.text)).toEqual(['h', 'i']);
    expect(steps[2].value).toEqual(result);
  });
});

describe('pure helpers', () => {
  test('Bash takes the whole string content; other shapes take nothing', () => {
    const row = (content: unknown, extra = {}) => ({ content: [{ type: 'tool_result', content, ...extra }] });
    expect(slots(row('x'), 'bash')).toEqual([{ slot: 0, block: 0, inner: null, text: 'x' }]);
    expect(slots(row([{ type: 'text', text: 'x' }]), 'bash')).toEqual([]);
    expect(slots(row('x'), 'mcp')).toEqual([]);
    expect(slots(row('x', { is_error: true }), 'bash')).toEqual([]);
  });

  test('a stored row that differs from the replacement counts as overridden', () => {
    const want = [{ slot: 0, block: 0, inner: null, text: '{ }' }];
    const stats = newStats();
    record(stats, want, [changed(0, '{}')], { message: { content: [{ content: 'other' }] } }, 'safe', true);
    record(stats, want, [changed(0, '{}')], { message: { content: [{ content: '{}' }] } }, 'safe', true);
    record(stats, want, [changed(0, '{}')], { message: { content: [{ content: '{ }' }] } }, 'safe', false);
    record(stats, want, null, null, 'safe', false);
    expect(stats.slots).toMatchObject({ overridden: 1, changed: 1, not_applied: 1, would_change: 0, bridge_error: 1 });
    expect(stats.bytes).toEqual({ original: 12, candidate: 4, stored: 10 });
  });

  test('a step without usage is unknown, never zero', () => {
    const stats = newStats();
    recordUsage(stats, { usage: null });
    recordUsage(stats, { usage: { input_tokens: 5, output_tokens: 2, cache_read_input_tokens: 1, cache_creation_input_tokens: 0 } });
    recordUsage(stats, { usage: { input_tokens: 9 } });
    recordUsage(stats, undefined);
    expect(stats.usage).toEqual({ steps: 4, unknown: 3, input: 5, output: 2, cache_read: 1, cache_creation: 0 });
  });

  test('parseResponse rejects a non-object envelope', () => {
    const want = [{ slot: 0 }];
    expect(parseResponse({ exitCode: 0, stdout: 'null' }, want)).toBeNull();
    expect(parseResponse({ exitCode: 0, stdout: envelope([{ slot: 0, outcome: 'bogus' }]) }, want)).toBeNull();
  });
});

// Another user-tier mod that rewrites the same row. Its hooks are serialized
// into a plugin of its own, so they cannot close over this file.
const other = {
  name: 'other-rewriter',
  register(on: any) {
    on('session.append', { door: 'tool-result' }, async (_$: unknown, e: any, next: any) => {
      const content = e.message.content.map((b: any) => ({ ...b, content: `OTHER(${b.content})` }));
      return next({ ...e, message: { ...e.message, content } });
    });
  },
};

describe('composition', () => {
  test('with another rewriting mod, the stored row is the composition, not the proposal', { plugins: [other] }, async ($, on) => {
    mock.env(on, { DESLAG_MOD_MODE: 'safe', DESLAG_MOD_BINARY: '/opt/deslag' });
    const calls = bridge(on, (e) => {
      const text = JSON.parse(e.init.stdin).items[0].text;
      return { value: { exitCode: 0, stdout: envelope([changed(0, `MIN(${text})`)]), stderr: '' } };
    });
    const stored = await $.session.append(bash('x'));
    // Records which mod ran first on this client: the inner call wraps the outer.
    console.log(`composition: stored=${stored.message.content[0].content} codecInput=${JSON.parse(calls[0].init.stdin).items[0].text}`);
    expect(['OTHER(MIN(x))', 'MIN(OTHER(x))']).toContain(stored.message.content[0].content);
  });
});
