import type { Client } from './client.generated';
declare const client: Client;
const controller = new AbortController();
client.call('greet', {name:'Orion 星', enthusiastic:true}, controller.signal).then(state => {
  const message: string = state.greeting;
  const count: number = state.submissions;
  void [message, count];
});
const unsubscribe = client.on('greeting', state => { const name: string = state.name; void name; }, {once:true});
unsubscribe();
