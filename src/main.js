const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let structure = null;
let activeSection = "limpeza";
let activeLevel = "normal";
let pendingTask = null;
let activeTaskId = null;
let activeExecutionId = null;
let consoleBuffer = "";
let activityLabel = "";
let activityDots = "";
let activityTimer = null;

const Qboa = {
  async analyze() {
    return invoke("get_tasks");
  },

  async runTask(taskId, shutdownOnComplete) {
    return invoke("run_task", { taskId, shutdownOnComplete });
  },

  async runPreset(presetId) {
    return invoke("run_preset", { presetId });
  },

  async getSessionHistory() {
    return invoke("get_session_history");
  },

  async createRestorePoint() {
    return invoke("create_restore_point");
  },

  async rollbackSession() {
    return invoke("rollback_session");
  },

  async cancelTask(taskId) {
    return invoke("cancel_task", { taskId });
  }
};

async function loadStructure() {
  structure = await fetch("./data/qboa-structure.json").then((res) => res.json());
}

function renderSectionSummary() {
  const summaryEl = document.getElementById("section-summary");
  const section = structure.sections.find((item) => item.id === activeSection);
  if (!section) return;

  summaryEl.textContent = `${section.title}: ${section.items.length} tarefa${section.items.length === 1 ? "" : "s"} disponíveis`;
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
      appendConsole(`Selecione uma tarefa de ${section.title.toLowerCase()} para executar.`);
    });
    tabsEl.appendChild(btn);
  });

  const contributionTab = document.createElement("button");
  contributionTab.className = "tab" + (activeSection === "contribuir" ? " active" : "");
  contributionTab.textContent = "♡ Contribuir";
  contributionTab.addEventListener("click", () => {
    activeSection = "contribuir";
    renderTabs();
    document.getElementById("section-summary").textContent = "Contribua com código, testes, ideias ou Pix.";
    renderCards();
    appendConsole("Área de contribuição do Qboa Digital.");
  });
  tabsEl.appendChild(contributionTab);
}

function renderCards() {
  const cardsEl = document.getElementById("cards");
  const dashboard = document.querySelector(".dashboard");
  const recoveryPanel = document.querySelector(".recovery-panel");
  cardsEl.innerHTML = "";
  if (activeSection === "contribuir") {
    dashboard.classList.add("contribution-view");
    recoveryPanel.hidden = true;
    cardsEl.innerHTML = `
      <article class="contribution-panel">
        <div class="contribution-copy">
          <span class="eyebrow">Apoie o projeto</span>
          <h2>Ajude o Qboa Digital a continuar evoluindo</h2>
          <p>Você pode contribuir com código, ideias, testes ou uma contribuição via Pix.</p>
          <div class="contribution-actions">
            <a class="contribution-link" href="https://github.com/bloodshinoda/qboa-digital" target="_blank" rel="noreferrer">Participar no GitHub</a>
            <button class="copy-pix-button" type="button" data-pix="00020126330014BR.GOV.BCB.PIX0111072603319765204000053039865802BR62070503***63041548">Copiar código Pix</button>
          </div>
          <p id="pix-copy-status" class="pix-copy-status" aria-live="polite"></p>
        </div>
        <div class="pix-block">
          <img src="./data/pix-qrcode.png" alt="QR Code para contribuição via Pix" class="pix-qrcode" />
          <strong>Contribua via Pix</strong>
          <span>Abra o aplicativo do seu banco e escaneie o código.</span>
        </div>
      </article>
    `;
    cardsEl.querySelector(".copy-pix-button").addEventListener("click", async (event) => {
      const status = document.getElementById("pix-copy-status");
      try {
        await navigator.clipboard.writeText(event.currentTarget.dataset.pix);
        status.textContent = "Código Pix copiado.";
      } catch (error) {
        status.textContent = "Não foi possível copiar automaticamente o código Pix.";
      }
    });
    return;
  }
  dashboard.classList.remove("contribution-view");
  recoveryPanel.hidden = false;
  const section = structure.sections.find((item) => item.id === activeSection);
  if (!section) return;

  section.items.forEach((item) => {
    const card = document.createElement("div");
    card.className = "card";
    card.dataset.taskId = item.id;
    card.innerHTML = `<h3>${item.name}</h3><p>${item.desc}</p>`;
    if (item.risk) {
      const risk = document.createElement("small");
      risk.className = "risk-pill";
      const riskLabel = item.risk === "safe" ? "🟢 Seguro" : item.risk === "moderate" ? "🟡 Moderado" : "🟠 Avançado";
      risk.textContent = riskLabel;
      card.appendChild(risk);
    }
    card.addEventListener("click", () => runTask(item, card));
    cardsEl.appendChild(card);
  });
}

