// Each row is a complete incoming USB bulk payload, with its PC capture time.
// Gesture labels come from the test log. The wire format does not identify who clicked.
const recordedBoostEvents = [
  [1793995811.278660, "Bypass in HX Edit", "21000018f003051000c400040902000000000400110000008269316a845200440579116a8262013bc2188317"],
  [1793995829.945602, "Enable on the Floor", "21000018f0030510007500040902000000000400110000008269316a845200440579116a8262013bc3188317"],
  [1793995872.084472, "Move in HX Edit", "21000018f0030510006e00040902000000000400110000008269316a8452004405790d6a824b014c02188317"]
];

function setupBoostReplay() {
  const panel = document.getElementById("boostReplay");
  const next = document.getElementById("nextRecordedEdit");
  const status = document.getElementById("replayStatus");
  let cells, index;
  function draw() {
    const row = {label: "1A", input: "Multi", output: "Multi", cells};
    document.getElementById("replayGraph").replaceChildren(signalPath([row]));
  }
  function restart() {
    cells = {1: {kind: "pedal", name: "Kinky Boost", bypassed: false}};
    index = 0;
    next.disabled = false;
    next.textContent = "Step through recording";
    status.textContent = "Starting state · Kinky Boost enabled in position 1.";
    draw();
  }
  next.onclick = () => {
    const [, label, hex] = recordedBoostEvents[index];
    const bytes = Uint8Array.from(hex.match(/../g), pair => parseInt(pair, 16));
    try {
      const event = decodeFloorBlockEvent(0x81, bytes);
      if (!event) throw new Error("Unrecognized recorded block change.");
      cells = applyFloorBlockEvent(cells, event);
      index++;
      draw();
      const [position, block] = Object.entries(cells)[0];
      status.textContent = label + " · Position " + position + " · " + (block.bypassed ? "Bypassed" : "Enabled");
      next.disabled = index === recordedBoostEvents.length;
      next.textContent = next.disabled ? "Recording finished" : "Next recorded edit";
    } catch (error) {
      status.textContent = error.message;
      next.disabled = true;
    }
  };
  document.getElementById("restartReplay").onclick = restart;
  restart();
  panel.hidden = false;
}
