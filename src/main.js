const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let structure = null;
let activeSection = "limpeza";
let pendingTask = null;
let pendingConfirmationResolver = null;
let activeTaskId = null;
let activeExecutionId = null;
let launchPending = false;
let eventListenersReady = false;
let currentOperationName = "";
let currentOperationStage = "";
let consoleBuffer = "";
let executionOutput = [];
let executionStartedAt = null;
let executionToastTimer = null;
let activityLabel = "";
let activityDots = "";
let activityTimer = null;
let progressFlavorTimer = null;
let currentOperationProgress = null;
let lastProgressFlavorKey = "";
const progressFlavorLastIndex = new Map();
let serviceProfileState = null;
let currentServiceProfilePreview = null;
let editedServiceProfileChanges = [];
let windowsServices = [];
let browserCleanupDecision = null;
let showAllSessionChanges = false;
let activeDiagnosticTarget = null;
let diagnosticHistoryRequestId = 0;
let activeDiagnosticMode = "quick";
let completeDiagnosticRunning = false;
let completeDiagnosticCancelRequested = false;
let completeDiagnosticCurrentSession = null;
let completeDiagnosticCancelSent = false;
let memoryTestSessionSnapshot = null;
let lastMemoryActivitySessionId = null;
let lastMemoryActivityLifecycle = null;
let lastMemoryActivityPhase = null;
let memoryInventorySnapshot = null;
let cpuTestSessionSnapshot = null;
let cpuInventorySnapshot = null;
let cpuTestSamples = [];
let lastCpuActivitySessionId = null;
let lastCpuActivityLifecycle = null;
let lastCpuActivitySampleAt = 0;
let activeStorageBenchmark = null;
let storageBenchmarkPlaceholder = null;
let lastStorageActivityLogAt = 0;
let gpuTestSessionSnapshot = null;
let gpuTestSamples = [];
let gpuInventorySnapshot = null;
const diagnosticActivityTargets = new Map();

function diagnosticActivityTarget(reason) {
  if (reason.startsWith("inventory:cpu")) return document.querySelector("#cpu-test-panel .cpu-inventory-panel");
  if (reason.startsWith("inventory:gpu")) return document.querySelector("#gpu-test-panel .cpu-inventory-panel");
  if (reason.startsWith("inventory:memory")) return document.getElementById("memory-inventory-panel");
  if (reason.startsWith("inventory:storage")) return null;
  if (reason.startsWith("starting:cpu") || reason.startsWith("session:cpu")) return document.querySelector("#cpu-test-panel .cpu-test-chart-panel");
  if (reason.startsWith("starting:gpu") || reason.startsWith("session:gpu")) return document.querySelector("#gpu-test-panel .cpu-test-progress");
  if (reason.startsWith("starting:memory") || reason.startsWith("session:memory")) return document.querySelector("#memory-test-panel .memory-test-progress");
  if (reason.startsWith("run:") && reason.endsWith(":armazenamento")) return document.querySelector("#diagnostic-results .storage-live-benchmark") || document.getElementById("diagnostic-results");
  if (reason.startsWith("run:")) return document.getElementById("diagnostic-status");
  // Sessões com gráfico ou barra de progresso já têm feedback visual próprio.
  return null;
}

function refreshDiagnosticActivityTarget(target) {
  if (!target) return;
  const active = [...diagnosticActivityTargets.values()].some((element) => element === target);
  target.classList.add("qboa-activity-target");
  target.dataset.activity = active ? "working" : "idle";
}

function setDiagnosticActivity(reason, active) {
  const previousTarget = diagnosticActivityTargets.get(reason);
  if (active) {
    const target = diagnosticActivityTarget(reason);
    if (target) diagnosticActivityTargets.set(reason, target);
    else diagnosticActivityTargets.delete(reason);
  } else {
    diagnosticActivityTargets.delete(reason);
  }
  if (previousTarget) refreshDiagnosticActivityTarget(previousTarget);
  const currentTarget = diagnosticActivityTargets.get(reason);
  if (currentTarget && currentTarget !== previousTarget) refreshDiagnosticActivityTarget(currentTarget);
}

let cleanupSessionPhase = "closed";
let cleanupSessionTaskName = "";
let cleanupExecutionStarted = false;
let preparedCleanupTask = null;
let cleanupSessionPlaceholders = new Map();

function ensureCleanupSessionModal() {
  if (document.getElementById("cleanup-session-modal")) return;
  const shell = document.createElement("div");
  shell.id = "cleanup-session-modal";
  shell.className = "cleanup-session-modal hidden";
  shell.setAttribute("aria-hidden", "true");
  shell.innerHTML = '<section class="cleanup-session-dialog" role="dialog" aria-modal="true" aria-labelledby="cleanup-session-title"><header class="cleanup-session-header"><div><span class="eyebrow">QBoa Digital · sessão de manutenção</span><h2 id="cleanup-session-title">Preparando a faxina</h2><p id="cleanup-session-description">Vamos conferir tudo antes de começar.</p></div></header><ol class="cleanup-session-steps" aria-label="Etapas da sessão"><li data-session-step="preparing" class="active"><span>1</span> Preparação</li><li data-session-step="running"><span>2</span> Execução</li><li data-session-step="result"><span>3</span> Resultado</li></ol><div id="cleanup-preparation-intro" class="cleanup-preparation-intro qboa-activity-target"><strong>Deixando tudo pronto</strong><p>O QBoa está conferindo as condições e reunindo as decisões necessárias. A faxina só começa depois desta etapa.</p></div><div id="cleanup-preparation-options" class="cleanup-preparation-options"><label class="shutdown-option"><input id="shutdown-on-complete" type="checkbox" /><span>Desligar ao concluir</span></label></div><div id="cleanup-preparation-actions" class="cleanup-preparation-actions" hidden><button id="cleanup-preparation-cancel" class="secondary-button" type="button">Cancelar</button><button id="cleanup-preparation-start" class="primary-button" type="button">Iniciar</button></div><div class="cleanup-session-content" id="cleanup-session-content"></div><div class="toolbar" hidden><button id="cancel-task-btn" class="secondary-button" type="button" hidden disabled>Parar tarefa</button></div></section>';
  document.body.appendChild(shell);
  const content = shell.querySelector("#cleanup-session-content");
  ["risk-modal", "confirmation-modal", "operation-progress", "execution-summary-modal"].forEach((id) => {
    const element = document.getElementById(id);
    if (!element || !element.parentNode) return;
    const placeholder = document.createComment("QBoa cleanup session: " + id);
    element.parentNode.insertBefore(placeholder, element);
    cleanupSessionPlaceholders.set(element, placeholder);
  });
  shell.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && cleanupSessionPhase === "result") closeExecutionSummary();
  });
}

function setCleanupSessionPhase(phase, taskName = "") {
  ensureCleanupSessionModal();
  cleanupSessionPhase = phase;
  if (taskName) cleanupSessionTaskName = taskName;
  const shell = document.getElementById("cleanup-session-modal");
  const title = document.getElementById("cleanup-session-title");
  const description = document.getElementById("cleanup-session-description");
  const intro = document.getElementById("cleanup-preparation-intro");
  const risk = document.getElementById("risk-modal");
  const confirmation = document.getElementById("confirmation-modal");
  const progress = document.getElementById("operation-progress");
  const summary = document.getElementById("execution-summary-modal");
  const toolbar = shell.querySelector(".toolbar");
  const content = document.getElementById("cleanup-session-content");
  const preparationOptions = document.getElementById("cleanup-preparation-options");
  const preparationActions = document.getElementById("cleanup-preparation-actions");
  const shutdownOption = document.querySelector(".shutdown-option");
  cleanupSessionPlaceholders.forEach((_placeholder, element) => {
    if (element.parentNode !== content) content.appendChild(element);
  });
  if (shutdownOption) {
    const target = phase === "running" ? toolbar : preparationOptions;
    if (target && shutdownOption.parentNode !== target) target.appendChild(shutdownOption);
  }
  preparationOptions.hidden = !["preparing", "ready", "starting"].includes(phase);
  if (preparationActions) preparationActions.hidden = phase !== "ready";
  const preparationPromptOpen = !risk?.classList.contains("hidden") || !confirmation?.classList.contains("hidden");
  const preparationActivity = document.getElementById("cleanup-preparation-intro");
  if (preparationActivity) preparationActivity.dataset.activity =
    (phase === "starting" || (phase === "preparing" && !preparationPromptOpen))
      ? "working" : "idle";
  shell.classList.remove("hidden");
  shell.setAttribute("aria-hidden", "false");
  shell.dataset.phase = phase;
  title.textContent = phase === "preparing" ? "Preparando a faxina" : phase === "ready" ? "Pronto para iniciar " + (cleanupSessionTaskName || "a limpeza") : phase === "starting" ? "Preparando a proteção" : phase === "running" ? "Mãos à obra!" : "Como ficou a faxina";
  description.textContent = phase === "preparing" ? (preparationPromptOpen ? "Precisamos da sua decisão antes de continuar." : "Vamos conferir tudo antes de começar.") : phase === "ready" ? "Tudo conferido. Quando quiser, inicie a limpeza; nenhuma etapa de limpeza começou ainda." : phase === "starting" ? "O QBoa está preparando a proteção do sistema. A limpeza ainda não começou." : phase === "running" ? (cleanupSessionTaskName || "A limpeza está em andamento.") : "A sessão terminou. Confira o resumo abaixo.";
  intro.hidden = !["preparing", "starting"].includes(phase) || preparationPromptOpen;
  if (progress) {
    progress.hidden = phase !== "running";
    progress.classList.add("qboa-activity-target");
    progress.dataset.activity = phase === "running" ? "working" : "idle";
  }
  if (summary) {
    summary.classList.toggle("hidden", phase !== "result");
    summary.setAttribute("aria-hidden", phase !== "result" ? "true" : "false");
  }
  if (toolbar) toolbar.hidden = phase !== "running";
  shell.querySelectorAll("[data-session-step]").forEach((step) => {
    const stepPhase = step.dataset.sessionStep;
    const order = { preparing: 0, running: 1, result: 2 };
    step.classList.toggle("active", stepPhase === (["ready", "starting"].includes(phase) ? "preparing" : phase));
    step.classList.toggle("complete", order[stepPhase] < (["ready", "starting"].includes(phase) ? 0 : order[phase]));
  });
  if (phase === "running") {
    updateOperationProgress(cleanupSessionTaskName || currentOperationName || taskDisplayName(activeTaskId), currentOperationStage || "Preparando a faxina…", currentOperationProgress);
    if (progressFlavorTimer === null) {
      scheduleProgressFlavorRotation();
    }
  } else if (progressFlavorTimer !== null) {
    window.clearTimeout(progressFlavorTimer);
    progressFlavorTimer = null;
  }
}

function closeCleanupSession() {
  const shell = document.getElementById("cleanup-session-modal");
  if (!shell) return;
  shell.classList.add("hidden");
  shell.setAttribute("aria-hidden", "true");
  const toolbar = shell.querySelector(".toolbar");
  const shutdownOption = document.querySelector(".shutdown-option");
  const preparationOptions = document.getElementById("cleanup-preparation-options");
  if (preparationOptions && shutdownOption && shutdownOption.parentNode !== preparationOptions) preparationOptions.appendChild(shutdownOption);
  cleanupSessionPlaceholders.forEach((placeholder, element) => {
    if (placeholder.parentNode) placeholder.parentNode.insertBefore(element, placeholder.nextSibling);
  });
  if (toolbar) toolbar.hidden = true;
  cleanupSessionPhase = "closed";
  cleanupSessionTaskName = "";
  cleanupExecutionStarted = false;
  syncExecutionControls();
}

const Qboa = {
  async analyze() {
    return invoke("get_tasks");
  },

  async getRunningBrowsers() {
    return invoke("get_running_browsers");
  },

  async prepareBrowserCleanup(closeRunning) {
    return invoke("prepare_browser_cleanup", { closeRunning });
  },

  async prepareCleanup(taskId) {
    return invoke("prepare_cleanup", { taskId });
  },

  async cancelCleanupPreparation(token) {
    return invoke("cancel_cleanup_preparation", { token });
  },

  async runTask(taskId, shutdownOnComplete, resetBaseAuthorized = false, preparationToken = null, proceedWithoutRestorePoint = false) {
    return invoke("run_task", {
      taskId,
      shutdownOnComplete,
      resetBaseAuthorized,
      preparationToken,
      proceedWithoutRestorePoint
    });
  },

  async setShutdownOnComplete(enabled) {
    return invoke("set_shutdown_on_complete", { enabled });
  },

  async respondRestorePointFailure(executionId, proceed) {
    return invoke("respond_restore_point_failure", { executionId, proceed });
  },


  async getSessionHistory() {
    return invoke("get_session_history");
  },

  async runDiagnosticCheck(mode) {
    return invoke("run_diagnostic_check", { mode });
  },

  async getDiagnosticReports() {
    return invoke("get_diagnostic_reports");
  },

  async updateCompleteDiagnosticReport(reportId, results, issues, system, executionStatus, durationMs = null) {
    return invoke("update_complete_diagnostic_report", { reportId, results, issues, system, executionStatus, durationMs });
  },

  async listStorageDisks() {
    return invoke("list_storage_disks");
  },

  async runStorageDiagnostic(diskId) {
    return invoke("run_storage_diagnostic", { diskId });
  },

  async getStorageDiagnosticReports() {
    return invoke("get_storage_diagnostic_reports");
  },

  async startMemoryTest(profile, memoryBudgetBytes = null) {
    return invoke("start_memory_test", {
      profile,
      memoryBudgetBytes,
      preferredBlockBytes: null,
      seed: null
    });
  },

  async getMemoryInventory() {
    return invoke("get_memory_inventory");
  },

  async getCpuInventory() {
    return invoke("get_cpu_inventory");
  },

  async installPawnIo() {
    return invoke("install_pawnio");
  },

  async startCpuTest(profile) {
    return invoke("start_cpu_test", { profile });
  },

  async getCpuTestSession() {
    return invoke("get_cpu_test_session");
  },

  async cancelCpuTest(sessionId) {
    return invoke("cancel_cpu_test", { sessionId });
  },

  async getCpuDiagnosticReports() {
    return invoke("get_cpu_diagnostic_reports");
  },

  async getGpuInventory() {
    return invoke("get_gpu_inventory");
  },

  async startGpuTest(profile) {
    return invoke("start_gpu_test", { profile });
  },

  async getGpuTestSession() {
    return invoke("get_gpu_test_session");
  },

  async cancelGpuTest(sessionId) {
    return invoke("cancel_gpu_test", { sessionId });
  },

  async getGpuDiagnosticReports() {
    return invoke("get_gpu_diagnostic_reports");
  },

  async getMemoryTestSession() {
    return invoke("get_memory_test_session");
  },

  async getMemoryDiagnosticReports() {
    return invoke("get_memory_diagnostic_reports");
  },

  async cancelMemoryTest(sessionId) {
    return invoke("cancel_memory_test", { sessionId });
  },

  async repairWindowsUpdate() {
    return invoke("repair_windows_update");
  },

  async listWindowsServices() { return invoke("list_windows_services"); },

  async manageWindowsService(name, action, startType = null) { return invoke("manage_windows_service", { name, action, startType }); },

  async getCapabilityInventory() {
    return invoke("get_capability_inventory");
  },

  async getServiceProfileState() {
    return invoke("get_service_profile_state");
  },

  async previewServiceProfile(profile) {
    return invoke("preview_service_profile", { profile });
  },

  async applyServiceProfile(profile, changes) {
    return invoke("apply_service_profile", { profile, changes });
  },

  async restoreWindowsServiceDefaults() {
    return invoke("restore_windows_service_defaults");
  },

  async undoServiceProfileChanges() {
    return invoke("undo_service_profile_changes");
  },

  async createRestorePoint() {
    return invoke("create_restore_point");
  },

  async rollbackSession() {
    return invoke("rollback_session");
  },

  async cancelTask(taskId) {
    return invoke("cancel_task", { taskId, executionId: activeExecutionId });
  },

  async finishStartup() {
    await new Promise((resolve) => window.setTimeout(resolve, 700));
    return invoke("finish_startup");
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

  const tabs = [
    {
      id: "limpeza",
      label: "🧹 Limpeza",
      summary: () => renderSectionSummary()
    },
    {
      id: "diagnostico",
      label: "🩺 Diagnóstico",
      summary: () => { document.getElementById("section-summary").textContent = "Testes de hardware e saúde do sistema"; }
    },
    {
      id: "servicos",
      label: "🛠️ Serviços",
      summary: () => { document.getElementById("section-summary").textContent = "Gerenciamento dos serviços do Windows"; }
    },
    {
      id: "capacidades",
      label: "🧩 Capacidades",
      summary: () => { document.getElementById("section-summary").textContent = "Inventário de hardware, firmware e recursos do Windows"; }
    },
    {
      id: "sobre",
      label: "ℹ️ Sobre",
      summary: () => { document.getElementById("section-summary").textContent = "Sobre o QBoa Digital · versão 3.2.3"; }
    }
  ];

  tabs.forEach((tab) => {
    const btn = document.createElement("button");
    btn.className = "tab" + (tab.id === activeSection ? " active" : "");
    btn.textContent = tab.label;
    btn.addEventListener("click", async () => {
      activeSection = tab.id;
      renderTabs();
      tab.summary();
      renderCards();
      if (tab.id === "servicos" && !serviceProfileState) await loadServiceProfileState();
      if (tab.id === "capacidades") void loadCapabilityInventory();
    });
    tabsEl.appendChild(btn);
  });
}

function syncExecutionControls() {
  const running = Boolean(activeTaskId || launchPending);
  const inCleanupSession = cleanupSessionPhase !== "closed";
  const cancelButton = document.getElementById("cancel-task-btn");
  const toolbar = document.querySelector("#cleanup-session-modal .toolbar");
  if (cancelButton) {
    cancelButton.hidden = !activeTaskId || (inCleanupSession && cleanupSessionPhase !== "running");
    cancelButton.disabled = !activeTaskId || (inCleanupSession && cleanupSessionPhase !== "running");
  }
  if (toolbar) toolbar.hidden = !inCleanupSession || cleanupSessionPhase !== "running";
  const progress = document.getElementById("operation-progress");
  if (progress) {
    progress.hidden = !running || (inCleanupSession && cleanupSessionPhase !== "running");
    progress.classList.add("qboa-activity-target");
    progress.dataset.activity = running ? "working" : "idle";
  }
  if (running && (!inCleanupSession || cleanupSessionPhase === "running")) updateOperationProgress(currentOperationName || taskDisplayName(activeTaskId), currentOperationStage || "Preparando a faxina…", null);
  else if (!running) clearTaskbarProgress();
}

function renderCards() {
  const cardsEl = document.getElementById("cards");
  const dashboard = document.querySelector(".dashboard");
  const recoveryPanel = document.querySelector(".recovery-panel");
  const serviceProfilePanel = document.getElementById("service-profile-panel");
  const capabilityPanel = document.getElementById("capability-inventory-panel");
  serviceProfilePanel.hidden = activeSection !== "servicos";
  capabilityPanel.hidden = activeSection !== "capacidades";
  dashboard.hidden = activeSection === "servicos" || activeSection === "capacidades";
  dashboard.classList.toggle("contribution-view", activeSection === "sobre");
  syncExecutionControls();
  cardsEl.innerHTML = "";
  if (activeSection === "servicos" || activeSection === "capacidades") { if (activeSection === "servicos" && !windowsServices.length) void loadWindowsServices(); return; }
  if (activeSection === "diagnostico") {
    recoveryPanel.hidden = true;
    const tests = [
      { id: "armazenamento", name: "Armazenamento", desc: "Espaço livre e informações disponíveis sobre os volumes." },
      { id: "teste_memoria", name: "Memória RAM", desc: "Inventaria os módulos e a frequência reportada, mostra as limitações de leitura de timings e executa o teste de integridade com perfil selecionável." },
      { id: "placa_de_video", name: "GPU", desc: "Teste de estabilidade com compute shader Direct3D 11, validação dos resultados e monitoramento térmico." },
      { id: "processador", name: "Processador", desc: "Teste de carga por 5, 15 ou 30 minutos, com gráfico de temperatura, utilização e frequência." },
      { id: "completo", name: "Diagnóstico completo", desc: "Executa a bateria completa e reúne os resultados em um laudo persistente." }
    ];
    tests.forEach((test) => {
      const card = document.createElement("article");
      card.className = "card";
      card.tabIndex = 0;
      card.setAttribute("role", "button");
      card.setAttribute("aria-label", `Abrir teste: ${test.name}`);
      card.innerHTML = `<h3>${test.name}</h3><p>${test.desc}</p>`;
      const open = () => openDiagnosticModal(test.id === "completo" ? null : test.id, test.id === "completo" ? "complete" : "quick");
      card.addEventListener("click", open);
      card.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") { event.preventDefault(); open(); }
      });
      cardsEl.appendChild(card);
    });

    const updateCard = document.createElement("article");
    updateCard.className = "card windows-update-card";
    updateCard.tabIndex = 0;
    updateCard.setAttribute("role", "button");
    updateCard.setAttribute("aria-label", "Windows Update: restaurar o funcionamento das atualizações");
    updateCard.innerHTML = '<h3>Windows Update</h3><p>Repara bloqueios locais conhecidos, recupera serviços essenciais e reativa tarefas de atualização.</p>';
    const openUpdate = () => openWindowsUpdateRepair();
    updateCard.addEventListener("click", openUpdate);
    updateCard.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") { event.preventDefault(); openUpdate(); }
    });
    cardsEl.appendChild(updateCard);
    return;
  }
  if (activeSection === "sobre") {
    dashboard.classList.add("contribution-view");
    recoveryPanel.hidden = true;
    cardsEl.innerHTML = `
      <article class="contribution-panel about-panel">
        <div class="contribution-copy">
          <span class="eyebrow">Sobre o aplicativo</span>
          <h2>QBoa Digital <span class="about-version">3.2.3</span></h2>
          <p>Utilitário independente de limpeza, manutenção, privacidade e diagnóstico do Windows, com foco em ferramentas práticas e atenção à segurança das operações.</p>
          <div class="about-meta">
            <p><strong>Versão:</strong> 3.2.3</p>
            <p><strong>Build:</strong> <span class="about-build-number">Local</span></p>
            <p><strong>Autoria e desenvolvimento:</strong> Vilson de Oliveira Junior</p>
            <p><strong>Tecnologias:</strong> Tauri, Rust, JavaScript, HTML e CSS</p>
          </div>
          <div class="contribution-actions">
            <a class="contribution-link" href="https://github.com/bloodshinoda/qboa-digital" target="_blank" rel="noreferrer">Repositório no GitHub</a>
            <button class="copy-pix-button" type="button" data-pix="00020126330014BR.GOV.BCB.PIX0111072603319765204000053039865802BR62070503***63041548">Copiar código Pix</button>
          </div>
          <p class="about-support">O QBoa Digital é um projeto independente. Contribuições ajudam a manter o desenvolvimento, os testes e a evolução do aplicativo.</p>
          <p id="pix-copy-status" class="pix-copy-status" aria-live="polite"></p>
        </div>
        <div class="pix-block">
          <img src="./data/pix-qrcode.png" alt="QR Code para contribuição via Pix" class="pix-qrcode" />
          <strong>Apoie via Pix</strong>
          <span>Escaneie o QR Code com o aplicativo do seu banco.</span>
        </div>
      </article>
    `;
    const buildLabel = cardsEl.querySelector(".about-build-number");
    const buildNumber = String(window.QBOA_BUILD_NUMBER || "Local");
    if (buildLabel) buildLabel.textContent = buildNumber === "Local" ? "Local" : `#${buildNumber}`;
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

function serviceStatusLabel(status) {
  return ({ running: "Em execução", stopped: "Parado", startpending: "Iniciando", stoppending: "Parando", paused: "Pausado" })[String(status).toLowerCase()] || status;
}

const WINDOWS_SERVICE_DESCRIPTIONS = {
  "phonesvc":"Serviço de telefonia",
  "phone service":"Serviço de telefonia",
  "dhcp":"Obtém endereços IP e configurações de rede automaticamente",
  "dnscache":"Armazena consultas DNS em cache para acelerar a navegação",
  "wuauserv":"Gerencia atualizações do Windows",
  "bits":"Transfere arquivos em segundo plano, inclusive para atualizações",
  "windefend":"Proteção antivírus do Microsoft Defender",
  "audiosrv":"Gerencia o áudio do Windows",
  "audioendpointbuilder":"Detecta e configura dispositivos de áudio",
  "cryptsvc":"Serviços criptográficos e validação de assinaturas digitais",
  "eventlog":"Registra eventos do Windows e dos aplicativos",
  "spooler":"Gerencia a fila e os trabalhos de impressão",
  "schedule":"Executa tarefas agendadas",
  "winmgmt":"Infraestrutura de gerenciamento do Windows (WMI)",
  "profsvc":"Carrega e gerencia perfis de usuário",
  "usermanager":"Gerencia usuários e sessões",
  "lanmanworkstation":"Conecta o computador a pastas e recursos compartilhados na rede",
  "lanmanserver":"Compartilha arquivos e impressoras pela rede",
  "power":"Gerencia planos e estados de energia",
  "nlasvc":"Identifica redes conectadas e suas propriedades",
  "netprofm":"Identifica propriedades e perfis de rede",
  "wlan svc":"Gerencia conexões Wi-Fi",
  "wlansvc":"Gerencia conexões Wi-Fi",
  "wwansvc":"Gerencia conexões de banda larga móvel",
  "bthserv":"Suporte básico a dispositivos Bluetooth",
  "deviceassociationservice":"Emparelha dispositivos e periféricos",
  "deviceinstall":"Gerencia a instalação de dispositivos",
  "plugplay":"Detecta e configura dispositivos conectados",
  "spooler":"Gerencia fila de impressão",
  "fax":"Envia e recebe fax",
  "themes":"Gerencia temas visuais do Windows",
  "fontcache":"Otimiza o carregamento de fontes",
  "sysmain":"Otimiza o desempenho e o carregamento de aplicativos",
  "defragsvc":"Otimiza e desfragmenta unidades de armazenamento",
  "vss":"Coordena cópias de sombra usadas por backup e restauração",
  "wbengine":"Motor de backup do Windows",
  "fhsvc":"Gerencia o Histórico de Arquivos",
  "storsvc":"Gerencia recursos e configurações de armazenamento",
  "vds":"Gerencia discos e volumes",
  "msiserver":"Instala, repara e remove programas no formato MSI",
  "installservice":"Infraestrutura de instalação da Microsoft Store",
  "clipsvc":"Infraestrutura de licenciamento da Microsoft Store",
  "appxs vc":"Infraestrutura de implantação de aplicativos da Store",
  "appxsvc":"Infraestrutura de implantação de aplicativos da Store",
  "state repository":"Armazena estado de aplicativos",
  "staterepository":"Armazena estado de aplicativos",
  "wscsvc":"Central de segurança e estado de proteção do Windows",
  "securityhealthservice":"Mantém informações de segurança do Windows",
  "sense":"Detecção e resposta avançada do Microsoft Defender",
  "wdnisvc":"Inspeção de rede do Microsoft Defender",
  "bdesvc":"Gerencia a criptografia de unidades com BitLocker",
  "efs":"Criptografa arquivos em unidades NTFS",
  "scardsvr":"Gerencia acesso a cartões inteligentes",
  "vaultsvc":"Armazena credenciais e segredos do Windows",
  "keyiso":"Isola chaves criptográficas e operações de autenticação",
  "termservice":"Gerencia sessões de Área de Trabalho Remota",
  "sessionenv":"Configura sessões de Área de Trabalho Remota",
  "umrdpservice":"Redireciona dispositivos em sessões remotas",
  "rasman":"Gerencia conexões VPN e de acesso remoto",
  "rasauto":"Cria conexões de acesso remoto sob demanda",
  "sstpsvc":"Suporte a conexões VPN SSTP",
  "remote registry":"Permite gerenciamento remoto do Registro",
  "remoteregistry":"Permite gerenciamento remoto do Registro",
  "winrm":"Gerenciamento remoto do Windows via WS-Management",
  "ssh-agent":"Armazena chaves SSH para autenticação",
  "iphlpsvc":"Suporte à conectividade IPv6",
  "sharedaccess":"Compartilha a conexão de Internet e o ponto de acesso móvel",
  "icssvc":"Gerencia o ponto de acesso móvel",
  "dnscache":"Armazena consultas DNS em cache",
  "lmhosts":"Suporte NetBIOS sobre TCP/IP",
  "netlogon":"Autentica usuários e computadores em domínios",
  "fdrespub":"Publica o computador e seus recursos para descoberta na rede",
  "fdphost":"Hospeda descoberta de dispositivos na rede",
  "ssdpsrv":"Descobre dispositivos UPnP na rede",
  "upnphost":"Hospeda dispositivos UPnP",
  "webclient":"Acesso a arquivos e pastas via WebDAV",
  "trkwks":"Mantém vínculos com arquivos em outros computadores",
  "diagnosticshub.standardcollector.service":"Coleta dados de diagnóstico para ferramentas de desenvolvimento",
  "dps":"Detecta e soluciona problemas do Windows",
  "diagsvc":"Executa serviços de diagnóstico",
  "wer svc":"Relatório de erros do Windows",
  "wersvc":"Relata falhas de aplicativos e do Windows",
  "diagtrack":"Coleta dados de diagnóstico e telemetria do Windows",
  "w32time":"Sincroniza a data e a hora do sistema",
  "tzautoupdate":"Atualiza automaticamente o fuso horário",
  "camsvc":"Gerencia acesso de aplicativos a recursos do sistema",
  "frame server":"Gerencia fluxos de câmera para aplicativos",
  "frameserver":"Gerencia fluxos de câmera para aplicativos",
  "lfsvc":"Gerencia recursos de localização geográfica",
  "sensorservice":"Gerencia sensores do dispositivo",
  "sensordataservice":"Fornece dados de sensores",
  "tabletinputservice":"Teclado virtual, escrita manual e entrada por toque",
  "hidserv":"Suporte a teclas especiais e dispositivos HID",
  "bthavctpsvc":"Gerencia comandos e controles de áudio Bluetooth",
  "phone svc":"Serviço de telefonia",
  "smsrouter":"Encaminha mensagens para aplicativos compatíveis",
  "tokenbroker":"Intermedeia tokens de autenticação de aplicativos",
  "wlidsvc":"Login com conta Microsoft",
  "licensemanager":"Gerencia licenças de aplicativos da Microsoft Store",
  "push to install":"Permite instalação remota de aplicativos da Store",
  "pushtoinstall":"Permite instalação remota de aplicativos da Store",
  "xbl authmanager":"Autenticação para serviços Xbox",
  "xblauthmanager":"Autenticação para serviços Xbox",
  "xblgamesave":"Sincroniza salvamentos de jogos Xbox",
  "xboxgipservice":"Gerencia periféricos Xbox",
  "vmcompute":"Gerencia máquinas virtuais e contêineres",
  "vmicguestinterface":"Comunicação entre máquina virtual Hyper-V e host",
  "vmicheartbeat":"Informa ao host se a máquina virtual está ativa",
  "vmickvpexchange":"Troca informações entre host e máquina virtual",
  "vmicrdv":"Redireciona dispositivos em sessões Hyper-V",
  "vmicshutdown":"Permite desligamento coordenado de máquinas virtuais",
  "vmictimesync":"Sincroniza o relógio de máquinas virtuais",
  "vmicvmsession":"Gerencia sessões de máquinas virtuais",
  "vmicvss":"Coordena cópias de sombra em máquinas virtuais",
  "hns":"Gerencia redes de contêineres e virtualização",
  "wmiapsrv":"Adaptador de desempenho do WMI",
  "pla":"Coleta dados de desempenho em conjuntos",
  "perfhost":"Hospeda provedores de desempenho",
  "retaildemo":"Controla o modo de demonstração de varejo",
  "fax":"Envia e recebe fax",
  "phone service":"Serviço de telefonia"
};
function serviceDescription(name) {
  const key = String(name || "").trim().toLowerCase();
  return WINDOWS_SERVICE_DESCRIPTIONS[key] || (key === "phoneservice" ? WINDOWS_SERVICE_DESCRIPTIONS.phonesvc : "");
}

function serviceStartTypeLabel(startType) {
  return ({ automatic: "Automático", manual: "Manual", disabled: "Desativado", unknown: "Desconhecido" })[String(startType).toLowerCase()] || startType;
}

async function loadWindowsServices() {
  const status = document.getElementById("service-inventory-status");
  const refresh = document.getElementById("service-inventory-refresh");
  refresh.disabled = true;
  status.textContent = "Consultando serviços do Windows...";
  try {
    windowsServices = await Qboa.listWindowsServices();
    status.textContent = `${windowsServices.length} serviço(s) encontrado(s). Serviços protegidos não podem ser parados, reiniciados ou desativados.`;
    renderWindowsServices();
  } catch (error) {
    windowsServices = [];
    renderWindowsServices();
    status.textContent = String(error);
  } finally {
    refresh.disabled = false;
  }
}

function renderWindowsServices() {
  const body = document.getElementById("service-inventory-rows");
  const filter = document.getElementById("service-inventory-filter").value.trim().toLocaleLowerCase();
  const hideMicrosoft = document.getElementById("service-inventory-hide-microsoft").checked;
  body.replaceChildren();
  const filtered = windowsServices.filter((service) =>
    (!hideMicrosoft || !service.is_microsoft) &&
    (service.name.toLocaleLowerCase().includes(filter) ||
     service.display_name.toLocaleLowerCase().includes(filter) ||
     String(service.vendor || "").toLocaleLowerCase().includes(filter))
  );
  document.getElementById("service-inventory-status").textContent =
    `${filtered.length} de ${windowsServices.length} serviço(s) exibido(s).` +
    (hideMicrosoft ? " Serviços Microsoft ocultos." : "");
  filtered.forEach((service) => {
    const row = document.createElement("tr");
    const nameCell = document.createElement("td");
    const name = document.createElement("strong");
    name.textContent = service.display_name;
    const key = document.createElement("small");
    key.textContent = service.name;
    nameCell.append(name, key);
    const identity = document.createElement("small");
    identity.className = "service-vendor-label";
    identity.textContent = service.vendor
      ? `Fornecedor identificado: ${service.vendor}`
      : service.classification === "microsoft_signed"
        ? "Assinatura Microsoft válida"
        : "Fornecedor não identificado · confirme antes de alterar";
    identity.title = [
      ...(service.identification_evidence || []),
      service.executable_path || ""
    ].filter(Boolean).join("\n");
    nameCell.appendChild(identity);
    const identityHint = service.vendor
      ? `Fornecedor identificado: ${service.vendor}.`
      : service.classification === "microsoft_signed"
        ? "O executável tem assinatura Microsoft válida."
        : `Fornecedor não identificado. Confira o executável antes de alterar. Caminho: ${service.executable_path || "não disponível"}.`;
    if (service.protected) {
      const protectedLabel = document.createElement("span");
      protectedLabel.className = "service-protected-label";
      protectedLabel.textContent = "Protegido";
      nameCell.appendChild(protectedLabel);
      nameCell.title = service.protection_reason || "Serviço crítico protegido pelo QBoa.";
    }
    const stateCell = document.createElement("td");
    stateCell.textContent = serviceStatusLabel(service.status);
    const startCell = document.createElement("td");
    const startSelect = document.createElement("select");
    startSelect.setAttribute("aria-label", `Inicialização de ${service.name}`);
    [["automatic", "Automático"], ["manual", "Manual"], ["disabled", "Desativado"]].forEach(([value, label]) => {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      startSelect.appendChild(option);
    });
    startSelect.value = ["automatic", "manual", "disabled"].includes(service.start_type) ? service.start_type : "manual";
    startSelect.disabled = service.start_type === "unknown";
    startSelect.addEventListener("change", async () => {
      const next = startSelect.value;
      const old = service.start_type;
      if (next === old) return;
      if (next === "disabled" && service.protected) {
        appendConsole(`Alteração bloqueada: ${service.name} é protegido pelo QBoa.`);
        startSelect.value = old;
        return;
      }
      if (!(await askConfirmation(
        `Alterar a inicialização de ${service.display_name} de ${serviceStartTypeLabel(old)} para ${serviceStartTypeLabel(next)}?\n${identityHint}\nA configuração atual será consultada novamente pelo Windows durante a operação.`,
        "Alterar serviço?"
      ))) {
        startSelect.value = old;
        return;
      }
      startSelect.disabled = true;
      try {
        appendConsole(await Qboa.manageWindowsService(service.name, "set_start_type", next));
        await loadWindowsServices();
      } catch (error) {
        appendConsole(String(error));
        startSelect.value = old;
      } finally {
        startSelect.disabled = false;
      }
    });
    startCell.appendChild(startSelect);
    const actionsCell = document.createElement("td");
    const addAction = (label, action, disabled, confirmation) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "service-action-button secondary-button";
      button.textContent = label;
      button.disabled = disabled;
      button.addEventListener("click", async () => {
        if (confirmation && !(await askConfirmation(
          `${confirmation}\n${identityHint}\nConfirme que este serviço pode ser alterado no seu sistema.`,
          `${label} serviço?`
        ))) return;
        button.disabled = true;
        try {
          appendConsole(await Qboa.manageWindowsService(service.name, action));
          await loadWindowsServices();
        } catch (error) {
          appendConsole(String(error));
        } finally {
          button.disabled = false;
        }
      });
      actionsCell.appendChild(button);
    };
    addAction("Iniciar", "start", service.status.toLowerCase() === "running", `Iniciar o serviço ${service.display_name}?`);
    addAction("Parar", "stop", service.protected || service.status.toLowerCase() !== "running", `Parar o serviço ${service.display_name}?`);
    addAction("Reiniciar", "restart", service.protected || service.status.toLowerCase() !== "running", `Reiniciar o serviço ${service.display_name}?`);
    row.append(nameCell, stateCell, startCell, actionsCell);
    body.appendChild(row);
  });
  if (!filtered.length) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = 4;
    cell.textContent = windowsServices.length ? "Nenhum serviço corresponde ao filtro." : "Nenhum serviço carregado.";
    row.appendChild(cell);
    body.appendChild(row);
  }
}


