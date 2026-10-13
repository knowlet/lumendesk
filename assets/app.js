'use strict';
function start() {
  const form=document.getElementById('form'),button=document.getElementById('submit'),output=document.getElementById('result');
  button.disabled=false; output.textContent='Ready when you are.';
  window.lumen.on('greeting',state=>{document.getElementById('events').textContent=`Typed event received · greeting ${state.submissions}`;});
  form.addEventListener('submit',async event=>{
    event.preventDefault(); button.disabled=true;
    try { const state=await window.lumen.call('greet',{name:document.getElementById('name').value,enthusiastic:document.getElementById('enthusiastic').checked}); output.textContent=state.greeting; }
    catch(error){output.textContent=`Could not say hello: ${error.message}`;}
    finally{button.disabled=false;}
  });
}
if(window.lumen)start();else window.addEventListener('lumendesk-ready',start,{once:true});
