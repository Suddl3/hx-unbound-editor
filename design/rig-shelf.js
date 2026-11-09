// One row per named rig. Compare revisions in the write transaction so another
// editor tab can't silently replace a newer save.
function openRigDatabase(){
 return new Promise((resolve,reject)=>{
  const request=indexedDB.open("helixnam-offline-rigs",1);
  request.onupgradeneeded=()=>request.result.createObjectStore("rigs",{keyPath:"key"});
  request.onsuccess=()=>resolve(request.result);
  request.onerror=()=>reject(request.error);
 });
}

async function shelfTransaction(write,run){
 const db=await openRigDatabase();
 try{
  return await new Promise((resolve,reject)=>{
   const tx=db.transaction("rigs",write?"readwrite":"readonly");
   let result,failure;
   tx.oncomplete=()=>resolve(result);
   tx.onabort=()=>reject(failure || tx.error || new Error("Rig storage unavailable."));
   run(tx.objectStore("rigs"),value=>{result=value;},message=>{
    failure=new Error(message);tx.abort();
   });
  });
 }finally{db.close();}
}

function readShelf(key){
 return shelfTransaction(false,(store,done)=>{
  const request=key===undefined?store.getAll():store.get(key);
  request.onsuccess=()=>done(request.result);
 });
}

function storeRig(doc,expected){
 const key=doc.name.normalize("NFC").toLowerCase();
 return shelfTransaction(true,(store,done,fail)=>{
  const read=store.get(key);
  read.onsuccess=()=>{
   const existing=read.result;
   if(existing?.revision!==expected){
    fail(expected?"A newer copy exists. Reopen it, or rename your draft to keep both.":
     "That name is already in My rigs. Open the saved rig to edit it, or rename this draft.");
    return;
   }
   const record={key,revision:crypto.randomUUID(),doc};
   store.put(record);done(record);
  };
 });
}

function setupRigShelf(editor){
 const panel=document.getElementById("myRigs"),list=document.getElementById("shelfList");
 const save=document.getElementById("shelfSave"),open=document.getElementById("shelfOpen");
 const route=document.getElementById("shelfRoute"),status=document.getElementById("shelfStatus");
 let records=[],active=null,busy=false;
 function display(selected){
  records.sort((a,b)=>a.doc.name.localeCompare(b.doc.name));
  list.replaceChildren(...records.map(r=>new Option(r.doc.name,r.key)));
  if(!records.length)list.add(new Option("No saved rigs",""));
  else if(records.some(r=>r.key===selected))list.value=selected;
  preview();
 }
 function preview(){
  const item=records.find(r=>r.key===list.value);
  route.textContent=item?item.doc.blocks.map((b,i)=>(b.position??i+1)+" "+b.label+(b.enabled?"":" (off)")).join(" → "):"";
 }
 async function action(run){
  if(busy)return;
  busy=true;save.disabled=open.disabled=list.disabled=true;
  try{await run();}
  catch(error){status.textContent=error.name==="QuotaExceededError"?"Browser storage is full. Use Save local rig to download a file.":
   error.name==="SecurityError"?"This browser blocked rig storage. Use Save local rig to download a file.":
   error.message || "Could not access My rigs.";}
  finally{busy=false;save.disabled=false;open.disabled=list.disabled=!records.length;}
 }
 const refresh=()=>action(async()=>{
  const selected=list.value;
  records=await readShelf();display(selected);
 });
 panel.ontoggle=()=>{if(panel.open)refresh();};
 window.addEventListener("focus",()=>{if(panel.open)refresh();});
 list.onchange=preview;
 save.onclick=()=>action(async()=>{
  const doc=editor.document(),generation=editor.generation();
  const key=doc.name.normalize("NFC").toLowerCase();
  const expected=active?.generation===generation && active.key===key?active.revision:undefined;
  const record=await storeRig(doc,expected);
  if(editor.generation()===generation){
   active={key,revision:record.revision,generation};editor.saved(doc);
  }
  records=records.filter(r=>r.key!==key);records.push(record);display(key);
  status.textContent="Saved "+doc.name+" in My rigs.";
 });
 open.onclick=()=>action(async()=>{
  const generation=editor.generation(),before=JSON.stringify(editor.document());
  const record=await readShelf(list.value);
  if(!record)throw new Error("This rig is unavailable. Reopen My rigs to refresh the list.");
  if(generation!==editor.generation() || before!==JSON.stringify(editor.document())){
   status.textContent="Your draft changed. Select Open again when ready.";return;
  }
  if(editor.dirty() && before!==JSON.stringify(record.doc) &&
     !window.confirm("Open "+record.doc.name+" and discard the current unsaved edits?"))return;
  editor.open(record.doc);
  active={key:record.key,revision:record.revision,generation:editor.generation()};
  records=records.map(r=>r.key===record.key?record:r);display(record.key);
  status.textContent="Opened "+record.doc.name+".";
 });
 status.textContent="Rename the draft before saving to keep a separate version.";
 panel.hidden=false;
}
