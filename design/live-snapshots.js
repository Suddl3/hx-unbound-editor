function createSnapshotControls(send) {
  const get=id=>document.getElementById(id);
  const select=get("activeSnapshot"), edit=get("editSnapshot"), dialog=get("snapshotDialog");
  let state={}, scope="", choices="", dialogScope="", original;
  const title=s=>(s.index+1)+" · "+(s.name||"Snapshot "+(s.index+1));
  const slots=()=>state.board?.snapshots?.slots||[];
  const active=()=>state.board?.snapshots?.active;
  const busy=()=>!state.online||state.pending;
  function drawBypasses(){
    const table=document.createElement("table"), header=table.createTHead().insertRow();
    function heading(row,text,scope,title){
      const cell=document.createElement("th");cell.scope=scope;cell.textContent=text;cell.title=title;row.append(cell);
    }
    heading(header,"Block","col","Signal path block");
    for(const s of slots())heading(header,String(s.index+1),"col",title(s));
    const body=table.createTBody();
    for(const b of state.board.blocks){
      const row=body.insertRow();heading(row,b.name,"row",b.name);
      for(const s of slots()){
        const enabled=s.index===active()?b.enabled:s.valid?s.enabled?.[b.slot]:null;
        const cell=row.insertCell();
        cell.textContent=enabled===true?"On":enabled===false?"Off":"—";
        cell.title=title(s)+" · "+(enabled==null?"Unknown":enabled?"Enabled":"Bypassed");
      }
    }
    get("snapshotMatrix").replaceChildren(state.board.blocks.length?table:"This preset has no processing blocks.");
  }
  function command(fields){
    if(busy() || !slots()[active()])return;
    send({...fields,from:active()});
  }
  select.onchange=()=>{
    const index=Number(select.value);
    select.value=String(active()); // The Floor's readback moves the selection.
    command({op:"snapshot-select",index});
  };
  edit.onclick=()=>{
    const s=slots()[active()];if(!s||busy())return;
    dialogScope=scope;
    original={wasName:s.name,wasColor:s.color};
    get("snapshotHeading").textContent=title(s);
    drawBypasses();
    const colors=state.board.snapshotColors||[];
    get("snapshotName").value=s.name;
    const color=get("snapshotColor");
    color.replaceChildren(...colors.map((text,i)=>new Option(text,i)));
    color.value=String(s.color);
    get("snapshotForm").querySelector("button").disabled=!Number.isInteger(s.color)||!colors[s.color];
    dialog.showModal();
  };
  get("closeSnapshot").onclick=()=>dialog.close();
  get("snapshotForm").onsubmit=e=>{
    e.preventDefault();
    if(scope!==dialogScope||busy())return;
    const name=get("snapshotName").value.trim(), color=Number(get("snapshotColor").value);
    if(!name)return;
    dialog.close();
    if(name!==original.wasName||color!==original.wasColor)command({op:"snapshot-metadata",...original,name,color});
  };
  return (next,working)=>{
    state={...next,pending:next.pending||working};
    scope=[state.session,state.generation,active()].join("/");
    if(dialog.open && (!state.online||scope!==dialogScope))dialog.close();
    else if(dialog.open && get("snapshotCompare").open)drawBypasses();
    const options=slots(), key=JSON.stringify(options.map(s=>[s.index,s.name]));
    if(choices!==key){
      choices=key;select.replaceChildren(...options.map(s=>new Option(title(s),s.index)));
    }
    const has=Number.isInteger(active())&&Boolean(options[active()]);
    get("snapshotControls").hidden=!options.length;
    select.value=has?String(active()):"";
    select.disabled=edit.disabled=busy()||!has;
  };
}
