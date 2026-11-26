// The page contract: parent.postMessage({bos: 1, id, cmd, args}); the answer comes back with the same id.
const out = document.getElementById("out");
let n = 0;
addEventListener("message", (e) => {
  const d = e.data;
  if (!d || d.bos !== 1 || !d.id) return;
  out.textContent = `${d.id}: ${d.ok ? "OK " + JSON.stringify(d.result) : "REFUSED " + d.error}\n` + out.textContent;
});
for (const b of document.querySelectorAll("button[data-cmd]")) {
  b.onclick = () => {
    const id = `r${++n}`;
    out.textContent = `${id}: ${b.dataset.cmd} ...\n` + out.textContent;
    parent.postMessage({ bos: 1, id, cmd: b.dataset.cmd, args: b.dataset.args ? JSON.parse(b.dataset.args) : undefined }, "*");
  };
}
