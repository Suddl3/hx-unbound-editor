function createConfiguredFavorites({getState,getTarget,getSlot,send,notice}){
  const host=document.getElementById("configuredFavorites");
  const make=(tag,text="")=>Object.assign(document.createElement(tag),{textContent:text});
  const title=make("h3","Configured blocks"),menu=make("select");
  menu.setAttribute("aria-label","Configured block favorites");
  const keep=make("button","Keep selected block…"),apply=make("button","Add to selected slot");
  keep.type=apply.type="button";
  const help=make("small");
  host.append(title,menu,keep,apply,help);
  let rows=[],revision=null,loading=false,pendingRead=false,key="",loadError="";
  async function read(){
    if(loading){pendingRead=true;return;}loading=true;
    try{
      const res=await fetch("/live/configured-favorites",{headers:{"X-Floor-Read":"1"},signal:AbortSignal.timeout(8000)});
      if(!res.ok)throw Error("Could not read configured favorites.");
      const records=await res.json(),selected=menu.value;
      rows=records;
      menu.replaceChildren(...records.map(r=>new Option(r.name,r.id)));
      if(records.some(r=>r.id===selected))menu.value=selected;
      if(records.length===0)menu.add(new Option("No saved blocks",""));
      loadError="";
    }catch(err){loadError=err.message;rows=[];menu.replaceChildren();}
    finally{loading=false;key="";render();if(pendingRead){pendingRead=false;read();}}
  }
  let busy=false;
  function render(){
    if(host.hidden)return;
    const s=getState(),b=getTarget(),slot=getSlot(),ready=s.online&&!busy&&!s.pending;
    const vacant=s.board?.insertSlots?.includes(slot)===true;
    const status=loadError||(!s.online?"Connect the Floor to keep or place a block.":
      vacant?"Choose a configured block to add.":b?.manage?"Keep this model with its current settings.":"Select a stock block to keep, or an empty 1A position to add.");
    const next=JSON.stringify([s.online,ready,b?.manage,b?.slot,slot,vacant,menu.value,status]);
    if(next===key)return;key=next;
    keep.disabled=!ready||!b?.manage;apply.disabled=!ready||!vacant||!menu.value;
    menu.disabled=!rows.length;help.textContent=status;
  }
  menu.onchange=render;
  keep.onclick=()=>{
    const b=getTarget();if(!b?.manage||keep.disabled)return;
    const name=prompt("Name this configured block. Its model and settings are saved on this computer.",b.name);
    if(name===null)return;
    if(!name.trim()||[...name.trim()].length>48){notice("Choose a name up to 48 characters.");return;}
    send({op:"configured-capture",slot:b.slot,model:b.model,name:name.trim()});
  };
  apply.onclick=()=>{
    const row=rows.find(r=>r.id===menu.value),slot=getSlot();if(!row||apply.disabled)return;
    if(confirm("Add “"+row.name+"” with its saved settings to position "+slot+" on path 1A?")){
      send({op:"configured-apply",slot,favorite:row.id});
    }
  };
  return {update(kind,inFlight){
    const opening=host.hidden&&kind==="favorites";host.hidden=kind!=="favorites";busy=inFlight;
    if(host.hidden)return;
    const mark=getState().configuredRevision||"";
    if(opening||revision!==mark){revision=mark;read();}
    render();
  }};
}
