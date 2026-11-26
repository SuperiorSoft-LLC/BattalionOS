let n = 0;
document.getElementById("b").addEventListener("click", () => { document.getElementById("n").textContent = String(++n); });
document.getElementById("where").textContent = "Served from " + location.origin;