const capabilityStatusLabels = {
  detected: "Detectado / habilitado",
  disabled: "Detectado, mas desabilitado",
  registered_not_present: "Registrado, não presente",
  not_detected: "Não detectado",
  unknown: "Desconhecido",
  not_applicable: "Não aplicável"
};

async function loadCapabilityInventory() {
  const button = document.getElementById("capability-refresh-btn");
  const status = document.getElementById("capability-status");
  button.disabled = true;
  status.textContent = "Coletando evidências do Windows…";
  try {
    const report = await Qboa.getCapabilityInventory();
    renderCapabilityInventory(report);
    status.textContent = `Inventário concluído: ${report.capabilities.length} capacidades avaliadas. A coleta não altera configurações.`;
  } catch (error) {
    status.textContent = `Não foi possível coletar o inventário: ${String(error)}`;
    document.getElementById("capability-system").replaceChildren();
    document.getElementById("capability-list").replaceChildren();
    document.getElementById("capability-warnings").replaceChildren();
  } finally {
    button.disabled = false;
  }
}

async function copyCapabilitySummary() {
  const status = document.getElementById("capability-copy-status");
  const system = document.getElementById("capability-system");
  const lines = Array.from(system.querySelectorAll("p")).map((item) => item.textContent.trim()).filter(Boolean);
  if (!lines.length) {
    status.textContent = "Colete o inventário antes de copiar o resumo.";
    return;
  }
  try {
    await navigator.clipboard.writeText(lines.join("\n"));
    status.textContent = "Resumo copiado para a área de transferência.";
  } catch (error) {
    status.textContent = "Não foi possível copiar automaticamente. Verifique a permissão da área de transferência.";
  }
}

function renderCapabilityInventory(report) {
  const system = document.getElementById("capability-system");
  const list = document.getElementById("capability-list");
  const warnings = document.getElementById("capability-warnings");
  system.replaceChildren();
  list.replaceChildren();
  warnings.replaceChildren();
  const summary = report.system || {};
  const deviceTypeLabels = { desktop: "Desktop", mobile: "Notebook / dispositivo móvel", virtual_machine: "Máquina virtual", unknown: "Desconhecido" };
  const confidenceLabels = { high: "alta", medium: "média", low: "baixa" };
  const systemLines = [
    [summary.os_name, summary.os_version ? `versão ${summary.os_version}` : null, summary.os_build ? `build ${summary.os_build}` : null].filter(Boolean).join(" · "),
    summary.motherboard ? `Placa mãe: ${summary.motherboard}` : [summary.manufacturer, summary.model].filter(Boolean).join(" "),
    summary.processor ? `Processador: ${summary.processor}` : null,
    summary.memory ? `Memória: ${summary.memory}` : null,
    summary.gpu ? `GPU: ${summary.gpu}` : null,
    summary.architecture ? `Arquitetura: ${summary.architecture}` : null,
    `Tipo de equipamento: ${deviceTypeLabels[summary.device_type] || deviceTypeLabels.unknown} (confiança ${confidenceLabels[summary.device_confidence] || "baixa"})`
  ].filter(Boolean);
  systemLines.forEach((line) => { const p = document.createElement("p"); p.textContent = line; system.appendChild(p); });
  (summary.device_evidence || []).forEach((evidence) => { const p = document.createElement("p"); p.className = "device-evidence"; p.textContent = `Evidência: ${evidence}`; system.appendChild(p); });
  (report.capabilities || []).filter((capability) => !(summary.device_type === "desktop" && summary.device_confidence === "high" && capability.id === "battery")).forEach((capability) => {
    const article = document.createElement("article");
    article.className = "capability-card";
    const heading = document.createElement("div");
    heading.className = "capability-card-heading";
    const title = document.createElement("h3"); title.textContent = capability.label;
    const badge = document.createElement("span"); badge.className = `capability-state state-${capability.status}`;
    badge.textContent = capabilityStatusLabels[capability.status] || capability.status;
    heading.append(title, badge);
    const description = document.createElement("p"); description.textContent = capability.summary;
    const confidence = document.createElement("small"); confidence.textContent = `Confiança: ${confidenceLabels[capability.confidence] || "não informada"} · Fonte: ${(capability.sources || []).join(", ") || "não informada"}`;
    article.append(heading, description, confidence);
    if ((capability.details || []).length) {
      const details = document.createElement("details");
      const disclosure = document.createElement("summary"); disclosure.textContent = "Ver evidências";
      const evidence = document.createElement("ul");
      capability.details.forEach((detail) => { const item = document.createElement("li"); item.textContent = detail; evidence.appendChild(item); });
      details.append(disclosure, evidence); article.appendChild(details);
    }
    list.appendChild(article);
  });
  (report.warnings || []).forEach((warning) => { const item = document.createElement("li"); item.textContent = warning; warnings.appendChild(item); });
}

function humanizeLogMessage(message) {
  const labels = {
    "task-started": "Tarefa iniciada",
    "task-output": "Etapa",
    "task-progress": "Progresso",
    "task-completed": "Tudo certo por aqui!",
    "task-cancelled": "Tudo bem, parei por aqui.",
    "task-error": "Opa! A tarefa encontrou um problema.",
    "restore-point-created": "Ponto de restauração criado",
    "restore-point-confirmation-required": "Só preciso confirmar uma coisinha",
    "snapshot-started": "Preparando um ponto de segurança",
    "restore-point-phase-started": "Criando ponto de restauração",
    "restore-point-phase-completed": "Ponto de restauração criado",
    "restore-point-phase-failed": "Não consegui criar o ponto de restauração",
    "registry-operation-failed": "Falha ao limpar o histórico do Registro",
    "task-executor-failed": "Não consegui terminar esta tarefa"
  };
  return String(message ?? "")
    .replace(/\[(task-started|task-output|task-progress|task-completed|task-cancelled|task-error|snapshot-started|restore-point-phase-started|restore-point-phase-completed|restore-point-phase-failed|registry-operation-failed|task-executor-failed)\]/gi,
      (match, key) => labels[key] || key)
    .replace(/status=exit code: 0/gi, "resultado: concluído")
    .replace(/exit code: 0/gi, "concluído");
}

function humanizeActivityMessage(message) {
  return String(message ?? "")
    .replace(/execuÃ§Ã£o/gi, "execução")
    .replace(/nÃ£o/gi, "não")
    .replace(/operaÃ§Ã£o/gi, "operação")
    .replace(/informaÃ§Ã£o/gi, "informação")
    .replace(/alteraÃ§Ãµes/gi, "alterações")
    .replace(/concluÃ­da/gi, "concluída")
    .replace(/concluÃ­do/gi, "concluído")
    .replace(/â€”/g, "—")
    .replace(/Deployment Image Servicing and Management tool/gi, "Ferramenta de Gerenciamento e Manutenção de Imagens do Windows (DISM)")
    .replace(/The restore operation completed successfully\.?/gi, "A operação de restauração foi concluída com sucesso.")
    .replace(/The operation completed successfully\.?/gi, "A operação foi concluída com sucesso.")
    .replace(/Beginning system scan\. This process will take some time\./gi, "Iniciando a verificação do sistema. Este processo pode levar algum tempo.")
    .replace(/Beginning verification phase of system scan\./gi, "Iniciando a etapa de verificação do sistema.")
    .replace(/Windows Resource Protection did not find any integrity violations\./gi, "A Proteção de Recursos do Windows não encontrou violações de integridade.")
    .replace(/Windows Resource Protection found corrupt files and successfully repaired them\./gi, "A Proteção de Recursos do Windows encontrou arquivos corrompidos e os reparou com sucesso.")
    .replace(/Windows Resource Protection found corrupt files but was unable to fix some of them\./gi, "A Proteção de Recursos do Windows encontrou arquivos corrompidos, mas não conseguiu reparar alguns.");
}

function appendConsole(message) {
  const line = humanizeActivityMessage(humanizeLogMessage(message)).trim();
  if (!line) return;
  const isDismProgress = /\[[= >-]*\s*\d+(?:[.,]\d+)?%\s*\]?/.test(line);
  const lines = consoleBuffer.split("\n");
  const last = lines[lines.length - 1] || "";
  const lastIsDismProgress = /\[[= >-]*\s*\d+(?:[.,]\d+)?%\s*\]?/.test(last);
  if (isDismProgress && lastIsDismProgress) {
    lines[lines.length - 1] = line;
    consoleBuffer = lines.join("\n");
  } else {
    consoleBuffer = (consoleBuffer + "\n" + line).trim();
  }
  renderConsole();
}
function writeStatus(text) {
  consoleBuffer = text;
  renderConsole();
}

