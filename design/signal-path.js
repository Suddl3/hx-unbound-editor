// Hardware positions 1..8 are the processing cells on path 1A; path 2A starts at 21.
function signalPath(rows, choose, inspectEmpty) {
 const graph=document.createElement("div");
 graph.className="signal-graph";
 for (const row of rows) {
  const lane=document.createElement("div");
  lane.className="signal-lane";
  lane.setAttribute("aria-label","Path "+row.label);
  for (let i=0;i<10;i++) {
   const item=row.cells[i], end=i===0 || i===9;
   const canInspect=inspectEmpty && (!row.emptyPositions || row.emptyPositions.includes(i));
   const cell=document.createElement(!end && (item || canInspect) ? "button":"span");
   cell.className="flow-cell "+(end?"terminal":item?"occupied tone-"+item.kind:"empty");
   if(item?.bypassed)cell.classList.add("bypassed");
   const icon=document.createElement("b"), caption=document.createElement("small");
   icon.textContent=end?(i===0?"↦":"→"):item?({amp:"▰",delay:"◷",pedal:"◈"}[item.kind]||"◇"):"＋";
   if(item?.color)cell.style.setProperty("--cat",item.color);
   if(item?.icon && !end){
    const img=document.createElement("img");img.alt="";img.src=item.icon;icon.replaceChildren(img);
   }
   caption.textContent=end?(i===0?row.input:row.output):item?item.name:String(i);
   cell.title="Path "+row.label+" · "+(end?caption.textContent:"Position "+i+" · "+(item?item.name:"Empty"));
   if(item?.bypassed){cell.title+=" · Bypassed";caption.textContent+=" · Off";}
   cell.setAttribute("aria-label",cell.title);
   cell.append(icon,caption);
   if (item && !end) {
    cell.disabled=!choose || item.index==null;
    cell.setAttribute("aria-pressed",String(Boolean(item.selected)));
    if(!cell.disabled) cell.onclick=()=>choose(item.index);
   } else if(!end && canInspect) {
    if(row.selectedEmpty===i)cell.setAttribute("aria-pressed","true");
    cell.onclick=()=>inspectEmpty(row.label,i);
   }
   lane.append(cell);
  }
  const label=document.createElement("strong");label.textContent=row.label;lane.prepend(label);
  graph.append(lane);
 }
 return graph;
}

// Scale around the point under the pointer; retain keyboard zoom and horizontal panning.
function straightFloorRows(data) {
 const rows=[], seen=new Set();
 for(const [i,lane] of data.lanes.entries()){
  const parts=lane.split(/\s+->\s+/),base=i*20;
  const input=/^(\d+) Input \[(\d+)\]$/.exec(parts.shift());
  const output=/^(\d+) Output \[(\d+)\]$/.exec(parts.pop());
  if(!input || !output || i>1 || Number(input[1])!==base || Number(output[1])!==base+9)return null;
  const cells={};
  for(const part of parts){
   const m=/^(\d+) (\d+)( \(off\))?$/.exec(part);if(!m)return null;
   const pos=Number(m[1]),column=pos-base,index=data.blocks.findIndex(b=>b.position===pos+1);
   if(column<1||column>8||index<0||seen.has(index))return null;
   const block=data.blocks[index],nam=block.model==="model 380";
   if(block.model!=="model "+m[2] || Boolean(m[3])!==!block.enabled)return null;
   const boost=block.model==="model 388";
   cells[column]={name:nam?(data.slots[block.params.param0]?.name||"NAM "+block.params.param0):boost?"Kinky Boost":block.model,
    kind:nam?"amp":boost?"pedal":"stock",index,bypassed:!block.enabled};
   seen.add(index);
  }
  rows.push({label:(i+1)+"A",input:input[2]==="0"?"Off":"Input",output:"Out",cells});
 }
 return rows.length===2 && seen.size===data.blocks.length ? rows : null;
}

function bindPathZoom(viewport,content,reset) {
 let scale=1;
 function resize(value,x) {
  const offset=(viewport.scrollLeft+x)/scale;
  scale=Math.max(.65,Math.min(2.6,value));
  content.style.zoom=String(scale);
  viewport.scrollLeft=offset*scale-x;
  reset.textContent=Math.round(scale*100)+"%";
 }
 viewport.addEventListener("wheel",e=>{
  if(e.shiftKey || Math.abs(e.deltaX)>Math.abs(e.deltaY))return;
  e.preventDefault();
  const dy=e.deltaY*(e.deltaMode===1?16:e.deltaMode===2?viewport.clientHeight:1);
  resize(scale*Math.exp(-dy*.0016),e.clientX-viewport.getBoundingClientRect().left);
 },{passive:false});
 viewport.onkeydown=e=>{
  if(!["+","=","-","0"].includes(e.key))return;
  e.preventDefault();resize(e.key==="0"?1:scale*(e.key==="-"?.9:1.1),viewport.clientWidth/2);
 };
 reset.onclick=()=>resize(1,viewport.clientWidth/2);
}
