const fields = document.querySelectorAll("[data-field]");
const status = document.getElementById("status");
const steps = [...document.querySelectorAll(".path li")];

/** Successful evidence only. Anything else must not paint a full green lifecycle. */
function isSuccessfulEvidence(data) {
  if (!data || typeof data !== "object") return false;
  if (data.ok !== true) return false;
  if (typeof data.finishedAt !== "string" || data.finishedAt.length < 10) return false;
  if (typeof data.programId !== "string" || !data.programId) return false;
  if (!data.steps || typeof data.steps !== "object") return false;
  const required = ["credited", "held", "claim", "expiry"];
  for (const key of required) {
    const step = data.steps[key];
    if (!step || step.ok !== true) return false;
    if (typeof step.signature !== "string" || step.signature.length < 32) return false;
  }
  return true;
}

function short(value) {
  if (!value || value === "—") return "—";
  if (value.length <= 16) return value;
  return `${value.slice(0, 6)}…${value.slice(-4)}`;
}

function clearSteps() {
  for (const step of steps) {
    step.classList.remove("is-done", "is-live", "is-bad");
  }
}

function fillSuccess(data) {
  clearSteps();
  for (const el of fields) {
    const key = el.dataset.field;
    const raw = data[key];
    if (raw == null) continue;
    el.textContent = key.endsWith("Balance") ? String(raw) : short(String(raw));
    if (key === "programId" || key === "mint" || key === "destination" || key === "guardToken") {
      el.title = String(raw);
    }
  }

  const finished = document.querySelector("[data-field='finishedAt']");
  if (finished) {
    finished.textContent = data.finishedAt;
    finished.title = data.finishedAt;
  }

  const explorers = data.explorers || {};
  for (const a of document.querySelectorAll("[data-link]")) {
    const key = a.dataset.link;
    const href = explorers[key] || data.before?.explorer;
    if (href && (key !== "before" || data.before?.ok)) {
      a.href = href;
      a.hidden = false;
    } else {
      a.removeAttribute("href");
      a.hidden = true;
    }
  }

  const beforeStep = document.querySelector('[data-step="before"]');
  if (beforeStep) {
    beforeStep.hidden = !(data.before && data.before.ok);
  }

  const headline = document.getElementById("headline");
  const lede = document.getElementById("lede");
  const isDevnet = data.cluster === "devnet";
  if (headline) {
    headline.textContent = isDevnet
      ? "Devnet before/after receive"
      : "Held delivery on a custom program";
  }
  if (lede) {
    lede.textContent = isDevnet
      ? "Real Devnet txs: ordinary SPL always-credits vs receive-policy credited → held → claim/expiry. Not canonical Token-2022. Not legacy USDC."
      : "Local Surfpool demo of credited, held, claim, and expiry. Not canonical Token-2022. Not legacy USDC.";
  }

  for (const step of steps) {
    if (step.hidden) continue;
    step.classList.add("is-done");
  }

  const localNote =
    data.usedLocalProgramId === true
      ? " Local deploy keypair (not the declared program ID)."
      : data.usedLocalProgramId === false
        ? " Declared program ID."
        : "";
  const where = isDevnet ? "Devnet" : "Surfpool";
  status.textContent = `Successful ${where} evidence from ${data.finishedAt}.${localNote}`;
  status.classList.add("is-ready");
  status.classList.remove("is-bad");

  const run = document.getElementById("run");
  if (run && isDevnet) {
    run.innerHTML =
      "From the repo root: <code>./scripts/devnet-lifecycle.sh</code> then serve the UI: <code>python3 -m http.server 8765 --directory demos/receive</code>";
  }
}

function fillInvalid(reason) {
  clearSteps();
  steps[0]?.classList.add("is-bad");
  status.textContent = reason;
  status.classList.remove("is-ready");
  status.classList.add("is-bad");
}

async function boot() {
  try {
    const res = await fetch("./last-run.json", { cache: "no-store" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    if (!isSuccessfulEvidence(data)) {
      fillInvalid(
        "last-run.json is present but is not successful evidence (missing ok/finishedAt/step signatures). Re-run ./scripts/surfpool-lifecycle.sh.",
      );
      return;
    }
    fillSuccess(data);
  } catch (err) {
    clearSteps();
    const fileProtocol = location.protocol === "file:";
    status.textContent = fileProtocol
      ? "Serve over http (module + fetch): python3 -m http.server 8765 --directory demos/receive"
      : "No last-run.json yet. Run ./scripts/surfpool-lifecycle.sh from the repo root, then refresh.";
    steps[0]?.classList.add("is-live");
    status.classList.remove("is-ready", "is-bad");
    void err;
  }
}

boot();
