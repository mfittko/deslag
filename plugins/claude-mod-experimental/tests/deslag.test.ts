// Run with `claude plugin test plugins/claude-mod-experimental`. The engine's
// own turn.step chain runs; no binary, network or model is involved.
import { describe, expect, test } from 'claude-code/testing';
import { newStats, recordUsage } from '../hooks/deslag.js';

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
  test('a step without usage is unknown, never zero', () => {
    const stats = newStats();
    recordUsage(stats, { usage: null });
    recordUsage(stats, { usage: { input_tokens: 5, output_tokens: 2, cache_read_input_tokens: 1, cache_creation_input_tokens: 0 } });
    recordUsage(stats, { usage: { input_tokens: 9 } });
    recordUsage(stats, undefined);
    expect(stats.usage).toEqual({ steps: 4, unknown: 3, input: 5, output: 2, cache_read: 1, cache_creation: 0 });
  });
});
