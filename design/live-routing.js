// Read endpoint names, select values and junction positions from the same
// preset revision as the signal path. Opening a route only inspects it.
function createFloorRouting(send) {
  const make = (tag, text = "") => Object.assign(document.createElement(tag), {textContent:text});
  let state = {}, busy = false, picked, dialog, dialogKey, returnFocus;
  function identity() { return [state.session,state.board?.bank,state.board?.label].join("/"); }
  function title(dsp, end) { return "Path " + (dsp + 1) + "A " + end; }
  function routeName(route) {
    return route?.name ?? (Number.isInteger(route?.value) ? "Route " + route.value : "Unavailable");
  }
  function close() {
    if (dialog?.open) dialog.close();
    dialog?.remove();
    picked = dialog = dialogKey = null;
    if (returnFocus?.isConnected) returnFocus.focus();
    returnFocus = null;
  }
  function location(node) {
    if (node?.column === 0) return "at input";
    if (node?.column === 9) return "at output";
    if (node?.column > 0 && node.column < 9) return "before position " + node.column;
    return Number.isInteger(node?.attach) ? "at native slot " + node.attach : "position unavailable";
  }
  function render() {
    if (!picked) return;
    if (identity() !== picked.identity) {close();return;}
    const path = state.board?.paths[picked.dsp];
    const route = path?.[picked.end];
    if (!route || route.slot !== picked.slot) {close();return;}
    const readonly = !Array.isArray(route.choices) || !route.writable || picked.dsp !== 0;
    const locked = !state.online || Boolean(state.pending || busy);
    const key = JSON.stringify([route,path.split,path.fork,path.merge,locked]);
    if (key === dialogKey) return;
    dialogKey = key;
    const heading = make("h2",title(picked.dsp,picked.end));
    heading.id = "floor-route-heading";
    dialog.replaceChildren(heading);
    if (readonly) {
      dialog.append(make("p",routeName(route) + " · Read only"));
    } else {
      const field = make("label",picked.end === "input" ? "Input from " : "Output to ");
      const select = make("select");
      for (const [value,name] of route.choices.entries()) select.add(new Option(name,String(value)));
      select.value = String(route.value);
      select.disabled = locked;
      select.onchange = () => {
        const value = Number(select.value);
        select.value = String(route.value);
        if (!dialog?.contains(select) || !picked || !state.online || state.pending || busy || value === route.value) return;
        send({op:"route-endpoint", endpoint:picked.end, dsp:picked.dsp, slot:route.slot,
              expected:route.value, value});
      };
      field.append(select);dialog.append(field);
    }
    if (path.split) {
      for (const [key,fallback] of [["fork","Split"],["merge","Merge"]]) {
        const node = path[key];
        if (node) dialog.append(make("p",(node.name || fallback) + " · " + location(node)));
      }
    }
    const done = make("button","Close");
    done.type = "button";done.onclick = close;dialog.append(done);
  }
  function open(dsp,end,button,route) {
    close();
    picked = {dsp, end, slot:route.slot, identity:identity()};
    returnFocus = button;
    dialog = make("dialog");
    dialog.setAttribute("aria-labelledby","floor-route-heading");
    dialog.oncancel = event => {event.preventDefault();close();};
    document.body.append(dialog);
    render();
    dialog?.showModal();
  }
  function bindPath(graph,board) {
    for (const [dsp,path] of board.paths.entries()) {
      const lane = graph.querySelector('[aria-label="Path ' + (dsp+1) + 'A"]');
      if (!lane) continue;
      const cells = lane.querySelectorAll(".flow-cell");
      for (const [end,idx] of [["input",0],["output",9]]) {
        const route = path[end], cell = cells[idx];
        if (!route || !cell) continue;
        const caption = routeName(route), button = make("button");
        button.type = "button";button.className = cell.className;
        button.title = title(dsp,end) + " · " + caption;
        button.setAttribute("aria-label",button.title);
        button.setAttribute("aria-haspopup","dialog");
        const icon = make("b",end === "input" ? "↦" : "→");
        button.append(icon,make("small",caption.split(" (")[0]));
        button.onclick = () => open(dsp,end,button,route);
        cell.replaceWith(button);
      }
      if (!path.split) continue;
      for (const [key,glyph,label] of [["fork","⑂","Split"],["merge","⑃","Merge"]]) {
        const node = path[key], column = node?.column;
        const cell = Number.isInteger(column) ? lane.querySelectorAll(".flow-cell")[column] : null;
        if (!cell) continue;
        const marker = make("sup",glyph);
        marker.setAttribute("aria-hidden","true");
        marker.style.cssText = "position:absolute;top:0;left:0;line-height:1";
        marker.title = (node.name || label) + " · " + location(node);
        cell.style.position = "relative";cell.append(marker);
        cell.setAttribute("aria-label",cell.getAttribute("aria-label") + " · " + marker.title);
      }
    }
  }
  return {bindPath,update(next,sending) {state=next;busy=sending;render();}};
}
