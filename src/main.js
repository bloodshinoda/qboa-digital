// Qboa Digital — frontend
//
// Carrega src/data/qboa-structure.json, monta as abas (Limpeza / Desengordurar / Diagnóstico)
// e os cards de cada uma. Ao clicar num card, chama o comando Rust
// `run_task(task_id)` via invoke() e mostra o resultado no console.

const { invoke } = window.__TAURI__.core;

let structure = null;
let activeSection = "limpeza";
let activeLevel = "normal";

async function loadStructure() {
  const res = await fetch("./data/qboa-structure.json");
  structure = await res.json();
}

function renderSectionSummary() {
  const summaryEl = document.getElementById("section-summary");
  const section = structure.sections.find((s) => s.id === activeSection);
  if (!section) return;

  const taskCount = section.items.length;
  summaryEl.textContent = `${section.title}: ${taskCount} tarefa${taskCount === 1 ? "" : "s"} disponíveis`;
}

function renderTabs() {
  const tabsEl = document.getElementById("tabs");
  tabsEl.innerHTML = "";
  structure.sections.forEach((section) => {
    const btn = document.createElement("button");
    btn.className = "tab" + (section.id === activeSection ? " active" : "");
    btn.textContent = `${section.icon} ${section.title}`;
    btn.addEventListener("click", () => {
      activeSection = section.id;
      renderTabs();
      renderSectionSummary();
      renderCards();
      const out = document.getElementById("console-output");
      out.textContent = `Selecione uma tarefa de ${section.title.toLowerCase()} para executar.`;
    });
    tabsEl.appendChild(btn);
  });
}

function renderCards() {
  const cardsEl = document.getElementById("cards");
  cardsEl.innerHTML = "";
  const section = structure.sections.find((s) => s.id === activeSection);
  if (!section) return;

  section.items.forEach((item) => {
    const card = document.createElement("div");
    card.className = "card";
    card.innerHTML = `<h3>${item.name}</h3><p>${item.desc}</p>`;
    card.addEventListener("click", () => runTask(item, card));
    cardsEl.appendChild(card);
  });
}

async function runTask(item, cardEl) {
  const out = document.getElementById("console-output");
  cardEl.classList.add("running");
  out.textContent = `> Rodando "${item.name}" (nível: ${activeLevel})...\n`;

  try {
    const result = await invoke("run_task", { taskId: item.id });
    out.textContent += result.output || "(sem saída)";
    out.textContent += result.ok ? "\n\n[OK]" : "\n\n[FALHOU]";
  } catch (err) {
    out.textContent += `\nErro ao chamar o backend: ${err}`;
  } finally {
    cardEl.classList.remove("running");
  }
}

function bindLevels() {
  document.querySelectorAll(".level").forEach((btn) => {
    if (btn.dataset.level === activeLevel) btn.classList.add("active");
    btn.addEventListener("click", () => {
      activeLevel = btn.dataset.level;
      document.querySelectorAll(".level").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      // TODO: níveis (express/normal/turbo) ainda não filtram/ordenam os
      // itens — decidir se cada nível dispara um preset de tasks em lote.
    });
  });
}

async function init() {
  await loadStructure();
  renderTabs();
  renderSectionSummary();
  renderCards();
  bindLevels();
  const out = document.getElementById("console-output");
  out.textContent = "Selecione uma tarefa para executar.";
}

init();
