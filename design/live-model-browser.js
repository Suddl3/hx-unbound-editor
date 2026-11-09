// Model navigation is independent of USB. Only the explicit tile action can request an edit.
function createModelBrowser({selectModel, notice, target, libraryChanged}) {
  const el = id => document.getElementById(id);
  const element = (tag, text = "") => Object.assign(document.createElement(tag), {textContent:text});
  const headers = {"X-Floor-Read":"1"};
  let catalog = {categories:[],models:[]}, captures = [], category = "Distortion", shelf = "";
  let engine = "A2 Lite";
  let stars = new Set(), picked = null, started = false, stockFailure = "", namFailure = "";
  const favKey="floor-editor-favorite-models-v1";
  const viewKey="floor-editor-model-view";
  const alphabet=new Intl.Collator(undefined,{sensitivity:"base",numeric:true});
  let view="grid";
  try {if(localStorage.getItem(viewKey)==="list")view="list";} catch {}
  try {const saved=JSON.parse(localStorage.getItem(favKey)||"[]");if(Array.isArray(saved))stars=new Set(saved);} catch {}
  const typeOrder = ["Amp head","Amp + cab","Pedal","Cab","Unknown"];
  const captureTypes = ["Amp head","Amp + cab","Pedal","Pedal + amp","Pedal + amp + cab","Preamp","Cab","Studio","Space","Outboard","Experimental"];
  const typeDisplay = kind=>kind==="Amp head"?"Amp only":kind==="Unknown"?"Unclassified":kind;
  function categoryInfo(name) { return catalog.categories.find(c=>c.name===name); }
  const tint = name => name==="NAM"?"#75cbbe":name==="Favorites"?"#e8bd59":categoryInfo(name)?.color||"#a8b0ae";
  function icon(url, fallback, cls=""){
    if(!url){const span=element("span",fallback);span.className="fallback "+cls;return span;}
    const image=element("img");image.alt="";image.loading="lazy";image.src=url;image.className=cls;
    image.onerror=()=>{const span=element("span",fallback);span.className="fallback "+cls;image.replaceWith(span);};
    return image;
  }
  // Role illustrations for user captures. Their colour identifies the capture across views.
  const namArtwork=new Map();
  function namIcon(kind,id=""){
    const key=kind+"/"+id;
    if(namArtwork.has(key))return namArtwork.get(key);
    const colors=["#deaa6c","#6bbfae","#91a8cf","#c18996","#b9ac74","#ac95c6"];
    let hash=0;for(const ch of String(id))hash=(hash*31+ch.charCodeAt(0))>>>0;
    const accent=colors[hash%colors.length];
    const knobs=(start,y,count,dx)=>Array.from({length:count},(_,i)=>
      '<g transform="translate('+(start+dx*i)+' '+y+')"><circle r="2.4" fill="#272c2d"/><path d="M0-1.5v1" stroke="#cbceca" stroke-width=".7"/></g>').join("");
    const head='<rect x="32" y="18" width="97" height="29" rx="4" fill="#383c3c" stroke="#828783"/><path d="M65 18v-4q0-2 3-2h24q3 0 3 2v4" fill="none" stroke="#929793" stroke-width="2"/><rect x="36" y="22" width="89" height="12" rx="1" fill="url(#mesh)"/><rect x="36" y="35" width="89" height="8" rx="1" fill="'+accent+'"/>'+knobs(46,39,7,8)+'<circle cx="116" cy="39" r="1.8" fill="#25332f"/><path d="M38 48h8m69 0h8" stroke="#8b918f" stroke-width="3"/>';
    const cab='<rect x="39" y="12" width="84" height="49" rx="3" fill="#34393a" stroke="#969993"/><rect x="43" y="16" width="76" height="40" rx="1" fill="#282c2d"/><g fill="#373e3f" stroke="#555c5c" stroke-width="1.2"><circle cx="62" cy="36" r="14"/><circle cx="99" cy="36" r="14"/></g><rect x="43" y="16" width="76" height="40" fill="url(#net)"/><path d="M43 16h76" stroke="'+accent+'" stroke-width="2"/><path d="M45 62h9m54 0h9" stroke="#838986" stroke-width="3"/>';
    let drawing;
    if(kind==="Pedal + amp" || kind==="Pedal + amp + cab"){
      const pedal='<g transform="translate(13 32) rotate(-6)"><rect width="23" height="30" rx="3" fill="'+accent+'" stroke="#abb2ab"/>'+knobs(6,5,2,10)+'<rect x="4" y="10" width="15" height="7" rx="1" fill="#343a39"/><circle cx="11" cy="23" r="3" fill="#c1c6be"/></g>';
      drawing=pedal+(kind==="Pedal + amp + cab"?'<g transform="translate(37 18) scale(.72)">'+cab+'</g><g transform="translate(45 -6) scale(.62)">'+head+'</g>':'<g transform="translate(20 4) scale(.95)">'+head+'</g>');
    }
    else if(kind==="Amp head" || kind==="Preamp")drawing='<g transform="translate(-8 -1) scale(1.1)">'+head+'</g>';
    else if(kind==="Amp + cab")drawing='<g transform="translate(22 18) scale(.72)">'+cab+'</g><g transform="translate(30 -6) scale(.62)">'+head+'</g>';
    else if(kind==="Cab")drawing=cab;
    else if(kind==="Pedal")drawing='<path d="M58 12l40-3 8 47-44 8-6-7z" fill="#343a39" stroke="#858c88"/><path d="M58 12l40-3 6 45-42 7z" fill="'+accent+'" stroke="#abb2ab"/><g transform="rotate(-6 80 35)">'+knobs(68,18,3,9)+'<rect x="66" y="26" width="30" height="12" rx="2" fill="#38403d"/><path d="M69 32h6l2-3 3 7 3-7 2 3h7" fill="none" stroke="'+accent+'" stroke-width="1.2"/><circle cx="81" cy="46" r="4" fill="#c1c6be" stroke="#4c5552"/><circle cx="91" cy="42" r="1.3" fill="#343b39"/></g>';
    else drawing='<rect x="34" y="20" width="95" height="28" rx="4" fill="#333a3a" stroke="#8e9692"/><rect x="40" y="25" width="52" height="18" rx="2" fill="#202626"/><path d="M44 35h8l4-6 5 10 5-13 6 14 6-7h9" stroke="'+accent+'" stroke-width="2" fill="none"/>'+knobs(101,33,3,8);
    const svg='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 68"><defs><pattern id="mesh" width="3" height="3" patternUnits="userSpaceOnUse"><rect width="3" height="3" fill="#272a2a"/><circle cx="1" cy="1" r=".68" fill="#6a6d67"/></pattern><pattern id="net" width="3" height="3" patternUnits="userSpaceOnUse"><path d="M0 0h3M0 0v3" stroke="#96988d" opacity=".4" stroke-width=".7"/></pattern></defs>'+drawing+'</svg>';
    const url="data:image/svg+xml,"+encodeURIComponent(svg);namArtwork.set(key,url);return url;
  }
  function namRows(){
    return captures.flatMap(c=>{
      const engines=c.engines?.length?[...new Set(c.engines)]:[c.engine||null];
      return engines.map(e=>({...c,key:"nam:"+c.id+(e==="A2 Full"?":full":""),category:"NAM",shelf:c.kind||"Unknown",
        engine:e,libraryOnly:e==="A2 Full",variant:e||c.compatibility||"",
        instrument:(c.instrument||"Unknown").toLowerCase(),icon:namIcon(c.kind,c.id)}));
    });
  }
  function infoFor(block){
    if(!block)return null;
    if(block.model===380)return {category:"NAM",icon:namIcon(block.captureKind||"Unknown",block.namId||block.name),name:block.name};
    return catalog.models.find(m=>m.model===block.model &&
      (block.pairModel!=null?m.pairedModel===block.pairModel:m.pairedModel==null));
  }
  function favorite(row){
    if(stars.has(row.key))stars.delete(row.key);else stars.add(row.key);
    try{localStorage.setItem(favKey,JSON.stringify([...stars]));}catch{notice("Favorites are available for this session.");}
    render();libraryChanged();
  }
  const special = name => ({Favorites:"☆",NAM:"∿"})[name]||"◇";
  function nav(name){
    category=name;shelf="";picked=null;el("modelSearch").value="";el("instrumentFilter").value="";
    render();
  }
  function render(){
    const group=categoryInfo(category), query=el("modelSearch").value.trim().toLowerCase();
    const rail=el("categories");const was=rail.scrollTop;rail.replaceChildren();
    const categories=["Favorites","NAM",...catalog.categories.map(c=>c.name).filter(n=>!["Favorites","None","NAM"].includes(n))];
    for(const name of categories){
      const btn=element("button");btn.type="button";btn.setAttribute("aria-label",name);btn.setAttribute("aria-pressed",String(name===category));
      btn.style.setProperty("--cat",tint(name));btn.append(icon(categoryInfo(name)?.icon,special(name)),element("span",name));
      btn.tabIndex=name===category?0:-1;
      btn.onclick=()=>{nav(name);rail.querySelector('[aria-pressed="true"]')?.focus({preventScroll:true});};rail.append(btn);
    }
    rail.scrollTop=was;
    const allRows=[...catalog.models,...namRows()];
    const globalSearch=Boolean(query)&&category!=="NAM"&&category!=="Favorites";
    const rows=category==="Favorites"?allRows.filter(m=>stars.has(m.key)):
      globalSearch?allRows:allRows.filter(m=>m.category===category);
    const title=globalSearch?"Search results":category;
    el("modelTitle").textContent=title;
    const header=el("modelShelves");header.replaceChildren();
    if(category==="NAM"){
      const importer=element("button","Import .nam");importer.type="button";importer.className="import-nam";
      importer.onclick=()=>el("namFile").click();header.append(importer);
      const engines=[...new Set(rows.map(m=>m.engine||"Unconverted"))];
      if(engines.length>1){
        const list=element("select");list.setAttribute("aria-label","NAM engine");list.add(new Option("All engines",""));
        engines.forEach(name=>list.add(new Option(name,name)));
        if(engine && !engines.includes(engine))engine=engines.includes("A2 Lite")?"A2 Lite":"";
        list.value=engine;list.onchange=()=>{engine=list.value;render();};header.append(list);
      }else if(engines.length && engine && !engines.includes(engine))engine=engines[0];
    }
    const instrumentShelves=group?.shelves?.length && group.shelves.every(s=>s==="Guitar"||s==="Bass");
    const showInstrument=category==="NAM" || (!instrumentShelves||globalSearch) &&
      rows.some(m=>["bass","guitar"].includes(m.instrument?.toLowerCase()));
    el("instrumentFilter").hidden=!showInstrument;
    const shelves=category==="NAM"?[...new Set([...typeOrder,...rows.map(m=>m.shelf)])]:
      category==="Favorites"||globalSearch?[]:group?.shelves||[];
    if(shelves.length){
      for(const s of ["",...shelves]){
        const tab=element("button",s?(category==="NAM"?typeDisplay(s):s):"All");
        tab.setAttribute("aria-pressed",String(s===shelf));tab.onclick=()=>{shelf=s;render();};header.append(tab);
      }
    }
    const filter=el("instrumentFilter").value;
    const tiles=el("modelTiles");tiles.replaceChildren();
    tiles.dataset.view=view;
    for(const button of el("modelView").children)button.setAttribute("aria-pressed",String(button.dataset.view===view));
    if(view==="list")rows.sort((a,b)=>alphabet.compare(a.name,b.name)||alphabet.compare(a.key,b.key));
    for(const row of rows){
      if(!globalSearch && shelf && shelf!==row.shelf)continue;
      if(showInstrument && filter && (row.instrument||"unknown").toLowerCase()!==filter)continue;
      if(category==="NAM" && engine && engine!==(row.engine||"Unconverted"))continue;
      if(query && ![row.name,row.shelf,row.category,row.instrument,row.variant].filter(Boolean).join(" ").toLowerCase().includes(query))continue;
      const card=element("div");card.className="model-tile";card.style.setProperty("--cat",tint(row.category));
      const btn=element("button");btn.type="button";btn.className="model-choice";
      const variant=row.category==="NAM"?[typeDisplay(row.shelf),row.engine,row.libraryOnly?"Library only":row.installed?"On Floor":"Local"].filter(Boolean).join(" · "):
        [...new Set([globalSearch||category==="Favorites"?row.category:"",row.shelf,row.variant].filter(Boolean))].join(" · ");
      btn.setAttribute("aria-label",[row.name,variant].filter(Boolean).join(" · "));
      const active=target();
      const matches=currentMatch(row,active);
      btn.setAttribute("aria-pressed",String(matches||row.key===picked));
      const image=icon(row.icon,special(row.category),"gear");
      btn.append(image,element("strong",row.name));
      if(variant)btn.append(element("small",variant));
      btn.onclick=()=>{
        picked=row.key;selectModel(row);render();
      };
      const starred=stars.has(row.key),star=element("button",starred?"★":"☆");star.className="star";
      star.type="button";star.setAttribute("aria-label",(starred?"Remove from favorites: ":"Favorite: ")+row.name);
      star.setAttribute("aria-pressed",String(starred));star.onclick=()=>favorite(row);
      card.append(btn,star);
      if(row.category==="NAM"){
        const tags=element("button","⋯");tags.type="button";tags.className="capture-tags";
        tags.setAttribute("aria-label","Edit tags for "+row.name);tags.title="Capture details and tags";
        tags.onclick=()=>editTags(row);card.append(tags);
      }
      tiles.append(card);
    }
    if(!tiles.childElementCount){
      const message=category==="Favorites"&&!stars.size?"Star a model to keep it here.":category==="NAM"&&namFailure?namFailure:
        category==="NAM"&&filter&&captures.length?"No captures match. Use the ⋯ on a capture to edit its tags.":
        stockFailure&&category!=="NAM"?stockFailure:"No matching models.";
      tiles.append(element("p",message));
    }
  }
  function currentMatch(row, block){
    if(!block)return false;
    if(row.category==="NAM")return Boolean(!row.libraryOnly && block.namId && block.namId===row.id);
    return row.model!=null && row.model===block.model && (row.pairedModel??null)===(block.pairModel??null);
  }
  function follow(block){
    picked=null;
    if(block){
      const found=infoFor(block);
      const next=found?.category||block.category;
      if(next==="NAM"||categoryInfo(next)){
        category=next;shelf="";el("modelSearch").value="";el("instrumentFilter").value="";
      }
    }
    render();
  }
  async function loadNam(){
    try{
      const r=await fetch("/live/nam",{headers,signal:AbortSignal.timeout(10000)});
      if(!r.ok)throw Error("Capture library unavailable.");
      const body=await r.json();captures=Array.isArray(body)?body:body.captures||[];namFailure="";
    }catch(e){namFailure=e.message;}
    render();return captures;
  }
  function editTags(row){
    const dialog=element("dialog");dialog.className="tag-dialog";
    const form=element("form"),title=element("h2",row.name),info=element("p",[row.engine,row.compatibility].filter(Boolean).join(" · "));
    const id="nam-tags-title";title.id=id;dialog.setAttribute("aria-labelledby",id);form.append(title,info);
    if(row.fileGear || row.listingGear){
      if(row.fileGear)form.append(element("p","NAM file · "+row.fileGear));
      if(row.listingGear)form.append(element("p","Tone 3000 listing · "+row.listingGear));
    }else form.append(element("p","No source gear label."));
    const choice=(label,values,value,source)=>{
      const text=element("label",label),select=element("select");
      select.add(new Option(source?"Use source metadata · "+source:"Use source metadata",""));
      for(const value of values)select.add(new Option(typeDisplay(value),value));
      select.setAttribute("aria-label",label);select.value=value||"";text.append(select);form.append(text);return select;
    };
    const kind=choice("Capture type",captureTypes,row.overrideKind,typeDisplay(row.sourceKind||"Unknown"));
    const instrument=choice("Instrument",["Guitar","Bass"],row.overrideInstrument,row.overrideInstrument?null:row.instrument);
    const actions=element("div"),cancel=element("button","Cancel"),save=element("button","Save tags");
    const error=element("p");error.setAttribute("role","status");
    cancel.type="button";cancel.onclick=()=>dialog.close();save.type="submit";actions.className="block-actions";actions.append(cancel,save);form.append(error,actions);dialog.append(form);
    dialog.onclose=()=>dialog.remove();document.body.append(dialog);dialog.showModal();
    form.onsubmit=async e=>{
      e.preventDefault();save.disabled=true;
      try{
        const r=await fetch("/live/nam/classify",{method:"POST",headers:{"Content-Type":"application/json"},
          body:JSON.stringify({capture:row.id,kind:kind.value||null,instrument:instrument.value||null}),signal:AbortSignal.timeout(10000)});
        if(!r.ok)throw Error((await r.json()).error||"Could not save tags");
        await loadNam();libraryChanged();dialog.close();notice("Saved tags for "+row.name+".");
      }catch(e){save.disabled=false;error.textContent=e.message;}
    };
  }
  async function load(){
    if(started)return;started=true;
    await Promise.all([loadNam(),(async()=>{
      try{
        const r=await fetch("/live/catalog",{headers,signal:AbortSignal.timeout(12000)});
        if(!r.ok)throw Error("Installed model catalog unavailable.");
        catalog=await r.json();stockFailure=catalog.notice||"";follow(target());
      }catch(e){stockFailure=e.message;render();}
    })()]);
  }
  const file=element("input");file.type="file";file.accept=".nam";file.id="namFile";file.hidden=true;
  el("modelShelves").after(file);
  file.onchange=async()=>{
    const selected=file.files[0];file.value="";if(!selected)return;
    try{
      notice("Importing "+selected.name+"…");
      const response=await fetch("/live/nam/import",{method:"POST",headers:{"Content-Type":"application/octet-stream","X-Capture-Filename":encodeURIComponent(selected.name)},body:selected,signal:AbortSignal.timeout(120000)});
      const body=await response.json();if(!response.ok)throw Error(body.error||"Import failed");
      await loadNam();nav("NAM");
      const imported=captures.find(c=>(body.ids||[]).includes(c.id));
      if(imported){showCapture(imported.id);shelf=imported.kind;render();}
      notice("Imported "+selected.name+" to the local NAM library.");
    }catch(e){notice(e.message);}
  };
  el("modelSearch").oninput=()=>{shelf="";render();};
  el("instrumentFilter").onchange=render;
  for(const button of el("modelView").children){
    button.onclick=()=>{
      if(view===button.dataset.view)return;
      view=button.dataset.view;
      try{localStorage.setItem(viewKey,view);}catch{}
      render();el("modelTiles").scrollTop=0;
    };
  }
  el("categories").onkeydown=e=>{
    if(!["ArrowDown","ArrowUp","Home","End"].includes(e.key))return;
    const rail=el("categories"), buttons=[...rail.querySelectorAll("button")];
    const current=buttons.indexOf(document.activeElement);if(current===-1)return;
    e.preventDefault();
    const i=e.key==="Home"?0:e.key==="End"?buttons.length-1:
      Math.max(0,Math.min(buttons.length-1,current+(e.key==="ArrowDown"?1:-1)));
    nav(buttons[i].getAttribute("aria-label"));rail.querySelector('[aria-pressed="true"]').focus();
  };
  el("modelTiles").onkeydown=e=>{
    if(!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown"].includes(e.key))return;
    const list=el("modelTiles"),choices=[...list.querySelectorAll(".model-choice")];
    const i=choices.indexOf(document.activeElement);if(i===-1)return;
    const columns=getComputedStyle(list).gridTemplateColumns.split(" ").length;
    const step={ArrowLeft:-1,ArrowRight:1,ArrowUp:-columns,ArrowDown:columns}[e.key];
    e.preventDefault();choices[Math.max(0,Math.min(choices.length-1,i+step))].focus();
  };
  render();
  function showCapture(id){
    const capture=captures.find(c=>c.id===id);
    if(capture && engine){
      const variants=capture.engines?.length?capture.engines:[capture.engine||"Unconverted"];
      if(!variants.includes(engine))engine=variants[0];
    }
    nav("NAM");
    if(capture){el("modelSearch").value=capture.name;render();}
  }
  return {load,loadNam,rows:()=>captures,nav,follow,infoFor,tint,categoryInfo,render,showCapture,
    favorites:()=>[...catalog.models,...namRows()].filter(m=>stars.has(m.key)),
    showFavorite(key){nav("Favorites");const m=[...catalog.models,...namRows()].find(r=>r.key===key);if(m){el("modelSearch").value=m.name;render();}}};
}