function appendConsole(message) {
  consoleBuffer = `${consoleBuffer}\n${message}`.trim();
  renderConsole();
}

function writeStatus(text) {
  consoleBuffer = text;
  renderConsole();
}

function renderConsole() {
  const out = document.getElementById("console-output");
  out.textContent = [consoleBuffer, activityLabel ? `${activityLabel}${activityDots}` : ""]
    .filter(Boolean)
    .join("\n");
}

function stopActivity() {
  if (activityTimer) window.clearInterval(activityTimer);
  activityTimer = null;
  activityLabel = "";
  activityDots = "";
  renderConsole();
}

function startActivity(taskName) {
  stopActivity();
  activityLabel = `Executando ${taskName}`;
  let dotCount = 0;
  const update = () => {
    activityDots = ".".repeat(dotCount % 4);
    dotCount += 1;
    renderConsole();
  };
  update();
  activityTimer = window.setInterval(update, 500);
}

function taskSuccessMessage(taskName) {
  return `${taskName} concluída com sucesso.`;
}

function taskDisplayName(taskId) {
  const task = structure?.sections
    ?.flatMap((section) => section.items)
    .find((item) => item.id === taskId);
  return task?.name || taskId;
}

function requiresConfirmation(item) {
  if (!item || !item.risk) return false;
  return item.risk === "moderate" || item.risk === "advanced";
}

function showRiskModal(item, cardEl) {
  pendingTask = { item, cardEl };
  const modal = document.getElementById("risk-modal");
  const message = document.getElementById("risk-message");

  const riskText = item.risk === "advanced" ? "avançada" : "moderada";
  message.textContent = `Esta operação é ${riskText} e altera configurações do Windows. Um ponto de restauração será criado quando possível. Alterações reversíveis serão registradas pelo Qboa.`;

  modal.classList.remove("hidden");
  modal.setAttribute("aria-hidden", "false");
}

async function executeTask(item, cardEl) {
  if (cardEl) cardEl.classList.add("running");
  activeTaskId = item.id;
  writeStatus(`> Rodando "${item.name}" (nível: ${activeLevel})...`);
  startActivity(item.name);
  const modal = document.getElementById("risk-modal");
  modal.classList.add("hidden");
  modal.setAttribute("aria-hidden", "true");
  pendingTask = null;
  const shutdownOnComplete = document.getElementById("shutdown-on-complete").checked;

  try {
    const result = await Qboa.runTask(item.id, shutdownOnComplete);
    const output = result.output || "(sem saída)";
    activeExecutionId = result.execution_id || activeExecutionId;
    appendConsole(output);
  } catch (err) {
    stopActivity();
    appendConsole(`Erro ao chamar o backend: ${err}`);
    if (cardEl) cardEl.classList.remove("running");
    activeTaskId = null;
  }
}

async function runTask(item, cardEl) {
  if (requiresConfirmation(item)) {
    showRiskModal(item, cardEl);
    return;
  }

  await executeTask(item, cardEl);
}

