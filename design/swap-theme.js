(() => {
 const key = "floor-editor-appearance";
 const system = matchMedia("(prefers-color-scheme: dark)");
 const choices = ["system", "light", "dark"];
 let preference = "system";
 try {
  const stored = localStorage.getItem(key);
  if (choices.includes(stored)) preference = stored;
 } catch {}
 function apply() {
  document.documentElement.dataset.appearance =
   preference === "system" ? (system.matches ? "dark" : "light") : preference;
 }
 apply();
 system.addEventListener("change", apply);
 document.addEventListener("DOMContentLoaded", () => {
  const select = document.querySelector("#appearance");
  select.value = preference;
  select.addEventListener("change", () => {
   preference = select.value;
   apply();
   try { localStorage.setItem(key, preference); } catch {}
  });
 });
})();
