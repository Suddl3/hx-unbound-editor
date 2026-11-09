function makeLibrarian(root, actions) {
 let tab="Presets", live=null, paused=true, bank=3, directory=null, imports=null;
 let picked=null, loading=false, recalling=false, message="", revision=0, irSlots=null;
 let favorites=[];
 try { const ids=JSON.parse(localStorage.getItem("helix-favorite-nams")); if(Array.isArray(ids)) favorites=ids; } catch {}
 root.innerHTML='<nav aria-label="Librarian"></nav><select aria-label="Setlist"></select><input type="search" aria-label="Filter library" placeholder="Filter list"><div class="lib-list"></div><section class="lib-detail" aria-live="polite"></section>';
 const nav=root.querySelector("nav"),banks=root.querySelector("select"),search=root.querySelector("input");
 const list=root.querySelector(".lib-list"),detail=root.querySelector(".lib-detail");
 function button(parent,label,click,disabled=false) {
  const el=document.createElement("button");el.type="button";el.textContent=label;el.disabled=disabled;el.onclick=click;parent.append(el);return el;
 }
 for(const t of ["Presets","IRs","Favorites","NAM"]){
  button(nav,t,()=>{tab=t;picked=null;message="";search.value="";render();load();});
 }
 const slots=()=>Object.entries(live?.slots||{});
 function rows() {
  if(tab==="Presets")return Object.entries(directory?.presets||{}).map(([id,name])=>({id,name,tag:id}));
   if(tab==="IRs")return irSlots?Array.from({length:128},(_,i)=>{
   const slot=i+1,c=irSlots[slot];return {id:String(slot),name:c?.name||"Empty",ref:c?.ref,tag:String(slot).padStart(3,"0")};
  }):[];
  return (imports||[]).filter(c=>tab!=="Favorites"||favorites.includes(c.id)).map(c=>({...c,ref:c.id,tag:c.kind}));
 }
 function render() {
  for(const b of nav.children)b.setAttribute("aria-pressed",String(b.textContent===tab));
  banks.hidden=tab!=="Presets"||!directory;banks.disabled=loading||recalling;
  list.replaceChildren();detail.replaceChildren();
  const entries=rows(),match=search.value.trim().toLowerCase();
  const visible=entries.filter(r=>(r.tag+" "+r.name).toLowerCase().includes(match));
  for(const r of visible){
   const b=button(list,"",()=>{picked=r.id;message="";render();list.querySelector('[aria-pressed=true]')?.focus({preventScroll:true});});
   const tag=document.createElement("small");tag.textContent=r.tag;b.append(tag,document.createTextNode(r.name));
   b.setAttribute("aria-pressed",String(r.id===picked));
   if(tab==="Presets" && r.id===live?.label && r.name===live?.name)b.title="Matches the active preset name and location";
  }
  if(!visible.length)list.textContent=loading?"Reading…":message?"":tab==="IRs"&&!live?"Waiting for the Floor IR directory.":tab==="Favorites"&&!favorites.length?"Star a profile in NAM to find it here.":"No matching entries.";
  const r=entries.find(row=>row.id===picked);
  if(r){
   const name=document.createElement("strong");name.textContent=r.name;detail.append(name);
   if(tab==="Presets"){
    button(detail,"Recall "+r.id,async()=>{
     recalling=true;message="Recalling "+r.id+"…";render();
     try{await actions.recall(bank,r.id);message="Requested "+r.id+" on the Floor.";}catch(e){message=e.message;}
     finally{recalling=false;render();}
    },!live||paused||recalling||loading);
   }else{
    const installed=slots().filter(([,c])=>c.ref && c.ref===r.ref);
    const slot=tab==="IRs"?Number(r.id):Number(installed[0]?.[0]);
    const note=document.createElement("small");
    note.textContent=tab==="IRs"?"Floor IR "+r.id:!live?"Installation status unavailable":installed.length?"On Floor · IR "+installed.map(([n])=>n).join(", "):"Local library only";
    detail.append(note);
    if(r.ref && actions.available(r.ref,slot))
     button(detail,"Audition on 06C",()=>actions.audition(r.ref),paused||recalling);
    if(tab==="NAM"||tab==="Favorites")button(detail,favorites.includes(r.id)?"★ Remove favorite":"☆ Favorite",()=>{
     favorites=favorites.includes(r.id)?favorites.filter(id=>id!==r.id):[...favorites,r.id];
     try{localStorage.setItem("helix-favorite-nams",JSON.stringify(favorites));}catch{} render();
    });
   }
  }
  if(message){const note=document.createElement("p");note.textContent=message;detail.append(note);}
  if(paused && irSlots && tab==="IRs"){
   const note=document.createElement("small");note.textContent="Last IR directory read · Live audition paused";detail.append(note);
  }
  if(!loading && (tab==="Presets"||tab==="IRs"))button(detail,"Refresh list",()=>load(true));
 }
 async function load(force=false){
  const request=++revision, isBank=tab==="Presets";
  loading=false;
  if(!force&&(isBank?directory:tab==="IRs"?irSlots:imports)){render();return;}
  loading=true;render();
  try{
   const data=await actions.read(isBank?"/presets/"+bank:tab==="IRs"?"/irs":"/nam");
   if(request!==revision)return;
   if(isBank){
    directory=data;banks.replaceChildren();
    for(const [id,name] of Object.entries(data.setlists))banks.add(new Option(name,id));
    banks.value=String(bank);
   }else if(tab==="IRs")irSlots=data;
   else imports=data;
   message="";
  }catch(e){if(request===revision)message=e.message;}
  finally{if(request===revision){loading=false;render();}}
 }
 banks.onchange=()=>{bank=Number(banks.value);directory=null;picked=null;message="";load();};
 search.oninput=render;
 render();
 return {
  update(data,stale){
   const first=!live; const changed=first||stale!==paused||JSON.stringify(live?.slots)!==JSON.stringify(data?.slots)||live?.label!==data?.label||live?.name!==data?.name;
   live=data;paused=stale;
   if(data)irSlots=data.slots;
   if(changed)render();
   if(first&&data&&tab==="Presets"&&!directory&&!loading)load();
  }
 };
}
