import type { Client, Methods, Events, JsonValue } from './client';

declare const client: Client;
const args: Methods['inspect']['args'] = {
  label: 'Orion 星', active: true, count: 2, ratio: 1.5, empty: null,
  anything: { nested: [true, null, 1, 'value'] }, tags: ['one', 'two'],
  nullable: null, nested: { child: 'Nova' },
};

async function positiveCases() {
  const results = await client.call('inspect', args, new AbortController().signal);
  const id: number = results[0].id;
  const payload: JsonValue = results[0].payload;
  const title: string = await client.call('no-args', null);
  const flag: boolean = await client.call('open-map', { known: null, arbitrary: [1, null] });
  await client.call('inspect', { ...args, optional: 3, optionalNullable: null });
  await client.call('inspect', { ...args, nullable: 'present', optionalNullable: false });
  const remove = client.on('changed', event => {
    const name: string = event.name;
    const revision: number = event.revision;
    void [name, revision];
  }, { once: true });
  client.on('cleared', event => { const cleared: null = event; void cleared; });
  remove();
  void [id, payload, title, flag];
}

// @ts-expect-error Unknown method names are rejected.
client.call('unknown', null);
// @ts-expect-error Required input properties cannot be omitted.
client.call('inspect', { label: 'Orion' });
// @ts-expect-error A null-only method requires null, not undefined.
client.call('no-args', undefined);
// @ts-expect-error String array elements are typed.
client.call('inspect', { ...args, tags: [1] });
// @ts-expect-error Optional non-nullable properties reject null.
client.call('inspect', { ...args, optional: null });
// @ts-expect-error A required nullable property cannot be undefined.
client.call('inspect', { ...args, nullable: undefined });
// @ts-expect-error Unknown object properties are rejected for object literals.
client.call('inspect', { ...args, unexpected: true });
// @ts-expect-error Closed object schemas preserve nested property types.
client.call('inspect', { ...args, nested: { child: false } });
// @ts-expect-error An open JSON object still rejects functions.
client.call('open-map', { executable: () => 1 });
// @ts-expect-error Event names are typed.
client.on('unknown-event', () => {});
// @ts-expect-error Event payloads are typed.
client.on('changed', (event: string) => {});
// @ts-expect-error Result properties are typed.
client.call('inspect', args).then(result => { const wrong: string = result[0].id; void wrong; });
// @ts-expect-error Events expose their declared fields.
const wrongEvent: Events['changed'] = { name: 'Orion', revision: 'one' };
void positiveCases;
void wrongEvent;
