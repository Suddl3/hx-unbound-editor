// Inline equipment drawings for the offline interaction prototypes.
function gearDiagram(kind, variant = 0) {
  const common = 'viewBox="0 0 240 120" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"';
  if (kind === "delay" || kind === "pedal") {
    const knobs = [81, 119, 157].map(x => '<circle cx="'+x+'" cy="29" r="8"/><path d="M'+x+' 23v5"/>').join("");
    return '<svg '+common+'><path d="M64 13h112l7 91-6 6H63l-6-6z"/><path d="M64 51h112M68 14h103"/>'+knobs+'<rect x="72" y="61" width="96" height="35" rx="4"/><path d="M81 70h78m-78 6h78m-78 6h78m-78 6h78"/><circle cx="120" cy="45" r="2"/></svg>';
  }
  const knobs = Array.from({length: variant === 0 ? 6 : 8}, (_,i) => '<circle cx="'+(77+i*13)+'" cy="46" r="3.2"/>').join("");
  const grille = Array.from({length: 37}, (_,i)=>'<path d="M'+(30+i*5)+' 67l6 24"/>').join("");
  return '<svg '+common+'><path d="M87 25v-5q0-5 7-5h50q7 0 7 5v5m-57-1v-4h50v4"/><rect x="18" y="25" width="204" height="76" rx="7"/><rect x="24" y="31" width="192" height="64" rx="4"/><path d="M25 59h190M34 101v5h19v-5m133 0v5h18v-5"/><circle cx="38" cy="45" r="4"/><circle cx="52" cy="45" r="2"/>'+knobs+'<path opacity=".55" d="M198 43h9m-9 6h9"/><g opacity=".5" stroke-width="1">'+grille+'</g></svg>';
}
