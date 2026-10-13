import type { Client } from './client.generated';
declare const client: Client;
client.call('unknown', {});
client.call('greet', {name:42, enthusiastic:true});
client.call('greet', {name:'Orion'});
client.on('missing-event', () => {});
