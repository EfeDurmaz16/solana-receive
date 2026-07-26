const fields = document.querySelectorAll("[data-field]");
const status = document.getElementById("status");
const steps = [...document.querySelectorAll(".path li")];

function short(value) {
  if (!value || value === "—") return "—";
  if (value.length <= 16) return value;
  return `${value.slice(0, 6)}…${value.slice(-4)}`;
}

function fill(data) {
  for (const el of fields) {
    const key = el.dataset.field;
    const raw = data[key];
    if (raw == null) continue;
    el.textContent = key.endsWith("Balance") ? String(raw) : short(String(raw));
    if (key === "programId" || key === "mint" || key === "destination" || key === "guardToken") {
      el.title = String(raw);
    }
  }

  for (const step of steps) step.classList.add("is-done");
  status.textContent = data.usedLocalProgramId
    ? "Loaded last Surfpool run (local deploy keypair; not the declared mainnet-facing ID)."
    : "Loaded last Surfpool run.";
  status.classList.add("is-ready");
}

async function boot() {
  try {
    const res = await fetch("./last-run.json", { cache: "no-store" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    fill(await res.json());
  } catch {
    status.textContent =
      "No last-run.json yet. Run ./scripts/surfpool-lifecycle.sh from the repo root, then refresh.";
    steps[0]?.classList.add("is-live");
  }
}

boot();
