// Local HX exports carry named Command Center fields. They are a separate
// data source from the running Floor's numeric native preset document.
(() => {
  const MiB=1024*1024, text=new TextDecoder("utf-8",{fatal:true});
  const object=v=>v!==null && typeof v==="object" && !Array.isArray(v);
  const commandKeys=obj=>Object.keys(obj||{}).filter(k=>/^command[A-Z]/.test(k));
  const natural=new Intl.Collator(undefined,{numeric:true,sensitivity:"base"});
  const labels={"@channel":"MIDI channel (stored)","@cc":"CC","@value":"Value","@bank0":"Bank MSB","@bank32":"Bank LSB",
    "@program":"Program (stored)","@waittime":"Wait time (stored)","@press":"Press (stored)","@relhold":"Release / hold (stored)",
    "@note":"Note number","@velocity":"Velocity","@toggle":"Toggle","@initial":"Initial","@current":"Current",
    "@keystroke":"Key (stored)","@modifier1":"Modifier 1","@modifier2":"Modifier 2","@modifier3":"Modifier 3",
    "@message":"Message (stored)","@select":"Selection (stored)","@behavior":"Behavior (stored)","@noteoff":"Note off (stored)"};
  function describe(tone,snapshot){
    const patch=snapshot==null?{}:tone["snapshot"+snapshot]?.commands||{};
    return [...new Set([...commandKeys(tone),...commandKeys(patch)])].sort(natural.compare).map(key=>{
      const base=tone[key],over=patch[key];
      if((base!==undefined&&!object(base))||(over!==undefined&&!object(over)))throw Error("Unsupported command record in this export.");
      const values={...base,...over};
      let type=values["@fs_label"];
      if(typeof type!=="string"||!type.trim()){
        if(values["@command"]===19&&["@program","@bank0","@bank32"].every(k=>k in values))type="Bank / Program";
        else if(values["@command"]===18&&"@cc" in values&&"@value" in values)type="MIDI CC";
        else type=values["@command"]===undefined?"Unidentified command":"Command "+values["@command"];
      }
      const source=key.replace(/^commandInst(\d+)$/,"Instant $1").replace(/^commandFS(\d+)$/,"FS $1");
      return {source,type,custom:values["@fs_customlabel"]||"",values,over:over||{}};
    });
  }
  function collect(setlists){
    const presets=[];
    for(const {data,group} of setlists){
      if(!object(data))throw Error("Missing preset data.");
      const isList=Array.isArray(data.presets), list=isList?data.presets:[data];
      list.forEach((p,index)=>{
        if(!object(p))throw Error("Invalid preset entry.");
        if(!p.tone && !p.meta && isList)return;
        if(!object(p.tone))throw Error("Missing tone in this export.");
        const snaps=Object.keys(p.tone).filter(k=>/^snapshot\d+$/.test(k)&&object(p.tone[k]))
          .map(k=>Number(k.slice(8))).sort((a,b)=>a-b);
        const keys=new Set([...commandKeys(p.tone),...snaps.flatMap(n=>commandKeys(p.tone["snapshot"+n].commands))]);
        if(isList && keys.size===0)return;
        const slot=p.device===2162689?String(1+Math.floor(index/4)).padStart(2,"0")+"ABCD"[index%4]:"#"+(index+1);
        const name=[group,isList?slot:"",p.meta?.name||"Unnamed preset"].filter(Boolean).join(" · ");
        presets.push({name,tone:p.tone,snaps});
      });
    }
    return presets;
  }
  async function unpackBackup(buf){
    const v=new DataView(buf);
    if(buf.byteLength<24 || text.decode(new Uint8Array(buf,0,4))!=="AF6L" || v.getUint32(4,true)!==1)
      throw Error("Unsupported Helix backup format.");
    const offset=Number(v.getBigUint64(8,true)),count=Number(v.getBigUint64(16,true));
    if(!Number.isSafeInteger(offset)||offset<24||count>2048||offset+count*36!==buf.byteLength)throw Error("Invalid backup index.");
    const sets=[],seen=new Set();let budget=64*MiB;
    for(let i=0;i<count;i++){
      const at=offset+36*i,tag=text.decode(new Uint8Array(buf,at,4));
      const normalized=/^\d\dLS$/.test(tag)?[...tag].reverse().join(""):tag;
      if(!/^SL\d\d$/.test(normalized))continue;
      if(seen.has(normalized))throw Error("Duplicate setlist in backup.");
      seen.add(normalized);
      const start=Number(v.getBigUint64(at+4,true)),size=Number(v.getBigUint64(at+12,true));
      const flag=v.getUint32(at+20,true),rawLen=Number(v.getBigUint64(at+24,true));
      if(start<24||!size||start+size>offset||flag>1||rawLen>budget||rawLen>16*MiB||rawLen<1)
        throw Error("Unsupported backup setlist size.");
      budget-=rawLen;
      let bytes=new Uint8Array(buf,start,size);
      if(flag){
        const reader=new Blob([bytes]).stream().pipeThrough(new DecompressionStream("deflate")).getReader();
        const output=new Uint8Array(rawLen);let end=0;
        try{
          while(true){const {done,value}=await reader.read();if(done)break;
            if(end+value.length>rawLen)throw Error("Setlist exceeds its declared size.");
            output.set(value,end);end+=value.length;}
        }finally{await reader.cancel().catch(()=>{});}
        if(end!==rawLen)throw Error("Incomplete setlist in backup.");
        bytes=output;
      }else if(size!==rawLen)throw Error("Invalid setlist length.");
      const data=JSON.parse(text.decode(bytes)).data;
      if(!Array.isArray(data?.presets))throw Error("Missing backup setlist.");
      sets.push({data,group:data.meta?.name||normalized});
    }
    if(!sets.length)throw Error("No setlists in this backup.");
    return sets;
  }
  async function readFile(file){
    if(!file || file.size>64*MiB)throw Error("Choose a Helix file up to 64 MiB.");
    const ext=file.name.split(".").pop().toLowerCase(),buf=await file.arrayBuffer();
    if(ext==="hxb")return collect(await unpackBackup(buf));
    if(!["hlx","hls"].includes(ext))throw Error("Choose an .hlx, .hls, or .hxb file.");
    const data=JSON.parse(text.decode(buf)).data;
    if(ext==="hls"&&!Array.isArray(data?.presets))throw Error("Missing setlist.");
    return collect([{data,group:ext==="hls"?data.meta?.name:""}]);
  }
  if(typeof module!=="undefined"&&module.exports)module.exports={readFile,describe};
  if(typeof document==="undefined")return;
  const opener=document.getElementById("commandCenter");
  if(!opener)return;
  const make=(tag,txt="")=>Object.assign(document.createElement(tag),{textContent:txt});
  opener.onclick=()=>{
    const d=make("dialog");d.className="tag-dialog command-inspector";d.style.width="min(620px,94vw)";
    const title=make("h2","Command Center · File inspector");title.id="ccHeading";d.setAttribute("aria-labelledby",title.id);
    const note=make("p","Inspect a preset, setlist or backup from this computer. Live Command Center readback is pending.");
    const picker=make("input");picker.type="file";picker.accept=".hlx,.hls,.hxb";picker.setAttribute("aria-label","Helix export");
    const preset=make("select"),snap=make("select"),out=make("div"),status=make("p"),close=make("button","Close");
    preset.setAttribute("aria-label","Exported preset");snap.setAttribute("aria-label","Command snapshot");status.setAttribute("role","status");
    d.append(title,note,picker,status,preset,snap,out,close);preset.hidden=snap.hidden=true;
    close.onclick=()=>d.close();d.onclose=()=>d.remove();document.body.append(d);d.showModal();
    let all=[],serial=0;
    function render(){
      out.replaceChildren();const p=all[Number(preset.value)];if(!p)return;
      let rows;try{rows=describe(p.tone,snap.value===""?null:Number(snap.value));}catch(e){out.append(make("p",e.message));return;}
      if(!rows.length){out.append(make("p","No command records in this export."));return;}
      for(const row of rows){
        const item=make("details"),top=make("summary",[row.source,row.type,row.custom].filter(Boolean).join(" · "));
        item.append(top);
        for(const [key,value] of Object.entries(row.values)){
          const val=typeof value==="object"?JSON.stringify(value):String(value);
          const line=make("p",(labels[key]||key)+" · "+val+(Object.hasOwn(row.over,key)?" · snapshot":""));
          item.append(line);
        }
        out.append(item);
      }
    }
    function choosePreset(){
      snap.replaceChildren(new Option("Preset defaults",""));const p=all[Number(preset.value)];if(!p)return;
      for(const n of p.snaps){const name=p.tone["snapshot"+n]["@name"];snap.add(new Option("Snapshot "+(n+1)+(name?" · "+name:""),n));}
      snap.hidden=!p.snaps.length;render();
    }
    preset.onchange=choosePreset;snap.onchange=render;
    picker.onchange=async()=>{
      const seq=++serial,file=picker.files[0];if(!file)return;
      status.textContent="Reading "+file.name+"…";all=[];out.replaceChildren();preset.hidden=snap.hidden=true;
      try{
        const result=await readFile(file);if(seq!==serial||!d.open)return;
        all=result;status.textContent=file.name+(all.length?"":" · No Command Center assignments found.");
        preset.replaceChildren(...all.map((p,i)=>new Option(p.name,i)));preset.hidden=!all.length;
        choosePreset();
      }catch(e){if(seq===serial)status.textContent=e.message;}
    };
  };
})();
