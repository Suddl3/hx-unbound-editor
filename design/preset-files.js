function createPresetFiles({getState, send, notice}) {
  const el=id=>document.getElementById(id);
  const dialog=el("presetFilesDialog"), save=el("exportNative"), info=el("fileSummary");
  let pending=null, timer=null;
  el("presetFiles").onclick=()=>dialog.showModal();
  el("closeFiles").onclick=()=>dialog.close();
  save.onclick=()=>{
    if(!getState().online||pending)return;
    const request=crypto.randomUUID();
    pending={request,session:getState().session};info.textContent="Reading current preset…";
    timer=setTimeout(()=>{pending=null;save.disabled=!getState().online;info.textContent="Export unconfirmed. Check the Floor connection and try again.";},12000);
    send({op:"preset-export",request});
  };
  el("inspectNative").onchange=async e=>{
    const file=e.target.files[0];e.target.value="";if(!file)return;
    if(file.size>1024*1024){info.textContent="Choose a preset up to 1 MiB.";return;}
    try{
      const response=await fetch("/live/inspect-file",{method:"POST",body:file,headers:{"Content-Type":"application/octet-stream"},signal:AbortSignal.timeout(11000)});
      const data=await response.json();if(!response.ok)throw Error(data.error);
      const lines=[file.name];
      for(const b of data.blocks)lines.push("Path "+(b.dsp+1)+" · "+b.name+" · "+(b.enabled?"on":"bypassed"));
      for(const s of data.snapshots.slots)lines.push((s.index===data.snapshots.active?"● ":"  ")+(s.index+1)+" · "+(s.name||"Snapshot "+(s.index+1)));
      info.textContent=lines.join("\n");
    }catch(error){info.textContent=error.message;}
  };
  return (state,busy)=>{
    save.disabled=!state.online||busy||state.pending||Boolean(pending);
    if(!pending)return;
    const file=state.presetFile;
    if(file?.request!==pending.request||state.session!==pending.session)return;
    clearTimeout(timer);pending=null;
    fetch("/live/export/"+file.request,{headers:{"X-Floor-Read":"1"},signal:AbortSignal.timeout(6000)})
      .then(async response=>{
        if(!response.ok)throw Error("The export expired. Read the preset again.");
        const url=URL.createObjectURL(await response.blob()), a=document.createElement("a");
        a.href=url;a.download=(file.name||"Floor preset").replace(/[^\w .-]/g,"_")+".hxpreset";a.click();
        setTimeout(()=>URL.revokeObjectURL(url),3000);
        info.textContent="Exported "+a.download;notice(info.textContent);
      }).catch(error=>{info.textContent=error.message;});
  };
}