function restoreConsoleHeight() {
  const output = document.getElementById("console-output");
  const handle = document.getElementById("console-resize-handle");
  if (!output || !handle) return;
  const minimum = 110;
  const maximum = () => Math.max(minimum, Math.floor(window.innerHeight * 0.6));
  const saveHeight = () => { try { localStorage.setItem("qboa-console-height", String(Math.round(output.getBoundingClientRect().height))); } catch (_) { /* Preferência opcional. */ } };
  const applyHeight = (height) => { const next = Math.max(minimum, Math.min(maximum(), Math.round(height))); output.style.height = next + "px"; handle.setAttribute("aria-valuenow", String(next)); handle.setAttribute("aria-valuemax", String(maximum())); };
  try { const savedHeight = Number(localStorage.getItem("qboa-console-height")); if (Number.isFinite(savedHeight) && savedHeight >= minimum) applyHeight(savedHeight); } catch (_) { /* Painel continua redimensionável sem persistência. */ }
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return; event.preventDefault();
    const startY = event.clientY; const startHeight = output.getBoundingClientRect().height; handle.setPointerCapture(event.pointerId);
    const onMove = (moveEvent) => { if (moveEvent.pointerId === event.pointerId) applyHeight(startHeight + startY - moveEvent.clientY); };
    const onEnd = (endEvent) => { if (endEvent.pointerId !== event.pointerId) return; handle.removeEventListener("pointermove", onMove); handle.removeEventListener("pointerup", onEnd); handle.removeEventListener("pointercancel", onEnd); saveHeight(); };
    handle.addEventListener("pointermove", onMove); handle.addEventListener("pointerup", onEnd); handle.addEventListener("pointercancel", onEnd);
  });
  handle.addEventListener("keydown", (event) => { const current = output.getBoundingClientRect().height; if (event.key === "ArrowDown") { event.preventDefault(); applyHeight(current + 16); saveHeight(); } else if (event.key === "ArrowUp") { event.preventDefault(); applyHeight(current - 16); saveHeight(); } else if (event.key === "Home") { event.preventDefault(); applyHeight(minimum); saveHeight(); } else if (event.key === "End") { event.preventDefault(); applyHeight(maximum()); saveHeight(); } });
  window.addEventListener("resize", () => applyHeight(output.getBoundingClientRect().height));
}
function renderConsole() {
  const out = document.getElementById("console-output");
  const content = [consoleBuffer, activityLabel ? `${activityLabel}${activityDots}` : ""]
    .filter(Boolean)
    .join("\n");
  const lines = content.split("\n");
  out.replaceChildren();
  lines.forEach((line, index) => {
    const item = document.createElement("span");
    item.className = "console-line";
    if (line === "RESUMO DA EXECUÇÃO") item.classList.add("service-summary-heading");
    else if (/^DESABILITADOS \(/.test(line)) item.classList.add("service-summary-disabled");
    else if (/^REATIVADOS \(/.test(line)) item.classList.add("service-summary-enabled");
    else if (/^ALTERADOS PARA AUTOMÁTICO \(/.test(line)) item.classList.add("service-summary-enabled");
    else if (/^ALTERADOS PARA MANUAL \(/.test(line)) item.classList.add("service-summary-manual");
    else if (/^  - /.test(line) && lines.slice(0, index).reverse().find((prior) => /^(DESABILITADOS|REATIVADOS|ALTERADOS PARA AUTOMÁTICO|ALTERADOS PARA MANUAL) \(/.test(prior))) {
      const section = lines.slice(0, index).reverse().find((prior) => /^(DESABILITADOS|REATIVADOS|ALTERADOS PARA AUTOMÁTICO|ALTERADOS PARA MANUAL) \(/.test(prior));
      item.classList.add(section.startsWith("DESABILITADOS") ? "service-summary-disabled" : section.startsWith("REATIVADOS") || section.startsWith("ALTERADOS PARA AUTOMÁTICO") ? "service-summary-enabled" : "service-summary-manual");
    }
    item.textContent = line;
    out.appendChild(item);
    if (index < lines.length - 1) out.appendChild(document.createTextNode("\n"));
  });
  out.scrollTop = out.scrollHeight;
  const copyButton = document.getElementById("copy-console-btn");
  if (copyButton) copyButton.disabled = !consoleBuffer.trim();
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

function isCleanupTask(taskId) {
  return [
    "limpeza_leve",
    "limpeza_media",
    "limpeza_pesada",
    "limpeza_windows_update",
    "limpeza_historico_navegadores",
    "desengordurar_telemetria",
    "desengordurar_tarefas",
    "desengordurar_telemetria_avancada"
  ].includes(taskId);
}

function showExecutionToast(status, title, message) {
  const toast = document.getElementById("execution-toast");
  toast.className = `execution-toast ${status}`;
  toast.replaceChildren();
  const heading = document.createElement("strong"); heading.textContent = title;
  const detail = document.createElement("span"); detail.textContent = message;
  toast.append(heading, detail); toast.hidden = false;
  if (executionToastTimer) window.clearTimeout(executionToastTimer);
  executionToastTimer = window.setTimeout(() => { toast.hidden = true; executionToastTimer = null; }, 6500);
}

function formatCleanupBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / (1024 ** index);
  return `${value.toLocaleString("pt-BR", { maximumFractionDigits: index >= 2 ? 2 : 1 })} ${units[index]}`;
}

function collectCleanupSummary() {
  const summary = { files: 0, bytes: 0, registry: 0, failures: 0 };
  executionOutput.forEach((raw) => {
    const line = String(raw ?? "");
    const metrics = line.match(/QBOA_METRICS\s+files=(\d+)\s+bytes=(\d+)\s+registry=(\d+)/i);
    if (metrics) {
      summary.files += Number(metrics[1]) || 0;
      summary.bytes += Number(metrics[2]) || 0;
      summary.registry += Number(metrics[3]) || 0;
    }
    const failureMatches = [...line.matchAll(/(\d+)\s+falha(?:s|\(s\))?/gi)];
    failureMatches.forEach((match) => { summary.failures += Number(match[1]) || 0; });
  });
  return summary;
}

function formatCleanupDuration() {
  if (!executionStartedAt) return "Não informado";
  const seconds = Math.max(0, Math.round((Date.now() - executionStartedAt) / 1000));
  if (seconds < 60) return `${seconds} s`;
  return `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

function showCleanupResult(taskId, status, detail = "") {
  const successful = status === "success"; const cancelled = status === "cancelled";
  const title = successful ? "Faxina concluída!" : cancelled ? "Tudo bem, parei por aqui." : "A faxina não terminou como esperado";
  const modal = document.getElementById("execution-summary-modal");
  const summary = collectCleanupSummary();
  document.getElementById("execution-summary-title").textContent = title;
  document.getElementById("execution-summary-task").textContent = taskDisplayName(taskId);
  document.getElementById("execution-summary-status").textContent = successful
    ? (summary.failures ? "A faxina terminou, mas alguns itens deram trabalho e não puderam ser tratados." : "Prontinho! A faxina foi concluída.")
    : cancelled ? "A faxina foi interrompida antes de terminar."
      : "Opa! Encontrei um problema e não consegui concluir a faxina.";
  document.getElementById("execution-summary-files").textContent = summary.files.toLocaleString("pt-BR");
  document.getElementById("execution-summary-registry").textContent = summary.registry.toLocaleString("pt-BR");
  document.getElementById("execution-summary-space").textContent = formatCleanupBytes(summary.bytes);
  document.getElementById("execution-summary-failures").textContent = summary.failures.toLocaleString("pt-BR");
  document.getElementById("execution-summary-duration").textContent = formatCleanupDuration();
  const statusIcon = document.getElementById("execution-summary-icon");
  statusIcon.textContent = successful ? "✓" : cancelled ? "Ⅱ" : "!";
  statusIcon.className = `execution-summary-icon ${successful ? "success" : cancelled ? "warning" : "error"}`;
  modal.classList.remove("hidden"); modal.setAttribute("aria-hidden", "false");
  setCleanupSessionPhase("result", taskDisplayName(taskId));
  showExecutionToast(successful ? "success" : cancelled ? "warning" : "error", title, taskDisplayName(taskId));
}

function closeExecutionSummary() {
  const modal = document.getElementById("execution-summary-modal");
  modal.classList.add("hidden"); modal.setAttribute("aria-hidden", "true");
  closeCleanupSession();
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

const PROGRESS_FLAVOR_BANK = {
  preparation: ["Preparando o terreno para a faxina.", "Reunindo os utensílios da limpeza digital.", "Conferindo se a vassoura está em condições de uso.", "Calculando o tamanho da bagunça.", "Organizando a equipe de desocupação de arquivos.", "Alinhando os baldes e conferindo os panos de microfibra.", "A equipe está discutindo quem vai cuidar dos cantinhos.", "Fazendo a vistoria antes de abrir a caixa de ferramentas.", "Preparando o detergente para a sujeira mais teimosa."],
  temporary: ["Desalojando arquivos que já perderam a validade.", "Removendo os entulhos da vida digital.", "Convencendo caches antigos a seguirem em frente.", "Passando a vassoura nos cantos esquecidos.", "Separando o lixo eletrônico do patrimônio histórico.", "Expulsando arquivos que já passaram da data de validade.", "Desocupando gavetas digitais esquecidas há séculos.", "Tirando o pó dos diretórios menos visitados.", "Dando destino correto à tralha temporária."],
  browser: ["Lavando os bastidores da navegação.", "Eliminando as pegadas digitais acumuladas.", "Desentupindo os corredores da internet.", "Dando banho nos caches mais encardidos.", "Removendo lembranças que ninguém pediu para guardar.", "Fazendo uma limpa nas lembranças do navegador.", "Tirando as migalhas digitais dos perfis.", "Desembaraçando o histórico da navegação.", "Lavando a louça que a internet deixou na pia."],
  dism: ["Examinando as estruturas do Windows.", "Procurando rachaduras na arquitetura do sistema.", "Consultando os engenheiros da Microsoft.", "Reorganizando os tijolos digitais.", "Aplicando massa corrida nos componentes do Windows.", "Avaliando a fundação do prédio antes da reforma.", "Chamando o mestre de obras dos componentes.", "Conferindo se os tijolos do Windows estão bem assentados.", "Fazendo perícia na estrutura do sistema."],
  resetbase: ["Removendo as últimas camadas de sujeira.", "Fazendo uma faxina sem direito a arrependimento.", "Desmontando o acúmulo histórico do Windows.", "Eliminando os vestígios das atualizações antigas.", "Passando QBoa nos componentes mais encardidos.", "Fechando a conta com as atualizações antigas.", "Compactando a história que o Windows não precisa mais carregar.", "Dando baixa no estoque de componentes substituídos.", "É a faxina pesada: sem borracha para desfazer depois."],
  sfc: ["Conferindo se todas as peças estão no lugar.", "Procurando componentes fora de conformidade.", "Verificando a certidão de nascimento dos arquivos.", "Investigando possíveis desvios de conduta.", "Conferindo se o Windows ainda tem salvação.", "Conferindo cada parafuso da instalação.", "Verificando se os arquivos do sistema estão em ordem.", "Chamando o controle de qualidade do Windows.", "Passando a régua nos arquivos protegidos."],
  blocked: ["Respeitando a privacidade dos arquivos ocupados.", "Alguns arquivos não quiseram sair de casa.", "Encontramos resistência. A negociação continua.", "Deixando os arquivos teimosos para a próxima.", "Não vamos arrombar a porta. Temos educação.", "Sem chute na porta: arquivo ocupado fica quietinho.", "Respeitando quem pediu mais cinco minutinhos.", "Tem arquivo agarrado no batente; vamos preservar.", "Nada de briga com processo em andamento."],
  diagnostic: ["Interrogando os componentes do equipamento.", "Medindo a temperatura das ideias.", "Procurando sinais de vida nos circuitos.", "Conferindo a saúde dos órgãos digitais.", "Verificando se o hardware está contando a verdade.", "O estetoscópio digital está trabalhando.", "Checando os sinais vitais da máquina.", "Investigando os bastidores do hardware.", "Conferindo se os componentes estão em plena forma."],
  finish: ["Recolhendo os materiais de limpeza.", "Conferindo se não esquecemos a vassoura.", "Finalizando os últimos detalhes da faxina.", "Guardando os produtos de limpeza.", "Registrando o antes e depois da operação.", "Última conferida antes de devolver a casa limpa.", "A equipe está recolhendo os últimos panos.", "Conferindo o resultado e fechando o expediente.", "Faxina quase entregue; só falta o laço no pacote."],
  qboa: ["Aplicando uma dose generosa de QBoa digital.", "Desinfetando os caches com QBoa.", "Alvejando os vestígios da bagunça com QBoa.", "QBoa aplicado. Agora é deixar agir.", "Passando QBoa nos cantos mais encardidos do Windows.", "QBoa na medida certa, sem desperdiçar produto.", "Um brilho a mais nos cantinhos digitais.", "A sujeira perdeu a discussão com o QBoa.", "QBoa trabalhando onde a vassoura não alcança."],
  tithe: ["Calculando o dízimo dos componentes.", "Separando 10% da sujeira para a igreja do Windows.", "Recolhendo a contribuição obrigatória do sistema.", "O dízimo foi pago. Podemos prosseguir.", "A bênção foi concedida. Prosseguindo com a faxina.", "O Windows já pode declarar a sujeira no imposto.", "A contribuição dos componentes foi devidamente recolhida.", "Mais uma taxa de ocupação cobrada dos arquivos antigos.", "O departamento fiscal da faxina está satisfeito."]
};
function progressFlavorCategory(taskId, stage) {
  const text = String(stage || "").toLowerCase();
  if (/resetbase|startcomponentcleanup/.test(text)) return "resetbase";
  if (/\bsfc\b|scannow|integridade/.test(text)) return "sfc";
  if (/\bdism\b|restorehealth|scanhealth|component store/.test(text)) return "dism";
  if (/bloquead|em uso|erro 32|ocupad/.test(text)) return "blocked";
  if (/navegador|browser|hist[oó]rico/.test(text) || taskId === "limpeza_historico_navegadores") return "browser";
  if (/diagn[oó]stic|teste de|mem[oó]ria|processador|\bgpu\b|armazenamento/.test(text) || String(taskId || "").includes("diagnostic")) return "diagnostic";
  if (/conclu|finaliz|encerr|resumo/.test(text)) return "finish";
  if (/tempor|cache|arquivo|pasta|limp|remov|exclu/.test(text) || isCleanupTask(taskId)) return "temporary";
  return "preparation";
}
function chooseProgressFlavor(taskId, stage) {
  let category = progressFlavorCategory(taskId, stage);
  if (taskId === "limpeza_pesada" && /resetbase|startcomponentcleanup/i.test(String(stage || "")) && Math.random() < 0.09) category = "tithe";
  else if (String(taskId || "").startsWith("limpeza_") && Math.random() < 0.035) category = "qboa";
  const options = PROGRESS_FLAVOR_BANK[category] || PROGRESS_FLAVOR_BANK.preparation;
  const previous = progressFlavorLastIndex.get(category);
  let index = Math.floor(Math.random() * options.length);
  if (options.length > 1 && index === previous) index = (index + 1 + Math.floor(Math.random() * (options.length - 1))) % options.length;
  progressFlavorLastIndex.set(category, index);
  return options[index];
}
function progressFlavorStageKey(taskId, stage) {
  const rawStage = String(stage || "preparacao");
  const category = progressFlavorCategory(taskId, rawStage);
  const step = rawStage.match(/\b(?:etapa|passo)\s*(\d+)\s*(?:\/|de)\s*(\d+)\b/i);
  const stableStage = rawStage
    .replace(/\b\d+(?:[.,]\d+)?\s*%/g, "")
    .replace(/\b(?:\d+(?:[.,]\d+)?\s*\/\s*\d+(?:[.,]\d+)?)\b/g, "")
    .replace(/\s+/g, " ").trim();
  // A descrição pode acompanhar cada avanço; a identidade da etapa não.
  const identity = step ? "step:" + step[1] + "/" + step[2] : stableStage;
  return String(taskId || "preparacao") + "|" + category + "|" + identity;
}
function scheduleProgressFlavorRotation() {
  if (progressFlavorTimer !== null) window.clearTimeout(progressFlavorTimer);
  if (cleanupSessionPhase !== "running" || !activeTaskId) {
    progressFlavorTimer = null;
    return;
  }
  progressFlavorTimer = window.setTimeout(() => {
    progressFlavorTimer = null;
    if (cleanupSessionPhase !== "running" || !activeTaskId) return;
    renderProgressFlavor(activeTaskId, currentOperationStage || "Preparando a faxina…", true);
  }, 9000);
}
function renderProgressFlavor(taskId, stage, forceRotation = false) {
  const flavor = document.getElementById("operation-progress-flavor");
  if (!flavor) return;
  const key = progressFlavorStageKey(taskId, stage);
  const stageChanged = key !== lastProgressFlavorKey;
  if (!stageChanged && !forceRotation) return;
  lastProgressFlavorKey = key;
  const phrase = chooseProgressFlavor(taskId, stage);
  const fragment = document.createDocumentFragment();
  Array.from(phrase).forEach((character, index) => {
    const letter = document.createElement("span");
    letter.className = "operation-progress-letter";
    letter.style.setProperty("--letter-index", String(Math.min(index, 24)));
    letter.textContent = character === " " ? "\u00a0" : character;
    fragment.appendChild(letter);
  });
  flavor.replaceChildren(fragment);
  flavor.classList.remove("is-changing");
  window.requestAnimationFrame(() => flavor.classList.add("is-changing"));
  scheduleProgressFlavorRotation();
}
function setTaskbarProgress(progress) {
  const currentWindow = window.__TAURI__?.window?.getCurrentWindow?.();
  if (!currentWindow?.setProgressBar) return;
  const state = Number.isFinite(progress) ? { status: "normal", progress: Math.max(0, Math.min(100, Math.round(progress))) } : { status: "indeterminate" };
  void currentWindow.setProgressBar(state).catch((error) => console.debug("Indicador da barra de tarefas indisponível:", error));
}
function clearTaskbarProgress() {
  const currentWindow = window.__TAURI__?.window?.getCurrentWindow?.();
  if (!currentWindow?.setProgressBar) return;
  void currentWindow.setProgressBar({ status: "none" }).catch((error) => console.debug("Não foi possível limpar o indicador da barra de tarefas:", error));
}
function updateOperationProgress(taskName, stage, progressValue) {
  const task = document.getElementById("operation-progress-task");
  const stageEl = document.getElementById("operation-progress-stage");
  const bar = document.getElementById("operation-progress-bar");
  const percentEl = document.getElementById("operation-progress-percent");
  if (!task || !stageEl || !bar || !percentEl) return;
  if (taskName) task.textContent = taskName;
  if (stage) stageEl.textContent = stage;
  renderProgressFlavor(activeTaskId, stage || currentOperationStage);
  const source = String(progressValue ?? stage ?? "");
  const match = source.match(/(?:^|\s)(\d{1,3}(?:[.,]\d+)?)\s*%/);
  if (match) {
    const receivedProgress = Math.max(0, Math.min(100, Number(match[1].replace(",", "."))));
    currentOperationProgress = currentOperationProgress === null
      ? receivedProgress
      : Math.max(currentOperationProgress, receivedProgress);
    bar.classList.add("determinate"); bar.style.width = currentOperationProgress + "%";
    percentEl.textContent = Math.round(currentOperationProgress) + "%";
    setTaskbarProgress(currentOperationProgress);
  } else if (currentOperationProgress !== null) {
    bar.classList.add("determinate"); bar.style.width = currentOperationProgress + "%";
    percentEl.textContent = Math.round(currentOperationProgress) + "%";
    setTaskbarProgress(currentOperationProgress);
  } else {
    bar.classList.remove("determinate"); bar.style.width = "";
    percentEl.textContent = "Em andamento";
    setTaskbarProgress(null);
  }
}
function showRunningOperationPopup(requestedName = "") {
  const modal = document.getElementById("running-operation-modal");
  document.getElementById("running-operation-task").textContent = currentOperationName || (activeTaskId ? taskDisplayName(activeTaskId) : "Uma operação está sendo iniciada.");
  document.getElementById("running-operation-stage").textContent = currentOperationStage || "O QBoa está processando a operação. Aguarde antes de iniciar outra tarefa.";
  document.getElementById("running-operation-requested").textContent = requestedName ? `Solicitação bloqueada: ${requestedName}.` : "Uma nova operação não será iniciada enquanto esta estiver ativa.";
  modal.classList.remove("hidden"); modal.setAttribute("aria-hidden", "false");
  document.getElementById("running-operation-close").focus();
}
function closeRunningOperationPopup() {
  const modal = document.getElementById("running-operation-modal");
  modal.classList.add("hidden"); modal.setAttribute("aria-hidden", "true");
}
function operationIsBusy(requestedName = "") {
  if (!(activeTaskId || launchPending)) return false;
  showRunningOperationPopup(requestedName); return true;
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
  if (isCleanupTask(item?.id)) setCleanupSessionPhase("preparing", item.name);
}

function closeRiskModal() {
  const modal = document.getElementById("risk-modal");
  modal.classList.add("hidden");
  modal.setAttribute("aria-hidden", "true");
  pendingTask = null;
  if (cleanupSessionPhase === "closed" || cleanupSessionPhase === "result" || cleanupSessionPhase === "ready") return;
  if (launchPending || (isCleanupTask(activeTaskId) && !cleanupExecutionStarted)) setCleanupSessionPhase("preparing");
  else if (activeTaskId) setCleanupSessionPhase("running");
  else closeCleanupSession();
}

function askConfirmation(message, title = "Vamos nessa?", confirmLabel = "Continuar", cancelLabel = "Cancelar") {
  if (cleanupSessionPhase !== "closed" || isCleanupTask(activeTaskId)) setCleanupSessionPhase("preparing", cleanupSessionTaskName || currentOperationName);
  const modal = document.getElementById("confirmation-modal");
  const titleElement = document.getElementById("confirmation-title");
  const messageElement = document.getElementById("confirmation-message");
  const confirmButton = document.getElementById("confirmation-confirm");
  const cancelButton = document.getElementById("confirmation-cancel");

  if (pendingConfirmationResolver) {
    pendingConfirmationResolver(false);
    pendingConfirmationResolver = null;
  }

  titleElement.textContent = title;
  messageElement.textContent = message;
  confirmButton.textContent = confirmLabel;
  cancelButton.textContent = cancelLabel;
  modal.classList.remove("hidden");
  modal.setAttribute("aria-hidden", "false");
  if (cleanupSessionPhase !== "closed" || isCleanupTask(activeTaskId)) setCleanupSessionPhase("preparing", cleanupSessionTaskName || currentOperationName);
  confirmButton.focus();

  return new Promise((resolve) => {
    pendingConfirmationResolver = resolve;
  });
}

function closeConfirmationModal(confirmed = false) {
  const modal = document.getElementById("confirmation-modal");
  if (modal.classList.contains("hidden")) return;

  modal.classList.add("hidden");
  modal.setAttribute("aria-hidden", "true");
  const resolve = pendingConfirmationResolver;
  pendingConfirmationResolver = null;
  if (resolve) resolve(confirmed);
  if (cleanupSessionPhase === "closed" || cleanupSessionPhase === "result") return;
  if (launchPending || (isCleanupTask(activeTaskId) && !cleanupExecutionStarted)) setCleanupSessionPhase("preparing");
  else if (activeTaskId) setCleanupSessionPhase("running");
  else closeCleanupSession();
}

async function prepareBrowserCleanupIfNeeded() {
  if (browserCleanupDecision !== null) return;
  try {
    const runningBrowsers = await Qboa.getRunningBrowsers();
    if (runningBrowsers.length) {
      const names = [...new Set(runningBrowsers)].map((name) => ({
        chrome: "Google Chrome", msedge: "Microsoft Edge", brave: "Brave", vivaldi: "Vivaldi", opera: "Opera", firefox: "Mozilla Firefox"
      }[name.toLowerCase()] || name)).join(", ");
      const closeRunning = await askConfirmation(
        `Estes navegadores estão abertos: ${names}.\n\nSe autorizar, o QBoa vai fechá-los antes de limpar os dados selecionados. Salve seu trabalho antes de continuar. Se recusar, os dados dos navegadores serão preservados.`,
        "Fechar navegadores abertos?",
        "Fechar Navegadores",
        "Não Fechar"
      );
      await Qboa.prepareBrowserCleanup(closeRunning);
      browserCleanupDecision = closeRunning;
      appendConsole(closeRunning
        ? "Navegadores autorizados para fechamento; a limpeza selecionada poderá continuar."
        : "Você optou por manter os navegadores abertos; os dados deles serão preservados.");
    } else {
      await Qboa.prepareBrowserCleanup(true);
      browserCleanupDecision = true;
    }
  } catch (error) {
    appendConsole(`Não foi possível preparar a limpeza dos navegadores: ${String(error)}`);
    await Qboa.prepareBrowserCleanup(false);
    browserCleanupDecision = false;
  }
}


async function executeTask(item, cardEl) {
  const cleanup = isCleanupTask(item?.id);
  if (cleanup) {
    cleanupExecutionStarted = false;
    setCleanupSessionPhase("preparing", item.name);
  }
  if (!eventListenersReady) {
    appendConsole("Não foi possível iniciar a operação porque o acompanhamento em tempo real não está disponível.");
    if (cleanup) closeCleanupSession();
    return false;
  }
  if (operationIsBusy(item?.name)) return false;

  launchPending = true;
  if (cleanup) activeTaskId = item.id;
  currentOperationProgress = null;
  lastProgressFlavorKey = "";
  currentOperationName = item.name;
  currentOperationStage = "Preparando a operação…";
  syncExecutionControls();

  if (cleanup) {
    if (["limpeza_leve", "limpeza_media", "limpeza_pesada", "limpeza_historico_navegadores"].includes(item.id)) {
      await prepareBrowserCleanupIfNeeded();
    }
    let resetBaseAuthorized = false;
    let restorePointAuthorized = false;
    let preparation;
    try {
      preparation = await Qboa.prepareCleanup(item.id);
    } catch (error) {
      appendConsole("Não foi possível preparar a proteção do sistema. A operação não será liberada: " + String(error));
      activeTaskId = null; activeExecutionId = null; launchPending = false;
      currentOperationName = ""; currentOperationStage = ""; browserCleanupDecision = null;
      closeCleanupSession();
      return false;
    }

    if (preparation.restore_point_error) {
      appendConsole("Detalhes técnicos da falha ao criar o ponto de restauração: " + preparation.restore_point_error);
      restorePointAuthorized = await askConfirmation(
        "Não foi possível criar um ponto de restauração porque um serviço necessário do Windows está desabilitado ou indisponível.\n\nSem essa proteção, você não poderá reverter as alterações desta operação pela Restauração do Sistema.\n\nDeseja continuar mesmo assim?",
        "Ponto de restauração indisponível",
        "Prosseguir sem proteção",
        "Cancelar operação"
      );
      if (!restorePointAuthorized) {
        try { await Qboa.cancelCleanupPreparation(preparation.token); } catch (error) {
          appendConsole("Não foi possível descartar a preparação: " + String(error));
        }
        appendConsole("Operação cancelada na preparação: o ponto de restauração não foi criado e nenhuma alteração começou.");
        activeTaskId = null; activeExecutionId = null; launchPending = false;
        currentOperationName = ""; currentOperationStage = ""; browserCleanupDecision = null;
        closeCleanupSession();
        return false;
      }
      appendConsole("Falha no ponto de restauração autorizada: a operação poderá continuar sem essa proteção.");
    } else if (preparation.restore_point_created) {
      appendConsole("Ponto de restauração criado antes do início da operação.");
    }

    if (preparation.reset_base_required) {
      resetBaseAuthorized = await askConfirmation(
        "A limpeza pesada executará DISM /StartComponentCleanup /ResetBase. Depois disso, não será possível desinstalar as atualizações do Windows que já estão instaladas. Se você não autorizar, a limpeza pesada não começará.",
        "Operação irreversível: DISM /ResetBase",
        "Sim, executar /ResetBase",
        "Cancelar limpeza pesada"
      );
      if (!resetBaseAuthorized) {
        try { await Qboa.cancelCleanupPreparation(preparation.token); } catch (error) {
          appendConsole("Não foi possível descartar a preparação: " + String(error));
        }
        appendConsole("Limpeza pesada cancelada na preparação: /ResetBase não autorizado. Nenhuma etapa de limpeza foi iniciada.");
        activeTaskId = null; activeExecutionId = null; launchPending = false;
        currentOperationName = ""; currentOperationStage = ""; browserCleanupDecision = null;
        closeCleanupSession();
        return false;
      }
    } else if (item.id === "limpeza_pesada") {
      appendConsole("Compatibilidade: ambiente restritivo ou não classificável; /ResetBase será omitido para preservar a reversibilidade.");
    }
    preparedCleanupTask = { item, cardEl, resetBaseAuthorized, preparationToken: preparation.token, restorePointAuthorized };
    launchPending = false;
    currentOperationStage = "Aguardando autorização para iniciar.";
    setCleanupSessionPhase("ready", item.name);
    syncExecutionControls();
    appendConsole("Preparação concluída. Aguardando o comando Iniciar.");
    return true;
  }

  if (cardEl) cardEl.classList.add("running");
  activeTaskId = item.id;
  currentOperationName = item.name;
  currentOperationStage = "Solicitando início ao mecanismo…";
  updateOperationProgress(currentOperationName, currentOperationStage, null);
  writeStatus(`> Bora cuidar de "${item.name}"...`);
  startActivity(item.name);
  closeRiskModal();
  const shutdownOnComplete = document.getElementById("shutdown-on-complete").checked;

  try {
    const result = await Qboa.runTask(item.id, shutdownOnComplete, false);
    if (activeTaskId === item.id) {
      activeExecutionId = result.execution_id || activeExecutionId;
      launchPending = false;
      currentOperationStage = "Mãos à obra! Aguardando o andamento da tarefa…";
      syncExecutionControls();
    }
    return true;
  } catch (err) {
    stopActivity();
    appendConsole(`Erro ao chamar o backend: ${err}`);
    if (cardEl) cardEl.classList.remove("running");
    activeTaskId = null;
    activeExecutionId = null;
    launchPending = false;
    currentOperationName = "";
    currentOperationStage = "";
    syncExecutionControls();
    return false;
  }
}

async function startPreparedCleanup() {
  const prepared = preparedCleanupTask;
  if (!prepared || cleanupSessionPhase !== "ready" || !activeTaskId) return false;
  const {
    item,
    cardEl,
    resetBaseAuthorized = false,
    preparationToken,
    restorePointAuthorized = false
  } = prepared;
  preparedCleanupTask = null;
  launchPending = true;
  cleanupExecutionStarted = false;
  currentOperationName = item.name;
  currentOperationStage = "Solicitando início ao mecanismo…";
  setCleanupSessionPhase("starting", item.name);
  syncExecutionControls();
  if (cardEl) cardEl.classList.add("running");
  updateOperationProgress(item.name, currentOperationStage, null);
  writeStatus(`> Bora cuidar de "${item.name}"...`);
  startActivity(item.name);
  const shutdownOnComplete = document.getElementById("shutdown-on-complete").checked;

  try {
    const result = await Qboa.runTask(
      item.id,
      shutdownOnComplete,
      resetBaseAuthorized,
      preparationToken,
      restorePointAuthorized
    );
    if (activeTaskId === item.id) {
      activeExecutionId = result.execution_id || activeExecutionId;
      launchPending = false;
      currentOperationStage = "Mãos à obra! Aguardando o andamento da tarefa…";
      syncExecutionControls();
    }
    return true;
  } catch (error) {
    stopActivity();
    browserCleanupDecision = null;
    appendConsole("Erro ao chamar o backend: " + String(error));
    if (cardEl) cardEl.classList.remove("running");
    activeTaskId = null;
    activeExecutionId = null;
    launchPending = false;
    currentOperationName = "";
    currentOperationStage = "";
    cleanupExecutionStarted = false;
    syncExecutionControls();
    closeCleanupSession();
    return false;
  }
}

async function cancelPreparedCleanup() {
  const prepared = preparedCleanupTask;
  if (!prepared || cleanupSessionPhase !== "ready") return;
  preparedCleanupTask = null;
  try {
    await Qboa.cancelCleanupPreparation(prepared.preparationToken);
  } catch (error) {
    appendConsole("Não foi possível descartar a preparação cancelada: " + String(error));
  }
  if (prepared.cardEl) prepared.cardEl.classList.remove("running");
  browserCleanupDecision = null;
  activeTaskId = null;
  activeExecutionId = null;
  launchPending = false;
  currentOperationName = "";
  currentOperationStage = "";
  currentOperationProgress = null;
  cleanupExecutionStarted = false;
  appendConsole("Preparação cancelada. Nenhuma etapa de limpeza foi iniciada.");
  closeCleanupSession();
}

async function runTask(item, cardEl) {
  if (operationIsBusy(item?.name)) return;
  if (requiresConfirmation(item)) {
    showRiskModal(item, cardEl);
    return;
  }

  await executeTask(item, cardEl);
}

function rollbackStatusLabel(status) {
  return {
    Pending: "Pendente",
    Applied: "Aplicada",
    RollbackAvailable: "Reversão disponível",
    RollingBack: "Revertendo",
    RolledBack: "Revertida",
    RollbackFailed: "Falha na reversão",
    RollbackPartial: "Reversão parcial",
    Unknown: "Desconhecido",
    Irreversible: "Irreversível"
  }[status] || status;
}

async function refreshHistory() {
  try {
    const history = await Qboa.getSessionHistory();
    const list = document.getElementById("session-history");
    if (!list) return;
    list.innerHTML = "";
    const heading = list.parentElement?.querySelector("h2, h3, h4, .section-title");
    if (heading && /Alterações desta sessão/i.test(heading.textContent)) heading.textContent = "Últimas Alterações";
    const grouped = new Map();
    (history || []).forEach((entry, index) => {
      const rawDescription = String(entry.description || "Alteração registrada");
      const status = rollbackStatusLabel(entry.rollback_status);
      const irreversible = /irreversível/i.test(rawDescription) || /irreversível/i.test(status);
      const description = rawDescription.replace(/\s*\(irreversível\)/gi, "").trim();
      const key = description + "\u0000" + (irreversible ? "Irreversível" : status);
      const existing = grouped.get(key);
      if (existing) {
        existing.count += 1;
        existing.latestIndex = index;
      } else {
        grouped.set(key, { description, status: irreversible ? "Irreversível" : status, irreversible, count: 1, latestIndex: index });
      }
    });
    const entries = Array.from(grouped.values()).sort((a, b) => b.latestIndex - a.latestIndex);
    const visible = showAllSessionChanges ? entries : entries.slice(0, 3);
    if (!visible.length) {
      const empty = document.createElement("li");
      empty.textContent = "Nenhuma alteração registrada.";
      list.appendChild(empty);
    } else {
      visible.forEach((entry) => {
        const item = document.createElement("li");
        const irreversible = /irreversível/i.test(entry.description) || /irreversível/i.test(entry.status);
        item.textContent = entry.description + (irreversible ? " (irreversível)" : "") + (entry.count > 1 ? " (" + entry.count + ")" : "") + (irreversible ? "" : " (" + entry.status + ")");
        list.appendChild(item);
      });
    }
    let toggle = document.getElementById("session-history-toggle");
    if (!toggle) {
      toggle = document.createElement("button");
      toggle.id = "session-history-toggle";
      toggle.type = "button";
      toggle.className = "secondary-button";
      list.insertAdjacentElement("afterend", toggle);
      toggle.addEventListener("click", () => { showAllSessionChanges = !showAllSessionChanges; void refreshHistory(); });
    }
    toggle.textContent = showAllSessionChanges ? "Mostrar menos" : "Ver tudo";
    toggle.hidden = entries.length <= 3;
  } catch (error) {
    console.error("Não foi possível ler o histórico da sessão.", error);
  }
}
function renderServiceProfilePreview(preview) {
  currentServiceProfilePreview = preview;
  const items = document.getElementById("service-profile-preview-list");
  const ignored = document.getElementById("service-profile-ignored-list");
  items.replaceChildren(); ignored.replaceChildren();
  document.getElementById("service-profile-preview-status").textContent = preview.message;
  const canApply = Boolean(serviceProfileState?.system?.supported) &&
    Boolean(serviceProfileState?.profile?.validated || serviceProfileState?.profile?.heuristic_apply_enabled) &&
    Boolean(preview.applicable);
  const modal = document.getElementById("service-profile-preview-modal");
  modal.classList.remove("hidden"); modal.setAttribute("aria-hidden", "false");
  const changes = (preview.services || []).filter((service) => {
    const current = String(service.current || "").trim().toLowerCase();
    const proposed = String(service.proposed || "").trim().toLowerCase();
    return proposed && current && proposed !== current && !service.ignored && !service.incompatible;
  });
  editedServiceProfileChanges = [];
  changes.forEach((service) => {
    const current = String(service.current || "").trim().toLowerCase();
    const proposed = String(service.proposed || "").trim().toLowerCase();
    const row = document.createElement("li");
    row.className = "service-profile-preview-row" + (proposed === "disabled" ? " preview-disable" : current === "disabled" ? " preview-enable" : "");
    const include = document.createElement("input"); include.type = "checkbox"; include.checked = true;
    include.setAttribute("aria-label", `Incluir alteração de ${service.service}`);
    const copy = document.createElement("div"); copy.className = "service-preview-copy";
    const name = document.createElement("strong"); name.className = "service-preview-name"; name.textContent = service.service;
    copy.appendChild(name);
    const descriptionText = serviceDescription(service.service);
    if (descriptionText) {
      const description = document.createElement("span");
      description.className = "service-preview-description";
      description.textContent = ` (${descriptionText})`;
      copy.appendChild(description);
    }
    const transition = document.createElement("span"); transition.className = "service-preview-transition";
    transition.textContent = `${serviceStartTypeLabel(service.current)} → ${serviceStartTypeLabel(proposed)}`; copy.appendChild(transition);
    const target = document.createElement("select"); target.setAttribute("aria-label", `Novo tipo de inicialização para ${service.service}`);
    [["automatic","Automático"],["manual","Manual"],["disabled","Desativado"]].forEach(([value,label]) => { const option=document.createElement("option"); option.value=value; option.textContent=label; target.appendChild(option); });
    target.value = proposed;
    const update = () => {
      const index = editedServiceProfileChanges.findIndex((item) => item.service.toLowerCase() === service.service.toLowerCase());
      if (include.checked) {
        const entry = { service: service.service, start_type: target.value };
        if (index >= 0) editedServiceProfileChanges[index] = entry; else editedServiceProfileChanges.push(entry);
      } else if (index >= 0) editedServiceProfileChanges.splice(index,1);
      target.disabled = !include.checked; row.style.opacity = include.checked ? "1" : ".5";
      transition.textContent = `${serviceStartTypeLabel(service.current)} → ${serviceStartTypeLabel(target.value)}${include.checked ? "" : " (não será aplicado)"}`;
      document.getElementById("service-profile-preview-count").textContent = `${editedServiceProfileChanges.length} alteração(ões) selecionada(s) de ${changes.length}`;
      document.getElementById("service-profile-modal-apply").disabled = !canApply || !editedServiceProfileChanges.length;
    };
    editedServiceProfileChanges.push({service:service.service,start_type:proposed});
    include.addEventListener("change", update); target.addEventListener("change", update);
    row.append(include,copy,target); items.appendChild(row);
  });
  if (!changes.length) { const empty=document.createElement("li"); empty.className="service-profile-preview-empty"; empty.textContent="Este perfil não propõe alterações aplicáveis neste sistema."; items.appendChild(empty); }
  const ignoredServices=(preview.services||[]).filter((service)=>service.ignored||service.incompatible||!service.proposed);
  ignoredServices.forEach((service)=>{const item=document.createElement("li");item.textContent=`${service.service}${service.incompatible ? " — "+service.incompatible : ""}`;ignored.appendChild(item);});
  if(!ignoredServices.length){const item=document.createElement("li");item.textContent="Nenhum serviço ignorado.";ignored.appendChild(item);}
  document.getElementById("service-profile-preview-count").textContent = `${editedServiceProfileChanges.length} alteração(ões) selecionada(s) de ${changes.length}`;
  document.getElementById("service-profile-modal-apply").disabled = !canApply || !editedServiceProfileChanges.length;
  document.getElementById("service-profile-apply-btn").disabled = !canApply || !changes.length;
  document.getElementById("service-profile-preview-close").focus();
}
function closeServiceProfilePreview() {
  const modal=document.getElementById("service-profile-preview-modal");
  modal.classList.add("hidden"); modal.setAttribute("aria-hidden","true");
}

function diagnosticCategoryLabel(category) {
  return {
    sistema: "Sistema",
    armazenamento: "Armazenamento",
    memoria_ram: "Memória RAM",
    placa_de_video: "Placa de vídeo",
    processador: "Processador"
  }[category] || category;
}

function diagnosticStatusLabel(status) {
  return {
    ok: "ok",
    atencao: "atenção",
    falha: "falha",
    indisponivel: "indisponível",
    informativo: "informativo"
  }[status] || "estado desconhecido";
}

function renderDiagnosticReport(report, target = null) {
  const list = document.getElementById("diagnostic-results");
  list.replaceChildren();
  report.results.filter((result) => !target || result.category === target).forEach((result) => {
    const item = document.createElement("li");
    const heading = document.createElement("strong");
    heading.textContent = `${diagnosticCategoryLabel(result.category)} · ${diagnosticStatusLabel(result.status)}`;
    const evidence = document.createElement("p");
    evidence.textContent = result.evidence.join(" · ");
    const source = document.createElement("small");
    source.textContent = `Origem: ${result.source.join(", ")}`;
    item.append(heading, evidence, source);
    list.appendChild(item);
  });
  if (!target) {
    (report.checks || []).forEach((check) => {
      const item = document.createElement("li");
      const heading = document.createElement("strong");
      heading.textContent = `Verificação · ${check.name} · ${diagnosticStatusLabel(check.status)}`;
      const evidence = document.createElement("p");
      evidence.textContent = check.evidence;
      const source = document.createElement("small");
      source.textContent = `Origem: ${check.source}`;
      item.append(heading, evidence, source);
      list.appendChild(item);
    });
  }
}


function storageMeasurementLabel(kind) {
  const labels = {
    seq_small_write: "Gravação sequencial · 128 KiB · fila 1",
    seq_small_read: "Leitura sequencial · 128 KiB · fila 1",
    seq_large_write: "Gravação sequencial · 1 MiB · fila 8",
    seq_large_read: "Leitura sequencial · 1 MiB · fila 8",
    random_small_write: "Gravação aleatória · 4 KiB · fila 1",
    random_small_read: "Leitura aleatória · 4 KiB · fila 1",
    random_large_write: "Gravação aleatória · 4 KiB · fila 32",
    random_large_read: "Leitura aleatória · 4 KiB · fila 32"
  };
  return labels[kind] || "Medição de armazenamento";
}

function prepareStorageBenchmarkPlaceholder() {
  const list = document.getElementById("diagnostic-results");
  const entry = document.createElement("li");
  entry.className = "storage-live-benchmark storage-live-placeholder";
  const heading = document.createElement("div");
  heading.className = "storage-live-heading";
  const copy = document.createElement("div");
  const title = document.createElement("strong");
  title.textContent = "Benchmark em tempo real";
  const subtitle = document.createElement("span");
  subtitle.textContent = "Preparando a primeira medição…";
  copy.append(title, subtitle);
  const status = document.createElement("span");
  status.className = "storage-live-status";
  status.textContent = "Aguardando";
  heading.append(copy, status);
  const waiting = document.createElement("div");
  waiting.className = "storage-live-waiting";
  waiting.setAttribute("role", "status");
  waiting.textContent = "Aguardando dados iniciais…";
  entry.append(heading, waiting);
  list.appendChild(entry);
  storageBenchmarkPlaceholder = entry;
  return entry;
}

function renderStorageBenchmarkProgress(payload) {
  if (!payload || !payload.kind) return;
  const list = document.getElementById("diagnostic-results");
  const random = String(payload.kind).startsWith("random_");
  const primaryKey = random ? "iops" : "mb_per_second";
  const primaryUnit = random ? "IOPS" : "MB/s";
  const secondaryKey = random ? "mb_per_second" : "iops";
  const secondaryUnit = random ? "MB/s" : "IOPS";
  const identity = String(payload.disk_id || "") + ":" + payload.kind;

  if (!activeStorageBenchmark) {
    const entry = storageBenchmarkPlaceholder || document.createElement("li");
    entry.className = "storage-live-benchmark";
    entry.classList.add("qboa-activity-target");
    entry.dataset.activity = "working";
    for (const [reason, target] of diagnosticActivityTargets) {
      if (reason.startsWith("run:") && reason.endsWith(":armazenamento")) {
        diagnosticActivityTargets.set(reason, entry);
        refreshDiagnosticActivityTarget(target);
        refreshDiagnosticActivityTarget(entry);
      }
    }
    const heading = document.createElement("div");
    heading.className = "storage-live-heading";
    const copy = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = "Benchmark em tempo real";
    const subtitle = document.createElement("span");
    subtitle.textContent = (payload.disk_id || "Unidade") + " · " + storageMeasurementLabel(payload.kind);
    copy.append(title, subtitle);
    const status = document.createElement("span");
    status.className = "storage-live-status";
    status.textContent = "Medindo";
    heading.append(copy, status);

    const metrics = document.createElement("div");
    metrics.className = "storage-live-metrics";
    const primary = document.createElement("div");
    primary.className = "storage-live-primary";
    const primaryLabel = document.createElement("span");
    primaryLabel.textContent = "Velocidade atual · " + primaryUnit;
    const primaryValue = document.createElement("strong");
    primaryValue.textContent = "—";
    primary.append(primaryLabel, primaryValue);
    const secondary = document.createElement("div");
    secondary.className = "storage-live-secondary";
    const secondaryLabel = document.createElement("span");
    secondaryLabel.textContent = "Métrica complementar · " + secondaryUnit;
    const secondaryValue = document.createElement("strong");
    secondaryValue.textContent = "—";
    secondary.append(secondaryLabel, secondaryValue);
    metrics.append(primary, secondary);

    const chart = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    chart.setAttribute("viewBox", "0 0 560 140");
    chart.setAttribute("preserveAspectRatio", "none");
    chart.setAttribute("role", "img");
    chart.setAttribute("aria-label", "Variação da velocidade durante a medição");
    for (let index = 1; index <= 3; index++) {
      const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
      const y = index * 35;
      line.setAttribute("x1", "0");
      line.setAttribute("x2", "560");
      line.setAttribute("y1", String(y));
      line.setAttribute("y2", String(y));
      line.setAttribute("class", "storage-live-gridline");
      chart.appendChild(line);
    }
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("class", "storage-live-line");
    chart.appendChild(path);
    const footer = document.createElement("div");
    footer.className = "storage-live-footer";
    const elapsed = document.createElement("span");
    const transferred = document.createElement("span");
    footer.append(elapsed, transferred);
    // O mesmo <li> reservado no estado inicial vira o gráfico real.
    // Substituir os filhos remove o cabeçalho e a mensagem de espera antigos.
    entry.replaceChildren(heading, metrics, chart, footer);
    if (entry.parentElement !== list) list.appendChild(entry);
    storageBenchmarkPlaceholder = null;
    activeStorageBenchmark = {
      identity, entry, path, primaryValue, secondaryValue, primaryLabel, secondaryLabel,
      subtitle, elapsed, transferred, samples: [], primaryKey
    };
  } else if (activeStorageBenchmark.identity !== identity) {
    activeStorageBenchmark.identity = identity;
    activeStorageBenchmark.primaryKey = primaryKey;
    activeStorageBenchmark.samples = [];
    activeStorageBenchmark.subtitle.textContent =
      (payload.disk_id || "Unidade") + " · " + storageMeasurementLabel(payload.kind);
    activeStorageBenchmark.primaryLabel.textContent = "Velocidade atual · " + primaryUnit;
    activeStorageBenchmark.secondaryLabel.textContent = "Métrica complementar · " + secondaryUnit;
    activeStorageBenchmark.primaryValue.textContent = "—";
    activeStorageBenchmark.secondaryValue.textContent = "—";
    activeStorageBenchmark.elapsed.textContent = "";
    activeStorageBenchmark.transferred.textContent = "";
    activeStorageBenchmark.path.setAttribute("d", "");
    activeStorageBenchmark.entry.querySelector(".storage-live-status").textContent = "Medindo";
    list.appendChild(activeStorageBenchmark.entry);
  }

  const state = activeStorageBenchmark;
  const sample = {
    elapsed_ms: Number(payload.elapsed_ms) || 0,
    bytes_transferred: Number(payload.bytes_transferred) || 0,
    operations: Number(payload.operations) || 0,
    mb_per_second: Number(payload.mb_per_second) || 0,
    iops: Number(payload.iops) || 0
  };
  state.samples.push(sample);
  if (state.samples.length > 80) state.samples.shift();
  const format = (value) => Number(value).toLocaleString("pt-BR", { maximumFractionDigits: 2 });
  state.primaryValue.textContent = format(sample[primaryKey]) + " " + primaryUnit;
  state.secondaryValue.textContent = format(sample[secondaryKey]) + " " + secondaryUnit;
  state.elapsed.textContent = (sample.elapsed_ms / 1000).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " s";
  state.transferred.textContent = (sample.bytes_transferred / (1024 ** 2)).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) +
    " MiB transferidos · " + format(sample.operations) + " operações";

  const maximum = Math.max(1, ...state.samples.map((point) => point[primaryKey]));
  const points = state.samples.map((point, index) => {
    const x = state.samples.length < 2 ? 0 : index * 560 / (state.samples.length - 1);
    const y = 126 - (point[primaryKey] / maximum) * 112;
    return (index === 0 ? "M" : "L") + x.toFixed(1) + " " + y.toFixed(1);
  }).join(" ");
  state.path.setAttribute("d", points);
  state.path.setAttribute("aria-label", "Última amostra: " + format(sample[primaryKey]) + " " + primaryUnit);
  const now = Date.now();
  if (now - lastStorageActivityLogAt >= 1000) {
    lastStorageActivityLogAt = now;
    appendConsole("Armazenamento · " + storageMeasurementLabel(payload.kind) + ": " +
      format(sample[primaryKey]) + " " + primaryUnit + " · " +
      format(sample[secondaryKey]) + " " + secondaryUnit);
  }
}

function appendStorageSparkline(card, samples, primaryKey) {
  if (!Array.isArray(samples) || samples.length < 2) return;
  const valid = samples.filter((sample) => Number.isFinite(Number(sample?.[primaryKey])));
  if (valid.length < 2) return;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 240 48");
  svg.setAttribute("preserveAspectRatio", "none");
  svg.setAttribute("class", "storage-measurement-sparkline");
  const maximum = Math.max(1, ...valid.map((sample) => Number(sample[primaryKey])));
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", valid.map((sample, index) => {
    const x = index * 240 / (valid.length - 1);
    const y = 44 - (Number(sample[primaryKey]) / maximum) * 38;
    return (index === 0 ? "M" : "L") + x.toFixed(1) + " " + y.toFixed(1);
  }).join(" "));
  svg.appendChild(path);
  card.appendChild(svg);
}

function renderStorageReport(report, append = false) {
  const list = document.getElementById("diagnostic-results");
  if (!append) list.replaceChildren();

  const fmt = (value, digits = 2) => value === null || value === undefined || !Number.isFinite(Number(value))
    ? "Indisponível"
    : Number(value).toLocaleString("pt-BR", { maximumFractionDigits: digits });
  const bytes = (value) => value === null || value === undefined || !Number.isFinite(Number(value)) || Number(value) < 0
    ? "Indisponível"
    : (Number(value) / (1024 ** 3)).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " GiB";
  const smartValue = (value, suffix = "") => value === null || value === undefined
    ? "Não informado" : fmt(value) + suffix;
  const smartSectorValue = (field, value) => {
    if (value !== null && value !== undefined) return fmt(value);
    const status = report.smart?.collection_status?.sector_attributes?.[field];
    return ({
      provider_unavailable: "Provedor não expôs",
      collection_error: "Falha na coleta",
      parse_error: "Falha ao interpretar",
      attribute_not_reported: "Atributo não reportado"
    })[status] || "Não informado";
  };
  const classNames = {
    notebook_hdd: "HD de notebook",
    desktop_hdd: "HD de desktop",
    sata_ssd: "SSD",
    nvme: "SSD",
    usb: "Armazenamento removível",
    unknown: "Classe não identificada"
  };
  const status = {
    completed: ["Concluído", "success"],
    partial: ["Parcial", "warning"],
    failed: ["Falhou", "danger"],
    cancelled: ["Cancelado", "neutral"],
    simulated: ["Simulado", "neutral"]
  }[report.execution_status] || ["Estado desconhecido", "neutral"];

  const entry = document.createElement("li");
  entry.className = "storage-report-entry";
  const header = document.createElement("header");
  header.className = "storage-report-heading";
  const headingCopy = document.createElement("div");
  const title = document.createElement("h3");
  title.textContent = report.disk_name || report.disk_id || "Unidade de armazenamento";
  const subtitle = document.createElement("p");
  subtitle.textContent = (report.volume || "Volume indisponível") + " · " +
    (classNames[report.class] || classNames.unknown) + " · " + (report.bus_type || "Barramento não identificado");
  headingCopy.append(title, subtitle);
  const statusBadge = document.createElement("span");
  statusBadge.className = "storage-status-badge " + status[1];
  statusBadge.textContent = status[0];
  header.append(headingCopy, statusBadge);
  entry.appendChild(header);
  if (activeStorageBenchmark &&
      activeStorageBenchmark.identity.startsWith(String(report.disk_id || "") + ":")) {
    const liveStatus = activeStorageBenchmark.entry.querySelector(".storage-live-status");
    if (liveStatus) liveStatus.textContent = "Medição concluída";
  }

  const deviceFacts = document.createElement("div");
  deviceFacts.className = "storage-device-facts";
  [
    ["Disco físico", bytes(report.capacity_bytes)],
    ["Volume " + (report.volume || ""), bytes(report.volume_capacity_bytes)],
    ["Espaço livre", bytes(report.free_bytes)]
  ].forEach(([label, value]) => {
    const fact = document.createElement("div");
    fact.className = "storage-device-fact";
    const name = document.createElement("span");
    name.textContent = label;
    const amount = document.createElement("strong");
    amount.textContent = value;
    fact.append(name, amount);
    deviceFacts.appendChild(fact);
  });
  entry.appendChild(deviceFacts);

  if (report.backend === "fake" || report.execution_status === "simulated") {
    const simulated = document.createElement("p");
    simulated.className = "storage-report-notice warning";
    simulated.textContent = "Modo simulado: valores fictícios para validar a interface; nenhum hardware foi medido.";
    entry.appendChild(simulated);
  }

  function createSection(titleText, descriptionText = "") {
    const section = document.createElement("section");
    section.className = "storage-report-section";
    const sectionHeading = document.createElement("div");
    sectionHeading.className = "storage-section-heading";
    const sectionTitle = document.createElement("h4");
    sectionTitle.textContent = titleText;
    sectionHeading.appendChild(sectionTitle);
    if (descriptionText) {
      const description = document.createElement("p");
      description.textContent = descriptionText;
      sectionHeading.appendChild(description);
    }
    section.appendChild(sectionHeading);
    entry.appendChild(section);
    return section;
  }

  const measurements = Array.isArray(report.measurements) ? report.measurements : [];
  const measurementSpecs = [
    { group: "Desempenho sequencial", description: "Transferência de blocos maiores; útil para observar leitura e gravação de arquivos.", kinds: [
      ["seq_large_read", "Leitura · 1 MiB · fila 8"],
      ["seq_large_write", "Gravação · 1 MiB · fila 8"],
      ["seq_small_read", "Leitura · 128 KiB · fila 1"],
      ["seq_small_write", "Gravação · 128 KiB · fila 1"]
    ] },
    { group: "Desempenho aleatório", description: "Acessos em blocos de 4 KiB; úteis para observar operações pequenas e concorrentes.", kinds: [
      ["random_small_read", "Leitura · fila 1"],
      ["random_small_write", "Gravação · fila 1"],
      ["random_large_read", "Leitura · fila 32"],
      ["random_large_write", "Gravação · fila 32"]
    ] }
  ];
  const threshold = { notebook_hdd: 60, desktop_hdd: 120, sata_ssd: 400, nvme: 1500 };

  measurementSpecs.forEach((group) => {
    const section = createSection(group.group, group.description);
    const grid = document.createElement("div");
    grid.className = "storage-measurement-grid";
    group.kinds.forEach(([kind, label]) => {
      const measurement = measurements.find((item) => item.kind === kind);
      const card = document.createElement("article");
      card.className = "storage-measurement-card";
      const name = document.createElement("span");
      name.textContent = label;
      const value = document.createElement("strong");
      const valid = measurement && measurement.value !== null && measurement.value !== undefined &&
        Number.isFinite(Number(measurement.value));
      value.textContent = valid ? fmt(measurement.value) : "Indisponível";
      const unit = document.createElement("small");
      unit.textContent = valid ? (measurement.unit || "") : (measurement?.reason || "Medição não retornada.");
      card.append(name, value, unit);
      if (valid && measurement) {
        const seconds = Number(measurement.duration_ms) / 1000;
        const transferred = Number(measurement.bytes_transferred);
        const blockBytes = Number(measurement.block_size_bytes) || 4096;
        const mbPerSecond = Number.isFinite(Number(measurement.mb_per_second))
          ? Number(measurement.mb_per_second)
          : seconds > 0 && Number.isFinite(transferred) ? transferred / 1048576 / seconds : null;
        const iops = Number.isFinite(Number(measurement.iops))
          ? Number(measurement.iops)
          : seconds > 0 && Number.isFinite(transferred) ? transferred / blockBytes / seconds : null;
        const secondary = document.createElement("small");
        secondary.className = "storage-measurement-secondary";
        secondary.textContent = measurement.unit === "MB/s"
          ? (iops === null ? "IOPS complementar indisponível" : fmt(iops) + " IOPS")
          : (mbPerSecond === null ? "MB/s complementar indisponível" : fmt(mbPerSecond) + " MB/s");
        card.appendChild(secondary);
        appendStorageSparkline(card, measurement.samples, measurement.unit === "MB/s" ? "mb_per_second" : "iops");
      }
      if (measurement && measurement.bytes_transferred !== null && measurement.bytes_transferred !== undefined) {
        const detail = document.createElement("small");
        detail.className = "storage-measurement-meta";
        detail.textContent = bytes(measurement.bytes_transferred) + " transferidos · " +
          fmt(measurement.duration_ms) + " ms";
        card.appendChild(detail);
      }
      if ((kind === "seq_large_read" || kind === "seq_large_write") && threshold[report.class]) {
        const reference = document.createElement("small");
        reference.className = "storage-measurement-reference";
        reference.textContent = "Referência interna da classe: ≥ " + threshold[report.class] + " MB/s";
        card.appendChild(reference);
      }
      grid.appendChild(card);
    });
    section.appendChild(grid);
  });

  const smart = report.smart || {};
  const smartSection = createSection("Dados SMART", smart.usable
    ? "Informações disponibilizadas pelo dispositivo e pelo provedor do Windows."
    : "O dispositivo ou controlador não expôs dados SMART utilizáveis. Ausência de leitura não significa que esteja saudável.");
  const smartState = document.createElement("p");
  smartState.className = "storage-report-notice neutral";
  if (smart.predicted_failure === true) {
    smartState.className = "storage-report-notice danger";
    smartState.textContent = "O provedor SMART sinalizou previsão de falha.";
  } else if (smart.predicted_failure === false) {
    smartState.textContent = "O provedor não sinalizou previsão de falha. Isso não garante, isoladamente, a saúde do dispositivo.";
  } else {
    smartState.textContent = "Previsão de falha não informada pelo provedor.";
  }
  smartSection.appendChild(smartState);

  const smartGrid = document.createElement("div");
  smartGrid.className = "storage-smart-grid";

  // As cores representam somente evidências SMART explícitas. Dados ausentes,
  // não expostos ou com erro de coleta permanecem neutros.
  const sectorTone = (field, value) => {
    if (value === null || value === undefined || !Number.isFinite(Number(value))) return "neutral";
    const count = Number(value);
    if (count === 0) return "success";
    return field === "reallocated" ? "warning" : "danger";
  };
  const predictedTone = smart.predicted_failure === true
    ? "danger" : smart.predicted_failure === false ? "success" : "neutral";
  const crcIncreased = smart.interface_crc_before !== null && smart.interface_crc_before !== undefined &&
    smart.interface_crc_after !== null && smart.interface_crc_after !== undefined &&
    Number(smart.interface_crc_after) > Number(smart.interface_crc_before);
  const smartCards = [
    ["Setores realocados", smartSectorValue("reallocated", smart.reallocated), sectorTone("reallocated", smart.reallocated)],
    ["Setores pendentes", smartSectorValue("pending", smart.pending), sectorTone("pending", smart.pending)],
    ["Setores incorrigíveis", smartSectorValue("uncorrectable", smart.uncorrectable), sectorTone("uncorrectable", smart.uncorrectable)],
    ["Desgaste informado", smartValue(smart.wear_percent, "%"), "neutral"],
    ["Temperatura", smartValue(smart.temperature_c, " °C"), "neutral"],
    ["Erros CRC · antes → depois", smartValue(smart.interface_crc_before) + " → " + smartValue(smart.interface_crc_after), crcIncreased ? "warning" : "neutral"],
    ["Previsão de falha", smart.predicted_failure === true ? "Sinalizada" : smart.predicted_failure === false ? "Não sinalizada" : "Não informada", predictedTone]
  ];
  smartCards.forEach(([label, value, tone]) => {
    const card = document.createElement("article");
    card.className = "storage-smart-card " + tone;
    const name = document.createElement("span");
    name.textContent = label;
    const amount = document.createElement("strong");
    amount.textContent = value;
    card.append(name, amount);
    smartGrid.appendChild(card);
  });
  smartSection.appendChild(smartGrid);

  const hasSmartAlert = smart.predicted_failure === true ||
    Number(smart.pending) > 0 || Number(smart.uncorrectable) > 0 ||
    Number(smart.reallocated) > 0 || crcIncreased;
  if (hasSmartAlert) {
    smartState.className = "storage-report-notice " +
      (smart.predicted_failure === true || Number(smart.pending) > 0 || Number(smart.uncorrectable) > 0 ? "danger" : "warning");
    if (smart.predicted_failure !== true) {
      smartState.textContent = "Um ou mais indicadores SMART merecem atenção. Consulte os itens destacados; erros CRC podem indicar problema na conexão.";
    }
  } else if (smart.usable && (smart.predicted_failure === false ||
      [smart.reallocated, smart.pending, smart.uncorrectable].some((value) => value !== null && value !== undefined))) {
    smartState.className = "storage-report-notice success";
    smartState.textContent = "Nenhum alerta identificado nos indicadores SMART disponíveis. Atributos não fornecidos não foram considerados saudáveis.";
  }
  if (smart.wear_limit_percent !== null && smart.wear_limit_percent !== undefined) {
    const wearLimit = document.createElement("p");
    wearLimit.className = "storage-report-footnote";
    wearLimit.textContent = "Limite de desgaste informado pelo dispositivo: " + fmt(smart.wear_limit_percent) + "%.";
    smartSection.appendChild(wearLimit);
  }
  if (smart.usb_slow === true) {
    const usbNotice = document.createElement("p");
    usbNotice.className = "storage-report-notice warning";
    usbNotice.textContent = "A conexão USB apresentou leitura baixa. Vale revisar cabo, porta ou gabinete; esse resultado não é usado para condenar o disco.";
    smartSection.appendChild(usbNotice);
  }

  const attributes = Array.isArray(smart.attributes) ? smart.attributes : [];
  const metrics = Array.isArray(smart.reliability_metrics) ? smart.reliability_metrics : [];
  const collectionStatus = smart.collection_status || null;
  if (attributes.length || metrics.length || smart.source || collectionStatus) {
    const details = document.createElement("details");
    details.className = "storage-technical-details";
    const summary = document.createElement("summary");
    summary.textContent = "Detalhes técnicos e dados brutos";
    details.appendChild(summary);
    if (smart.source) {
      const source = document.createElement("p");
      source.textContent = "Fontes: " + smart.source;
      details.appendChild(source);
    }
    if (collectionStatus) {
      const statusHeading = document.createElement("h5");
      statusHeading.textContent = "Estado da coleta SMART";
      const statusList = document.createElement("ul");
      const statusLabels = {
        available: "Disponível",
        parsed: "Interpretado",
        not_exposed: "Não exposto pelo provedor",
        provider_unavailable: "Provedor indisponível",
        no_matching_instance: "Nenhuma instância correspondente",
        matched: "Dispositivo correspondente localizado",
        collection_error: "Falha durante a coleta",
        query_error: "Falha na consulta",
        parse_error: "Falha ao interpretar os dados",
        not_attempted: "Interpretação não realizada",
        attribute_not_reported: "Atributo não reportado"
      };
      [
        ["Dados SMART WMI", collectionStatus.failure_predict_data],
        ["Instância do dispositivo", collectionStatus.instance_match],
        ["Interpretação dos atributos", collectionStatus.attributes_parse],
        ["Contador de confiabilidade", collectionStatus.storage_reliability_counter]
      ].forEach(([label, value]) => {
        if (!value) return;
        const item = document.createElement("li");
        item.textContent = label + ": " + (statusLabels[value] || value);
        statusList.appendChild(item);
      });
      const sectorStatuses = collectionStatus.sector_attributes || {};
      [
        ["Realocados", sectorStatuses.reallocated],
        ["Pendentes", sectorStatuses.pending],
        ["Incorrigíveis", sectorStatuses.uncorrectable]
      ].forEach(([label, value]) => {
        if (!value) return;
        const item = document.createElement("li");
        item.textContent = "Setores " + label.toLowerCase() + ": " + (statusLabels[value] || value);
        statusList.appendChild(item);
      });
      details.append(statusHeading, statusList);
      const errors = Array.isArray(collectionStatus.errors) ? collectionStatus.errors : [];
      if (errors.length) {
        const errorHeading = document.createElement("h5");
        errorHeading.textContent = "Falhas registradas";
        const errorList = document.createElement("ul");
        errors.forEach((error) => {
          const item = document.createElement("li");
          item.textContent = String(error);
          errorList.appendChild(item);
        });
        details.append(errorHeading, errorList);
      }
    }
    if (metrics.length) {
      const metricHeading = document.createElement("h5");
      metricHeading.textContent = "Métricas adicionais";
      const metricList = document.createElement("ul");
      metrics.forEach((metric) => {
        const item = document.createElement("li");
        item.textContent = (metric.name || "Métrica") + ": " + (metric.value ?? "Indisponível");
        metricList.appendChild(item);
      });
      details.append(metricHeading, metricList);
    }
    if (attributes.length) {
      const attributeHeading = document.createElement("h5");
      attributeHeading.textContent = "Atributos SMART brutos";
      const attributeList = document.createElement("ul");
      attributes.forEach((attribute) => {
        const item = document.createElement("li");
        item.textContent = "ID " + attribute.id + " · " + (attribute.name || "Atributo") +
          " · atual " + (attribute.current ?? "Indisponível") +
          " · pior " + (attribute.worst ?? "Indisponível") +
          " · limite " + (attribute.threshold ?? "Indisponível") +
          " · bruto " + (attribute.raw_value ?? "Indisponível");
        attributeList.appendChild(item);
      });
      details.append(attributeHeading, attributeList);
    }
    smartSection.appendChild(details);
  }

  if (report.space_available === false) {
    const warning = document.createElement("p");
    warning.className = "storage-report-notice warning";
    warning.textContent = "Benchmark não executado: o volume não tinha pelo menos 320 MiB livres para criar o arquivo temporário de 256 MiB com margem de segurança. A coleta SMART foi mantida quando disponível.";
    entry.appendChild(warning);
  }
  const issues = Array.isArray(report.issues) ? report.issues : [];
  if (issues.length) {
    const issueSection = createSection("Avisos da coleta");
    const issueList = document.createElement("ul");
    issueList.className = "storage-issue-list";
    issues.forEach((issue) => {
      const item = document.createElement("li");
      item.textContent = String(issue);
      issueList.appendChild(item);
    });
    issueSection.appendChild(issueList);
  }

  if (append && list.querySelector(".storage-report-entry")) {
    const separator = document.createElement("li");
    separator.className = "storage-report-separator";
    separator.textContent = "Próxima unidade";
    list.appendChild(separator);
  }
  list.appendChild(entry);
}


function diagnosticExecutionLabel(status) {
  return {
    running: "em execução",
    completed: "concluído",
    partial: "parcial",
    cancelled: "cancelado",
    failed: "falhou",
    simulated: "simulado"
  }[status] || "estado desconhecido";
}

async function loadDiagnosticHistory(selectLatest = false) {
  const list = document.getElementById("diagnostic-history");
  if (!list) return [];
  const requestId = ++diagnosticHistoryRequestId;
  const target = activeDiagnosticTarget;
  try {
    const storageOnly = target === "armazenamento";
    const cpuOnly = target === "processador";
    const reportPromise = storageOnly || cpuOnly ? Promise.resolve([]) : Qboa.getDiagnosticReports();
    const storagePromise = Qboa.getStorageDiagnosticReports().catch(() => []);
    const cpuPromise = Qboa.getCpuDiagnosticReports().catch(() => []);
    const results = await Promise.all([reportPromise, storagePromise, cpuPromise]);
    if (requestId !== diagnosticHistoryRequestId) return [];
    const reports = results[0];
    const storageReports = results[1];
    const cpuReports = results[2];
    list.replaceChildren();
    const combined = (storageOnly
      ? storageReports.map((report) => ({ report, storage: true, cpu: false }))
      : [
        ...storageReports.map((report) => ({ report, storage: true, cpu: false })),
        ...cpuReports.map((report) => ({ report, storage: false, cpu: true })),
        ...reports.map((report) => ({ report, storage: false, cpu: false }))
      ]).sort((a, b) => String(b.report.started_at || "").localeCompare(String(a.report.started_at || "")));

    if (!combined.length) {
      const empty = document.createElement("li");
      empty.textContent = "Nenhum laudo salvo ainda.";
      list.appendChild(empty);
    }
    combined.forEach(({ report, storage, cpu }) => {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "diagnostic-history-item";
      const date = document.createElement("span");
      const parsedDate = report.started_at ? new Date(report.started_at) : null;
      date.textContent = parsedDate && !Number.isNaN(parsedDate.valueOf())
        ? parsedDate.toLocaleString("pt-BR") : "Data indisponível";
      const detail = document.createElement("small");
      detail.textContent = storage
        ? "Armazenamento · " + (report.disk_name || report.disk_id || "disco") + " · " + diagnosticExecutionLabel(report.execution_status)
        : cpu
          ? "CPU · " + (report.processor_name || "processador") + " · " + cpuTestStatusLabel(report.execution_status)
          : (report.mode === "complete" ? "Completo" : "Rápido") + " · " + diagnosticExecutionLabel(report.execution_status);
      button.append(date, detail);
      button.addEventListener("click", () => {
        if (storage) {
          renderStorageReport(report);
          document.getElementById("diagnostic-status").textContent =
            "Laudo de armazenamento recuperado · " + (report.report_id || "ID indisponível");
        } else if (cpu) {
          renderCpuReport(report);
          document.getElementById("diagnostic-status").textContent =
            "Laudo de CPU recuperado · " + (report.report_id || "ID indisponível");
        } else {
          renderDiagnosticReport(report);
          document.getElementById("diagnostic-status").textContent =
            "Laudo recuperado do histórico · " + diagnosticExecutionLabel(report.execution_status) + " · " + report.report_id;
        }
      });
      item.appendChild(button);
      list.appendChild(item);
    });

    if (selectLatest && combined.length) {
      const latestEntry = storageOnly
        ? combined.find((entry) => entry.storage)
        : cpuOnly
          ? combined.find((entry) => entry.cpu)
          : combined[0];
      if (latestEntry?.storage) {
        renderStorageReport(latestEntry.report);
        document.getElementById("diagnostic-status").textContent =
          "Último laudo de armazenamento recuperado · " + (latestEntry.report.report_id || "ID indisponível");
      } else if (latestEntry?.cpu) {
        renderCpuReport(latestEntry.report);
        document.getElementById("diagnostic-status").textContent =
          "Último laudo de CPU recuperado · " + cpuTestStatusLabel(latestEntry.report.execution_status);
      } else if (latestEntry) {
        renderDiagnosticReport(latestEntry.report);
        document.getElementById("diagnostic-status").textContent =
          "Último laudo recuperado · " + diagnosticExecutionLabel(latestEntry.report.execution_status);
      }
    }
    return cpuOnly ? cpuReports : storageOnly ? storageReports : reports;
  } catch (error) {
    if (requestId !== diagnosticHistoryRequestId) return [];
    const item = document.createElement("li");
    item.textContent = "Não foi possível carregar o histórico: " + String(error);
    list.replaceChildren(item);
    return [];
  }
}

function cpuTestStatusLabel(status) {
  return {
    running: "em execução",
    completed: "concluído",
    errors_detected: "divergências detectadas",
    cancelled: "cancelado",
    thermal_limit_reached: "interrompido por temperatura",
    sensor_lost: "sensor indisponível",
    partial: "parcial / interrompido",
    failed: "falhou"
  }[status] || "estado desconhecido";
}

function cpuFormat(value, suffix = "", decimals = 1) {
  const number = Number(value);
  return value !== null && value !== undefined && Number.isFinite(number)
    ? number.toLocaleString("pt-BR", { minimumFractionDigits: decimals, maximumFractionDigits: decimals }) + suffix
    : "—";
}

async function loadCpuInventory() {
  const summary = document.getElementById("cpu-inventory-summary");
  const details = document.getElementById("cpu-inventory-details");
  const status = document.getElementById("cpu-sensor-status");
  const refresh = document.getElementById("cpu-inventory-refresh");
  const start = document.getElementById("cpu-test-start");
  refresh.disabled = true;
  start.disabled = true;
  setDiagnosticActivity("inventory:cpu", true);
  summary.textContent = "Consultando identificação da CPU e sensores...";
  details.replaceChildren();
  status.textContent = "";
  status.classList.remove("available");
  try {
    const inventory = await Qboa.getCpuInventory();
    cpuInventorySnapshot = inventory;
    summary.textContent = inventory.name || "Processador sem identificação";
    const values = [
      ["Fabricante", inventory.manufacturer || "Não informado"],
      ["Soquete", inventory.socket || "Não informado"],
      ["Núcleos físicos", inventory.physical_cores || "Não informado"],
      ["Threads lógicos", inventory.logical_processors || "Não informado"],
      ["Clock máximo reportado", inventory.max_clock_mhz ? Number(inventory.max_clock_mhz).toLocaleString("pt-BR") + " MHz" : "Não informado"],
      ["Sensores de temperatura", (inventory.temperature_sensors || []).join(", ") || "Nenhum"]
    ];
    values.forEach(([label, value]) => {
      const item = document.createElement("article");
      item.className = "cpu-inventory-detail";
      const title = document.createElement("span");
      title.textContent = label;
      const valueElement = document.createElement("strong");
      valueElement.textContent = String(value);
      item.append(title, valueElement);
      details.appendChild(item);
    });
    const providerLabel = inventory.sensor_provider || "coletor térmico integrado";
    status.textContent = inventory.sensor_available
      ? "Temperatura disponível via " + providerLabel + ". " + (inventory.sensor_note || "")
      : (inventory.sensor_note || "Sensor térmico de CPU não encontrado. O teste não poderá ser iniciado.");
    status.classList.toggle("available", Boolean(inventory.sensor_available));
    if (!inventory.pawnio_installed) {
      const installButton = document.createElement("button");
      installButton.type = "button";
      installButton.className = refresh.className;
      installButton.textContent = "Instalar PawnIO";
      installButton.addEventListener("click", async () => {
        const confirmed = await askConfirmation(
          "O PawnIO é um driver de sistema (kernel) usado para acessar sensores de hardware. Ao continuar, o instalador oficial será aberto e poderá solicitar autorização do Windows (UAC). Uma reinicialização pode ser necessária. O QBoa não fará a instalação sem esta confirmação.",
          "Instalar dependência térmica PawnIO"
        );
        if (!confirmed) return;
        installButton.disabled = true;
        installButton.textContent = "Aguardando instalador...";
        status.appendChild(document.createTextNode("\\n"));
        try {
          const message = await Qboa.installPawnIo();
          status.textContent = message;
          if (/reinicializa/i.test(message)) {
            installButton.remove();
            start.disabled = true;
            return;
          }
          await loadCpuInventory();
        } catch (error) {
          status.textContent = "Falha na instalação do PawnIO: " + String(error);
          installButton.disabled = false;
          installButton.textContent = "Tentar instalar novamente";
          status.appendChild(installButton);
        }
      });
      status.appendChild(document.createTextNode(" "));
      status.appendChild(installButton);
    }
    start.disabled = !inventory.sensor_available || ["running", "cancelling"].includes(cpuTestSessionSnapshot?.lifecycle);
  } catch (error) {
    cpuInventorySnapshot = null;
    summary.textContent = "Não foi possível consultar o processador: " + String(error);
    status.textContent = "Não foi possível consultar a identificação do processador.";
    appendConsole("CPU · falha ao consultar inventário e sensores: " + String(error));
    start.disabled = true;
  } finally {
    refresh.disabled = false;
    setDiagnosticActivity("inventory:cpu", false);
  }
}

function renderCpuFrequencyChart(samples) {
  const canvas = document.getElementById("cpu-frequency-chart");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const width = Math.max(280, canvas.clientWidth || 720);
  const height = 160;
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);

  const left = 52, right = width - 12, top = 12, bottom = height - 25;
  const plotHeight = bottom - top;
  const values = (Array.isArray(samples) ? samples : [])
    .map((sample) => Number(sample.frequency_mhz))
    .filter((value) => Number.isFinite(value) && value > 0);
  ctx.font = "11px sans-serif";
  ctx.lineWidth = 1;

  if (!values.length) {
    for (let tick = 0; tick <= 4; tick += 1) {
      const y = bottom - (tick / 4) * plotHeight;
      ctx.strokeStyle = "rgba(255,255,255,.13)";
      ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(right, y); ctx.stroke();
    }
    ctx.fillStyle = "rgba(255,255,255,.65)";
    ctx.textAlign = "center";
    ctx.fillText("A frequência aparecerá quando houver leituras válidas.", width / 2, height / 2);
    document.getElementById("cpu-frequency-chart-range").textContent = "Sem leituras de clock";
    return;
  }

  const minValue = Math.min(...values);
  const maxValue = Math.max(...values);
  const padding = Math.max(100, (maxValue - minValue) * 0.15);
  const minScale = Math.max(0, Math.floor((minValue - padding) / 250) * 250);
  const maxScale = Math.max(minScale + 250, Math.ceil((maxValue + padding) / 250) * 250);
  for (let tick = 0; tick <= 4; tick += 1) {
    const value = minScale + ((maxScale - minScale) * tick / 4);
    const y = bottom - (tick / 4) * plotHeight;
    ctx.strokeStyle = "rgba(255,255,255,.13)";
    ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(right, y); ctx.stroke();
    ctx.fillStyle = "rgba(255,255,255,.68)";
    ctx.textAlign = "right";
    ctx.fillText(Math.round(value).toLocaleString("pt-BR"), left - 7, y + 4);
  }

  const firstElapsed = Number(samples[0]?.elapsed_ms || 0);
  const lastElapsed = Number(samples[samples.length - 1]?.elapsed_ms || firstElapsed);
  const elapsedRange = Math.max(1, lastElapsed - firstElapsed);
  ctx.strokeStyle = "#2dd4bf";
  ctx.lineWidth = 2;
  ctx.beginPath();
  let drawing = false;
  samples.forEach((sample) => {
    const value = Number(sample.frequency_mhz);
    if (!Number.isFinite(value) || value <= 0) { drawing = false; return; }
    const elapsed = Number(sample.elapsed_ms || 0);
    const x = samples.length === 1 ? left : left + ((elapsed - firstElapsed) / elapsedRange) * (right - left);
    const y = bottom - ((value - minScale) / (maxScale - minScale)) * plotHeight;
    if (!drawing) { ctx.moveTo(x, y); drawing = true; } else ctx.lineTo(x, y);
  });
  ctx.stroke();

  const durationSeconds = Math.round(lastElapsed / 1000);
  document.getElementById("cpu-frequency-chart-range").textContent =
    values.length + " leitura(s) · " + Math.floor(durationSeconds / 60) + "m " + (durationSeconds % 60) + "s";
}

function renderCpuChart(samples) {
  const canvas = document.getElementById("cpu-test-chart");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const width = Math.max(280, canvas.clientWidth || 720);
  const height = 190;
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);
  const left = 42, right = width - 12, top = 12, bottom = height - 26;
  const plotHeight = bottom - top;
  ctx.font = "11px sans-serif";
  ctx.lineWidth = 1;
  for (let tick = 0; tick <= 100; tick += 25) {
    const y = bottom - (tick / 100) * plotHeight;
    ctx.strokeStyle = "rgba(255,255,255,.13)";
    ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(right, y); ctx.stroke();
    ctx.fillStyle = "rgba(255,255,255,.68)";
    ctx.textAlign = "right";
    ctx.fillText(String(tick), left - 7, y + 4);
  }
  if (!samples?.length) {
    ctx.fillStyle = "rgba(255,255,255,.65)";
    ctx.textAlign = "center";
    ctx.fillText("As amostras aparecerão durante o teste.", width / 2, height / 2);
    document.getElementById("cpu-test-chart-range").textContent = "Sem amostras";
    renderCpuFrequencyChart(samples);
    return;
  }
  const firstElapsed = Number(samples[0]?.elapsed_ms || 0);
  const lastElapsed = Number(samples[samples.length - 1]?.elapsed_ms || firstElapsed);
  const elapsedRange = Math.max(1, lastElapsed - firstElapsed);
  const drawSeries = (field, color) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    let drawing = false;
    samples.forEach((sample) => {
      const value = Number(sample[field]);
      if (!Number.isFinite(value)) { drawing = false; return; }
      const elapsed = Number(sample.elapsed_ms || 0);
      const x = samples.length === 1 ? left : left + ((elapsed - firstElapsed) / elapsedRange) * (right - left);
      const y = bottom - (Math.max(0, Math.min(100, value)) / 100) * plotHeight;
      if (!drawing) { ctx.moveTo(x, y); drawing = true; } else ctx.lineTo(x, y);
    });
    ctx.stroke();
  };
  drawSeries("temperature_c", "#fbbf24");
  drawSeries("cpu_utilization_percent", "#60a5fa");
  renderCpuFrequencyChart(samples);
  const durationSeconds = Math.round(Number(samples[samples.length - 1]?.elapsed_ms || 0) / 1000);
  document.getElementById("cpu-test-chart-range").textContent =
    samples.length + " amostra(s) · " + Math.floor(durationSeconds / 60) + "m " + (durationSeconds % 60) + "s";
}

function renderGpuChart(samples) {
  const canvas = document.getElementById("gpu-test-chart");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const width = Math.max(280, canvas.clientWidth || 720);
  const height = 190;
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);
  const left = 42, right = width - 12, top = 12, bottom = height - 26;
  const plotHeight = bottom - top;
  ctx.font = "11px sans-serif";
  ctx.lineWidth = 1;
  for (let tick = 0; tick <= 100; tick += 25) {
    const y = bottom - (tick / 100) * plotHeight;
    ctx.strokeStyle = "rgba(255,255,255,.13)";
    ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(right, y); ctx.stroke();
    ctx.fillStyle = "rgba(255,255,255,.68)";
    ctx.textAlign = "right";
    ctx.fillText(String(tick), left - 7, y + 4);
  }
  if (!samples?.length) {
    ctx.fillStyle = "rgba(255,255,255,.65)";
    ctx.textAlign = "center";
    ctx.fillText("As amostras aparecerão durante o teste.", width / 2, height / 2);
    document.getElementById("gpu-test-chart-range").textContent = "Sem amostras";
    return;
  }
  const drawSeries = (field, color) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    let drawing = false;
    samples.forEach((sample, index) => {
      const value = Number(sample[field]);
      if (!Number.isFinite(value)) { drawing = false; return; }
      const x = samples.length === 1 ? left : left + (index / (samples.length - 1)) * (right - left);
      const y = bottom - (Math.max(0, Math.min(100, value)) / 100) * plotHeight;
      if (!drawing) { ctx.moveTo(x, y); drawing = true; } else ctx.lineTo(x, y);
    });
    ctx.stroke();
  };
  drawSeries("temperature_c", "#fbbf24");
  drawSeries("utilization_percent", "#60a5fa");
  const durationSeconds = Math.round(Number(samples[samples.length - 1]?.elapsed_ms || 0) / 1000);
  document.getElementById("gpu-test-chart-range").textContent =
    samples.length + " amostra(s) · " + Math.floor(durationSeconds / 60) + "m " + (durationSeconds % 60) + "s";
}

function updateGpuLiveMetrics(samples, session = {}) {
  const valid = Array.isArray(samples) ? samples : [];
  const latest = valid[valid.length - 1] || {};
  const temperatures = valid.map((item) => Number(item.temperature_c)).filter(Number.isFinite);
  const usages = valid.map((item) => Number(item.utilization_percent)).filter(Number.isFinite);
  const frequencies = valid.map((item) => Number(item.frequency_mhz)).filter(Number.isFinite);
  const maxOf = (items) => items.length ? Math.max(...items) : null;
  const source = latest.temperature_source === 'cpu' ? 'CPU · referência térmica' : 'GPU';
  document.getElementById('gpu-test-temperature').textContent = latest.temperature_c == null ? '—' : Number(latest.temperature_c).toLocaleString('pt-BR', { maximumFractionDigits: 1 }) + ' °C';
  document.getElementById('gpu-test-temperature-max').textContent = 'Máxima: ' + (maxOf(temperatures) == null ? '—' : maxOf(temperatures).toLocaleString('pt-BR', { maximumFractionDigits: 1 }) + ' °C');
  document.getElementById('gpu-test-temperature-source').textContent = 'Sensor: ' + (latest.temperature_c == null ? 'indisponível' : source);
  document.getElementById('gpu-test-utilization').textContent = latest.utilization_percent == null ? '—' : Number(latest.utilization_percent).toLocaleString('pt-BR', { maximumFractionDigits: 0 }) + '%';
  document.getElementById('gpu-test-utilization-max').textContent = 'Pico: ' + (maxOf(usages) == null ? '—' : maxOf(usages).toLocaleString('pt-BR', { maximumFractionDigits: 0 }) + '%');
  document.getElementById('gpu-test-frequency').textContent = latest.frequency_mhz == null ? '—' : Number(latest.frequency_mhz).toLocaleString('pt-BR', { maximumFractionDigits: 0 }) + ' MHz';
  document.getElementById('gpu-test-frequency-max').textContent = 'Pico: ' + (maxOf(frequencies) == null ? '—' : maxOf(frequencies).toLocaleString('pt-BR', { maximumFractionDigits: 0 }) + ' MHz');
  document.getElementById('gpu-test-dispatches').textContent = Number(session.dispatches || 0).toLocaleString('pt-BR');
  document.getElementById('gpu-test-validation-errors').textContent = Number(session.validation_errors || 0).toLocaleString('pt-BR');
  const legend = document.getElementById('gpu-test-chart-temperature-legend');
  if (legend) legend.textContent = latest.temperature_source === 'cpu' ? 'CPU · referência térmica (°C)' : 'Temperatura da GPU (°C)';
}
function updateCpuLiveMetrics(samples) {
  const valid = Array.isArray(samples) ? samples : [];
  const latest = valid[valid.length - 1] || {};
  const temperatures = valid.map((item) => Number(item.temperature_c)).filter(Number.isFinite);
  const usages = valid.map((item) => Number(item.cpu_utilization_percent)).filter(Number.isFinite);
  const frequencies = valid.map((item) => Number(item.frequency_mhz)).filter(Number.isFinite);
  const maxOf = (items) => items.length ? Math.max(...items) : null;
  document.getElementById("cpu-test-temperature").textContent = cpuFormat(latest.temperature_c, " °C");
  document.getElementById("cpu-test-temperature-max").textContent = "Máxima: " + cpuFormat(maxOf(temperatures), " °C");
  document.getElementById("cpu-test-temperature-source").textContent = "Sensor: " + (latest.temperature_sensor || "indisponível");
  document.getElementById("cpu-test-utilization").textContent = cpuFormat(latest.cpu_utilization_percent, "%");
  document.getElementById("cpu-test-utilization-max").textContent = "Pico: " + cpuFormat(maxOf(usages), "%");
  document.getElementById("cpu-test-frequency").textContent = cpuFormat(latest.frequency_mhz, " MHz", 0);
  document.getElementById("cpu-test-frequency-max").textContent = "Pico: " + cpuFormat(maxOf(frequencies), " MHz", 0);
  document.getElementById("cpu-test-frequency-source").textContent = latest.frequency_source
    ? "Fonte: " + latest.frequency_source
    : "Fonte: não informada nesta amostra";
}

function renderCpuReport(report) {
  cpuTestSamples = Array.isArray(report.samples) ? report.samples.slice() : [];
  renderCpuChart(cpuTestSamples);
  updateCpuLiveMetrics(cpuTestSamples);
  const result = document.getElementById("cpu-test-result");
  result.replaceChildren();
  const heading = document.createElement("p");
  heading.textContent = "Resultado: " + cpuTestStatusLabel(report.execution_status);
  result.appendChild(heading);
  const profileLabel = {
    quick: "Rápido · 5 min",
    standard: "Padrão · 15 min",
    extended: "Estendido · 30 min"
  }[report.profile] || "Perfil não informado";
  const summary = document.createElement("p");
  summary.textContent = (report.processor_name || "CPU") + " · " + profileLabel + " · " +
    Math.floor(Number(report.duration_ms || 0) / 60000) + " min efetivos · " +
    Number(report.worker_threads || report.logical_processors || 0) + " threads de carga · " +
    Number(report.iterations || 0).toLocaleString("pt-BR") + " verificações";
  result.appendChild(summary);
  const metrics = document.createElement("p");
  const temperatureSummary = report.temperature_max_c === null || report.temperature_max_c === undefined
    ? "Temperatura: não avaliada"
    : "Temperatura mín./média/máx.: " +
      cpuFormat(report.temperature_min_c, " °C") + " / " +
      cpuFormat(report.temperature_average_c, " °C") + " / " +
      cpuFormat(report.temperature_max_c, " °C");
  metrics.textContent = temperatureSummary +
    " · utilização média: " + cpuFormat(report.cpu_utilization_average_percent, "%") +
    " · pico de frequência: " + cpuFormat(report.frequency_peak_mhz, " MHz", 0);
  result.appendChild(metrics);
  if (Array.isArray(report.warnings) && report.warnings.length) {
    const list = document.createElement("ul");
    report.warnings.forEach((warning) => {
      const item = document.createElement("li");
      item.textContent = warning;
      list.appendChild(item);
    });
    result.appendChild(list);
  }
  const targetDurationMs = Math.max(1, Number(report.target_duration_seconds || 1) * 1000);
  const percent = report.execution_status === "completed"
    ? 100
    : Math.max(0, Math.min(100, Math.floor(Number(report.duration_ms || 0) * 100 / targetDurationMs)));
  const progressBar = document.getElementById("cpu-test-progress-bar");
  progressBar.style.width = percent + "%";
  const progressTrack = document.querySelector(".cpu-test-progress-track");
  progressTrack.setAttribute("aria-valuenow", String(percent));
  document.getElementById("cpu-test-progress-percent").textContent = percent + "%";
  document.getElementById("cpu-test-phase").textContent = cpuTestStatusLabel(report.execution_status);
  document.getElementById("cpu-test-progress-label").textContent = cpuTestStatusLabel(report.execution_status);
  document.getElementById("cpu-test-progress-detail").textContent =
    Math.floor(Number(report.duration_ms || 0) / 60000) + " min efetivos · " +
    Number(report.samples?.length || 0).toLocaleString("pt-BR") + " amostras de telemetria · " +
    Number(report.calculation_errors || 0) + " divergência(s)";
}

function logCpuTestActivity(session) {
  if (lastCpuActivitySessionId !== session.session_id) {
    lastCpuActivitySessionId = session.session_id;
    lastCpuActivityLifecycle = null;
    lastCpuActivitySampleAt = 0;
  }

  if (session.lifecycle !== lastCpuActivityLifecycle) {
    if (session.lifecycle === "running") {
      appendConsole("CPU · teste de estabilidade iniciado · perfil " + String(session.profile || "não informado"));
    } else if (session.lifecycle === "failed") {
      appendConsole("CPU · falha no teste de estabilidade: " + String(session.error || "erro sem detalhes"));
    } else if (session.lifecycle === "completed" && session.report) {
      appendConsole("CPU · teste finalizado: " + cpuTestStatusLabel(session.report.execution_status) +
        " · " + Number(session.report.samples?.length || 0).toLocaleString("pt-BR") + " amostras · " +
        Number(session.report.calculation_errors || 0) + " divergência(s)");
    } else if (session.lifecycle === "cancelling") {
      appendConsole("CPU · cancelamento solicitado; encerrando trabalhadores com segurança.");
    }
    lastCpuActivityLifecycle = session.lifecycle;
  }

  const sample = session.latest_sample;
  const elapsed = Number(sample?.elapsed_ms || 0);
  if (session.lifecycle === "running" && sample && elapsed - lastCpuActivitySampleAt >= 5000) {
    const temperature = cpuFormat(sample.temperature_c, " °C");
    const utilization = cpuFormat(sample.cpu_utilization_percent, "%");
    const frequency = cpuFormat(sample.frequency_mhz, " MHz", 0);
    const source = sample.frequency_source ? " · " + sample.frequency_source : "";
    appendConsole("CPU · telemetria " + Math.floor(elapsed / 1000) + " s · " + temperature +
      " · utilização " + utilization + " · clock reportado " + frequency + source);
    lastCpuActivitySampleAt = elapsed;
  }
}

function renderCpuTestSession(session, logActivity = true) {
  if (!session) { cpuTestSessionSnapshot = null; return; }
  cpuTestSessionSnapshot = session;
  if (logActivity) logCpuTestActivity(session);
  if (session.report) {
    renderCpuReport(session.report);
  } else if (session.latest_sample) {
    const sample = session.latest_sample;
    const last = cpuTestSamples[cpuTestSamples.length - 1];
    if (!last || Number(sample.elapsed_ms) > Number(last.elapsed_ms)) cpuTestSamples.push(sample);
    renderCpuChart(cpuTestSamples);
    updateCpuLiveMetrics(cpuTestSamples);
  }
  const active = ["running", "cancelling"].includes(session.lifecycle);
  setDiagnosticActivity("session:cpu", active);
  const start = document.getElementById("cpu-test-start");
  const cancel = document.getElementById("cpu-test-cancel");
  const profile = document.getElementById("cpu-test-profile");
  const percent = Math.max(0, Math.min(100, Number(session.progress_percent || 0)));
  start.disabled = active;
  start.textContent = active ? "Teste em andamento" : "Iniciar teste de CPU";
  cancel.hidden = !active;
  cancel.disabled = session.lifecycle === "cancelling";
  cancel.textContent = session.lifecycle === "cancelling" ? "Cancelando…" : "Cancelar teste";
  profile.disabled = active;
  document.getElementById("cpu-test-progress-bar").style.width = percent + "%";
  document.querySelector(".cpu-test-progress-track").setAttribute("aria-valuenow", String(percent));
  document.getElementById("cpu-test-progress-percent").textContent = percent + "%";
  const elapsedSeconds = Math.floor(Number(session.elapsed_ms || 0) / 1000);
  const elapsed = Math.floor(elapsedSeconds / 60) + "m " + (elapsedSeconds % 60) + "s";
  const targetSeconds = Math.floor(Number(session.target_duration_ms || 0) / 1000);
  const target = Math.floor(targetSeconds / 60) + "m";
  const phase = session.lifecycle === "cancelling" ? "Encerrando com segurança…" :
    session.lifecycle === "running" ? "Teste em execução" :
    session.lifecycle === "failed" ? "Falha ao iniciar" : "Execução finalizada";
  document.getElementById("cpu-test-phase").textContent = phase;
  document.getElementById("cpu-test-progress-label").textContent = active ? elapsed + " / " + target : cpuTestStatusLabel(session.report?.execution_status || session.lifecycle);
  document.getElementById("cpu-test-progress-detail").textContent =
    active ? elapsed + " de " + target + " · " + Number(session.iterations || 0).toLocaleString("pt-BR") +
      " verificações · " + Number(session.calculation_errors || 0) + " divergência(s)" :
      session.error || session.report?.warnings?.join(" ") || "Teste finalizado.";
  const result = document.getElementById("cpu-test-result");
  if (active) {
    result.textContent = "Teste de CPU em execução. As amostras e métricas aparecerão abaixo.";
  } else if (session.error) {
    result.textContent = session.error;
  }
}

async function loadCpuTestSession() {
  try {
    const session = await Qboa.getCpuTestSession();
    if (session) renderCpuTestSession(session, false);
  } catch (error) {
    document.getElementById("cpu-test-result").textContent = "Não foi possível consultar a sessão: " + String(error);
    appendConsole("CPU · falha ao recuperar a sessão do teste: " + String(error));
  }
}

async function startCpuTestFromUi() {
  const profile = document.getElementById("cpu-test-profile").value;
  const start = document.getElementById("cpu-test-start");
  const result = document.getElementById("cpu-test-result");
  start.disabled = true;
  setDiagnosticActivity("starting:cpu", true);
  result.textContent = "Preparando o teste de CPU…";
  cpuTestSamples = [];
  renderCpuChart(cpuTestSamples);
  try {
    const session = await Qboa.startCpuTest(profile);
    renderCpuTestSession(session);
    await loadCpuTestSession();
  } catch (error) {
    result.textContent = String(error);
    appendConsole("CPU · não foi possível iniciar o teste: " + String(error));
    start.disabled = false;
  } finally {
    setDiagnosticActivity("starting:cpu", false);
  }
}

async function cancelCpuTestFromUi() {
  const sessionId = cpuTestSessionSnapshot?.session_id;
  if (!sessionId) return;
  const cancel = document.getElementById("cpu-test-cancel");
  cancel.disabled = true;
  try {
    renderCpuTestSession(await Qboa.cancelCpuTest(sessionId));
  } catch (error) {
    document.getElementById("cpu-test-result").textContent = String(error);
    appendConsole("CPU · falha ao solicitar cancelamento: " + String(error));
    cancel.disabled = false;
  }
}


function gpuTestStatusLabel(status) {
  return ({
    completed: "Teste concluído sem divergências observadas.",
    errors_detected: "Foram detectadas divergências nos resultados calculados pela GPU.",
    cancelled: "Teste cancelado. Este resultado não representa aprovação.",
    thermal_limit_reached: "Teste interrompido pelo limite térmico conservador do QBoa.",
    sensor_lost: "Teste interrompido porque a telemetria térmica foi perdida.",
    failed: "Falha operacional: o teste não pôde ser concluído.",
    device_error: "O dispositivo gráfico deixou de responder ou foi reiniciado pelo driver."
  })[status] || "Aguardando resultado.";
}

async function loadGpuInventory() {
  const summary = document.getElementById("gpu-inventory-summary");
  const details = document.getElementById("gpu-inventory-details");
  const sensor = document.getElementById("gpu-sensor-status");
  const refresh = document.getElementById("gpu-inventory-refresh");
  if (!summary || !details || !sensor || !refresh) return;
  refresh.disabled = true;
  setDiagnosticActivity("inventory:gpu", true);
  summary.textContent = "Consultando adaptadores gráficos e sensores…";
  details.replaceChildren();
  sensor.textContent = "";
  try {
    const inventory = await Qboa.getGpuInventory();
    gpuInventorySnapshot = inventory;
    const adapters = Array.isArray(inventory.adapters) ? inventory.adapters : [];
    summary.textContent = adapters.length + " adaptador(es) gráfico(s) identificado(s)";
    adapters.forEach((adapter) => {
      const card = document.createElement("article");
      card.className = "cpu-inventory-detail";
      const title = document.createElement("strong");
      title.textContent = adapter.name || "Adaptador gráfico";
      const meta = document.createElement("small");
      const memory = Number(adapter.memory_bytes || 0);
      const adapterType = adapter.adapter_type === "integrated" ? "Vídeo integrado" :
        adapter.adapter_type === "dedicated" ? "Vídeo dedicado" : "Tipo não determinado";
      meta.textContent = [
        adapterType,
        adapter.driver_version ? "Driver " + adapter.driver_version : "Driver não informado",
        memory > 0 ? formatMemoryBytes(memory) + " reportados pelo Windows" : "Memória dedicada não reportada"
      ].join(" · ");
      card.append(title, meta);
      details.appendChild(card);
    });
    sensor.textContent = inventory.sensor_note || (inventory.sensor_available
      ? "Referência térmica disponível pelo coletor integrado."
      : "Referência térmica indisponível. O teste de carga não será iniciado.");
    sensor.dataset.available = inventory.sensor_available ? "true" : "false";
    sensor.classList.toggle("available", inventory.sensor_available);
    document.getElementById("gpu-test-start").disabled =
      !inventory.sensor_available || ["running", "cancelling"].includes(gpuTestSessionSnapshot?.lifecycle);
  } catch (error) {
    gpuInventorySnapshot = null;
    summary.textContent = "Não foi possível consultar a GPU: " + String(error);
    sensor.textContent = "Sem inventário confiável, o teste não será iniciado.";
    document.getElementById("gpu-test-start").disabled = true;
  } finally {
    refresh.disabled = false;
    setDiagnosticActivity("inventory:gpu", false);
  }
}

function renderGpuTestSession(session) {
  if (!session) { gpuTestSessionSnapshot = null; return; }
  gpuTestSessionSnapshot = session;
  if (session.report?.samples) gpuTestSamples = session.report.samples.slice();
  else if (session.latest_sample) {
    const last = gpuTestSamples[gpuTestSamples.length - 1];
    if (!last || Number(session.latest_sample.elapsed_ms) > Number(last.elapsed_ms)) gpuTestSamples.push(session.latest_sample);
  }
  renderGpuChart(gpuTestSamples);
  updateGpuLiveMetrics(gpuTestSamples, session);
  const active = ["running", "cancelling"].includes(session.lifecycle);
  setDiagnosticActivity("session:gpu", active);
  const start = document.getElementById("gpu-test-start");
  const cancel = document.getElementById("gpu-test-cancel");
  const profile = document.getElementById("gpu-test-profile");
  const percent = Math.max(0, Math.min(100, Number(session.progress_percent || 0)));
  start.disabled = active || !gpuInventorySnapshot?.sensor_available;
  start.textContent = active ? "Teste em andamento" : "Iniciar teste de GPU";
  cancel.hidden = !active;
  cancel.disabled = session.lifecycle === "cancelling";
  cancel.textContent = session.lifecycle === "cancelling" ? "Cancelando…" : "Cancelar teste";
  profile.disabled = active;
  document.getElementById("gpu-test-progress-bar").style.width = percent + "%";
  document.getElementById("gpu-test-progress-track").setAttribute("aria-valuenow", String(percent));
  document.getElementById("gpu-test-progress-percent").textContent = percent + "%";
  const elapsedSeconds = Math.floor(Number(session.elapsed_ms || 0) / 1000);
  const elapsed = Math.floor(elapsedSeconds / 60) + "m " + (elapsedSeconds % 60) + "s";
  const targetSeconds = Math.floor(Number(session.target_duration_ms || 0) / 1000);
  const target = Math.floor(targetSeconds / 60) + "m";
  document.getElementById("gpu-test-phase").textContent =
    session.lifecycle === "cancelling" ? "Encerrando com segurança…" :
    active ? "Teste em execução" : session.lifecycle === "failed" ? "Falha ao iniciar" : "Execução finalizada";
  document.getElementById("gpu-test-progress-label").textContent = active ? elapsed + " / " + target :
    gpuTestStatusLabel(session.report?.execution_status || session.lifecycle);
  const live = session.latest_sample;
  const liveTemperatureSource = live?.temperature_source === "cpu" ? "CPU · referência térmica" : "GPU";
  const liveTemperature = live?.temperature_c == null ? "temperatura indisponível" :
    liveTemperatureSource + " " + Number(live.temperature_c).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " °C";
  const liveLoad = live?.utilization_percent == null ? "utilização indisponível" :
    Number(live.utilization_percent).toLocaleString("pt-BR", { maximumFractionDigits: 0 }) + "% de utilização";
  document.getElementById("gpu-test-progress-detail").textContent = active
    ? elapsed + " de " + target + " · " + liveTemperature + " · " + liveLoad + " · " +
      Number(session.dispatches || 0).toLocaleString("pt-BR") + " lotes · " +
      Number(session.validation_errors || 0) + " divergência(s)"
    : session.error || session.report?.warnings?.join(" ") || "Teste finalizado.";
  const result = document.getElementById("gpu-test-result");
  const details = document.getElementById("gpu-test-result-details");
  details.replaceChildren();
  if (session.report) {
    const report = session.report;
    const summary = document.createElement("p");
    summary.textContent = gpuTestStatusLabel(report.execution_status);
    result.replaceChildren(summary);
    const metrics = document.createElement("p");
    const temperatureSource = report.temperature_source === "cpu" ? "CPU (referência térmica)" : "GPU";
    const temp = report.temperature_max_c == null ? "temperatura máxima indisponível" :
      "máxima " + temperatureSource + " " + Number(report.temperature_max_c).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " °C";
    const load = report.utilization_average_percent == null ? "utilização indisponível" :
      "utilização média " + Number(report.utilization_average_percent).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + "%";
    metrics.textContent = Math.floor(Number(report.duration_ms || 0) / 1000) + " s efetivos · " +
      Number(report.dispatches || 0).toLocaleString("pt-BR") + " lotes · " + temp + " · " + load +
      " · " + Number(report.validation_errors || 0) + " divergência(s)";
    result.appendChild(metrics);
    (report.warnings || []).forEach((warning) => {
      const item = document.createElement("li");
      item.textContent = warning;
      details.appendChild(item);
    });
  } else if (session.error) {
    result.textContent = session.error;
  } else {
    result.textContent = active ? "Carga gráfica controlada em execução. A temperatura é monitorada continuamente." :
      "A sessão ainda não tem um relatório final.";
  }
}


async function loadGpuDiagnosticReports() {
  const list = document.getElementById("gpu-diagnostic-history");
  if (!list) return;
  list.replaceChildren();
  try {
    const reports = await Qboa.getGpuDiagnosticReports();
    if (!Array.isArray(reports) || !reports.length) {
      const empty = document.createElement("li");
      empty.textContent = "Nenhum laudo de GPU salvo neste computador.";
      list.appendChild(empty);
      return;
    }
    reports.forEach((report) => {
      const item = document.createElement("li");
      const heading = document.createElement("strong");
      const date = report.started_at ? new Date(report.started_at).toLocaleString("pt-BR") : "Data não informada";
      heading.textContent = date + " · " + gpuTestStatusLabel(report.execution_status);
      const detail = document.createElement("p");
      const temperatureSource = report.temperature_source === "cpu" ? "CPU (referência térmica)" : "GPU";
      const temperature = report.temperature_max_c == null ? "temperatura indisponível" :
        "máxima " + temperatureSource + " " + Number(report.temperature_max_c).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " °C";
      detail.textContent = Number(report.duration_ms || 0) / 1000 + " s · " +
        Number(report.dispatches || 0).toLocaleString("pt-BR") + " lotes · " + temperature +
        " · " + Number(report.validation_errors || 0) + " divergência(s)";
      item.append(heading, detail);
      list.appendChild(item);
    });
  } catch (error) {
    const item = document.createElement("li");
    item.textContent = "Não foi possível carregar os laudos de GPU: " + String(error);
    list.appendChild(item);
  }
}

async function loadGpuTestSession() {
  try {
    const session = await Qboa.getGpuTestSession();
    if (session) renderGpuTestSession(session);
  } catch (error) {
    document.getElementById("gpu-test-result").textContent = "Não foi possível consultar a sessão: " + String(error);
  }
}

async function startGpuTestFromUi() {
  const profile = document.getElementById("gpu-test-profile").value;
  const start = document.getElementById("gpu-test-start");
  const result = document.getElementById("gpu-test-result");
  start.disabled = true;
  setDiagnosticActivity("starting:gpu", true);
  result.textContent = "Preparando o teste de GPU e validando sensores…";
  gpuTestSamples = [];
  renderGpuChart(gpuTestSamples);
  updateGpuLiveMetrics(gpuTestSamples);
  try {
    const session = await Qboa.startGpuTest(profile);
    renderGpuTestSession(session);
    await loadGpuTestSession();
  } catch (error) {
    result.textContent = String(error);
    start.disabled = !gpuInventorySnapshot?.sensor_available;
  } finally {
    setDiagnosticActivity("starting:gpu", false);
  }
}

async function cancelGpuTestFromUi() {
  const sessionId = gpuTestSessionSnapshot?.session_id;
  if (!sessionId) return;
  const cancel = document.getElementById("gpu-test-cancel");
  cancel.disabled = true;
  try {
    renderGpuTestSession(await Qboa.cancelGpuTest(sessionId));
  } catch (error) {
    document.getElementById("gpu-test-result").textContent = String(error);
    cancel.disabled = false;
  }
}

function selectedStorageDiskIds() {
  const list = document.getElementById("storage-disk-list");
  if (!list) return [];
  return Array.from(list.querySelectorAll('input[type="checkbox"][data-storage-disk]:checked'))
    .map((checkbox) => checkbox.value)
    .filter(Boolean);
}

function normalizeStorageDisk(entry) {
  let disk = entry;
  while (Array.isArray(disk) && disk.length === 1) disk = disk[0];
  if (!disk || typeof disk !== "object" || Array.isArray(disk)) return null;

  const volume = disk.volume || disk.mount_point || disk.MountPoint || "";
  const driveMatch = String(volume).match(/^([A-Z]):\\?$/i);
  const driveLetter = disk.drive_letter || disk.DriveLetter || (driveMatch ? driveMatch[1] : "");
  const diskNumber = disk.disk_number ?? disk.DiskNumber ?? disk.Number;
  const id = disk.id || disk.disk_id ||
    (diskNumber !== undefined && diskNumber !== null && driveLetter
      ? String(Number(diskNumber)) + ":" + String(driveLetter).toUpperCase()
      : "");
  const bus = String(disk.bus_type || disk.BusType || disk.busType || "");
  let diskClass = disk.class || disk.disk_class || "unknown";
  if (/^nvme$/i.test(bus) || /NVME/i.test(String(disk.pnp_device_id || disk.PNPDeviceID || ""))) diskClass = "nvme";
  else if (/^usb$/i.test(bus)) diskClass = "usb";

  if (!id || !/^\d+:[A-Z]$/.test(id)) return null;
  return {
    ...disk,
    id,
    disk_number: diskNumber === undefined || diskNumber === null ? Number(id.split(":")[0]) : Number(diskNumber),
    model: disk.model || disk.Model || "Disco",
    volume: volume || (driveLetter ? String(driveLetter).toUpperCase() + ":\\" : ""),
    capacity_bytes: disk.capacity_bytes ?? disk.CapacityBytes ?? disk.size_bytes ?? disk.Size,
    volume_capacity_bytes: disk.volume_capacity_bytes ?? disk.VolumeCapacityBytes ?? disk.volume_size_bytes,
    free_bytes: disk.free_bytes ?? disk.FreeBytes ?? disk.SizeRemaining,
    class: diskClass,
    bus_type: bus || "Barramento desconhecido"
  };
}

async function refreshStorageDisks(preserveSelection = true) {
  const list = document.getElementById("storage-disk-list");
  const refreshButton = document.getElementById("storage-refresh-disks");
  const runButton = document.getElementById("diagnostic-run-selected");
  const selectAll = document.getElementById("storage-select-all");
  if (!list || !refreshButton || !runButton) return;

  const previous = preserveSelection ? new Set(selectedStorageDiskIds()) : new Set();
  const scanner = document.getElementById("storage-scan-indicator");
  if (scanner) scanner.hidden = false;
  refreshButton.disabled = true;
  setDiagnosticActivity("inventory:storage", true);
  list.replaceChildren();
  const loading = document.createElement("li");
  loading.className = "storage-disk-empty";
  loading.textContent = "Localizando discos e volumes...";
  list.appendChild(loading);
  runButton.disabled = true;

  const formatBytes = (value) => value !== null && value !== undefined && Number.isFinite(Number(value)) && Number(value) >= 0
    ? (Number(value) / (1024 ** 3)).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " GiB"
    : "não informado";

  try {
    let result = await Qboa.listStorageDisks();
    if (!Array.isArray(result) && result && Array.isArray(result.disks)) result = result.disks;
    const entries = Array.isArray(result) ? result.flat(Infinity).map(normalizeStorageDisk).filter(Boolean) : [];
    const disks = Array.from(new Map(entries.map((disk) => [disk.id, disk])).values());
    const groups = new Map();
    disks.forEach((disk) => {
      const key = Number.isFinite(disk.disk_number) ? "disk:" + disk.disk_number : "volume:" + disk.id;
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(disk);
    });
    list.replaceChildren();

    if (!disks.length) {
      const empty = document.createElement("li");
      empty.className = "storage-disk-empty";
      empty.textContent = "Nenhum volume com letra foi identificado. Unidades sem letra não podem receber este benchmark.";
      list.appendChild(empty);
    }

    let index = 0;
    groups.forEach((volumes) => {
      const first = volumes[0];
      const group = document.createElement("li");
      group.className = "storage-disk-group";
      const heading = document.createElement("div");
      heading.className = "storage-disk-group-heading";
      const model = document.createElement("strong");
      model.textContent = first.model || "Disco físico";
      const physicalMeta = document.createElement("small");
      const classNames = {
        notebook_hdd: "HD de notebook", desktop_hdd: "HD de desktop", sata_ssd: "SSD",
        nvme: "SSD", usb: "Armazenamento removível", unknown: "Classe não identificada"
      };
      physicalMeta.textContent = "Disco " + first.disk_number + " · " +
        (classNames[first.class] || classNames.unknown) + " · " + first.bus_type +
        " · Capacidade física: " + formatBytes(first.capacity_bytes);
      heading.append(model, physicalMeta);
      group.appendChild(heading);

      const volumeList = document.createElement("ul");
      volumeList.className = "storage-disk-volumes";
      volumes.forEach((disk) => {
        const item = document.createElement("li");
        item.className = "storage-disk-option";
        const label = document.createElement("label");
        const checkbox = document.createElement("input");
        checkbox.type = "checkbox";
        checkbox.value = disk.id;
        checkbox.dataset.storageDisk = "true";
        checkbox.checked = preserveSelection ? previous.has(disk.id) : index === 0;
        index++;
        const description = document.createElement("span");
        description.className = "storage-disk-description";
        const name = document.createElement("strong");
        name.textContent = disk.volume || "Volume sem letra";
        const meta = document.createElement("small");
        meta.textContent = "Volume: " + formatBytes(disk.volume_capacity_bytes) +
          " · Livre: " + formatBytes(disk.free_bytes);
        description.append(name, meta);
        label.append(checkbox, description);
        item.appendChild(label);
        volumeList.appendChild(item);
        checkbox.addEventListener("change", () => {
          if (selectAll) {
            const boxes = Array.from(list.querySelectorAll('input[type="checkbox"][data-storage-disk]'));
            selectAll.checked = boxes.length > 0 && boxes.every((box) => box.checked);
            selectAll.indeterminate = boxes.some((box) => box.checked) && !selectAll.checked;
          }
          runButton.disabled = selectedStorageDiskIds().length === 0;
        });
      });
      group.appendChild(volumeList);
      list.appendChild(group);
    });

    if (selectAll) {
      const boxes = Array.from(list.querySelectorAll('input[type="checkbox"][data-storage-disk]'));
      selectAll.checked = boxes.length > 0 && boxes.every((box) => box.checked);
      selectAll.indeterminate = boxes.some((box) => box.checked) && !selectAll.checked;
      selectAll.disabled = boxes.length === 0;
    }
    runButton.disabled = selectedStorageDiskIds().length === 0;
    document.getElementById("diagnostic-status").textContent = disks.length
      ? disks.length + " volume(s) em " + groups.size + " disco(s) físico(s). Selecione os volumes que deseja testar."
      : "Nenhum volume elegível encontrado.";
  } catch (error) {
    list.replaceChildren();
    const failed = document.createElement("li");
    failed.className = "storage-disk-empty";
    failed.textContent = "Falha ao localizar unidades: " + String(error);
    list.appendChild(failed);
    if (selectAll) {
      selectAll.checked = false;
      selectAll.indeterminate = false;
      selectAll.disabled = true;
    }
    document.getElementById("diagnostic-status").textContent = "Não foi possível atualizar as unidades.";
    runButton.disabled = true;
  } finally {
    refreshButton.disabled = false;
    if (scanner) scanner.hidden = true;
    setDiagnosticActivity("inventory:storage", false);
  }
}


function openWindowsUpdateRepair() {
  const confirmed = askConfirmation(
    "O QBoa vai remover somente políticas locais conhecidas por bloquear atualizações, reconfigurar os serviços essenciais e reativar as tarefas do Windows Update. Políticas impostas por domínio ou gerenciamento corporativo podem ser reaplicadas pelo próprio ambiente. Deseja continuar?",
    "Chuck, esta é a sua chance de se redimir",
    "Restaurar Windows Update",
    "Cancelar"
  );
  Promise.resolve(confirmed).then(async (accepted) => {
    if (!accepted) return;
    try {
      appendConsole("Iniciando a recuperação do Windows Update…");
      const result = await Qboa.repairWindowsUpdate();
      appendConsole(result);
      appendConsole("Recuperação concluída. Abra Configurações > Windows Update e procure atualizações para confirmar o funcionamento.");
    } catch (error) {
      appendConsole("Falha ao recuperar o Windows Update: " + String(error));
      await askConfirmation(String(error), "Não foi possível concluir", "Concluído", "Fechar");
    }
  });
}

function openDiagnosticModal(target = null, mode = "quick") {
  diagnosticHistoryRequestId++;
  activeDiagnosticTarget = target;
  activeDiagnosticMode = mode;
  const modal = document.getElementById("diagnostic-modal");
  const title = document.getElementById("diagnostic-modal-title");
  const intro = document.getElementById("diagnostic-modal-intro");
  const runButton = document.getElementById("diagnostic-run-selected");
  const oldPicker = document.getElementById("storage-disk-picker");
  if (oldPicker) oldPicker.remove();
  const memoryPanel = document.getElementById("memory-test-panel");
  const cpuPanel = document.getElementById("cpu-test-panel");
  const gpuPanel = document.getElementById("gpu-test-panel");
  const isMemoryTest = target === "teste_memoria";
  const isCpuTest = target === "processador";
  const isGpuTest = target === "placa_de_video";
  memoryPanel.hidden = !isMemoryTest;
  cpuPanel.hidden = !isCpuTest;
  gpuPanel.hidden = !isGpuTest;
  modal.querySelector(".diagnostic-modal-actions").hidden = isMemoryTest || isCpuTest || isGpuTest;
  document.getElementById("diagnostic-results").hidden = isMemoryTest || isCpuTest || isGpuTest;
  const generalSavedReports = modal.querySelector(".diagnostic-saved-reports:not(#gpu-saved-reports)");
  if (generalSavedReports) generalSavedReports.hidden = isMemoryTest || isGpuTest;
  const gpuSavedReports = document.getElementById("gpu-saved-reports");
  if (gpuSavedReports) gpuSavedReports.hidden = !isGpuTest;

  if (target === "armazenamento") {
    const picker = document.createElement("div");
    picker.className = "storage-disk-picker";
    picker.id = "storage-disk-picker";

    const toolbar = document.createElement("div");
    toolbar.className = "storage-disk-toolbar";
    const selectAllLabel = document.createElement("label");
    selectAllLabel.className = "storage-select-all-label";
    const selectAll = document.createElement("input");
    selectAll.type = "checkbox";
    selectAll.id = "storage-select-all";
    selectAll.disabled = true;
    selectAllLabel.append(selectAll, document.createTextNode("Selecionar todas"));
    const refreshButton = document.createElement("button");
    refreshButton.id = "storage-refresh-disks";
    refreshButton.type = "button";
    refreshButton.className = "secondary-button";
    refreshButton.textContent = "↻ Atualizar unidades";
    toolbar.append(selectAllLabel, refreshButton);

    const label = document.createElement("label");
    label.textContent = "Volumes que receberão o teste";
    label.htmlFor = "storage-disk-list";
    const help = document.createElement("p");
    help.className = "storage-disk-help";
    help.textContent = "O QBoa agrupa os volumes pelo disco físico. O benchmark cria um arquivo temporário de 256 MiB no volume escolhido e exige pelo menos 320 MiB livres. Não escreve diretamente nos setores do disco; o arquivo é removido ao final.";
    const list = document.createElement("ul");
    list.id = "storage-disk-list";
    list.className = "storage-disk-list";
    list.setAttribute("aria-label", "Selecione os volumes para o teste");
    const scanner = document.createElement("div");
    scanner.id = "storage-scan-indicator";
    scanner.className = "storage-scan-indicator";
    scanner.hidden = true;
    scanner.setAttribute("role", "status");
    scanner.innerHTML = '<span class="storage-scan-icon" aria-hidden="true"></span><span><strong>Procurando unidades</strong><small>Consultando discos físicos e volumes disponíveis…</small></span>';
    picker.append(toolbar, label, help, scanner, list);
    modal.querySelector(".diagnostic-modal-actions").before(picker);
    runButton.disabled = true;

    refreshButton.addEventListener("click", () => { void refreshStorageDisks(true); });
    selectAll.addEventListener("change", () => {
      list.querySelectorAll('input[type="checkbox"][data-storage-disk]').forEach((checkbox) => {
        checkbox.checked = selectAll.checked;
      });
      selectAll.indeterminate = false;
      runButton.disabled = selectedStorageDiskIds().length === 0;
    });
    void refreshStorageDisks(false);
  } else {
    runButton.disabled = false;
  }

  const labels = {
    armazenamento: ["Teste de armazenamento", "Coleta dados SMART disponíveis e executa oito medições sequenciais e aleatórias de leitura/escrita em arquivo temporário. Não executa teste de superfície nem escreve diretamente em setores do disco."],
    teste_memoria: ["Memória RAM", "Inventário dos módulos físicos reportados pelo Windows e teste de integridade com cobertura configurável. O teste usa buffers próprios do QBoa, não acessa endereços físicos e não certifica toda a RAM física."],
    placa_de_video: ["Teste de estabilidade da GPU", "Executa carga sintética em compute shader Direct3D 11 no adaptador de hardware selecionado pelo Windows, valida os resultados e acompanha temperatura. Exige sensor térmico compatível; não testa toda a VRAM nem todos os recursos gráficos."],
    processador: ["Teste de processador", "Executa uma carga computacional controlada por 5, 15 ou 30 minutos, acompanhada de temperatura, utilização, frequência e verificações aritméticas. O resultado é salvo em um laudo local."],
    temperatura: ["Teste de temperaturas", "Consulta leituras de sensores compatíveis quando o Windows ou os provedores disponíveis as expõem. Sensor indisponível não significa temperatura normal."]
  };
  if (target && labels[target]) {
    title.textContent = labels[target][0];
    intro.textContent = labels[target][1];
    runButton.textContent = "Executar teste";
  } else {
    title.textContent = "Diagnóstico completo";
    intro.textContent = "Executa a coleta completa das categorias disponíveis e salva um laudo local persistente. Itens sem leitura compatível serão marcados como indisponíveis, não como saudáveis.";
    runButton.textContent = "Executar diagnóstico completo";
  }
  document.getElementById("diagnostic-results").replaceChildren();
  if (target !== "armazenamento") document.getElementById("diagnostic-status").textContent = "";
  modal.classList.remove("hidden");
  modal.setAttribute("aria-hidden", "false");
  if (isMemoryTest) {
    void loadMemoryInventory();
    void loadMemoryTestSession();
    void loadMemoryDiagnosticReports();
    return;
  }
  if (isCpuTest) {
    void loadCpuInventory();
    void loadCpuTestSession();
    void loadDiagnosticHistory(true);
    return;
  }
  if (isGpuTest) {
    void loadGpuInventory();
    void loadGpuTestSession();
    void loadGpuDiagnosticReports();
    return;
  }
  void loadDiagnosticHistory(true);
}
function formatMemoryBytes(value) {
  const bytes = Number(value);
  if (!Number.isFinite(bytes) || bytes < 0) return "Indisponível";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let amount = bytes;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit++;
  }
  return amount.toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " " + units[unit];
}

const MIN_CUSTOM_MEMORY_BYTES = 128 * 1024 * 1024;
const CUSTOM_MEMORY_STEP_BYTES = 1024 * 1024;

function updateCustomMemoryBudgetLabel() {
  const slider = document.getElementById("memory-test-custom-budget");
  const output = document.getElementById("memory-test-custom-size");
  if (slider && output) output.textContent = formatMemoryBytes(slider.value);
}

async function loadMemoryInventory() {
  const summary = document.getElementById("memory-inventory-summary");
  const modulesList = document.getElementById("memory-modules-list");
  const note = document.getElementById("memory-inventory-note");
  const refresh = document.getElementById("memory-inventory-refresh");
  if (!summary || !modulesList || !note) return;
  refresh.disabled = true;
  setDiagnosticActivity("inventory:memory", true);
  summary.textContent = "Consultando módulos, slots e memória disponível…";
  modulesList.replaceChildren();
  try {
    const inventory = await Qboa.getMemoryInventory();
    memoryInventorySnapshot = inventory;
    const modules = Array.isArray(inventory.modules) ? inventory.modules : [];
    const total = Number(inventory.total_physical_bytes || 0);
    const available = Number(inventory.available_physical_bytes || 0);
    const slots = Number(inventory.slot_count || 0);
    summary.textContent = formatMemoryBytes(total) + " de RAM física · " +
      modules.length + " módulo(s) detectado(s)" +
      (slots > 0 ? " · " + modules.length + " de " + slots + " slot(s) ocupado(s)" : "") +
      " · " + formatMemoryBytes(available) + " disponível(is) agora";

    modules.forEach((module, index) => {
      const card = document.createElement("article");
      card.className = "memory-module-card";
      const heading = document.createElement("h4");
      heading.textContent = [module.manufacturer, module.part_number].filter(Boolean).join(" · ") || ("Módulo de RAM " + (index + 1));
      card.appendChild(heading);
      const detailGrid = document.createElement("dl");
      detailGrid.className = "memory-module-details";
      const profileLabel = /DDR5/i.test(module.memory_type || "")
        ? "Perfil XMP/EXPO máximo"
        : /DDR4|DDR3/i.test(module.memory_type || "")
          ? "Perfil XMP/DOCP máximo"
          : "Perfil de memória máximo";
      const fields = [
        ["Capacidade", formatMemoryBytes(module.capacity_bytes)],
        ["Tipo", module.memory_type || "Não informado"],
        ["Formato", module.form_factor || "Não informado"],
        ["Slot / banco", [module.device_locator, module.bank_label].filter(Boolean).join(" · ") || "Não informado"],
        ["Frequência configurada", module.configured_speed_mhz ? module.configured_speed_mhz + " MHz" : "Não informada pelo SMBIOS"],
        ["Ciclo reportado pelo SMBIOS", module.smbios_speed_ns ? module.smbios_speed_ns + " ns" : "Não informado"],
        [profileLabel, module.xmp_expo_max_speed_mhz ? module.xmp_expo_max_speed_mhz + " MHz" : "Não exposto pela API padrão do Windows"],
        ["Latência CAS", module.cas_latency ? "CL" + module.cas_latency : "Não exposta pela API padrão do Windows"],
        ["Largura de dados", module.data_width_bits ? module.data_width_bits + " bits" : "Não informada"],
        ["Largura total", module.total_width_bits ? module.total_width_bits + " bits" : "Não informada"],
        ["Tensão configurada", module.configured_voltage_mv ? (module.configured_voltage_mv / 1000).toLocaleString("pt-BR") + " V" : "Não informada"]
      ];
      fields.forEach(([label, value]) => {
        const dt = document.createElement("dt");
        dt.textContent = label;
        const dd = document.createElement("dd");
        dd.textContent = value;
        detailGrid.append(dt, dd);
      });
      card.appendChild(detailGrid);
      modulesList.appendChild(card);
    });
    if (!modules.length) {
      const empty = document.createElement("p");
      empty.className = "memory-inventory-empty";
      empty.textContent = "O Windows não retornou módulos individuais. O teste de integridade continua disponível.";
      modulesList.appendChild(empty);
    }
    const slider = document.getElementById("memory-test-custom-budget");
    const maxBytes = Math.max(MIN_CUSTOM_MEMORY_BYTES, total);
    slider.min = String(Math.min(MIN_CUSTOM_MEMORY_BYTES, total));
    slider.max = String(maxBytes);
    slider.step = String(CUSTOM_MEMORY_STEP_BYTES);
    if (!slider.dataset.userAdjusted) {
      slider.value = String(Math.min(1024 * 1024 * 1024, maxBytes));
    } else {
      slider.value = String(Math.min(Number(slider.value), maxBytes));
    }
    slider.disabled = !total || ["running", "cancelling"].includes(memoryTestSessionSnapshot?.lifecycle);
    document.getElementById("memory-test-custom-maximum").textContent = formatMemoryBytes(total) + " (RAM total)";
    updateCustomMemoryBudgetLabel();
    note.textContent = inventory.timing_limitation ||
      "Alguns dados de temporização e perfis de memória não são expostos pela API padrão do Windows.";
    (inventory.warnings || []).forEach((warning) => {
      const line = document.createElement("p");
      line.textContent = warning;
      modulesList.appendChild(line);
    });
  } catch (error) {
    memoryInventorySnapshot = null;
    summary.textContent = "Não foi possível consultar o inventário físico: " + String(error);
    note.textContent = "O teste de integridade continua disponível, mas o perfil personalizado precisa da leitura da RAM física para definir seu limite.";
    document.getElementById("memory-test-custom-budget").disabled = true;
  } finally {
    refresh.disabled = false;
    setDiagnosticActivity("inventory:memory", false);
  }
}

function memoryPhaseLabel(phase) {
  const labels = {
    bloco_alocado: "Bloco de memória alocado",
    bloco_concluido: "Bloco concluído",
    concluido: "Execução concluída",
    divergencias_detectadas: "Divergências detectadas",
    cancelado: "Cancelamento concluído",
    inconclusivo: "Execução inconclusiva",
    falhou: "Falha operacional"
  };
  if (labels[phase]) return labels[phase];
  const match = /^(executando|concluido)_(A0[1-5])$/.exec(String(phase || ""));
  if (match) return (match[1] === "executando" ? "Executando " : "Concluído ") + match[2];
  return phase || "Preparando execução";
}

function memoryTestStatusLabel(status) {
  return ({
    passed: "Concluído sem divergências observadas nos testes executados.",
    errors_detected: "Foram detectadas divergências nos buffers testados.",
    cancelled: "Teste cancelado. Este resultado não representa aprovação.",
    inconclusive: "Teste inconclusivo: a cobertura planejada não foi concluída.",
    failed: "Falha operacional: a execução não pôde ser concluída com confiança."
  })[status] || "Aguardando resultado.";
}

function logMemoryTestActivity(session) {
  if (lastMemoryActivitySessionId !== session.session_id) {
    lastMemoryActivitySessionId = session.session_id;
    lastMemoryActivityLifecycle = null;
    lastMemoryActivityPhase = null;
  }

  if (session.lifecycle !== lastMemoryActivityLifecycle) {
    if (session.lifecycle === "running") {
      appendConsole("Memória RAM · teste de integridade iniciado · perfil " +
        String(session.profile || "não informado"));
    } else if (session.lifecycle === "cancelling") {
      appendConsole("Memória RAM · cancelamento solicitado; aguardando encerramento seguro.");
    } else if (session.lifecycle === "failed") {
      appendConsole("Memória RAM · falha ao executar o teste: " +
        String(session.error || "erro sem detalhes"));
    } else if (session.lifecycle === "completed" && session.report) {
      const report = session.report;
      appendConsole("Memória RAM · teste finalizado: " + memoryTestStatusLabel(report.status) +
        " · " + formatMemoryBytes(report.memory_tested_bytes) + " testados · " +
        Number(report.duration_ms || 0).toLocaleString("pt-BR") + " ms · " +
        Number(report.mismatches_total || 0).toLocaleString("pt-BR") + " divergência(s)");
    }
    lastMemoryActivityLifecycle = session.lifecycle;
  }

  const phase = session.progress?.phase;
  if (session.lifecycle === "running" && phase && phase !== lastMemoryActivityPhase) {
    appendConsole("Memória RAM · " + memoryPhaseLabel(phase));
    lastMemoryActivityPhase = phase;
  }
}

function renderMemoryReport(report) {
  const result = document.getElementById("memory-test-result");
  const details = document.getElementById("memory-test-result-details");
  details.replaceChildren();
  const summary = document.createElement("p");
  summary.textContent = memoryTestStatusLabel(report.status);
  result.replaceChildren(summary);
  const meta = document.createElement("p");
  meta.className = "memory-test-report-meta";
  const parsed = report.started_at ? new Date(report.started_at) : null;
  const started = parsed && !Number.isNaN(parsed.valueOf()) ? parsed.toLocaleString("pt-BR") : "data indisponível";
  meta.textContent = "Perfil " + report.profile + " · " + formatMemoryBytes(report.memory_tested_bytes) +
    " testados · " + Number(report.duration_ms || 0).toLocaleString("pt-BR") + " ms · " +
    Number(report.mismatches_total || 0).toLocaleString("pt-BR") + " divergência(s) agregada(s) · " + started;
  result.appendChild(meta);
  if (report.coverage_description) {
    const coverage = document.createElement("p");
    coverage.textContent = report.coverage_description;
    result.appendChild(coverage);
  }
  (report.warnings || []).forEach((warning) => {
    const item = document.createElement("li");
    item.textContent = warning;
    details.appendChild(item);
  });
  if (report.mismatches_total > 0) {
    const note = document.createElement("p");
    note.className = "memory-test-warning";
    note.textContent = "Os offsets informados são posições dentro dos buffers virtuais do processo, não endereços físicos. A divergência observada, por si só, não identifica qual componente causou o problema.";
    result.appendChild(note);
  }
}

async function loadMemoryDiagnosticReports() {
  const list = document.getElementById("memory-diagnostic-history");
  if (!list) return;
  try {
    const reports = await Qboa.getMemoryDiagnosticReports();
    list.replaceChildren();
    if (!Array.isArray(reports) || !reports.length) {
      const empty = document.createElement("li");
      empty.textContent = "Nenhum laudo de memória salvo ainda.";
      list.appendChild(empty);
      return;
    }
    reports.forEach((report) => {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "diagnostic-history-item";
      const date = document.createElement("span");
      const parsed = report.started_at ? new Date(report.started_at) : null;
      date.textContent = parsed && !Number.isNaN(parsed.valueOf()) ? parsed.toLocaleString("pt-BR") : "Data indisponível";
      const detail = document.createElement("small");
      detail.textContent = "Memória RAM · " + report.profile + " · " + memoryTestStatusLabel(report.status);
      button.append(date, detail);
      button.addEventListener("click", () => {
        renderMemoryReport(report);
        const status = document.getElementById("diagnostic-status");
        if (status) status.textContent = "Laudo de memória recuperado · " + (report.report_id || "ID indisponível");
      });
      item.appendChild(button);
      list.appendChild(item);
    });
  } catch (error) {
    list.replaceChildren();
    const failed = document.createElement("li");
    failed.textContent = "Não foi possível carregar o histórico de memória: " + String(error);
    list.appendChild(failed);
  }
}

function renderMemoryTestSession(session) {
  if (!session) { memoryTestSessionSnapshot = null; return; }
  logMemoryTestActivity(session);
  if (memoryTestSessionSnapshot) {
    const sameSession = memoryTestSessionSnapshot.session_id === session.session_id;
    const lifecycleRank = { running: 1, cancelling: 2, completed: 3, failed: 3 };
    const currentRank = lifecycleRank[memoryTestSessionSnapshot.lifecycle] || 0;
    const incomingRank = lifecycleRank[session.lifecycle] || 0;
    if (sameSession) {
      if (incomingRank < currentRank) return;
      const currentBytes = Number(memoryTestSessionSnapshot.progress?.memory_tested_bytes || 0);
      const incomingBytes = Number(session.progress?.memory_tested_bytes || 0);
      if (incomingRank === currentRank && incomingBytes < currentBytes) return;
    } else if (currentRank < 3 || String(session.started_at || "") <= String(memoryTestSessionSnapshot.started_at || "")) return;
  }
  memoryTestSessionSnapshot = session;
  const active = ["running", "cancelling"].includes(session.lifecycle);
  setDiagnosticActivity("session:memory", active);
  const profile = document.getElementById("memory-test-profile");
  if (session.profile && ["fast", "standard", "extended", "custom"].includes(session.profile)) profile.value = session.profile;
  const customPicker = document.getElementById("memory-test-custom-picker");
  const customBudget = document.getElementById("memory-test-custom-budget");
  customPicker.hidden = profile.value !== "custom";
  customBudget.disabled = active || !memoryInventorySnapshot?.total_physical_bytes;
  const start = document.getElementById("memory-test-start");
  const cancel = document.getElementById("memory-test-cancel");
  const progress = session.progress;
  const percent = Math.max(0, Math.min(100, Number(progress?.percent || 0)));
  profile.disabled = active;
  start.disabled = active;
  start.textContent = active ? "Teste em andamento" : "Iniciar teste de RAM";
  cancel.hidden = !active;
  cancel.disabled = session.lifecycle === "cancelling";
  cancel.textContent = session.lifecycle === "cancelling" ? "Cancelando…" : "Cancelar teste";
  document.getElementById("memory-test-progress-bar").style.width = percent + "%";
  document.querySelector(".memory-test-progress-track").setAttribute("aria-valuenow", String(percent));
  document.getElementById("memory-test-progress-percent").textContent = percent + "%";
  const tested = formatMemoryBytes(progress?.memory_tested_bytes);
  const budget = formatMemoryBytes(progress?.memory_budget_bytes);
  const block = progress?.current_block_bytes ? " · bloco atual " + formatMemoryBytes(progress.current_block_bytes) : "";
  document.getElementById("memory-test-progress-detail").textContent = progress
    ? tested + " de " + budget + " contabilizados · " + (progress.blocks_completed || 0) + " bloco(s) concluído(s)" + block + " · " + (progress.mismatches_total || 0) + " divergência(s)"
    : "Aguardando a primeira atualização do runner.";
  document.getElementById("memory-test-progress-phase").textContent =
    session.lifecycle === "cancelling" ? "Cancelamento solicitado; aguardando o término seguro do bloco atual." :
    session.lifecycle === "running" ? memoryPhaseLabel(progress?.phase) :
    session.lifecycle === "failed" ? "Falha ao iniciar a sessão" : "Execução finalizada";
  if (session.report) renderMemoryReport({ ...session.report, started_at: session.started_at });
  else {
    const result = document.getElementById("memory-test-result");
    if (session.error) result.textContent = session.error;
    else if (active) result.textContent = session.lifecycle === "cancelling"
      ? "Cancelamento solicitado. A sessão ainda está encerrando com segurança."
      : "O teste está sendo executado. Você pode cancelar a qualquer momento.";
    else result.textContent = "A sessão ainda não tem um relatório final.";
  }
}
async function loadMemoryTestSession() {
  try {
    const session = await Qboa.getMemoryTestSession();
    renderMemoryTestSession(session);
  } catch (error) {
    document.getElementById("memory-test-result").textContent =
      "Não foi possível consultar a sessão: " + String(error);
  }
}

async function startMemoryTestFromUi() {
  const profile = document.getElementById("memory-test-profile").value;
  const customBudget = document.getElementById("memory-test-custom-budget");
  const memoryBudgetBytes = profile === "custom" ? Number(customBudget.value) : null;
  const result = document.getElementById("memory-test-result");
  const start = document.getElementById("memory-test-start");
  start.disabled = true;
  setDiagnosticActivity("starting:memory", true);
  result.textContent = "Preparando a sessão de teste de RAM…";
  try {
    const session = await Qboa.startMemoryTest(profile, memoryBudgetBytes);
    renderMemoryTestSession(session);
    await loadMemoryTestSession();
  } catch (error) {
    result.textContent = String(error);
    start.disabled = false;
  } finally {
    setDiagnosticActivity("starting:memory", false);
  }
}

async function cancelMemoryTestFromUi() {
  const sessionId = memoryTestSessionSnapshot?.session_id;
  if (!sessionId) {
    document.getElementById("memory-test-result").textContent =
      "Não há identificador de sessão disponível para cancelamento.";
    return;
  }
  const cancel = document.getElementById("memory-test-cancel");
  cancel.disabled = true;
  try {
    renderMemoryTestSession(await Qboa.cancelMemoryTest(sessionId));
  } catch (error) {
    document.getElementById("memory-test-result").textContent = String(error);
    cancel.disabled = false;
  }
}

function closeDiagnosticModal() {
  if (completeDiagnosticRunning) return;
  const modal = document.getElementById("diagnostic-modal");
  modal.classList.add("hidden");
  modal.setAttribute("aria-hidden", "true");
}


const COMPLETE_DIAGNOSTIC_STAGES = [
  { label: "Sistema e identificação do Windows", start: 0, span: 8 },
  { label: "Armazenamento", start: 8, span: 20 },
  { label: "Memória RAM", start: 28, span: 18 },
  { label: "Processador", start: 46, span: 27 },
  { label: "Placa de vídeo", start: 73, span: 27 }
];

function setCompleteDiagnosticStage(index, state, detail = "") {
  const item = document.querySelector('#complete-diagnostic-steps [data-step="' + index + '"]');
  if (!item) return;
  item.dataset.state = state;
  const status = item.querySelector("small");
  if (status) status.textContent = detail || ({
    pending: "Aguardando",
    running: "Em andamento",
    completed: "Concluído",
    warning: "Concluído com alerta",
    failed: "Falha",
    skipped: "Não executado"
  }[state] || state);
}

function updateCompleteDiagnosticProgress(index, localPercent, detail) {
  const stage = COMPLETE_DIAGNOSTIC_STAGES[index];
  const percent = Math.max(0, Math.min(100, Math.round(stage.start + stage.span * localPercent / 100)));
  document.getElementById("complete-diagnostic-stage").textContent = stage.label;
  document.getElementById("complete-diagnostic-detail").textContent = detail || "";
  document.getElementById("complete-diagnostic-percent").textContent = percent + "%";
  const bar = document.getElementById("complete-diagnostic-progress-bar");
  bar.style.width = percent + "%";
  document.getElementById("complete-diagnostic-progress-track").setAttribute("aria-valuenow", String(percent));
  setCompleteDiagnosticStage(index, "running", detail || "Em andamento");
}

function addCompleteDiagnosticResult(results, category, status, evidence, source) {
  const result = {
    category,
    status,
    evidence: Array.isArray(evidence) && evidence.length ? evidence : ["Não foi possível obter evidências confiáveis."],
    source: Array.isArray(source) && source.length ? source : ["Motor de diagnóstico QBoa"]
  };
  const index = results.findIndex((item) => item.category === category);
  if (index >= 0) results[index] = result;
  else results.push(result);
  return result;
}

function completeDiagnosticStatus(status, errors = 0) {
  if (status === "passed" || status === "completed") return errors > 0 ? "falha" : "ok";
  if (status === "errors_detected" || status === "failed" || status === "device_error") return "falha";
  if (status === "thermal_limit_reached" || status === "inconclusive" || status === "partial") return "atencao";
  return "indisponivel";
}

function completeDiagnosticNumber(value, suffix = "", digits = 1) {
  const number = Number(value);
  return value === null || value === undefined || !Number.isFinite(number)
    ? "indisponível"
    : number.toLocaleString("pt-BR", { maximumFractionDigits: digits }) + suffix;
}

async function waitForCompleteDiagnosticSession(kind, initial, getSession, progressIndex) {
  const cancelMethods = {
    memory: (id) => Qboa.cancelMemoryTest(id),
    cpu: (id) => Qboa.cancelCpuTest(id),
    gpu: (id) => Qboa.cancelGpuTest(id)
  };
  completeDiagnosticCurrentSession = { kind, sessionId: initial.session_id };
  let session = initial;
  while (["running", "cancelling"].includes(session?.lifecycle)) {
    if (completeDiagnosticCancelRequested && !completeDiagnosticCancelSent) {
      completeDiagnosticCancelSent = true;
      try { await cancelMethods[kind](initial.session_id); } catch (error) {
        console.warn("Não foi possível solicitar cancelamento do teste " + kind + ":", error);
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
    const latest = await getSession();
    if (latest && latest.session_id === initial.session_id) session = latest;
    const local = kind === "memory"
      ? Number(session?.progress?.percent || 0)
      : Number(session?.progress_percent || 0);
    const label = kind === "memory"
      ? (session?.progress?.phase || "Executando teste de memória")
      : "Execução " + Math.max(0, Math.min(100, local)) + "%";
    updateCompleteDiagnosticProgress(progressIndex, local, label);
    if (kind === "memory") renderMemoryTestSession(session);
    if (kind === "cpu") renderCpuTestSession(session);
    if (kind === "gpu") renderGpuTestSession(session);
  }
  completeDiagnosticCurrentSession = null;
  return session;
}

async function cancelCompleteDiagnostic() {
  if (!completeDiagnosticRunning || completeDiagnosticCancelRequested) return;
  completeDiagnosticCancelRequested = true;
  const button = document.getElementById("complete-diagnostic-cancel");
  button.disabled = true;
  button.textContent = "Cancelamento solicitado…";
  const current = completeDiagnosticCurrentSession;
  if (!current) return;
  completeDiagnosticCancelSent = true;
  try {
    if (current.kind === "memory") await Qboa.cancelMemoryTest(current.sessionId);
    else if (current.kind === "cpu") await Qboa.cancelCpuTest(current.sessionId);
    else if (current.kind === "gpu") await Qboa.cancelGpuTest(current.sessionId);
  } catch (error) {
    document.getElementById("complete-diagnostic-detail").textContent =
      "Não foi possível confirmar o cancelamento: " + String(error);
  }
}

async function runCompleteDiagnostic() {
  if (completeDiagnosticRunning) return;
  if (activeTaskId || launchPending) {
    document.getElementById("diagnostic-status").textContent = "Conclua a limpeza ou manutenção em andamento antes de iniciar o diagnóstico completo.";
    return;
  }
  const confirmed = await askConfirmation(
    "O diagnóstico completo executa medições em todas as unidades de armazenamento, um teste padrão de RAM e testes de carga de CPU e GPU. Os testes de CPU e GPU duram 5 minutos cada; o tempo total depende da quantidade e velocidade dos discos e do hardware. Durante os testes de carga, o computador pode ficar mais lento. A temperatura é monitorada e os testes são interrompidos pelos limites de segurança do QBoa. Deseja iniciar?",
    "Diagnóstico completo · operação demorada"
  );
  if (!confirmed) return;

  const runButton = document.getElementById("diagnostic-run-selected");
  const progressPanel = document.getElementById("complete-diagnostic-progress");
  const summaryPanel = document.getElementById("complete-diagnostic-summary");
  const resultsList = document.getElementById("diagnostic-results");
  const statusLine = document.getElementById("diagnostic-status");
  const closeButton = document.getElementById("diagnostic-modal-close");
  const cancelButton = document.getElementById("complete-diagnostic-cancel");
  const startedAt = Date.now();
  let reportId = null;
  let system = null;
  const results = [];
  const issues = [];
  completeDiagnosticRunning = true;
  setDiagnosticActivity("complete:diagnostic", true);
  completeDiagnosticCancelRequested = false;
  completeDiagnosticCancelSent = false;
  completeDiagnosticCurrentSession = null;
  runButton.hidden = true;
  runButton.disabled = true;
  closeButton.disabled = true;
  progressPanel.hidden = false;
  summaryPanel.hidden = true;
  resultsList.hidden = true;
  resultsList.replaceChildren();
  statusLine.textContent = "Preparando diagnóstico completo…";
  cancelButton.disabled = false;
  cancelButton.textContent = "Cancelar diagnóstico";
  COMPLETE_DIAGNOSTIC_STAGES.forEach((stage, index) => setCompleteDiagnosticStage(index, "pending"));
  updateCompleteDiagnosticProgress(0, 0, "Iniciando coleta do sistema.");

  const issue = (severity, component, code, message) => {
    issues.push({ severity, component, code, message: String(message).slice(0, 1000), occurred_at: new Date().toISOString() });
  };
  const checkpoint = async (executionStatus = "running") => {
    if (!reportId) return;
    await Qboa.updateCompleteDiagnosticReport(
      reportId, results, issues, system, executionStatus,
      executionStatus === "running" ? null : Date.now() - startedAt
    );
  };

  try {
    // Esta coleta cria o laudo versionado e fornece a identificação do Windows.
    const base = await Qboa.runDiagnosticCheck("complete");
    reportId = base.report_id;
    system = base.system || null;
    results.push(...(base.results || []));
    await checkpoint("running");
    setCompleteDiagnosticStage(0, "completed", "Identificação do Windows registrada.");
    updateCompleteDiagnosticProgress(0, 100, "Sistema identificado e laudo iniciado.");

    // Armazenamento: todas as unidades locais elegíveis, sem exibir recomendações.
    updateCompleteDiagnosticProgress(1, 0, "Localizando unidades de armazenamento.");
    let disks = [];
    try {
      const raw = await Qboa.listStorageDisks();
      const entries = Array.isArray(raw) ? raw.flat(Infinity) : Array.isArray(raw?.disks) ? raw.disks : [];
      disks = Array.from(new Map(entries.map(normalizeStorageDisk).filter(Boolean).map((disk) => [disk.id, disk])).values());
    } catch (error) {
      issue("warning", "storage", "disk_inventory_failed", "Não foi possível listar as unidades: " + String(error));
    }
    const storageEvidence = [];
    let storageCompleted = 0;
    let storagePartial = 0;
    let storageSmartAlerts = false;
    if (!disks.length) {
      storageEvidence.push("Nenhuma unidade elegível foi identificada para o teste.");
    }
    for (let index = 0; index < disks.length; index++) {
      if (completeDiagnosticCancelRequested) break;
      const disk = disks[index];
      updateCompleteDiagnosticProgress(1, index * 100 / Math.max(1, disks.length), "Testando unidade " + disk.id + " (" + (index + 1) + "/" + disks.length + ").");
      try {
        const report = await Qboa.runStorageDiagnostic(disk.id);
        const name = report.disk_name || report.volume || disk.id;
        storageEvidence.push(name + ": " + (report.execution_status || "resultado recebido"));
        const measurementNames = {
          seq_small_write: "Sequencial pequeno · gravação",
          seq_small_read: "Sequencial pequeno · leitura",
          seq_large_write: "Sequencial grande · gravação",
          seq_large_read: "Sequencial grande · leitura",
          random_small_write: "Aleatório pequeno · gravação",
          random_small_read: "Aleatório pequeno · leitura",
          random_large_write: "Aleatório grande · gravação",
          random_large_read: "Aleatório grande · leitura"
        };
        (report.measurements || []).forEach((measurement) => {
          storageEvidence.push((measurementNames[measurement.kind] || String(measurement.kind || "Medição")) + ": " +
            completeDiagnosticNumber(measurement.value, measurement.unit ? " " + measurement.unit : "", 2));
        });
        const smart = report.smart || {};
        storageEvidence.push("SMART: " + (smart.usable ? "dados utilizáveis" : "dados utilizáveis indisponíveis"));
        if (smart.predicted_failure !== null && smart.predicted_failure !== undefined) {
          storageEvidence.push("Previsão de falha SMART: " + (smart.predicted_failure ? "sinalizada" : "não sinalizada"));
        }
        [["reallocated", "Setores realocados"], ["pending", "Setores pendentes"],
          ["uncorrectable", "Setores incorrigíveis"], ["temperature_c", "Temperatura SMART"]].forEach(([key, label]) => {
          if (smart[key] !== null && smart[key] !== undefined) {
            storageEvidence.push(label + ": " + completeDiagnosticNumber(smart[key], key === "temperature_c" ? " °C" : "", 1));
          }
        });
        if (smart.interface_crc_before !== null && smart.interface_crc_before !== undefined) {
          storageEvidence.push("Erros CRC da interface: " + completeDiagnosticNumber(smart.interface_crc_before, "", 0) +
            " → " + completeDiagnosticNumber(smart.interface_crc_after, "", 0));
        }
        if (smart.wear_percent !== null && smart.wear_percent !== undefined) {
          storageEvidence.push("Desgaste SMART: " + completeDiagnosticNumber(smart.wear_percent, "%", 1));
        }
        if (smart.pio_mode === true) storageEvidence.push("Modo PIO: detectado");
        if (smart.slow_sata_link === true) storageEvidence.push("Link SATA lento: detectado");
        const crcIncreased = Number(smart.interface_crc_after) > Number(smart.interface_crc_before) &&
          smart.interface_crc_before !== null && smart.interface_crc_before !== undefined &&
          smart.interface_crc_after !== null && smart.interface_crc_after !== undefined;
        const diskSmartAlert = smart.predicted_failure === true ||
          Number(smart.reallocated) > 0 || Number(smart.pending) > 0 ||
          Number(smart.uncorrectable) > 0 || crcIncreased ||
          smart.pio_mode === true || smart.slow_sata_link === true;
        if (diskSmartAlert) {
          storageSmartAlerts = true;
          issue("warning", "storage", "smart_or_link_alert", name + ": o laudo SMART ou a interface reportou uma condição que merece atenção.");
        }
        (report.warnings || []).forEach((warning) => storageEvidence.push("Aviso: " + warning));
        if (report.execution_status === "completed") storageCompleted++;
        else storagePartial++;
      } catch (error) {
        storagePartial++;
        storageEvidence.push(disk.id + ": falha ao executar o teste — " + String(error));
        issue("warning", "storage", "storage_test_failed", disk.id + ": " + String(error));
      }
      await checkpoint("running");
      updateCompleteDiagnosticProgress(1, (index + 1) * 100 / Math.max(1, disks.length),
        "Unidade " + (index + 1) + " de " + disks.length + " processada.");
    }
    const storageStatus = !disks.length || (!storageCompleted && !storagePartial)
      ? "indisponivel"
      : !storageCompleted && storagePartial ? "falha"
      : storagePartial || storageSmartAlerts ? "atencao" : "informativo";
    if (completeDiagnosticCancelRequested && storageCompleted + storagePartial < disks.length) {
      storageEvidence.push("As unidades restantes não foram testadas porque o cancelamento foi solicitado.");
      issue("warning", "storage", "storage_test_cancelled", "O diagnóstico foi cancelado durante a etapa de armazenamento.");
    }
    addCompleteDiagnosticResult(results, "armazenamento", storageStatus, storageEvidence,
      ["Motor de diagnóstico de armazenamento QBoa", "Laudo individual com medições e dados SMART quando disponíveis"]);
    setCompleteDiagnosticStage(1, completeDiagnosticCancelRequested || !disks.length || storagePartial || storageSmartAlerts ? "warning" : "completed",
      storageCompleted + " concluída(s), " + storagePartial + " parcial(is)/falha(s), " + disks.length + " unidade(s).");
    await checkpoint("running");

    // RAM: perfil padrão de 3 GiB, com cobertura descrita no próprio laudo.
    if (!completeDiagnosticCancelRequested) {
      updateCompleteDiagnosticProgress(2, 0, "Preparando teste de integridade da RAM (perfil padrão).");
      try {
        const initial = await Qboa.startMemoryTest("standard");
        const session = await waitForCompleteDiagnosticSession("memory", initial, () => Qboa.getMemoryTestSession(), 2);
        const report = session?.report;
        if (report) {
          const status = completeDiagnosticStatus(report.status);
          const evidence = [
            "Resultado: " + report.status,
            "Memória testada: " + completeDiagnosticNumber(report.memory_tested_bytes / (1024 ** 3), " GiB", 2),
            "Divergências: " + completeDiagnosticNumber(report.mismatches_total, "", 0),
            report.coverage_description || "Cobertura não informada.",
            "Passes: " + report.passes_completed + "/" + report.passes_planned
          ];
          (report.warnings || []).forEach((warning) => evidence.push("Aviso: " + warning));
          addCompleteDiagnosticResult(results, "memoria_ram", status, evidence, ["QBoa Memory Test · A01–A05"]);
          if (status !== "ok") issue(status === "falha" ? "error" : "warning", "memory", "memory_test_" + report.status, "O teste de RAM terminou com estado " + report.status + ".");
          setCompleteDiagnosticStage(2, status === "ok" ? "completed" : status === "falha" ? "failed" : "warning", "Teste de RAM: " + report.status + ".");
        } else {
          throw new Error(session?.error || "O motor não retornou um laudo de RAM.");
        }
      } catch (error) {
        addCompleteDiagnosticResult(results, "memoria_ram", "indisponivel", ["Não foi possível concluir o teste de RAM: " + String(error)], ["QBoa Memory Test"]);
        issue("warning", "memory", "memory_test_unavailable", String(error));
        setCompleteDiagnosticStage(2, "warning", "Teste de RAM indisponível.");
      }
      await checkpoint("running");
    } else {
      addCompleteDiagnosticResult(results, "memoria_ram", "indisponivel", ["Não executado: diagnóstico cancelado antes desta etapa."], ["QBoa Memory Test"]);
      setCompleteDiagnosticStage(2, "skipped", "Não executado por cancelamento.");
    }

    // CPU: perfil rápido de cinco minutos, com monitoramento térmico.
    if (!completeDiagnosticCancelRequested) {
      updateCompleteDiagnosticProgress(3, 0, "Preparando teste de carga da CPU (5 minutos).");
      try {
        const initial = await Qboa.startCpuTest("quick");
        const session = await waitForCompleteDiagnosticSession("cpu", initial, () => Qboa.getCpuTestSession(), 3);
        const report = session?.report;
        if (report) {
          const status = completeDiagnosticStatus(report.execution_status, report.calculation_errors);
          const evidence = [
            "Resultado: " + report.execution_status,
            "Duração: " + completeDiagnosticNumber(report.duration_ms / 60000, " min", 2),
            "Temperatura média/máxima: " + completeDiagnosticNumber(report.temperature_average_c, " °C") + " / " + completeDiagnosticNumber(report.temperature_max_c, " °C"),
            "Utilização média/máxima: " + completeDiagnosticNumber(report.cpu_utilization_average_percent, "%") + " / " + completeDiagnosticNumber(report.cpu_utilization_max_percent, "%"),
            "Iterações: " + completeDiagnosticNumber(report.iterations, "", 0) + " · erros de cálculo: " + completeDiagnosticNumber(report.calculation_errors, "", 0)
          ];
          (report.warnings || []).forEach((warning) => evidence.push("Aviso: " + warning));
          addCompleteDiagnosticResult(results, "processador", status, evidence, ["QBoa CPU Stress Test", report.sensor_provider || "Telemetria térmica"]);
          if (status !== "ok") issue(status === "falha" ? "error" : "warning", "cpu", "cpu_test_" + report.execution_status, "O teste de CPU terminou com estado " + report.execution_status + ".");
          setCompleteDiagnosticStage(3, status === "ok" ? "completed" : status === "falha" ? "failed" : "warning", "Teste de CPU: " + report.execution_status + ".");
        } else {
          throw new Error(session?.error || "O motor não retornou um laudo de CPU.");
        }
      } catch (error) {
        addCompleteDiagnosticResult(results, "processador", "indisponivel", ["Não foi possível concluir o teste de CPU: " + String(error)], ["QBoa CPU Stress Test"]);
        issue("warning", "cpu", "cpu_test_unavailable", String(error));
        setCompleteDiagnosticStage(3, "warning", "Teste de CPU indisponível.");
      }
      await checkpoint("running");
    } else {
      addCompleteDiagnosticResult(results, "processador", "indisponivel", ["Não executado: diagnóstico cancelado antes desta etapa."], ["QBoa CPU Stress Test"]);
      setCompleteDiagnosticStage(3, "skipped", "Não executado por cancelamento.");
    }

    // GPU: perfil rápido de cinco minutos, com validação Direct3D 11.
    if (!completeDiagnosticCancelRequested) {
      updateCompleteDiagnosticProgress(4, 0, "Preparando teste de estabilidade da GPU (5 minutos).");
      try {
        const initial = await Qboa.startGpuTest("quick");
        const session = await waitForCompleteDiagnosticSession("gpu", initial, () => Qboa.getGpuTestSession(), 4);
        const report = session?.report;
        if (report) {
          const status = completeDiagnosticStatus(report.execution_status, report.validation_errors);
          const evidence = [
            "Resultado: " + report.execution_status,
            "Adaptadores: " + ((report.adapter_names || []).join(", ") || "não identificados"),
            "Duração: " + completeDiagnosticNumber(report.duration_ms / 60000, " min", 2),
            "Temperatura média/máxima: " + completeDiagnosticNumber(report.temperature_average_c, " °C") + " / " + completeDiagnosticNumber(report.temperature_max_c, " °C"),
            "Utilização média/máxima: " + completeDiagnosticNumber(report.utilization_average_percent, "%") + " / " + completeDiagnosticNumber(report.utilization_max_percent, "%"),
            "Despachos: " + completeDiagnosticNumber(report.dispatches, "", 0) + " · divergências: " + completeDiagnosticNumber(report.validation_errors, "", 0)
          ];
          (report.warnings || []).forEach((warning) => evidence.push("Aviso: " + warning));
          addCompleteDiagnosticResult(results, "placa_de_video", status, evidence, ["QBoa GPU Stability Test · Direct3D 11 Compute Shader"]);
          if (status !== "ok") issue(status === "falha" ? "error" : "warning", "gpu", "gpu_test_" + report.execution_status, "O teste de GPU terminou com estado " + report.execution_status + ".");
          setCompleteDiagnosticStage(4, status === "ok" ? "completed" : status === "falha" ? "failed" : "warning", "Teste de GPU: " + report.execution_status + ".");
        } else {
          throw new Error(session?.error || "O motor não retornou um laudo de GPU.");
        }
      } catch (error) {
        addCompleteDiagnosticResult(results, "placa_de_video", "indisponivel", ["Não foi possível concluir o teste de GPU: " + String(error)], ["QBoa GPU Stability Test"]);
        issue("warning", "gpu", "gpu_test_unavailable", String(error));
        setCompleteDiagnosticStage(4, "warning", "Teste de GPU indisponível.");
      }
      await checkpoint("running");
    } else {
      addCompleteDiagnosticResult(results, "placa_de_video", "indisponivel", ["Não executado: diagnóstico cancelado antes desta etapa."], ["QBoa GPU Stability Test"]);
      setCompleteDiagnosticStage(4, "skipped", "Não executado por cancelamento.");
    }

    const finalStatus = completeDiagnosticCancelRequested ? "cancelled" : "completed";
    await checkpoint(finalStatus);
    if (completeDiagnosticCancelRequested) {
      document.getElementById("complete-diagnostic-detail").textContent = "Diagnóstico cancelado; os resultados já coletados foram preservados.";
    } else {
      updateCompleteDiagnosticProgress(4, 100, "Todas as etapas foram processadas.");
    }
    const counts = results.reduce((acc, item) => { acc[item.status] = (acc[item.status] || 0) + 1; return acc; }, {});
    const summary = document.getElementById("complete-diagnostic-summary");
    summary.hidden = false;
    summary.textContent = (completeDiagnosticCancelRequested ? "Diagnóstico interrompido. " : "Diagnóstico finalizado. ") +
      (counts.ok || 0) + " aprovado(s), " + (counts.informativo || 0) + " informativo(s), " +
      (counts.atencao || 0) + " alerta(s), " + (counts.falha || 0) + " falha(s) e " +
      (counts.indisponivel || 0) + " indisponível(is).";
    renderDiagnosticReport({ results, checks: [] });
    resultsList.hidden = false;
    progressPanel.hidden = true;
    statusLine.textContent = "Laudo consolidado salvo localmente. Os laudos individuais de cada motor também foram preservados.";
    try { await loadDiagnosticHistory(false); } catch (historyError) {
      console.warn("Não foi possível atualizar o histórico após o diagnóstico completo:", historyError);
    }
  } catch (error) {
    issue("error", "complete_diagnostic", "complete_diagnostic_failed", String(error));
    if (reportId) {
      try { await checkpoint("failed"); } catch (saveError) {
        console.error("Falha ao preservar laudo completo:", saveError);
      }
    }
    statusLine.textContent = "Não foi possível concluir o diagnóstico completo: " + String(error);
    summaryPanel.hidden = false;
    summaryPanel.textContent = "A execução foi interrompida. Os laudos individuais já gravados continuam preservados.";
  } finally {
    completeDiagnosticRunning = false;
    setDiagnosticActivity("complete:diagnostic", false);
    completeDiagnosticCurrentSession = null;
    runButton.hidden = false;
    runButton.disabled = false;
    closeButton.disabled = false;
    cancelButton.disabled = true;
    cancelButton.textContent = "Diagnóstico encerrado";
    if (!progressPanel.hidden) progressPanel.hidden = true;
    try { await loadDiagnosticHistory(false); } catch (historyError) {
      console.warn("Não foi possível atualizar o histórico de laudos:", historyError);
    }
  }
}

async function runDiagnostic(mode, target = null) {
  if (mode === "complete" && target === null) {
    await runCompleteDiagnostic();
    return;
  }
  const status = document.getElementById("diagnostic-status");
  document.getElementById("diagnostic-results").replaceChildren();
  const runButton = document.getElementById("diagnostic-run-selected");
  runButton.disabled = true;
  const activityReason = "run:" + mode + ":" + (target || "all");
  setDiagnosticActivity(activityReason, true);
  status.textContent = "Preparando a consulta…";
  try {
    if (target === "armazenamento") {
      activeStorageBenchmark = null;
      storageBenchmarkPlaceholder = null;
      lastStorageActivityLogAt = 0;
      appendConsole("Diagnóstico de armazenamento iniciado.");
      const selectedIds = selectedStorageDiskIds();
      if (!selectedIds.length) throw new Error("Marque uma ou mais unidades antes de iniciar o teste.");
      prepareStorageBenchmarkPlaceholder();
      setDiagnosticActivity(activityReason, true);
      let completed = 0;
      const failures = [];
      for (let index = 0; index < selectedIds.length; index++) {
        const diskId = selectedIds[index];
        status.textContent = "Testando unidade " + (index + 1) + " de " + selectedIds.length + " · " + diskId + "...";
        try {
          const report = await Qboa.runStorageDiagnostic(diskId);
          const hasLiveBenchmark = Boolean(document.querySelector("#diagnostic-results .storage-live-benchmark"));
          renderStorageReport(report, completed > 0 || failures.length > 0 || hasLiveBenchmark);
          completed++;
          if (report.execution_status === "partial") failures.push((report.disk_name || diskId) + ": teste parcial");
        } catch (error) {
          failures.push(diskId + ": " + String(error));
          const item = document.createElement("li");
          item.className = "storage-test-error";
          const heading = document.createElement("strong");
          heading.textContent = "Falha no teste · " + diskId;
          const detail = document.createElement("p");
          detail.textContent = String(error);
          item.append(heading, detail);
          document.getElementById("diagnostic-results").appendChild(item);
        }
      }
      // O benchmark ao vivo é apenas acompanhamento da execução.
      // Ao finalizar todas as unidades, mantém na tela somente os laudos completos.
      if (activeStorageBenchmark?.entry) {
        activeStorageBenchmark.entry.remove();
      }
      activeStorageBenchmark = null;
      if (storageBenchmarkPlaceholder) {
        storageBenchmarkPlaceholder.remove();
        storageBenchmarkPlaceholder = null;
      }
      await loadDiagnosticHistory(false);
      appendConsole("Diagnóstico de armazenamento finalizado · " + completed + " unidade(s) concluída(s) · " + failures.length + " ocorrência(s).");
      if (!completed && failures.length) {
        status.textContent = "Nenhuma unidade concluiu o teste. Consulte os erros listados.";
      } else if (failures.length) {
        status.textContent = completed + " unidade(s) testada(s); " + failures.length + " resultado(s) parcial(is) ou falha(s).";
      } else {
        status.textContent = "Teste concluído para " + completed + " unidade(s). Abaixo estão somente as oito medições e os dados SMART de cada unidade.";
      }
      return;
    }

    const report = await Qboa.runDiagnosticCheck(mode);
    renderDiagnosticReport(report, target);
    await loadDiagnosticHistory(false);
    status.textContent = report.execution_status === "failed"
      ? "A execução falhou; o laudo e o motivo foram preservados."
      : target
      ? "Consulta de " + diagnosticCategoryLabel(target) + " concluída. Confira as evidências coletadas e as limitações informadas."
      : "Diagnóstico completo finalizado; confira cada resultado e sua disponibilidade.";
  } catch (error) {
    status.textContent = String(error);
  } finally {
    setDiagnosticActivity(activityReason, false);
    const list = document.getElementById("storage-disk-list");
    runButton.disabled = target === "armazenamento"
      ? !list || selectedStorageDiskIds().length === 0
      : false;
  }
}

function serviceStartTypeSummary(beforeState, afterState, operation) {
  const before = beforeState?.system?.services;
  const after = afterState?.system?.services;
  if (!Array.isArray(before) || !Array.isArray(after)) return null;

  const previous = new Map(before.map((service) => [String(service.name).toLowerCase(), service]));
  const disabled = [];
  const enabled = [];
  const automatic = [];
  const manual = [];

  after.forEach((service) => {
    const old = previous.get(String(service.name).toLowerCase());
    if (!old || String(old.start_type).toLowerCase() === String(service.start_type).toLowerCase()) return;
    const currentType = String(service.start_type).toLowerCase();
    const previousType = String(old.start_type).toLowerCase();
    if (currentType === "disabled") disabled.push(service.name);
    else if (previousType === "disabled") enabled.push(`${service.name} (${serviceStartTypeLabel(service.start_type)})`);
    else if (currentType === "automatic") automatic.push(service.name);
    else if (currentType === "manual") manual.push(service.name);
  });

  const changed = disabled.length + enabled.length + automatic.length + manual.length;
  const lines = [
    "=================",
    "RESUMO DA EXECUÇÃO",
    "=================",
    `Operação: ${operation}`,
    ...(disabled.length ? [`DESABILITADOS (${disabled.length})`, ...disabled.map((name) => `  - ${name}`)] : []),
    ...(enabled.length ? [`REATIVADOS (${enabled.length})`, ...enabled.map((name) => `  - ${name}`)] : []),
    ...(automatic.length ? [`ALTERADOS PARA AUTOMÁTICO (${automatic.length})`, ...automatic.map((name) => `  - ${name}`)] : []),
    ...(manual.length ? [`ALTERADOS PARA MANUAL (${manual.length})`, ...manual.map((name) => `  - ${name}`)] : []),
    changed ? `Total de serviços alterados: ${changed}` : "Nenhum serviço teve a inicialização alterada."
  ];
  return lines.join("\n");
}

async function executeServiceProfileOperation(operation, action, fallbackMessage) {
  const beforeState = await Qboa.getServiceProfileState().catch(() => null);
  await action();
  const afterState = await Qboa.getServiceProfileState().catch(() => null);
  const summary = serviceStartTypeSummary(beforeState, afterState, operation);
  if (summary) {
    consoleBuffer = consoleBuffer.trimEnd()
      ? `${consoleBuffer.trimEnd()}\n\n\n${summary}`
      : summary;
    renderConsole();
  } else {
    appendConsole(fallbackMessage);
  }
  await loadServiceProfileState();
}

async function loadServiceProfileState() {
  const status = document.getElementById("service-profile-status");
  const details = document.getElementById("service-system-details");
  const controls = [
    "service-profile-apply-btn",
    "service-profile-defaults-btn",
    "service-profile-undo-btn"
  ].map((id) => document.getElementById(id));
  controls.forEach((control) => { control.disabled = true; });

  try {
    serviceProfileState = await Qboa.getServiceProfileState();
    const { system, profile } = serviceProfileState;
    if (!system.supported) {
      details.textContent = "";
      status.textContent = `Edição detectada: ${system.edition_id}. Não há perfil para ela.`;
      controls.forEach((control) => { control.disabled = true; });
      return;
    }
    const formFactor = system.form_factor
      ? ` · ${system.form_factor}`
      : " · formato do equipamento não identificado; regras específicas de hardware serão ignoradas";
    details.textContent = system.edition_id === "IoTEnterpriseS"
      ? `Windows 11 IoT Enterprise LTSC ${system.version} (${system.build})`
      : `${system.os === "windows11" ? "Windows 11" : "Windows 10"} ${system.version} (${system.build}) · ${system.edition}${formFactor}`;
    status.textContent = profile.validated
      ? "Perfil validado disponível."
      : profile.heuristic_apply_enabled
        ? "Catálogo heurístico com aplicação restrita; confira a prévia. A configuração inicial é guardada para restauração."
        : profile.services?.length
          ? "Catálogo candidato disponível para prévia; aplicação bloqueada até validação."
          : "Nenhum perfil validado. Aplicação indisponível.";
    const canApplyProfile = profile.validated || profile.heuristic_apply_enabled;
    document.getElementById("service-profile-apply-btn").disabled = !canApplyProfile;
    document.getElementById("service-profile-defaults-btn").disabled = !(profile.services?.length);
    document.getElementById("service-profile-undo-btn").disabled = !canApplyProfile;
  } catch (error) {
    serviceProfileState = null;
    const detail = String(error);
    status.textContent = `Falha ao consultar os serviços do Windows: ${detail}`;
    details.textContent = "";
    appendConsole(`Falha ao carregar o estado do perfil de serviços: ${detail}`);
  }
}

async function registerEvents() {
  // Instala os listeners de RAM antes de habilitar os controles que podem iniciar
  // uma sessão, para não perder o estado inicial nem as primeiras atualizações.
  await Promise.all([
    listen("qboa-memory-test-progress", (event) => {
      renderMemoryTestSession(event.payload?.session);
    }),
    listen("qboa-memory-test-state", (event) => {
      renderMemoryTestSession(event.payload?.session);
    }),
    listen("qboa-memory-test-finished", (event) => {
      renderMemoryTestSession(event.payload?.session);
      void loadMemoryDiagnosticReports();
    }),
    listen("qboa-cpu-test-progress", (event) => {
      renderCpuTestSession(event.payload?.session);
    }),
    listen("qboa-cpu-test-state", (event) => {
      renderCpuTestSession(event.payload?.session);
    }),
    listen("qboa-cpu-test-finished", (event) => {
      renderCpuTestSession(event.payload?.session);
      void loadDiagnosticHistory(false);
    }),
    listen("qboa-gpu-test-progress", (event) => {
      renderGpuTestSession(event.payload?.session);
    }),
    listen("qboa-gpu-test-state", (event) => {
      renderGpuTestSession(event.payload?.session);
    }),
    listen("qboa-gpu-test-finished", (event) => {
      renderGpuTestSession(event.payload?.session);
      void loadGpuDiagnosticReports();
    }),
    listen("qboa-storage-benchmark-progress", (event) => {
      renderStorageBenchmarkProgress(event.payload);
    })
  ].map((registration) => registration.catch((error) => {
    console.error("Falha ao registrar eventos de diagnóstico:", error);
  })));

  // O listener principal precisa estar pronto antes de liberar qualquer operação.
  await listen("qboa-event", (event) => {
    const payload = event.payload || {};
    const isStartingThisLaunch = payload.event === "task-started" && launchPending && activeTaskId === payload.task_id;
    if (payload.event === "task-started") {
      if (!isStartingThisLaunch && (payload.execution_id !== activeExecutionId || payload.task_id !== activeTaskId)) return;
    } else if (!payload.execution_id) {
      if (payload.event === "task-error" && payload.message) appendConsole(`Falha ao iniciar ${taskDisplayName(payload.task_id)}: ${payload.message}`);
      return;
    } else if (payload.execution_id !== activeExecutionId || payload.task_id !== activeTaskId) {
      if (launchPending && activeTaskId === payload.task_id) activeExecutionId = payload.execution_id;
      else return;
    }
    if (payload.event === "task-started") {
      executionOutput = []; executionStartedAt = Date.now(); activeTaskId = payload.task_id; activeExecutionId = payload.execution_id; launchPending = false; currentOperationProgress = null; lastProgressFlavorKey = "";
      currentOperationName = taskDisplayName(payload.task_id);
      if (isCleanupTask(payload.task_id)) {
        cleanupExecutionStarted = true;
        setCleanupSessionPhase("running", currentOperationName);
      }
      currentOperationStage = "Iniciando etapa…"; updateOperationProgress(currentOperationName, currentOperationStage, payload.progress); syncExecutionControls();
      startActivity(taskDisplayName(payload.task_id));
      appendConsole(`Tarefa iniciada: ${taskDisplayName(payload.task_id)}.`);
      document.querySelector(`[data-task-id="${payload.task_id}"]`)?.classList.add("running");
    }
    if (payload.event === "restore-point-confirmation-required") {
      void (async () => {
        const proceed = await askConfirmation(
          "Não foi possível criar um ponto de restauração. Se você optar por prosseguir, as alterações feitas não poderão ser revertidas. Deseja continuar?",
          "Ponto de restauração indisponível"
        );
        try {
          await Qboa.respondRestorePointFailure(payload.execution_id, proceed);
        } catch (error) {
          appendConsole(`Não foi possível responder à confirmação: ${String(error)}`);
        }
      })();
    }

    if (payload.event === "task-progress" && payload.message) {
      currentOperationStage = payload.message;
      updateOperationProgress(currentOperationName, currentOperationStage, payload.progress);
    } else if (payload.event === "task-output" && payload.message) {
      executionOutput.push(payload.message);
      if (!/^\s*\[[=\s>.-]*\d+(?:[.,]\d+)?%[=\s>.-]*\]\s*$/.test(payload.message)) appendConsole(payload.message);
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
      if (isCleanupTask(payload.task_id)) {
        const resultStatus = payload.event === "task-completed" ? "success" : payload.event === "task-cancelled" ? "cancelled" : "error";
        showCleanupResult(payload.task_id, resultStatus, payload.message || "");
        browserCleanupDecision = null;
      }
      document.querySelector(`[data-task-id="${payload.task_id}"]`)?.classList.remove("running");
      if (activeTaskId === payload.task_id) activeTaskId = null;
      launchPending = false;
      if (isCleanupTask(payload.task_id)) cleanupExecutionStarted = false;
      if (!payload.execution_id || payload.execution_id === activeExecutionId) activeExecutionId = null;
      currentOperationName = "";
      currentOperationStage = "";
      currentOperationProgress = null;
      lastProgressFlavorKey = "";
      syncExecutionControls();
      refreshHistory();
    }
  }).then(() => {
    eventListenersReady = true;
  }).catch((error) => {
    eventListenersReady = false;
    console.error("Não foi possível registrar os eventos do QBoa:", error);
    appendConsole(`Não foi possível preparar o acompanhamento das tarefas: ${String(error)}. As operações foram bloqueadas por segurança.`);
  });
}

(async function init() {
  ensureCleanupSessionModal();
  restoreConsoleHeight();
  await loadStructure();
  syncExecutionControls();
  renderTabs();
  renderSectionSummary();
  await registerEvents();
  renderCards();
  if (!eventListenersReady) {
    writeStatus("Acompanhamento de tarefas indisponível. Nenhuma limpeza será iniciada enquanto os eventos não estiverem prontos.");
  }
  await refreshHistory();
  document.getElementById("copy-console-btn")?.addEventListener("click", async (event) => {
    event.stopPropagation();
    const status = document.getElementById("console-copy-status");
    try {
      await navigator.clipboard.writeText(consoleBuffer);
      status.textContent = "Registro copiado.";
    } catch (error) {
      status.textContent = "Não foi possível copiar o registro. Verifique a permissão da área de transferência.";
    }
  });
  await loadDiagnosticHistory(true);

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

  document.getElementById("cleanup-preparation-start").addEventListener("click", () => { void startPreparedCleanup(); });
  document.getElementById("cleanup-preparation-cancel").addEventListener("click", () => { void cancelPreparedCleanup(); });

  document.getElementById("risk-cancel").addEventListener("click", () => {
    closeRiskModal();
  });

  document.getElementById("running-operation-close").addEventListener("click", closeRunningOperationPopup);
  document.getElementById("running-operation-console").addEventListener("click", () => { closeRunningOperationPopup(); const panel = document.getElementById("console"); panel.open = true; panel.scrollIntoView({ behavior: "smooth", block: "nearest" }); });
  document.getElementById("running-operation-modal").addEventListener("click", (event) => { if (event.target === event.currentTarget) closeRunningOperationPopup(); });
  document.getElementById("execution-summary-close").addEventListener("click", closeExecutionSummary);
  document.getElementById("execution-summary-ok").addEventListener("click", closeExecutionSummary);
  document.getElementById("execution-summary-modal").addEventListener("click", (event) => { if (event.target === event.currentTarget) closeExecutionSummary(); });

  document.getElementById("confirmation-cancel").addEventListener("click", () => {
    closeConfirmationModal(false);
  });
  document.getElementById("confirmation-confirm").addEventListener("click", () => {
    closeConfirmationModal(true);
  });

  document.getElementById("confirmation-modal").addEventListener("click", (event) => {
    if (event.target === event.currentTarget) closeConfirmationModal(false);
  });
  document.getElementById("risk-modal").addEventListener("click", (event) => {
    if (event.target === event.currentTarget) {
      closeRiskModal();
    }
  });
  // O inspetor existe somente no servidor local de desenvolvimento.
  // Builds empacotados usam file:// e nunca interceptam este atalho.
  if (window.location.hostname === "localhost" || window.location.hostname === "127.0.0.1") {
    document.addEventListener("keydown", (event) => {
      if (!(event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "i")) return;
      event.preventDefault();
      invoke("open_devtools").catch((error) => console.error("Não foi possível abrir o inspetor:", error));
    });
  }

  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    const confirmationModal = document.getElementById("confirmation-modal");
    const riskModal = document.getElementById("risk-modal");
    const serviceInventoryModal = document.getElementById("service-inventory-modal");
    const executionSummaryModal = document.getElementById("execution-summary-modal");
    const runningOperationModal = document.getElementById("running-operation-modal");
    const diagnosticModal = document.getElementById("diagnostic-modal");
    if (!runningOperationModal.classList.contains("hidden")) { closeRunningOperationPopup(); }
    else if (!diagnosticModal.classList.contains("hidden")) { closeDiagnosticModal(); }
    else if (!executionSummaryModal.classList.contains("hidden")) {
      closeExecutionSummary();
    } else if (!serviceInventoryModal.classList.contains("hidden")) {
      serviceInventoryModal.classList.add("hidden");
      serviceInventoryModal.setAttribute("aria-hidden", "true");
    } else if (!confirmationModal.classList.contains("hidden")) {
      closeConfirmationModal(false);
    } else if (!riskModal.classList.contains("hidden")) {
      closeRiskModal();
    }
  });

  document.getElementById("risk-confirm").addEventListener("click", async (event) => {
    const confirmButton = event.currentTarget;
    // Evita iniciar a mesma operação duas vezes por cliques rápidos.
    if (confirmButton.disabled || !pendingTask) return;

    const task = pendingTask;
    confirmButton.disabled = true;
    // Desvincula a confirmação antes de fechar o modal: a operação passa a
    // pertencer ao handler, e não a um estado global que outros eventos limpam.
    pendingTask = null;
    closeRiskModal();

    try {
      const started = await executeTask(task.item, task.cardEl);
    } catch (error) {
      appendConsole(`Falha ao iniciar ${task.item.name}: ${String(error)}`);
      if (task.cardEl) task.cardEl.classList.remove("running");
    } finally {
      confirmButton.disabled = false;
      closeRiskModal();
    }
  });

  document.getElementById("shutdown-on-complete").addEventListener("change", async (event) => {
    const checkbox = event.currentTarget;
    const enabled = checkbox.checked;
    try {
      await Qboa.setShutdownOnComplete(enabled);
      appendConsole(enabled
        ? "Desligamento automático ativado para o término bem-sucedido da tarefa."
        : "Desligamento automático desativado.");
    } catch (error) {
      checkbox.checked = !enabled;
      appendConsole("Não foi possível atualizar a opção de desligamento: " + String(error));
    }
  });

  document.addEventListener("click", async (event) => {
    if (!event.target.closest("#cancel-task-btn")) return;
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

  document.getElementById("diagnostic-run-selected").addEventListener("click", () => {
    void runDiagnostic(activeDiagnosticMode, activeDiagnosticTarget);
  });
  document.getElementById("complete-diagnostic-cancel").addEventListener("click", () => {
    void cancelCompleteDiagnostic();
  });
  document.getElementById("memory-test-start").addEventListener("click", () => {
    void startMemoryTestFromUi();
  });
  document.getElementById("memory-test-cancel").addEventListener("click", () => {
    void cancelMemoryTestFromUi();
  });
  document.getElementById("cpu-test-start").addEventListener("click", () => {
    void startCpuTestFromUi();
  });
  document.getElementById("cpu-test-cancel").addEventListener("click", () => {
    void cancelCpuTestFromUi();
  });
  document.getElementById("gpu-test-start").addEventListener("click", () => {
    void startGpuTestFromUi();
  });
  document.getElementById("gpu-test-cancel").addEventListener("click", () => {
    void cancelGpuTestFromUi();
  });
  document.getElementById("gpu-inventory-refresh").addEventListener("click", () => {
    void loadGpuInventory();
  });
  document.getElementById("cpu-inventory-refresh").addEventListener("click", () => {
    void loadCpuInventory();
  });
  document.getElementById("memory-test-profile").addEventListener("change", () => {
    const profile = document.getElementById("memory-test-profile").value;
    const descriptions = {
      fast: "Rápido · 256 MiB · A01/A03 em cada bloco e A04 amostrado. Cobertura parcial.",
      standard: "Padrão · 3 GiB · executa A01–A05 nos blocos concluídos.",
      extended: "Prolongado · 6 GiB · dois passes de A01–A05 e seeds diferentes no A04.",
      custom: "Personalizado · executa A01–A05 em um passe, no orçamento escolhido pelo slider."
    };
    document.getElementById("memory-test-profile-description").textContent = descriptions[profile];
    document.getElementById("memory-test-custom-picker").hidden = profile !== "custom";
  });
  document.getElementById("memory-test-custom-budget").addEventListener("input", (event) => {
    event.currentTarget.dataset.userAdjusted = "true";
    updateCustomMemoryBudgetLabel();
  });
  document.getElementById("memory-inventory-refresh").addEventListener("click", () => { void loadMemoryInventory(); });
  document.getElementById("diagnostic-modal-close").addEventListener("click", closeDiagnosticModal);
  document.getElementById("diagnostic-modal").addEventListener("click", (event) => {
    if (event.target === event.currentTarget) closeDiagnosticModal();
  });

  document.getElementById("copy-capability-summary").addEventListener("click", () => { void copyCapabilitySummary(); });
  document.getElementById("capability-refresh-btn").addEventListener("click", () => {
    void loadCapabilityInventory();
  });

  const serviceInventoryModal = document.getElementById("service-inventory-modal");
  const closeServiceInventory = () => {
    serviceInventoryModal.classList.add("hidden");
    serviceInventoryModal.setAttribute("aria-hidden", "true");
  };
  document.getElementById("service-inventory-filter").addEventListener("input", renderWindowsServices);
  document.getElementById("service-inventory-hide-microsoft").addEventListener("change", renderWindowsServices);
  document.getElementById("open-service-inventory").addEventListener("click", () => {
    serviceInventoryModal.classList.remove("hidden");
    serviceInventoryModal.setAttribute("aria-hidden", "false");
    document.getElementById("close-service-inventory").focus();
    if (!windowsServices.length) void loadWindowsServices();
  });
  document.getElementById("close-service-inventory").addEventListener("click", closeServiceInventory);
  serviceInventoryModal.addEventListener("click", (event) => {
    if (event.target === serviceInventoryModal) closeServiceInventory();
  });

  document.getElementById("service-profile-preview-btn").addEventListener("click", async (event) => {
    const button = event.currentTarget;
    button.disabled = true;
    try {
      if (!serviceProfileState) await loadServiceProfileState();
      if (!serviceProfileState) return;
      const profile = document.getElementById("service-profile-select").value;
      const preview = await Qboa.previewServiceProfile(profile);
      renderServiceProfilePreview(preview);
    } catch (error) {
      const detail = String(error);
      document.getElementById("service-profile-preview-status").textContent = detail;
      appendConsole(`Falha ao gerar prévia do perfil de serviços: ${detail}`);
    } finally {
      button.disabled = false;
    }
  });

  document.getElementById("service-profile-select").addEventListener("change", () => {
    document.getElementById("service-profile-apply-btn").disabled = true;
    currentServiceProfilePreview = null;
    editedServiceProfileChanges = [];
    document.getElementById("service-profile-preview-list").replaceChildren();
    document.getElementById("service-profile-ignored-list").replaceChildren();
    document.getElementById("service-profile-preview-status").textContent = "";
    closeServiceProfilePreview();
  });

  document.getElementById("service-profile-apply-btn").addEventListener("click", () => {
    if (currentServiceProfilePreview) {
      const modal=document.getElementById("service-profile-preview-modal");
      modal.classList.remove("hidden"); modal.setAttribute("aria-hidden","false");
    } else document.getElementById("service-profile-preview-btn").click();
  });

  async function applyEditedServiceProfile() {
    if (!(serviceProfileState?.profile?.validated || serviceProfileState?.profile?.heuristic_apply_enabled) || !editedServiceProfileChanges.length) return;
    const profile = document.getElementById("service-profile-select").value;
    const aggressive = profile === "aggressive";
    const message = aggressive
      ? "O preset Agressivo pode restringir funcionalidades opcionais do Windows. Confira a prévia, confirme que os recursos afetados não são necessários e mantenha um meio de recuperação disponível. Deseja continuar?"
      : "O QBoa aplicará somente as alterações exibidas na prévia e registrará os tipos de inicialização anteriores para permitir a reversão. Deseja continuar?";
    const title = aggressive ? "Aplicar preset Agressivo?" : "Aplicar preset de serviços?";
    if (!(await askConfirmation(message, title))) return;
    try {
      await executeServiceProfileOperation(
        `Aplicação do perfil ${document.getElementById("service-profile-select").selectedOptions[0]?.textContent || profile}`,
        () => Qboa.applyServiceProfile(profile, editedServiceProfileChanges),
        "Preset de serviços aplicado."
      );
    } catch (error) {
      appendConsole(String(error));
    } finally {
      closeServiceProfilePreview();
    }
  }

  document.getElementById("service-profile-modal-apply").addEventListener("click", applyEditedServiceProfile);
  document.getElementById("service-profile-preview-close").addEventListener("click", closeServiceProfilePreview);
  document.getElementById("service-profile-preview-cancel").addEventListener("click", closeServiceProfilePreview);
  document.getElementById("service-profile-preview-modal").addEventListener("click", (event) => {
    if (event.target === event.currentTarget) closeServiceProfilePreview();
  });

  document.getElementById("service-profile-defaults-btn").addEventListener("click", async () => {
    if (!serviceProfileState?.profile?.services?.length) return;
    if (!(await askConfirmation("Restaurar os tipos de inicialização salvos na primeira execução deste sistema?", "Restaurar configuração inicial?"))) return;
    try {
      await executeServiceProfileOperation(
        "Restauração da configuração inicial",
        () => Qboa.restoreWindowsServiceDefaults(),
        "Configuração inicial restaurada."
      );
    } catch (error) {
      appendConsole(String(error));
    }
  });

  document.getElementById("service-profile-undo-btn").addEventListener("click", async () => {
    if (!(serviceProfileState?.profile?.validated || serviceProfileState?.profile?.heuristic_apply_enabled)) return;
    if (!(await askConfirmation("Desfazer minhas alterações?", "Desfazer alterações?"))) return;
    try {
      await executeServiceProfileOperation(
        "Reversão das alterações de serviços",
        () => Qboa.undoServiceProfileChanges(),
        "Alterações desfeitas."
      );
    } catch (error) {
      appendConsole(String(error));
    }
  });

  try {
    await Qboa.finishStartup();
  } catch (error) {
    console.error("Não foi possível finalizar a abertura do QBoa:", error);
    appendConsole(`A janela principal não pôde ser exibida automaticamente: ${String(error)}`);
  }
})();
