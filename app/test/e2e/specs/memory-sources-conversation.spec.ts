// @ts-nocheck
/**
 * Memory sources — Conversation kind CRUD via RPC.
 *
 * Verifies the conversation source kind can be added, listed, toggled,
 * and removed via the core JSON-RPC surface. Does not test the full UI
 * wizard (covered by unit tests) — focuses on the RPC round-trip that
 * backs it.
 *
 * Flow:
 *   1. Add a conversation source via RPC
 *   2. List sources and verify it appears
 *   3. Get source by id
 *   4. Update (disable) the source
 *   5. Remove the source
 *   6. List confirms removal
 */
import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc, expectRpcOk } from '../helpers/core-rpc';
import { resetApp } from '../helpers/reset-app';

const LOG_PREFIX = '[memory-sources-conversation]';
const USER_ID = 'e2e-memory-sources-conversation';

describe('Memory sources — conversation kind', () => {
  let sourceId: string;

  before(async () => {
    console.log(`${LOG_PREFIX} Waiting for app`);
    await waitForApp();
    await resetApp(USER_ID);
    console.log(`${LOG_PREFIX} Setup complete`);
  });

  it('adds a conversation source via RPC', async () => {
    const resp = await callOpenhumanRpc('openhuman.memory_sources_add', {
      kind: 'conversation',
      label: 'Agent Conversations',
      enabled: true,
    });
    const result = expectRpcOk(resp, 'memory_sources_add');
    expect(result.source).toBeDefined();
    expect(result.source.kind).toBe('conversation');
    expect(result.source.label).toBe('Agent Conversations');
    expect(result.source.enabled).toBe(true);
    expect(result.source.id).toBeTruthy();
    sourceId = result.source.id;
    console.log(`${LOG_PREFIX} Added source: ${sourceId}`);
  });

  it('lists sources including the conversation source', async () => {
    const resp = await callOpenhumanRpc('openhuman.memory_sources_list', {});
    const result = expectRpcOk(resp, 'memory_sources_list');
    const sources = result.sources ?? [];
    const convSources = sources.filter((s: { kind: string }) => s.kind === 'conversation');
    expect(convSources.length).toBeGreaterThanOrEqual(1);
    expect(convSources[0].id).toBe(sourceId);
  });

  it('gets the source by id', async () => {
    const resp = await callOpenhumanRpc('openhuman.memory_sources_get', { id: sourceId });
    const result = expectRpcOk(resp, 'memory_sources_get');
    expect(result.source).toBeDefined();
    expect(result.source.kind).toBe('conversation');
    expect(result.source.id).toBe(sourceId);
  });

  it('updates the source to disabled', async () => {
    const resp = await callOpenhumanRpc('openhuman.memory_sources_update', {
      id: sourceId,
      enabled: false,
    });
    const result = expectRpcOk(resp, 'memory_sources_update');
    expect(result.source.enabled).toBe(false);
    expect(result.source.kind).toBe('conversation');
  });

  it('removes the conversation source', async () => {
    const resp = await callOpenhumanRpc('openhuman.memory_sources_remove', { id: sourceId });
    const result = expectRpcOk(resp, 'memory_sources_remove');
    expect(result.removed).toBe(true);
  });

  it('list confirms source is removed', async () => {
    const resp = await callOpenhumanRpc('openhuman.memory_sources_list', {});
    const result = expectRpcOk(resp, 'memory_sources_list');
    const sources = result.sources ?? [];
    const convSources = sources.filter((s: { kind: string }) => s.kind === 'conversation');
    expect(convSources.length).toBe(0);
  });
});