function bindLevels() {
  document.querySelectorAll(".level").forEach((btn) => {
    if (btn.dataset.level === activeLevel) btn.classList.add("active");
    btn.addEventListener("click", () => {
      activeLevel = btn.dataset.level;
      document.querySelectorAll(".level").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      if (activeLevel === "express" || activeLevel === "normal" || activeLevel === "turbo") {
        renderCards();
      }
    });
  });

  document.getElementById("preset-run-btn").addEventListener("click", async () => {
    if (activeTaskId) {
      appendConsole("Já existe uma tarefa em execução.");
      return;
    }

    const presetTasks = structure.sections.flatMap((section) =>
      section.items.filter((item) => item.preset && item.preset.includes(activeLevel))
    );
    if (!presetTasks.length) {
      appendConsole(`Nenhuma tarefa disponível para o preset ${activeLevel}.`);
      return;
    }

    if (activeLevel === "turbo" && !window.confirm("O preset Turbo executará alterações avançadas no Windows. Deseja continuar?")) {
      return;
    }

    try {
      const ids = await Qboa.runPreset(activeLevel);
      appendConsole(`Preset ${activeLevel} iniciado: ${ids.length} tarefas em sequência.`);
    } catch (error) {
      appendConsole(String(error));
    }
  });
}

async function refreshHistory() {
  try {
    const history = await Qboa.getSessionHistory();
    const list = document.getElementById("session-history");
    list.innerHTML = "";

    if (!history || history.length === 0) {
      list.innerHTML = "<li>Nenhuma alteração registrada.</li>";
      return;
    }

    history.slice(-6).forEach((entry) => {
      const item = document.createElement("li");
      item.textContent = `${entry.description} (${entry.rollback_status})`;
      list.appendChild(item);
    });
  } catch (error) {
    console.error("Unable to read session history", error);
  }
}

async function registerEvents() {
  const unlisten = await listen("qboa-event", (event) => {
    const payload = event.payload;
    if (payload.event === "task-started") {
      activeTaskId = payload.task_id;
      activeExecutionId = payload.execution_id;
      startActivity(taskDisplayName(payload.task_id));
      document.querySelector(`[data-task-id="${payload.task_id}"]`)?.classList.add("running");
    }

    if (payload.event === "task-output" && payload.message) {
      appendConsole(payload.message);
    } else if (payload.event === "task-completed") {
      stopActivity();
      appendConsole(taskSuccessMessage(taskDisplayName(payload.task_id)));
    } else if (payload.event === "task-cancelled") {
      stopActivity();
      appendConsole(`Tarefa ${taskDisplayName(payload.task_id)} cancelada.`);
    } else if (payload.event === "task-error") {
      stopActivity();
      appendConsole(`Erro em ${taskDisplayName(payload.task_id)}: ${payload.message || "falha não especificada."}`);
    } else if (payload.message) {
      appendConsole(`[${payload.event}] ${payload.message}`);
    }

    if (["task-completed", "task-error", "task-cancelled"].includes(payload.event)) {
      document.querySelector(`[data-task-id="${payload.task_id}"]`)?.classList.remove("running");
      if (activeTaskId === payload.task_id) activeTaskId = null;
      if (!payload.execution_id || payload.execution_id === activeExecutionId) activeExecutionId = null;
      refreshHistory();
    }
  });

  return unlisten;
}

(async function init() {
  await loadStructure();
  renderTabs();
  renderSectionSummary();
  renderCards();
  bindLevels();
  await registerEvents();
  await refreshHistory();
  writeStatus("Selecione uma tarefa para executar.");

  document.getElementById("restore-point-btn").addEventListener("click", async () => {
    try {
      const result = await Qboa.createRestorePoint();
      document.getElementById("restore-point-status").textContent = result;
      appendConsole(`Ponto de restauração criado: ${result}`);
    } catch (error) {
      document.getElementById("restore-point-status").textContent = `Proteção do Sistema não está disponível neste computador.`;
      appendConsole(String(error));
    }
  });

  document.getElementById("risk-cancel").addEventListener("click", () => {
    const modal = document.getElementById("risk-modal");
    modal.classList.add("hidden");
    modal.setAttribute("aria-hidden", "true");
    pendingTask = null;
  });

  document.getElementById("risk-confirm").addEventListener("click", async () => {
    if (!pendingTask) return;
    await executeTask(pendingTask.item, pendingTask.cardEl);
  });

  document.getElementById("cancel-task-btn").addEventListener("click", async () => {
    if (!activeTaskId) {
      appendConsole("Nenhuma tarefa em execução para cancelar.");
      return;
    }

    try {
      const result = await Qboa.cancelTask(activeTaskId);
      appendConsole(result);
    } catch (error) {
      appendConsole(String(error));
    }
  });
})();
