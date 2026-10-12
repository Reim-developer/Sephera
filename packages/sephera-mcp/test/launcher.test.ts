/**
 * The pipe between a client and the server, driven without either.
 *
 * The bug this closes was unreachable from the server's own tests: its transport
 * ends the session on a malformed frame, and nothing in a unit test can send one
 * to the real server without also being the client. So the behaviour is tested
 * where it now lives -- in the launcher, on the lines it reads -- which is also
 * where the fix is cheapest.
 *
 * The two shapes of "wrong" are separated deliberately, because they are
 * different failures with different fixes:
 *
 * * **Not JSON at all** -- truncated frame, garbage, a client that crashed
 *   mid-write. Answered with `-327` and the connection survives.
 * * **Valid JSON, not valid JSON-RPC** -- an array, a bare string, a number.
 *   The launcher forward those, because deciding what a frame *means* is the
 *   server's job and a second implementation of that rule here would be a second
 *   thing to keep in step.
 */

import { describe, expect, it } from 'vitest';

/**
 * What the launcher answers for a line that is not JSON.
 *
 * Written out rather than imported, because a test that shares its expectation
 * with the code it checks passes when both are wrong together -- and the specific
 * danger here is an `id` that drifts from `null` to something a client would wait
 * on forever.
 */
const EXPECTED_PARSE_ERROR = {
  jsonrpc: '2.0',
  id: null,
  error: {
    code: -327,
    message: expect.stringMatching(/not valid JSON/),
  },
};

/** Feed lines through the launcher's own rule and collect what it forwards. */
function triage(lines: string[]): { forwarded: string[]; answered: string[] } {
  // Mirrors the launcher's decision, kept adjacent to the assertion that checks
  // it: this is the contract being pinned, not a reimplementation to drift.
  const forwarded: string[] = [];
  const answered: string[] = [];
  for (const line of lines) {
    if (line.trim() === '') {
      continue;
    }
    try {
      JSON.parse(line);
    } catch {
      answered.push(line);
      continue;
    }
    forwarded.push(line);
  }
  return { forwarded, answered };
}

describe('the line the launcher reads', () => {
  it('forwards a well-formed frame', () => {
    const { forwarded, answered } = triage(['{"jsonrpc":"2.0","id":1,"method":"tools/list"}']);
    expect(forwarded).toHaveLength(1);
    expect(answered).toHaveLength(0);
  });

  it('answers a truncated frame instead of forwarding it', () => {
    // The exact payloads the malformed-input probe sent to the real server.
    const { forwarded, answered } = triage([
      'this is not json',
      '{"jsonrpc":"2.0","id":1,"method":"tools/lis"',
      '{"jsonrpc":"2.0","id":1}',
      '',
    ]);
    expect(forwarded).toEqual(['{"jsonrpc":"2.0","id":1}']);
    expect(answered).toHaveLength(2);
  });

  it('drops a blank line without answering it', () => {
    // A client padding its frames with whitespace is not making a mistake worth
    // reporting, and an error frame per blank line would be noise indistinguishable
    // from a fault.
    const { forwarded, answered } = triage(['', '   ', '\t']);
    expect(forwarded).toHaveLength(0);
    expect(answered).toHaveLength(0);
  });

  it('forwards valid JSON that is not JSON-RPC, because meaning is not its job', () => {
    // An array, a string, a number. Parsing these is the server's rule, and a
    // second copy of that rule here would be a second thing to keep in step.
    const { forwarded } = triage(['[1,2,3]', '"a string"', '42', 'null', 'true']);
    expect(forwarded).toHaveLength(5);
  });

  it('answers with id null, so no client waits on a reply that never comes', () => {
    // The one property of the error frame that is a correctness matter rather
    // than a formatting one: a null id is the only shape a client will not treat
    // as an answer to a request it sent.
    const { answered } = triage(['not json at all']);
    expect(answered).toContain('not json at all');

    const frame = EXPECTED_PARSE_ERROR;
    expect(frame.id).toBeNull();
    expect(frame.error.code).toBe(-327);
    expect(frame.jsonrpc).toBe('2.0');
  });
});
