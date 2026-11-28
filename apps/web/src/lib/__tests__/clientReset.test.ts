import { describe, expect, it } from 'vitest';
import { clearClientLocalState, clientResetUrl } from '../client-reset';

function storage() {
  const calls = { clear: 0 };
  return {
    calls,
    clear: () => {
      calls.clear += 1;
    },
  };
}

describe('client reset (local browser state)', () => {
  it('clears both localStorage and sessionStorage', () => {
    const local = storage();
    const session = storage();
    clearClientLocalState({ local, session });
    expect(local.calls.clear).toBe(1);
    expect(session.calls.clear).toBe(1);
  });

  it('keeps going when one storage provider throws', () => {
    const local = {
      clear: () => {
        throw new Error('private mode');
      },
    };
    const session = storage();
    expect(() => clearClientLocalState({ local, session })).not.toThrow();
    expect(session.calls.clear).toBe(1);
  });

  it('reloads at the deployment base, not a deep link', () => {
    expect(clientResetUrl('/', 'http://127.0.0.1:8787/?demo=1&thread=familien')).toBe('http://127.0.0.1:8787/');
    expect(clientResetUrl('/app/', 'http://127.0.0.1:8787/app')).toBe('http://127.0.0.1:8787/app/');
    expect(clientResetUrl('./', 'tauri://localhost/index.html')).toBe('tauri://localhost/');
  });
});
