// Observed incoming Floor 3.80 notifications for one stock block on straight path 1A.
// Callers supply the current preset and block identity from an independent baseline.
function decodeFloorBlockEvent(endpoint, frame) {
  if (endpoint !== 0x81 || frame.length !== 44) return null;
  const matches = (offset, bytes) => bytes.every((byte, i) => frame[offset + i] === byte);
  if (!matches(0, [0x21, 0, 0, 0x18, 0xf0, 3, 5, 0x10]) ||
      !matches(10, [0, 4, 9, 2, 0, 0, 0, 0, 4, 0, 17, 0, 0, 0])) return null;

  // The outer event map and nested route below are common to both recorded gestures.
  if (!matches(24, [0x82, 0x69, 0x31, 0x6a, 0x84, 0x52, 0, 0x44, 5, 0x79]) ||
      !matches(35, [0x6a, 0x82])) return null;
  const validPosition = value => value >= 1 && value <= 8;
  if (frame[34] === 0x11 && frame[37] === 0x62 && validPosition(frame[38]) &&
      frame[39] === 0x3b && [0xc2, 0xc3].includes(frame[40])) {
    return {kind: "enabled", position: frame[38], enabled: frame[40] === 0xc3};
  }
  if (frame[34] === 0x0d && frame[37] === 0x4b && frame[39] === 0x4c &&
      validPosition(frame[38]) && validPosition(frame[40]) && frame[38] !== frame[40]) {
    return {kind: "move", position: frame[38], destination: frame[40]};
  }
  return null;
}

function applyFloorBlockEvent(cells, event) {
  const block = cells[event.position];
  if (!block) throw new Error("The recorded position has no known block.");
  if (event.kind === "enabled") {
    return {...cells, [event.position]: {...block, bypassed: !event.enabled}};
  }
  if (event.kind !== "move" || cells[event.destination]) {
    throw new Error("The change conflicts with the known signal path.");
  }
  const next = {...cells, [event.destination]: block};
  delete next[event.position];
  return next;
}
