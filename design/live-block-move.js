// Drag and keyboard-accessible move actions share the same guarded command.
function createBlockMoves(read, submit) {
  let gesture = null;

  function source(slot) {
    const state = read();
    const block = state.board?.blocks.find(b => b.slot === slot);
    if (!state.online || state.pending || !block?.canMove) return null;
    return {session:state.session, rev:state.rev, slot, model:block.model};
  }

  function move(origin, to) {
    const state = read(), current = source(origin?.slot);
    if (!origin || !current || origin.session !== current.session || origin.rev !== current.rev ||
        origin.model !== current.model || !state.board.insertSlots.includes(to)) return;
    submit({op:"move-block", slot:origin.slot, model:origin.model, to});
  }

  function bindPath(graph, board) {
    const cells = graph.querySelector('[aria-label="Path 1A"]')?.querySelectorAll(".flow-cell");
    if (!cells) return;
    for (const b of board.blocks.filter(b => b.canMove)) {
      const cell = cells[b.slot];
      if (!cell) continue;
      cell.draggable = true;
      cell.ondragstart = e => {
        gesture = source(b.slot);
        if (!gesture) {e.preventDefault(); return;}
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain",b.name);
      };
      cell.ondragend = () => {gesture=null;};
    }
    for (const slot of board.insertSlots) {
      const cell = cells[slot];
      if (!cell) continue;
      cell.ondragover = e => {
        if (!gesture) return;
        e.preventDefault();e.dataTransfer.dropEffect="move";
      };
      cell.ondrop = e => {
        if (!gesture) return;
        e.preventDefault();const from=gesture;gesture=null;move(from,slot);
      };
    }
  }

  function controls(container, board, block) {
    if (!block.canMove || !board.insertSlots.length) return;
    const group=document.createElement("div");
    group.className="block-actions";
    const dest=document.createElement("select");
    dest.setAttribute("aria-label","Move block destination");
    for (const slot of board.insertSlots) dest.add(new Option("1A · Position "+slot,String(slot)));
    const button=document.createElement("button");
    button.type="button";button.textContent="Move block";
    const session=read().session, generation=read().generation;
    button.onclick=()=>{
      const state=read();
      if(session===state.session && generation===state.generation) move(source(block.slot),Number(dest.value));
    };
    group.append(dest,button);container.append(group);
    if(block.canCopy){
      const copy=document.createElement("button");
      copy.type="button";copy.textContent="Copy block";
      copy.title="Copy this stock block’s settings and snapshot bypass states to the selected position";
      copy.onclick=()=>{
        const state=read(), current=source(block.slot), to=Number(dest.value);
        if(!current || session!==state.session || generation!==state.generation ||
            !state.board.insertSlots.includes(to))return;
        submit({op:"copy-block",slot:block.slot,model:block.model,to});
      };
      group.append(copy);
      dest.setAttribute("aria-label","Move or copy destination");
    }
  }

  return {bindPath,controls};
}
