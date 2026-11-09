(() => {
  const el = id => document.getElementById(id);
  const node = (tag, text) => Object.assign(document.createElement(tag), {textContent: text});
  let state = {}, selected = null, graphKey = "", fieldKey = "", viewKey = "";
  let previewPolicy = {deviceEnabled:false, transfersEnabled:false};
  let queued = null, sending = false, connecting = false, leaving = false, dragging = null;
  let localError = "";
  let writeEpoch = 0;
  const widgets = new Map();
  let directoryKey="", bankToBrowse=null, visiblePreset="";
  const inspected=new Map();
  let inspectionScope="", lastInspection=null;
  const presetNames=new Map();
  let dirSession="", listedRevision=null, listAttempt=null, forceRead=false;
  let picker=null;
  const models=createModelBrowser({selectModel,notice,target:currentBlock,libraryChanged:drawDirectory});
  const snapshotControls=createSnapshotControls(libraryCommand);
  const parameterControllers=createParameterControllers(libraryCommand);
  const moves=createBlockMoves(()=>state,libraryCommand);
  const routing=createFloorRouting(libraryCommand);
  const presetSaves=createPresetSave({getState:()=>state,send:libraryCommand});
  const presetFiles=createPresetFiles({getState:()=>state,send:libraryCommand,notice});
  let lastSaveReceipt="";
  function currentBlock(){return state.board?.blocks.find(b=>b.slot===selected);}
  function notice(message){localError=message;el("status").textContent=message;}
  const context = () => [state.session,state.generation,selected].join("/");
  const current = () => state.board?.blocks.find(b => b.slot === selected);
  const configured=createConfiguredFavorites({getState:()=>state,getTarget:current,getSlot:()=>selected,send:libraryCommand,notice});
  async function post(path, body = {}) {
    const response = await fetch("/live/" + path, {method:"POST", headers:{"Content-Type":"application/json"},
      body:JSON.stringify(body),signal:AbortSignal.timeout(10000)});
    if (!response.ok) throw Error((await response.json()).error || "Floor connection failed");
  }
  el("reportIssue").onclick=()=>{
    el("issueForm").reset();el("issueError").textContent="";el("issueDialog").showModal();
  };
  el("cancelIssue").onclick=()=>el("issueDialog").close();
  el("issueForm").onsubmit=async event=>{
    event.preventDefault();
    const button=el("issueForm").querySelector('[type="submit"]');button.disabled=true;
    try{
      await post("incident",Object.fromEntries(new FormData(el("issueForm"))));
      el("issueDialog").close();notice("Problem recorded locally with recent editor activity.");
    }catch(error){el("issueError").textContent=error.message;}finally{button.disabled=false;}
  };
  function choose(slot) {
    picker=null;
    queued = dragging = null;
    selected = slot;localError="";
    models.follow(current());draw();
  }
  function enqueue(op,value,index) {
    if (!state.online || !current()?.editable) return;
    localError = "";
    queued = {op,value,index,context:context()};
    flush();
  }
  async function flush() {
    if (!queued || sending || state.pending || !state.online) return;
    const next = queued;
    queued = null;
    if (next.context !== context()) return;
    sending = true;
    ++writeEpoch;
    const start = state;
    state = {...state,pending:true};
    draw();
    try {
      await post("edit", {op:next.op,index:next.index,value:next.value,session:start.session,rev:start.rev,
        model:current().model,slot:selected});
    } catch (e) { queued=null; localError=e.message; }
    finally {sending=false;}
  }
  function pathView(board) {
    const key=JSON.stringify([board.bank,board.label,board.paths,board.insertSlots,models.categoryInfo("Amp")?.icon,state.online,selected,board.blocks.map(b=>[b.slot,b.model,b.name,b.enabled])]);
    if(key===graphKey)return;
    graphKey=key;
    const rows=[];
    let mapped=0;
    for(const [dsp,path] of board.paths.entries()){
      for(let branch=0;branch<=(path.split?1:0);branch++){
        const cells={};
        for(const b of board.blocks.filter(b=>b.dsp===dsp && b.place?.[0]===branch)){
          const column=b.place[1]+1;
          if(column<1||column>8||cells[column]) continue;
          mapped++;
          const family=b.model===380?"NAM":models.infoFor(b)?.category||b.category;
          cells[column]={index:b.slot,name:b.name,bypassed:!b.enabled,selected:b.slot===selected,color:models.tint(family),
            icon:family==="NAM"?models.infoFor(b)?.icon:models.categoryInfo(family)?.icon,
            kind:/amp|NAM/i.test(b.category)?"amp":/delay/i.test(b.category)?"delay":"pedal"};
        }
        const emptyPositions=state.online && dsp===0 && branch===0 ? board.insertSlots||[] : [];
        rows.push({label:String(dsp+1)+(branch?"B":"A"),input:branch?"Branch":"Input",output:branch?"Join":"Out",cells,emptyPositions,selectedEmpty:!current()&&dsp===0&&branch===0?selected:null});
      }
    }
    const graph=el("path");
    if(mapped===board.blocks.length) graph.replaceChildren(signalPath(rows,choose,(_,slot)=>openPicker(slot)));
    else graph.replaceChildren(node("p","This routing is available in the block list."));
    moves.bindPath(graph,board);
    routing.bindPath(graph,board);
    const list=el("blocks");list.replaceChildren();
    for(const b of board.blocks){
      const btn=node("button",b.name);
      btn.setAttribute("aria-pressed",String(selected===b.slot));
      btn.onclick=()=>choose(b.slot); list.append(btn);
    }
    if(!board.blocks.length)list.append(node("p","Empty preset"));
  }
  function inspector(b) {
    const art=models.infoFor(b);const key=JSON.stringify([context(),b?.model,b?.pairModel,b?.name,b?.cabParams,art?.icon,Boolean(picker)]);
    if(key!==fieldKey){
      fieldKey=key; widgets.clear();const target=el("inspector");target.replaceChildren();
      if(!b){
        if(selected!=null && state.board?.insertSlots?.includes(selected)){
          target.append(node("h2","Empty block"),node("p","Choose a model from the browser to add it here."));
          const cancel=node("button","Cancel");cancel.onclick=()=>choose(state.board.blocks[0]?.slot??null);target.append(cancel);
        }else target.append(node("p","Select a block or an empty position on the signal path."));
        return;
      }
      const heading=node("div","");heading.className="inspect-head";
      target.style.setProperty("--cat",models.tint(art?.category||b.category));
      if(art?.icon){const img=node("img","");img.src=art.icon;img.alt="";heading.append(img);}
      const title=node("div","");title.append(node("small",art?.category||b.category),node("h2",b.name));heading.append(title);
      const bypass=node("button","");bypass.id="blockBypass";
      bypass.onclick=()=>enqueue("bypass",!current().enabled);heading.append(bypass);target.append(heading);
      if(b.pair)target.append(node("p","Paired with "+b.pair));
      if(b.cabParams?.length){
        const details=node("details","");details.append(node("summary",b.pair+" · cab settings"));
        for(const p of b.cabParams){
          const row=node("div","");row.className="param";row.append(node("span",p.name),node("span",p.display));details.append(row);
        }
        target.append(details);
      }
      if(!b.editable)target.append(node("p",b.namTarget?"Choose a capture in NAM to switch this block.":"Parameters are read-only for this block."));
      const fields=node("div","");fields.id="fields";target.append(fields);
      for(const p of b.params){
        const row=node("div","");row.className="param";
        const label=node("label",p.name||("Parameter "+(p.index+1)));row.append(label);
        let input;
        if(p.edit){
          if(p.kind==="Switch"){
            input=node("input","");input.type="checkbox";input.onchange=()=>enqueue("param",Number(input.checked),p.index);
          }else if(p.choices){
            input=node("select","");
            p.choices.forEach((name,n)=>input.add(new Option(name,String(p.min+n))));
            input.onchange=()=>enqueue("param",Number(input.value),p.index);
          }else{
            input=node("input","");input.type="range";input.min=p.min;input.max=p.max;
            input.step=p.kind==="Enum"?1:(p.max-p.min)/1000;
            input.onpointerdown=()=>{dragging=p.index;};
            input.onpointercancel=input.onlostpointercapture=()=>{dragging=null;};
            input.oninput=()=>enqueue("param",Number(input.value),p.index);
          }
          label.htmlFor=input.id="param-"+p.index;row.append(input);
        }
        const out=node("output","");row.append(out);fields.append(row);widgets.set(p.index,{input,out});
      }
      if(b.manage){
        const actions=node("div","");actions.className="block-actions";
        const replace=node("button","Replace block"),remove=node("button","Remove block");
        replace.onclick=()=>openPicker(b.slot);
        remove.onclick=()=>{
          if(confirm("Remove "+b.name+" from position "+b.slot+" in this unsaved edit?")){
            libraryCommand({op:"remove",slot:b.slot,model:b.model,pairModel:b.pairModel});
          }
        };
        actions.append(replace,remove);target.append(actions);
      }
      moves.controls(target,state.board,b);
    }
    if(!b)return;
    const busy=sending||state.pending;
    for(const action of el("inspector").querySelectorAll(".block-actions button"))action.disabled=!state.online||busy;
    const by=el("blockBypass");by.disabled=!state.online||!b.editable||busy;
    by.textContent=b.enabled?"Bypass block":"Enable block";
    for(const p of b.params){
      const {input,out}=widgets.get(p.index);
      out.value=p.display;
      if(!input)continue;
      input.disabled=!state.online||(input.type!=="range"&&busy);
      if(input.type==="checkbox"){if(!busy)input.checked=Boolean(p.value);}
      else if(dragging!==p.index && queued?.index!==p.index && !busy)input.value=p.value;
    }
  }
  function openPicker(slot) {
    if(!state.online || sending || state.pending)return;
    const existing=state.board.blocks.find(b=>b.slot===slot);
    if(existing?!existing.manage:!state.board.insertSlots?.includes(slot))return;
    selected=slot;queued=dragging=null;fieldKey="";localError="";
    picker={context:context()};
    if(existing)models.follow(existing);
    draw();
    el("categories").querySelector("button[aria-pressed=true]")?.focus({preventScroll:true});
  }
  function selectModel(row){
    el("inspector").querySelector(".capture-install-panel")?.remove();
    const b=current(), empty=state.board?.insertSlots?.includes(selected);
    const reason=message=>notice(row.name+" · "+message);
    if(row.libraryOnly)return reason("A2 Full · library only");
    if(!state.online)return reason("Connect the Floor to use this model.");
    if(state.pending||sending)return reason("Wait for the current edit.");
    if(row.category==="NAM"){
      if(!previewPolicy.transfersEnabled)return reason(previewPolicy.transferMessage||"Device NAM selection is unavailable in this preview.");
      if(!row.ready)return reason(row.compatibility||"A compatible A2 Lite conversion is required.");
      if(!state.board?.namReady)return reason("Recall a preset with a NAM A2 Lite block to identify the installed engine.");
      if(!row.installed){
        const action=node("button","Install on Floor");action.title=row.name;action.className="install-capture";
        const panel=node("section","");panel.className="capture-install-panel";
        panel.append(node("strong",row.name),action);
        const connection=state.session;
        action.onclick=()=>{
          if(!state.online||state.session!==connection||state.pending||sending)return;
          if(!confirm("Install "+row.name+" in an empty storage slot on the Floor?"))return;
          panel.remove();libraryCommand({op:"nam-install",capture:row.id});
        };
        el("inspector").prepend(panel);return reason("Local capture. Use Install on Floor to add it to the device.");
      }
      if(!empty && !b?.namTarget)return reason("Select an empty 1A cell or an existing NAM block.");
      picker=null;libraryCommand({op:"nam",slot:selected,model:b?.model??null,capture:row.id});return;
    }
    if(!row.placeable || row.model==null)return reason("Browse only for this model.");
    if(!empty&&!b?.manage)return reason("Select an empty 1A cell or a supported stock block.");
    if(b && row.model===b.model && (row.pairedModel??null)===(b.pairModel??null))return reason("Already selected.");
    if(b && !confirm("Replace "+b.name+" with "+row.name+" and its default settings?"))return;
    picker=null;libraryCommand({op:b?"replace":"add",slot:selected,model:b?.model,pairModel:b?.pairModel,
      next:row.model,nextPair:row.pairedModel});
  }
  function draw(){
    snapshotControls(state,sending||queued!==null||dragging!==null);
    routing.update(state,sending||queued!==null||dragging!==null);
    presetSaves.update(state,sending||queued!==null||dragging!==null);
    presetFiles(state,sending||queued!==null||dragging!==null);
    document.body.classList.toggle("online",Boolean(state.online));
    el("connect").textContent=state.online?"Disconnect":state.recoveryRequired?"Reconnect Floor":"Connect Floor";
    el("connect").disabled=!previewPolicy.deviceEnabled||connecting||state.message==="Connecting";
    el("status").textContent=localError||state.message||"Connecting";
    const b=state.board;
    if(picker && (!state.online||picker.context!==context())){picker=null;fieldKey="";}
    if(!b){inspector(null);parameterControllers.render(el("inspector"),state,null,sending);drawDirectory();return;}
    const id=[state.session,state.generation,b.bank,b.label].join("/");
    if(viewKey!==id){
      const prevPreset=viewKey.split("/").slice(2).join("/");
      if(prevPreset!==[b.bank,b.label].join("/")||!current()){
        selected=b.blocks[0]?.slot??null;if(current())models.follow(current());
      }
      if(state.online)refreshCaptures();
      viewKey=id;queued=dragging=null;
    }
    el("preset").textContent=b.label+" · "+b.name;
    el("tempo").textContent=b.tempo?Number(b.tempo).toFixed(1)+" BPM":"";
    const here=current();
    el("location").textContent=here?.place?"Path "+(here.dsp+1)+(here.place[0]?"B":"A")+" · "+(here.place[1]+1):
      b.insertSlots?.includes(selected)?"Path 1A · "+selected:"";
    pathView(b);inspector(here);parameterControllers.render(el("inspector"),state,here,sending);drawDirectory();autoReadLibrary();
  }
  function drawDirectory(){
    const saved=state.presetSave;
    if(saved?.phase==="saved"&&saved.request!==lastSaveReceipt){
      lastSaveReceipt=saved.request;presetNames.delete(saved.bank);listedRevision=null;listAttempt=null;
    }
    const kind=el("shelf").value;
    configured.update(kind,sending||queued!==null||dragging!==null);
    for(const tab of el("libraryTabs").querySelectorAll("button")){
      tab.setAttribute("aria-pressed",String(tab.dataset.shelf===kind));
    }
    el("sidebarTitle").textContent={presets:"Setlist",irs:"Impulse responses",nam:"NAM captures",favorites:"Favorites",blocks:"Blocks"}[kind];
    el("find").setAttribute("aria-label",kind==="favorites"?"Filter model bookmarks":"Filter library");
    el("find").placeholder=kind==="presets"?"Filter presets":kind==="favorites"?"Filter model bookmarks":"Filter library";
    const liveDir=state.directory;
    if(state.session && dirSession!==state.session){
      dirSession=state.session;presetNames.clear();listedRevision=null;listAttempt=null;bankToBrowse=null;forceRead=false;
    }
    if(liveDir?.kind==="presets" && state.online && listedRevision!==state.version){
      presetNames.set(liveDir.bank,liveDir);listedRevision=state.version;
    }else if(liveDir?.kind==="presets" && state.version==null && state.online){
      presetNames.set(liveDir.bank,liveDir);
    }
    const wantedBank=bankToBrowse??state.board?.bank;
    const dir=kind==="presets"?presetNames.get(wantedBank):liveDir;
    const valid=dir?.kind===kind && (kind==="presets"||dir.generation===state.generation);
    const sets=dir?.setlists||[...presetNames.values()].at(-1)?.setlists,menu=el("setlist");
    menu.hidden=kind!=="presets"||!sets?.length;
    if(!menu.hidden){
      if(menu.dataset.names!==JSON.stringify(sets)){
        menu.replaceChildren();sets.forEach((name,i)=>menu.add(new Option(name,i)));menu.dataset.names=JSON.stringify(sets);
      }
      menu.value=String(bankToBrowse??state.board?.bank??0);
    }
    const captureRows=models.rows();
    // Preset edits and adding an impulse can advance generation while existing
    // storage slots stay in place. Keep observations until refresh or reconnect.
    const scope=state.online?state.session:"";
    if(scope!==inspectionScope){inspected.clear();lastInspection=null;inspectionScope=scope;}
    const observed=state.inspectedSlot;
    const observationKey=JSON.stringify(observed);
    if(state.online && observed && observed.generation===state.generation && observationKey!==lastInspection){
      inspected.set(observed.index,observed);lastInspection=observationKey;
    }
    const slotsById=new Map(captureRows.flatMap(c=>(c.floorCopies||[]).map(([i,name])=>[i,{name,c}])));
    const occupied=state.online?(liveDir?.kind==="irs" && liveDir.generation===state.generation?liveDir.rows:
      state.board?.captureDirectory?.map(([index,name])=>({index,name}))):null;
    if(occupied && inspected.size){
      const names=new Map(occupied.map(row=>[row.index,row.name]));
      for(const [index,entry] of inspected)if(names.get(index)!==entry.name)inspected.delete(index);
    }
    const storage=(occupied||[]).map(row=>{
      const seen=inspected.get(row.index);
      const copy=slotsById.get(row.index);
      const capture=copy?.name===row.name?copy.c:null;
      const family=seen?.name===row.name?seen.family:/^IR\s/i.test(row.name)?"IR":
        capture||/^NAM\s/i.test(row.name)?"NAM":"Unidentified";
      return {...row,family,capture,checked:seen?.name===row.name};
    });
    el("irTools").hidden=kind!=="irs";
    el("installIR").disabled=!previewPolicy.transfersEnabled||!state.online||state.pending||sending||!el("irFile").files.length;
    const totals={NAM:0,IR:0,Unidentified:0};
    for(const row of storage)totals[row.family]++;
    const unknownCount=totals.Unidentified;
    el("slotFilter").options[1].textContent="Unidentified ("+unknownCount+")";
    el("storageSummary").hidden=kind!=="irs"&&kind!=="nam";
    const summary=occupied
      ? [totals.NAM+" NAM",totals.IR+(totals.IR===1?" IR":" IRs"),unknownCount+" unidentified",(128-storage.length)+" free"].join(" · ")
      : state.online?"Reading Floor storage…":"Connect the Floor to see storage.";
    if(el("capacity").textContent!==summary)el("capacity").textContent=summary;
    const diskCaptures=storage.filter(r=>r.family==="NAM"&&!r.capture);
    const rows=kind==="nam"?[...captureRows,...diskCaptures]:kind==="favorites"?models.favorites():
      kind==="irs"?storage.filter(r=>el("slotFilter").value==="all"||el("slotFilter").value===r.family):valid?dir.rows:[];
    const filter=el("find").value.trim().toLowerCase();
    const exhausted=listAttempt && listAttempt.bank===wantedBank && listAttempt.count>=3 && !state.pending;
    const key=JSON.stringify([kind,filter,rows,state.online,state.pending,state.board?.bank,state.board?.label,wantedBank,valid,exhausted,
      kind==="irs"?[unknownCount,el("slotFilter").value]:null]);
    if(key===directoryKey)return;directoryKey=key;
    const list=el("entries");list.replaceChildren();
    for(const row of rows){
      const text=[row.label??(kind==="irs"?row.index:null),row.name,kind==="nam"?row.kind:null].filter(Boolean).join(" · ");
      if(!text.toLowerCase().includes(filter))continue;
      const inspect=kind==="irs"&&row.family==="Unidentified"&&!row.checked;
      const item=node((kind==="irs"&&!inspect)||(kind==="nam"&&!row.id)?"p":"button","");
      if(row.label||kind==="irs"){const num=node("span",row.label||row.index);num.className="preset-num";item.append(num);}
      item.append(node("span",row.name));
      if(kind==="irs"){
        const badge=node("small",inspect?"Identify":row.family);
        badge.className="storage-kind";item.append(badge);
        if(inspect){
          item.title="Read this slot’s samples to identify its type";
          item.disabled=!previewPolicy.transfersEnabled||state.pending||sending||!state.online;
          item.onclick=()=>libraryCommand({op:"inspect-slot",index:row.index,name:row.name});
        }
      }else if(kind==="nam"&&!row.id){
        item.append(node("small","Floor · "+row.index));
      }
      if(kind==="presets"){
        const active=dir.bank===state.board?.bank && row.label===state.board?.label;
        if(active)item.className="current";
        item.disabled=!state.online||state.pending||active;
        item.onclick=async()=>{
          if(!confirm("Recall "+row.label+" "+row.name+"? Unsaved edits on the current preset will be discarded."))return;
          await libraryCommand({op:"recall",index:row.index,name:row.name,bank:dir.bank});
        };
      }else if(kind==="nam"&&row.id){item.onclick=()=>models.showCapture(row.id);}
      else if(kind==="favorites"){item.onclick=()=>models.showFavorite(row.key);}
      list.append(item);
    }
    if(!list.children.length)list.append(node("p",kind==="favorites"?"Star a model to keep it here.":
      kind==="irs"&&occupied?(el("slotFilter").value==="IR"&&unknownCount?"No identified IRs match. Check Unidentified to identify older slots.":"No matching entries."):
      kind!=="nam"&&!valid?(!state.online?"Connect the Floor to read this list.":kind==="presets"?
        (exhausted?"Setlist read interrupted. Click refresh to retry.":"Reading setlist…"):"Read the library to see entries."):"No matching entries."));
    const active=list.querySelector(".current");
    const where=state.session+"/"+dir?.bank+"/"+state.board?.label;
    if(active && !filter && visiblePreset!==where){
      visiblePreset=where;
      const row=active.getBoundingClientRect(),view=list.getBoundingClientRect();
      if(row.top<view.top || row.bottom>view.bottom)list.scrollTop+=row.top-view.top-(view.height-row.height)/2;
    }else if(!active)visiblePreset="";
  }
  function autoReadLibrary(){
    if(!state.online||!state.board||state.pending||sending||el("shelf").value!=="presets")return;
    const bank=bankToBrowse??state.board.bank;
    if(presetNames.has(bank)&&!forceRead)return;
    if(!listAttempt||listAttempt.bank!==bank)listAttempt={bank,count:0,at:0};
    if(listAttempt.count>=3 || Date.now()-listAttempt.at<900)return;
    ++listAttempt.count;listAttempt.at=Date.now();forceRead=false;
    libraryCommand({op:"presets",bank});
  }
  let namRead=null, namAgain=false;
  function refreshCaptures(){
    if(namRead){namAgain=true;return namRead;}
    namRead=models.loadNam().then(drawDirectory).finally(()=>{
      namRead=null;if(namAgain){namAgain=false;refreshCaptures();}
    });
    return namRead;
  }
  async function libraryCommand(command){
    if(!state.online||state.pending||sending)return;
    ++writeEpoch;sending=true;queued=null;localError="";
    state={...state,pending:true};draw();
    try {
      await post("edit",{...command,session:state.session,rev:state.rev});
    } catch(e){localError=e.message;state={...state,pending:false};}
    finally{sending=false;draw();}
  }
  async function browse(){
    const kind=el("shelf").value;
    drawDirectory();
    if(kind==="nam"){models.nav("NAM");await refreshCaptures();}
    else if(kind==="favorites"){models.nav("Favorites");}
    else if(kind==="presets")autoReadLibrary();
    else if(kind==="irs")await libraryCommand({op:"irs"});
  }
  for(const button of el("libraryTabs").querySelectorAll("button")){
    button.onclick=()=>{el("shelf").value=button.dataset.shelf;el("find").value="";browse();};
  }
  el("shelf").onchange=()=>{el("find").value="";browse();};
  el("slotFilter").onchange=drawDirectory;
  el("irFile").onchange=drawDirectory;
  el("installIR").onclick=async()=>{
    const file=el("irFile").files[0];
    if(!file||!state.online||state.pending||sending)return;
    const kind=el("irType").value;
    if(!confirm("Install "+file.name+" as "+el("irType").selectedOptions[0].textContent+" in shared Floor storage?"))return;
    sending=true;++writeEpoch;localError="";queued=null;state={...state,pending:true};draw();
    try{
      const response=await fetch("/live/ir/install",{method:"POST",body:file,signal:AbortSignal.timeout(15000),headers:{
        "Content-Type":"application/octet-stream","X-IR-Name":encodeURIComponent(file.name),"X-IR-Kind":kind,
        "X-Floor-Session":state.session,"X-Floor-Rev":String(state.rev)}});
      if(!response.ok)throw Error((await response.json()).error||"IR import failed");
      el("slotFilter").value="IR";el("irFile").value="";
    }catch(error){localError=error.message;state={...state,pending:false};}
    finally{sending=false;draw();}
  };
  el("setlist").onchange=()=>{bankToBrowse=Number(el("setlist").value);listAttempt=null;drawDirectory();autoReadLibrary();};
  el("updateList").onclick=()=>{if(el("shelf").value==="presets"){forceRead=true;listAttempt=null;}
    if(el("shelf").value==="irs"){inspected.clear();lastInspection=JSON.stringify(state.inspectedSlot);}
    browse();};el("find").oninput=drawDirectory;
  async function connect(manual=false){
    if(!previewPolicy.deviceEnabled){notice(previewPolicy.deviceMessage||"Device access disabled in this preview.");return;}
    connecting=true;localError="";
    try{await post("connect",{manual});}catch(e){localError=e.message;}
    finally{connecting=false;draw();}
  }
  async function poll(){
    if(leaving)return;
    const atStart=writeEpoch, overlapped=sending;
    try{
      const headers={"X-Floor-Read":"1"};
      if(state.online && !state.pending && state.version!=null)headers["X-Floor-Version"]=state.session+"/"+state.version;
      const r=await fetch("/live/state",{headers,signal:AbortSignal.timeout(6000)});
      if(r.status===204){autoReadLibrary();return;}
      if(!r.ok)throw Error("Bridge unavailable");
      const next=await r.json();
      // A poll launched before POST acceptance may still carry the old baseline.
      if(!sending && !overlapped && atStart===writeEpoch){
        if(next.session!==state.session || next.generation!==state.generation || !next.online)queued=null;
        if(!next.board && state.board)next.board=state.board;
        state=next;
        draw();flush();
      }
    }catch(e){state={...state,online:false,message:e.message};queued=null;draw();}
    finally{if(!leaving)setTimeout(poll,document.hidden?750:85);}
  }
  el("connect").onclick=async()=>{
    if(!state.online){await connect(true);return;}
    queued=null;
    try{await post("release");}catch(e){localError=e.message;draw();}
  };
  el("library").onclick=async()=>{
    leaving=true;queued=null;
    try{await post("release");location.href="/transfer";}catch(e){leaving=false;localError=e.message;draw();poll();}
  };
  bindPathZoom(el("viewport"),el("path"),el("zoom"));
  const theme=el("appearance"), system=matchMedia("(prefers-color-scheme: dark)");
  try{theme.value=localStorage.getItem("floor-editor-appearance")||"dark";}catch{}
  const applyTheme=()=>document.documentElement.dataset.appearance=theme.value==="system"?(system.matches?"dark":"light"):theme.value;
  theme.onchange=()=>{applyTheme();try{localStorage.setItem("floor-editor-appearance",theme.value);}catch{}};
  applyTheme();system.addEventListener("change",applyTheme);
  models.load().then(()=>{draw();});
  state={online:false,pending:false,message:"Offline · local library"};draw();
  el("connect").addEventListener("click",()=>poll(),{once:true});
  fetch('/live/capabilities',{signal:AbortSignal.timeout(5000)}).then(r=>{
    if(!r.ok)throw Error('Preview settings unavailable');return r.json();
  }).then(policy=>{
    previewPolicy=policy;
    el('previewNotice').textContent=policy.deviceEnabled
      ? 'Experimental device access enabled · Capture/IR transfers and named NAM selection unavailable.'
      : 'Offline preview · Import and organize NAM locally. Device testing requires launching with -ExperimentalDevice.';
    el('connect').title=policy.deviceEnabled?'Start an experimental Floor session':policy.deviceMessage;
    draw();
  }).catch(error=>{notice(error.message);draw();});
})();
