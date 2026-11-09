// Persistent operations use a fresh read-only preview of both hardware slots.
function createPresetSave({getState, send}) {
  const make=(tag,text="")=>Object.assign(document.createElement(tag),{textContent:text});
  const host=document.getElementById("presetSaveActions");
  const save=make("button","Save"),saveAs=make("button","Save As…");
  save.type=saveAs.type="button";
  save.title="Save the current preset on the Floor";
  saveAs.title="Save a copy to another preset slot";
  host.append(save,saveAs);
  let active=null;

  function open(copy){
    const state=getState(),board=state.board;
    if(!state.online||state.pending||!board||active)return;
    const source={bank:board.bank,label:board.label,name:board.name};
    const dialog=make("dialog"),title=make("h2",copy?"Save preset as":"Save preset");
    dialog.className="tag-dialog";title.id="savePresetTitle";dialog.setAttribute("aria-labelledby",title.id);
    const from=make("p",source.label+" · "+source.name),bank=make("select"),slot=make("select");
    const name=make("input");name.type="text";name.maxLength=16;name.value=source.name;
    function control(caption,widget){const wrap=make("label",caption);wrap.append(widget);return wrap;}
    const choices=make("div");
    if(copy)choices.append(control("Destination setlist",bank),control("Destination preset",slot),control("New preset name",name));
    const status=make("p","Reading preset locations…");status.setAttribute("role","status");
    const actions=make("div");actions.className="block-actions";
    const cancel=make("button","Cancel"),confirm=make("button","Review destination");
    cancel.type=confirm.type="button";confirm.disabled=true;actions.append(cancel,confirm);
    dialog.append(title,from,choices,status,actions);document.body.append(dialog);
    const ctx={dialog,source,session:state.session,bank,slot,name,copy,confirm,cancel,status,choices,
      bankNumber:source.bank,rows:[],phase:"loading",id:"",receipt:null};
    active=ctx;
    dialog.addEventListener("close",()=>{clearTimeout(ctx.timer);if(active===ctx)active=null;dialog.remove();});
    cancel.onclick=()=>dialog.close();
    dialog.showModal();
    function request(op,extras={}){
      if(op!=="preset-save-commit")ctx.id=crypto.randomUUID();
      ctx.receipt=null;
      clearTimeout(ctx.timer);
      ctx.timer=setTimeout(()=>{
        if(active!==ctx)return;
        status.textContent=ctx.phase==="writing"?"Waiting for stored preset readback. Check the destination before retrying.":
          "The Floor did not return this preview. Close and reopen Save to read it again.";
        cancel.textContent="Close";
      },12000);
      send({op,request:ctx.id,...extras});
    }
    function loadTargets(){
      ctx.phase="loading";ctx.rows=[];slot.replaceChildren();ctx.confirm.disabled=true;
      ctx.status.textContent="Reading preset locations…";request("preset-targets",{bank:ctx.bankNumber});
    }
    function preview(){
      const row=copy?ctx.rows.find(r=>r.index===Number(slot.value)):ctx.rows.find(r=>r.label===source.label);
      if(!row||copy&&(slot.value===""||(ctx.bankNumber===source.bank&&row.label===source.label)))return;
      ctx.phase="reviewing";ctx.status.textContent="Reading source and destination…";
      ctx.confirm.disabled=true;bank.disabled=slot.disabled=name.disabled=true;
      request("preset-save-preview",{bank:ctx.bankNumber,index:row.index,source,expectedName:row.name,
        name:copy?name.value.trim():source.name,mode:copy?"copy":"current"});
    }
    bank.onchange=()=>{ctx.bankNumber=Number(bank.value);loadTargets();};
    function validate(){
      confirm.disabled=ctx.phase!=="choosing"||slot.value===""||!/^[ -~]{1,16}$/.test(name.value.trim());
    }
    slot.onchange=name.oninput=validate;
    confirm.onclick=()=>{
      if(ctx.phase==="choosing")preview();
      else if(ctx.phase==="ready"){
        ctx.phase="writing";ctx.status.textContent="Saving and checking the stored preset…";confirm.disabled=true;
        request("preset-save-commit");
      }
    };
    ctx.preview=preview;ctx.validate=validate;
    if(!copy && Number.isInteger(board.index)){
      ctx.rows=[{index:board.index,label:board.label,name:board.name}];
      preview();
    }else loadTargets();
  }
  save.onclick=()=>open(false);saveAs.onclick=()=>open(true);
  function update(state,busy){
    save.disabled=saveAs.disabled=!state.online||state.pending||busy||!state.board;
    const c=active;if(!c)return;
    const receipt=state.presetSave;
    if(receipt?.request===c.id&&receipt.phase==="saved"){
      c.dialog.close();return;
    }
    if(!state.online||state.session!==c.session||state.board?.label!==c.source.label||
        state.board.bank!==c.source.bank||state.board.name!==c.source.name){
      c.dialog.close();return;
    }
    if(!receipt||receipt.request!==c.id||state.pending||busy)return;
    const key=JSON.stringify(receipt);
    if(c.receipt===key)return;
    c.receipt=key;
    if(c.phase==="writing" && receipt.phase==="preview")return;
    clearTimeout(c.timer);
    if(receipt.error){
      c.confirm.disabled=true;c.status.textContent=receipt.error;c.cancel.textContent="Close";return;
    }
    if(receipt.phase==="targets"&&c.phase==="loading"){
      c.rows=receipt.rows;c.phase="choosing";
      if(!c.copy){c.preview();return;}
      c.bank.replaceChildren(...receipt.setlists.map((label,i)=>new Option(label,i)));
      c.bank.value=String(receipt.bank);
      c.slot.replaceChildren(new Option("Choose a destination",""));
      for(const row of c.rows){
        const opt=new Option(row.label+" · "+row.name,row.index);
        opt.disabled=receipt.bank===c.source.bank&&row.label===c.source.label;c.slot.add(opt);
      }
      c.status.textContent="The current preset stays loaded when you save a copy.";c.validate();
    }else if(receipt.phase==="preview"&&c.phase==="reviewing"){
      c.phase="ready";c.choices.hidden=true;c.confirm.disabled=false;
      c.status.textContent=c.copy?
        "Replace "+c.bank.selectedOptions[0].text+" / "+receipt.label+" “"+receipt.previous+"” with “"+receipt.name+"”?":
        "Store the current edits in "+receipt.label+" “"+receipt.name+"”?";
      c.confirm.textContent=c.copy?"Save copy to "+receipt.label:"Save to "+receipt.label;
    }
  }
  return {update};
}
