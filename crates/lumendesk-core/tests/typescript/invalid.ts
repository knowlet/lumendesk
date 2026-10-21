import type { Client } from './client';
declare const client: Client;
client.call('missing-method', null);
client.call('no-args', undefined);
client.call('inspect', { label: 'missing required fields' });
client.on('changed', (payload: string) => {});
client.stream('missing-stream', null);
client.stream('count', 'invalid argument');
