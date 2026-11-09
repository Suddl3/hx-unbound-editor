// The live inspector asks for an edit only in response to an explicit input
// gesture. Every displayed source, range and CC comes from the Floor preset.
function createParameterControllers(send) {
  let context = "", selected = 0, open = false, lastKey = "", panel;
  const make = (tag, text = "") => Object.assign(document.createElement(tag), {textContent:text});
  function labeled(label, input) {
    const field = make("label", label + " ");
    input.setAttribute("aria-label",label);
    field.append(input);
    return field;
  }
  function render(host, state, block, sending) {
    const data=state.board?.controllers;
    if(!block || !data || !host) { panel?.remove();panel=null;lastKey="";return; }
    const here=[state.session,state.board.bank,state.board.label,block.slot,block.model].join("/");
    if(here!==context){context=here;selected=0;open=false;}
    else if(panel)open=panel.open;
    const rows=data.rows.filter(r=>r.slot===block.slot);
    const links=Array.isArray(data.switchLinks)?data.switchLinks.filter(r=>r.slot===block.slot):null;
    const writable=block.dsp===0 && block.slot>=1 && block.slot<=8 &&
      block.place?.[0]===0 && block.place[1]===block.slot-1;
    const params=writable?block.params.filter(p=>(p.edit && block.editable)||(p.index===0 && block.namTarget)):[];
    if(!params.length && !rows.length && !links?.length){panel?.remove();panel=null;lastKey="";return;}
    if(!params.some(p=>p.index===selected))selected=params[0]?.index;
    const busy=sending || !state.online || state.pending;
    const key=JSON.stringify([here,selected,params.map(p=>[p.index,p.name]),rows,links,data.sources,busy]);
    if(panel?.parentNode===host && key===lastKey)return;
    const old=panel;
    lastKey=key;panel=make("details");panel.className="controller-panel";panel.open=open;
    panel.ontoggle=()=>{if(panel===next && panel.isConnected)open=panel.open;};
    const next=panel;
    panel.append(make("summary","Controllers"));
    const body=make("div");body.className="controller-fields";panel.append(body);
    for(const r of rows){
      const name=r.target==="parameter"?(block.params.find(p=>p.index===r.index)?.name||"Parameter "+(r.index+1)):
        r.target==="mic"?"Cab mic":"Bypass";
      const source=r.cc==null?r.name:r.name+" "+r.cc;
      const range=r.target==="parameter" && r.min!=null && r.max!=null?
        " ("+Number(r.min.toFixed(3))+" to "+Number(r.max.toFixed(3))+")":"";
      body.append(make("small",name+" · "+source+range));
    }
    for(const link of links||[])body.append(make("small",
      "FS"+link.switch+" · "+(link.sharedParameter?"Carries a parameter":"Block bypass")));
    if(writable && links && (block.editable||block.namTarget) && block.pairModel==null && links.length<=1 &&
        !links.some(link=>link.sharedParameter)){
      const pick=make("select");pick.add(new Option("None",""));
      for(const s of data.sources){
        if(!s.switch || (links.length && s.switch!==links[0].switch))continue;
        pick.add(new Option("FS"+s.switch,s.switch));
      }
      pick.value=String(links[0]?.switch||"");pick.disabled=busy;
      pick.onchange=()=>{
        const assigning=Boolean(pick.value),number=assigning?Number(pick.value):links[0].switch;
        pick.value=String(links[0]?.switch||"");
        send({op:"controller-bypass",slot:block.slot,model:block.model,switch:number,value:assigning,expected:links});
      };
      body.append(labeled("Bypass footswitch",pick));
    }
    if(params.length){
      const parameter=make("select");
      for(const p of params)parameter.add(new Option(p.name||"Parameter "+(p.index+1),p.index));
      parameter.value=String(selected);
      parameter.onchange=()=>{selected=Number(parameter.value);render(host,state,block,sending);};
      body.append(labeled("Parameter",parameter));
      const expected=rows.filter(r=>r.target==="parameter" && r.index===selected);
      if(expected.length<=1){
        const assignment=expected[0], select=make("select");
        select.add(new Option("None",0));
        for(const s of data.sources)select.add(new Option(s.name,s.value));
        if(assignment&&!data.sources.some(s=>s.value===assignment.source)){
          const retained=new Option(assignment.name,assignment.source);retained.disabled=true;select.add(retained);
        }
        select.value=String(assignment?.source||0);
        const payload={op:"controller-param",slot:block.slot,model:block.model,index:selected,expected};
        select.disabled=busy;
        select.onchange=()=>{
          const value=Number(select.value);
          select.value=String(assignment?.source||0);
          send({...payload,field:"source",value});
        };
        body.append(labeled("Controller",select));
        if(data.sources.some(s=>s.midi && s.value===assignment?.source)){
          const cc=make("input");cc.type="number";cc.min="0";cc.max="127";cc.step="1";
          cc.value=assignment.cc==null?"":String(assignment.cc);cc.disabled=busy;
          cc.onchange=()=>{
            const value=cc.valueAsNumber;
            cc.value=assignment.cc==null?"":String(assignment.cc);
            if(Number.isInteger(value) && value>=0 && value<=127 && value!==assignment.cc)send({...payload,field:"cc",value});
          };
          body.append(labeled("MIDI CC number",cc));
        }
        const p=params.find(param=>param.index===selected);
        const source=data.sources.find(s=>s.value===assignment?.source);
        const normalized=block.model===380 ? selected>=1 && selected<=6 :
          p.kind==="Continuous" && p.min===0 && p.max===1;
        if(source && !source.snapshot && normalized){
          for(const [field,caption] of [["min","Minimum (%)"],["max","Maximum (%)"]]){
            const endpoint=make("input");endpoint.type="number";endpoint.min="0";endpoint.max="100";endpoint.step="any";
            const percent=String(Number((assignment[field]*100).toFixed(3)));
            endpoint.value=percent;endpoint.disabled=busy;
            endpoint.onchange=()=>{
              const value=endpoint.valueAsNumber;endpoint.value=percent;
              if(Number.isFinite(value) && value>=0 && value<=100 && value!==Number(percent)){
                send({...payload,field,value:value/100});
              }
            };
            body.append(labeled(caption,endpoint));
          }
        }
      }else body.append(make("small","Multiple controllers assigned on the Floor."));
    }
    if(old?.parentNode===host)old.replaceWith(panel);else host.append(panel);
  }
  return {render};
}
