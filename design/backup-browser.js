// Opens original HX Edit backups through the local file inspector. Browsing
// changes only this panel's selection; no live-session commands are issued.
(function () {
  function bind() {
    const root = document.getElementById("backupBrowser");
    if (!root) return;
    const make = (tag, text) => {
      const element = document.createElement(tag);
      if (text !== undefined) element.textContent = text;
      return element;
    };
    const field = (title, control) => {
      const label = make("label", title);
      control.setAttribute("aria-label", title);
      label.append(control);
      return label;
    };
    const file = make("input");
    file.type = "file";
    file.accept = ".hxb";
    const status = make("p");
    status.setAttribute("role", "status");
    const panel = make("section");
    panel.hidden = true;
    const setlist = make("select"), search = make("input"), preset = make("select"), snapshot = make("select");
    search.type = "search";
    const details = make("ol");
    const message = make("p");
    panel.append(field("Setlist", setlist), field("Find a preset in this setlist", search),
      field("Preset", preset), field("Snapshot", snapshot), message, details);
    root.append(field("Browse HX Edit backup (.hxb)", file), status, panel);

    let lists = [], ticket = 0, controller;
    const selectedList = () => lists[setlist.selectedIndex];
    const selectedPreset = () => selectedList()?.presets.find(p => p.index === Number(preset.value));

    function renderBlocks() {
      const p = selectedPreset();
      details.replaceChildren();
      if (preset.selectedIndex < 0 || !p) { message.textContent = ""; return; }
      const index = snapshot.value === "" ? null : Number(snapshot.value);
      const s = p.snapshots.find(s => s.index === index);
      const notes = [];
      if (s?.valid === false) notes.push("Snapshot marked unused");
      if (s?.valid === true && Number.isFinite(s.tempo)) notes.push(s.tempo + " BPM");
      if (!p.blocks.length) notes.push("No effect blocks in this preset");
      message.textContent = notes.join(" · ");
      for (const block of p.blocks) {
        const enabled = index === null ? block.enabled : block.states[index];
        const state = enabled === true ? "On" : enabled === false ? "Bypassed" : "State unavailable";
        const lane = ["A", "B"][block.path] ?? "?";
        const slot = Number.isInteger(block.position) ? block.position + 1 : "?";
        const row = make("li", (block.dsp + 1) + lane + " · " + slot + " · " + block.name + " · " + state);
        if (block.symbol) row.title = block.symbol;
        details.append(row);
      }
    }

    function renderSnapshots() {
      const p = preset.selectedIndex < 0 ? null : selectedPreset();
      snapshot.replaceChildren();
      if (p) {
        snapshot.add(new Option("Preset block states", ""));
        for (const s of p.snapshots) {
          let label = (s.index + 1) + " · " + (s.name || "Snapshot " + (s.index + 1));
          if (s.index === p.currentSnapshot) label += " (active in backup)";
          snapshot.add(new Option(label, String(s.index)));
        }
        if (p.snapshots.some(s => s.index === p.currentSnapshot)) snapshot.value = String(p.currentSnapshot);
      }
      snapshot.disabled = !p?.snapshots.length;
      renderBlocks();
    }

    function renderPresets() {
      const query = search.value.trim().toLocaleLowerCase();
      preset.replaceChildren();
      for (const p of selectedList()?.presets || []) {
        const title = p.label + " · " + (p.name || (p.blocks.length ? "Unnamed preset" : "Empty"));
        if (title.toLocaleLowerCase().includes(query)) preset.add(new Option(title, String(p.index)));
      }
      preset.disabled = !preset.options.length;
      renderSnapshots();
      if (preset.disabled) message.textContent = "No matching presets";
    }
    search.oninput = renderPresets;
    setlist.onchange = () => { search.value = ""; renderPresets(); };
    preset.onchange = renderSnapshots;
    snapshot.onchange = renderBlocks;
    file.onchange = async () => {
      const choice = file.files[0];
      file.value = "";
      if (!choice) return;
      const current = ++ticket;
      controller?.abort();
      controller = new AbortController();
      const active = controller;
      panel.hidden = true;
      lists = [];
      const limit = 16 * 1024 * 1024;
      if (!choice.size || choice.size > limit) { status.textContent = "Choose a backup up to 16 MiB."; return; }
      status.textContent = "Opening " + choice.name + "…";
      const timer = setTimeout(() => active.abort(), 25000);
      try {
        const response = await fetch("/live/inspect-backup", {method: "POST", body: choice,
          headers: {"Content-Type": "application/octet-stream"}, signal: active.signal});
        const result = await response.json();
        if (current !== ticket) return;
        if (!response.ok || !result.setlists?.length) throw Error(result.error || "Could not read this backup.");
        lists = result.setlists;
        setlist.replaceChildren(...lists.map(s => new Option((s.index + 1) + " · " + s.name, String(s.index))));
        search.value = "";
        renderPresets();
        status.textContent = choice.name + " · " + lists.reduce((total, s) => total + s.presets.length, 0) + " preset slots";
        panel.hidden = false;
      } catch (error) {
        if (current === ticket) status.textContent = error.name === "AbortError" ? "Reading the backup timed out." : error.message;
      } finally { clearTimeout(timer); }
    };
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", bind, {once:true});
  else bind();
})();
