const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let structure = null;
let activeSection = "limpeza";
let activeLevel = "normal";
let pendingTask = null;
let pendingConfirmationResolver = null;
let activeTaskId = null;
let activeExecutionId = null;
let consoleBuffer = "";
let activityLabel = "";
let activityDots = "";
let activityTimer = null;
let serviceProfileState = null;
let serviceSignatureFindings = [];
let windowsServices = [];
let selectedTweakTaskIds = new Set();
let debloatProfileQueue = null;
let browserCleanupDecision = null;
let showAllSessionChanges = false;

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

  async runTask(taskId, shutdownOnComplete) {
    return invoke("run_task", { taskId, shutdownOnComplete });
  },

  async respondRestorePointFailure(executionId, proceed) {
    return invoke("respond_restore_point_failure", { executionId, proceed });
  },

  async runPreset(presetId) {
    return invoke("run_preset", { presetId });
  },

  async getDebloatProfileTasks(profile, selectedIds) {
    return invoke("get_debloat_profile_tasks", { profile, selectedIds });
  },

  async getSessionHistory() {
    return invoke("get_session_history");
  },

  async runDiagnosticCheck(mode) {
    return invoke("run_diagnostic_check", { mode });
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

  async auditUnsignedServices() {
    return invoke("audit_unsigned_services");
  },

  async applyServiceProfile(profile, selectedToDisable = []) {
    return invoke("apply_service_profile", { profile, selectedToDisable });
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
    });
    tabsEl.appendChild(btn);
  });

  const serviceTab = document.createElement("button");
  serviceTab.className = "tab" + (activeSection === "servicos" ? " active" : "");
  serviceTab.textContent = "🛠️ Serviços";
  serviceTab.addEventListener("click", async () => {
    activeSection = "servicos";
    renderTabs();
    document.getElementById("section-summary").textContent = "Alterar perfil de serviços do Windows";
    renderCards();
    if (!serviceProfileState) await loadServiceProfileState();
  });
  tabsEl.appendChild(serviceTab);

  const capabilityTab = document.createElement("button");
  capabilityTab.className = "tab" + (activeSection === "capacidades" ? " active" : "");
  capabilityTab.textContent = "🧩 Capacidades";
  capabilityTab.addEventListener("click", () => {
    activeSection = "capacidades";
    renderTabs();
    document.getElementById("section-summary").textContent = "Inventário de hardware, firmware e recursos do Windows";
    renderCards();
    void loadCapabilityInventory();
  });
  tabsEl.appendChild(capabilityTab);

  const contributionTab = document.createElement("button");
  contributionTab.className = "tab" + (activeSection === "contribuir" ? " active" : "");
  contributionTab.textContent = "🤝 Contribuir";
  contributionTab.addEventListener("click", () => {
    activeSection = "contribuir";
    renderTabs();
    document.getElementById("section-summary").textContent = "Contribua com código, testes, ideias ou Pix.";
    renderCards();
  });
  tabsEl.appendChild(contributionTab);
}

function syncExecutionControls() {
  const running = Boolean(activeTaskId);
  const presetButton = document.getElementById("preset-run-btn");
  const debloatButton = document.getElementById("debloat-profile-run-btn");
  const cancelButton = document.getElementById("cancel-task-btn");

  cancelButton.hidden = !running;
  cancelButton.disabled = !running;
  presetButton.disabled = running || Boolean(debloatProfileQueue) || activeSection === "servicos";
  debloatButton.disabled = running || Boolean(debloatProfileQueue);
}

function renderCards() {
  const cardsEl = document.getElementById("cards");
  const dashboard = document.querySelector(".dashboard");
  const recoveryPanel = document.querySelector(".recovery-panel");
  const profileControls = document.getElementById("debloat-profile-controls");
  const serviceProfilePanel = document.getElementById("service-profile-panel");
  const capabilityPanel = document.getElementById("capability-inventory-panel");
  profileControls.hidden = activeSection !== "limpeza";
  serviceProfilePanel.hidden = activeSection !== "servicos";
  capabilityPanel.hidden = activeSection !== "capacidades";
  dashboard.hidden = activeSection === "servicos" || activeSection === "capacidades";
  document.querySelector(".levels").hidden = activeSection !== "limpeza";
  syncExecutionControls();
  cardsEl.innerHTML = "";
  if (activeSection === "servicos" || activeSection === "capacidades") { if (activeSection === "servicos" && !windowsServices.length) void loadWindowsServices(); return; }
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
    if (
      activeSection === "limpeza" && item.id.startsWith("desengordurar_") &&
      document.getElementById("debloat-profile-select").value === "personalizado"
    ) {
      const selection = document.createElement("label");
      selection.className = "custom-profile-selection";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.checked = selectedTweakTaskIds.has(item.id);
      checkbox.addEventListener("click", (event) => event.stopPropagation());
      checkbox.addEventListener("change", () => {
        if (checkbox.checked) selectedTweakTaskIds.add(item.id);
        else selectedTweakTaskIds.delete(item.id);
      });
      selection.append(checkbox, document.createTextNode("Incluir no perfil Personalizado"));
      selection.addEventListener("click", (event) => event.stopPropagation());
      card.appendChild(selection);
    }
    card.addEventListener("click", () => runTask(item, card));
    cardsEl.appendChild(card);
  });
}

function serviceStatusLabel(status) {
  return ({ running: "Em execução", stopped: "Parado", startpending: "Iniciando", stoppending: "Parando", paused: "Pausado" })[String(status).toLowerCase()] || status;
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
     service.display_name.toLocaleLowerCase().includes(filter))
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
      if (!(await askConfirmation(`Alterar a inicialização de ${service.display_name} para ${serviceStartTypeLabel(next)}?`, "Alterar serviço?"))) {
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
        if (confirmation && !(await askConfirmation(confirmation, `${label} serviço?`))) return;
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
    [summary.manufacturer, summary.model].filter(Boolean).join(" "),
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
    "task-completed": "Tarefa concluída",
    "task-cancelled": "Tarefa cancelada",
    "task-error": "Falha na tarefa",
    "restore-point-created": "Ponto de restauração criado",
    "restore-point-confirmation-required": "Confirmação necessária para continuar",
    "snapshot-started": "Preparando cópia de segurança",
    "restore-point-phase-started": "Criando ponto de restauração",
    "restore-point-phase-completed": "Ponto de restauração criado",
    "restore-point-phase-failed": "Não foi possível criar o ponto de restauração",
    "registry-operation-failed": "Falha ao limpar o histórico do Registro",
    "task-executor-failed": "A tarefa não pôde ser concluída"
  };
  return String(message ?? "")
    .replace(/\[(task-started|task-output|task-progress|task-completed|task-cancelled|task-error|snapshot-started|restore-point-phase-started|restore-point-phase-completed|restore-point-phase-failed|registry-operation-failed|task-executor-failed)\]/gi,
      (match, key) => labels[key] || key)
    .replace(/status=exit code: 0/gi, "resultado: concluído")
    .replace(/exit code: 0/gi, "concluído");
}

function appendConsole(message) {
  const line = String(message ?? "").trim();
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

function renderConsole() {
  const out = document.getElementById("console-output");
  out.textContent = [consoleBuffer, activityLabel ? `${activityLabel}${activityDots}` : ""]
    .filter(Boolean)
    .join("\n");
  out.scrollTop = out.scrollHeight;
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

function showRiskModal(item, cardEl, profileQueue = false) {
  pendingTask = { item, cardEl, profileQueue };
  const modal = document.getElementById("risk-modal");
  const message = document.getElementById("risk-message");

  const riskText = item.risk === "advanced" ? "avançada" : "moderada";
  message.textContent = `Esta operação é ${riskText} e altera configurações do Windows. Um ponto de restauração será criado quando possível. Alterações reversíveis serão registradas pelo Qboa.`;

  modal.classList.remove("hidden");
  modal.setAttribute("aria-hidden", "false");
}

function closeRiskModal() {
  const modal = document.getElementById("risk-modal");
  modal.classList.add("hidden");
  modal.setAttribute("aria-hidden", "true");
  pendingTask = null;
}

function askConfirmation(message, title = "Confirmar operação") {
  const modal = document.getElementById("confirmation-modal");
  const titleElement = document.getElementById("confirmation-title");
  const messageElement = document.getElementById("confirmation-message");
  const confirmButton = document.getElementById("confirmation-confirm");

  if (pendingConfirmationResolver) {
    pendingConfirmationResolver(false);
    pendingConfirmationResolver = null;
  }

  titleElement.textContent = title;
  messageElement.textContent = message;
  modal.classList.remove("hidden");
  modal.setAttribute("aria-hidden", "false");
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
        `Estes navegadores estão abertos: ${names}.\n\nSe autorizar, o QBoa vai fechá-los antes de limpar os caches. Salve seu trabalho antes de continuar. Se recusar, a limpeza dos caches será ignorada.`,
        "Fechar navegadores abertos?"
      );
      await Qboa.prepareBrowserCleanup(closeRunning);
      browserCleanupDecision = closeRunning;
      appendConsole(closeRunning
        ? "Navegadores autorizados para fechamento; a limpeza dos caches será executada."
        : "Você optou por manter os navegadores abertos; seus caches serão ignorados.");
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
  if (["limpeza_leve", "limpeza_media", "limpeza_pesada"].includes(item.id)) {
    await prepareBrowserCleanupIfNeeded();
  }
  if (cardEl) cardEl.classList.add("running");
  activeTaskId = item.id;
  writeStatus(`> Rodando "${item.name}" (nível: ${activeLevel})...`);
  startActivity(item.name);
  closeRiskModal();
  const shutdownOnComplete = document.getElementById("shutdown-on-complete").checked;

  try {
    const result = await Qboa.runTask(item.id, shutdownOnComplete);
    const output = result.output || "(sem saída)";
    activeExecutionId = result.execution_id || activeExecutionId;
    appendConsole(output);
    return true;
  } catch (err) {
    stopActivity();
    appendConsole(`Erro ao chamar o backend: ${err}`);
    if (cardEl) cardEl.classList.remove("running");
    activeTaskId = null;
    return false;
  }
}

async function runTask(item, cardEl) {
  if (debloatProfileQueue) {
    appendConsole("Aguarde a conclusão do perfil de limpeza avançada.");
    return;
  }
  if (requiresConfirmation(item)) {
    showRiskModal(item, cardEl);
    return;
  }

  await executeTask(item, cardEl);
}

function stopDebloatProfileQueue(message) {
  if (!debloatProfileQueue) return;
  debloatProfileQueue = null;
  document.getElementById("debloat-profile-status").textContent = message;
  syncExecutionControls();
}

async function runNextDebloatProfileTask() {
  if (!debloatProfileQueue || activeTaskId) return;
  if (!debloatProfileQueue.items.length) {
    stopDebloatProfileQueue("Perfil concluído.");
    return;
  }

  const item = debloatProfileQueue.items.shift();
  debloatProfileQueue.currentTaskId = item.id;
  const card = document.querySelector(`[data-task-id="${item.id}"]`);
  if (requiresConfirmation(item)) {
    showRiskModal(item, card, true);
    document.getElementById("debloat-profile-status").textContent =
      `Confirme a tarefa ${item.name} para continuar.`;
    return;
  }
  if (!(await executeTask(item, card))) {
    stopDebloatProfileQueue(`Perfil interrompido em ${item.name}.`);
  }
}

async function startDebloatProfile() {
  const button = document.getElementById("debloat-profile-run-btn");
  const status = document.getElementById("debloat-profile-status");
  if (activeTaskId || debloatProfileQueue) {
    status.textContent = "Aguarde a tarefa atual terminar.";
    return;
  }
  const profile = document.getElementById("debloat-profile-select").value;
  const selectedIds = profile === "personalizado" ? [...selectedTweakTaskIds] : [];
  if (profile === "personalizado" && selectedIds.length === 0) {
    status.textContent = "Marque ao menos uma tarefa para o perfil Personalizado.";
    return;
  }
  button.disabled = true;
  status.textContent = "Preparando perfil...";
  try {
    const ids = await Qboa.getDebloatProfileTasks(profile, selectedIds);
    const debloatItems = structure.sections.flatMap((section) => section.items).filter((item) => item.id.startsWith("desengordurar_"));
    const items = ids.map((id) => debloatItems.find((item) => item.id === id));
    if (items.some((item) => !item)) throw new Error("O catálogo retornou uma tarefa desconhecida.");
    debloatProfileQueue = { items, currentTaskId: null };
    syncExecutionControls();
    status.textContent = "Executando tarefas do perfil.";
    await runNextDebloatProfileTask();
  } catch (error) {
    button.disabled = false;
    status.textContent = String(error);
  }
}

function presetDisplayName(level) {
  return {
    express: "Rápido",
    normal: "Equilibrado",
    turbo: "Completo"
  }[level] || "Personalizado";
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
    if (activeTaskId || debloatProfileQueue) {
      appendConsole("Já existe uma tarefa em execução.");
      return;
    }

    const presetTasks = structure.sections.flatMap((section) =>
      section.items.filter((item) => item.preset && item.preset.includes(activeLevel))
    );
    if (!presetTasks.length) {
      appendConsole(`Nenhuma tarefa disponível para o perfil ${presetDisplayName(activeLevel)}.`);
      return;
    }

    if (activeLevel === "turbo" && !(await askConfirmation("O preset Turbo executará alterações avançadas no Windows. Deseja continuar?", "Executar preset Turbo?"))) {
      return;
    }

    try {
      if (presetTasks.some((item) => ["limpeza_leve", "limpeza_media", "limpeza_pesada"].includes(item.id))) {
        await prepareBrowserCleanupIfNeeded();
      }
      const ids = await Qboa.runPreset(activeLevel);
      appendConsole(`Perfil ${presetDisplayName(activeLevel)} iniciado: ${ids.length} tarefas em sequência.`);
    } catch (error) {
      appendConsole(String(error));
    }
  });

  document.getElementById("debloat-profile-select").addEventListener("change", () => {
    if (activeSection === "limpeza") renderCards();
  });
  document.getElementById("debloat-profile-run-btn").addEventListener("click", () => {
    void startDebloatProfile();
  });
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
function renderServiceSignatureAudit(findings) {
  const panel = document.getElementById("service-signature-review");
  const list = document.getElementById("service-signature-findings");
  const status = document.getElementById("service-signature-status");
  const applyButton = document.getElementById("service-signature-apply-btn");
  list.replaceChildren();
  serviceSignatureFindings = Array.isArray(findings) ? findings : [];

  if (!serviceSignatureFindings.length) {
    status.textContent = "Nenhum serviço com assinatura ausente, inválida ou não verificável foi encontrado.";
    applyButton.textContent = "Aplicar preset";
  } else {
    status.textContent = `Foram encontrados ${serviceSignatureFindings.length} serviço(s) para revisão. A ausência de assinatura não prova que um serviço seja malicioso; selecione somente os que deseja desativar.`;
    applyButton.textContent = "Aplicar preset e desativar selecionados";
    serviceSignatureFindings.forEach((finding) => {
      const item = document.createElement("li");
      item.className = "service-signature-finding";
      const label = document.createElement("label");
      label.className = "service-signature-label";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = finding.name;
      checkbox.dataset.serviceName = finding.name;
      checkbox.disabled = Boolean(finding.protected);
      const details = document.createElement("span");
      const title = document.createElement("strong");
      title.textContent = finding.display_name || finding.name;
      const serviceName = document.createElement("small");
      serviceName.textContent = finding.name;
      const statusLine = document.createElement("small");
      const labels = {
        unsigned: "Sem assinatura digital",
        invalid: "Assinatura inválida ou não confiável",
        inconclusive: "Não foi possível verificar a assinatura",
        missing_file: "Executável não encontrado",
        unknown_path: "Caminho do executável não identificado"
      };
      statusLine.textContent = `Verificação: ${labels[finding.signature_status] || finding.signature_status}`;
      const path = document.createElement("small");
      path.textContent = `Arquivo: ${finding.binary_path || "caminho indisponível"}`;
      const publisher = document.createElement("small");
      publisher.textContent = `Publicador: ${finding.publisher || "não identificado"}`;
      details.append(title, serviceName, statusLine, path, publisher);
      label.append(checkbox, details);
      item.appendChild(label);
      if (finding.protected) {
        const protectedNote = document.createElement("small");
        protectedNote.className = "service-signature-protected";
        protectedNote.textContent = finding.protection_reason || "Serviço protegido pelo QBoa; não pode ser desativado.";
        item.appendChild(protectedNote);
      }
      list.appendChild(item);
    });
  }
  panel.hidden = false;
  panel.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

function renderServiceProfilePreview(preview) {
  const items = document.getElementById("service-profile-preview");
  const ignored = document.getElementById("service-profile-ignored");
  const ignoredTitle = document.getElementById("service-profile-ignored-title");
  items.replaceChildren();
  ignored.replaceChildren();
  ignoredTitle.hidden = true;
  document.getElementById("service-profile-preview-status").textContent = preview.message;
  document.getElementById("service-profile-apply-btn").disabled =
    !serviceProfileState?.system?.supported ||
    !(serviceProfileState?.profile?.validated || serviceProfileState?.profile?.heuristic_apply_enabled) ||
    !preview.applicable;

  preview.services.forEach((service) => {
    const item = document.createElement("li");
    const proposed = service.proposed || "Sem alteração";
    const incompatibility = service.incompatible ? ` — ${service.incompatible}` : "";
    item.textContent = `${service.service}: ${service.current} → ${proposed}${incompatibility}`;
    if (service.ignored) {
      ignored.appendChild(item);
    } else {
      items.appendChild(item);
    }
  });
  ignoredTitle.hidden = ignored.children.length === 0;
}

function diagnosticCategoryLabel(category) {
  return {
    sistema: "Sistema",
    armazenamento: "Armazenamento",
    memoria_ram: "Memória RAM",
    placa_de_video: "Placa de vídeo",
    processador: "Processador",
    temperatura: "Temperatura"
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

function renderDiagnosticReport(report) {
  const list = document.getElementById("diagnostic-results");
  list.replaceChildren();
  report.results.forEach((result) => {
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

async function runDiagnostic(mode) {
  const status = document.getElementById("diagnostic-status");
  document.getElementById("diagnostic-results").replaceChildren();
  const buttons = [
    document.getElementById("diagnostic-quick-btn"),
    document.getElementById("diagnostic-complete-btn")
  ];
  buttons.forEach((item) => { item.disabled = true; });
  status.textContent = "Verificando...";
  try {
    const report = await Qboa.runDiagnosticCheck(mode);
    renderDiagnosticReport(report);
    status.textContent = mode === "quick"
      ? "Coleta rápida concluída. Informações não representam aprovação de saúde."
      : "Verificação completa finalizada; confira cada resultado e sua disponibilidade.";
  } catch (error) {
    status.textContent = String(error);
  } finally {
    buttons.forEach((item) => { item.disabled = false; });
  }
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
    details.textContent = `${system.os} ${system.version} (${system.build}) · ${system.edition}${formFactor}`;
    status.textContent = profile.validated
      ? "Perfil validado disponível."
      : profile.heuristic_apply_enabled
        ? "Perfil histórico com aplicação heurística restrita; confira a prévia. Restaurar padrões continua bloqueado."
        : profile.services?.length
          ? "Catálogo candidato disponível para prévia; aplicação bloqueada até validação."
          : "Nenhum perfil validado. Aplicação indisponível.";
    const canApplyProfile = profile.validated || profile.heuristic_apply_enabled;
    document.getElementById("service-profile-apply-btn").disabled = !canApplyProfile;
    document.getElementById("service-profile-defaults-btn").disabled = !profile.defaults_validated;
    document.getElementById("service-profile-undo-btn").disabled = !canApplyProfile;
  } catch (error) {
    serviceProfileState = null;
    status.textContent = "Não foi possível consultar os serviços do Windows.";
    details.textContent = "";
  }
}

async function registerEvents() {
  // Não bloqueia o vínculo dos controles da interface esperando o listener Tauri.
  // Se listen() demorar ou falhar, os botões de confirmação continuam funcionais.
  void listen("qboa-event", (event) => {
    const payload = event.payload;
    if (payload.event === "task-started") {
      activeTaskId = payload.task_id;
      activeExecutionId = payload.execution_id;
      syncExecutionControls();
      startActivity(taskDisplayName(payload.task_id));
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
          if (!proceed) {
            // Atualiza a interface imediatamente após a decisão; o evento terminal
            // do backend continua sendo responsável pelo histórico e pelo registro final.
            stopActivity();
            document.querySelector(`[data-task-id="${payload.task_id}"]`)?.classList.remove("running");
            if (activeExecutionId === payload.execution_id) {
              activeExecutionId = null;
              if (activeTaskId === payload.task_id) activeTaskId = null;
            }
            syncExecutionControls();
            if (debloatProfileQueue?.currentTaskId === payload.task_id) {
              stopDebloatProfileQueue("Perfil interrompido porque o ponto de restauração não foi criado.");
            }
          }
        } catch (error) {
          appendConsole(`Não foi possível responder à confirmação: ${String(error)}`);
        }
      })();
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
      syncExecutionControls();
      refreshHistory();
      if (debloatProfileQueue?.currentTaskId === payload.task_id) {
        debloatProfileQueue.currentTaskId = null;
        if (payload.event === "task-completed") {
          void runNextDebloatProfileTask();
        } else {
          stopDebloatProfileQueue(`Perfil interrompido em ${taskDisplayName(payload.task_id)}.`);
        }
      }
    }
  }).catch((error) => {
    console.error("Não foi possível registrar os eventos do Qboa:", error);
    appendConsole(`Aviso: eventos em tempo real indisponíveis: ${String(error)}`);
  });
}

(async function init() {
  await loadStructure();
  syncExecutionControls();
  renderTabs();
  renderSectionSummary();
  renderCards();
  bindLevels();
  await registerEvents();
  await refreshHistory();

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
    if (pendingTask?.profileQueue) {
      stopDebloatProfileQueue("Perfil cancelado antes da confirmação.");
    }
    closeRiskModal();
  });

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
      if (pendingTask?.profileQueue) {
        stopDebloatProfileQueue("Perfil cancelado antes da confirmação.");
      }
      closeRiskModal();
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    const confirmationModal = document.getElementById("confirmation-modal");
    const riskModal = document.getElementById("risk-modal");
    const serviceInventoryModal = document.getElementById("service-inventory-modal");
    if (!serviceInventoryModal.classList.contains("hidden")) {
      serviceInventoryModal.classList.add("hidden");
      serviceInventoryModal.setAttribute("aria-hidden", "true");
    } else if (!confirmationModal.classList.contains("hidden")) {
      closeConfirmationModal(false);
    } else if (!riskModal.classList.contains("hidden")) {
      if (pendingTask?.profileQueue) {
        stopDebloatProfileQueue("Perfil cancelado antes da confirmação.");
      }
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
      if (!started && task.profileQueue) {
        stopDebloatProfileQueue(`Perfil interrompido em ${task.item.name}.`);
      }
    } catch (error) {
      appendConsole(`Falha ao iniciar ${task.item.name}: ${String(error)}`);
      if (task.cardEl) task.cardEl.classList.remove("running");
      if (task.profileQueue) {
        stopDebloatProfileQueue(`Perfil interrompido em ${task.item.name}.`);
      }
    } finally {
      confirmButton.disabled = false;
      closeRiskModal();
    }
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

  document.getElementById("diagnostic-quick-btn").addEventListener("click", () => {
    void runDiagnostic("quick");
  });
  document.getElementById("diagnostic-complete-btn").addEventListener("click", () => {
    void runDiagnostic("complete");
  });

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
      document.getElementById("service-profile-preview-status").textContent = String(error);
    } finally {
      button.disabled = false;
    }
  });

  document.getElementById("service-profile-select").addEventListener("change", () => {
    document.getElementById("service-signature-review").hidden = true;
    document.getElementById("service-signature-findings").replaceChildren();
    serviceSignatureFindings = [];
    document.getElementById("service-profile-apply-btn").disabled = true;
    document.getElementById("service-profile-preview").replaceChildren();
    document.getElementById("service-profile-ignored").replaceChildren();
    document.getElementById("service-profile-preview-status").textContent = "";
    document.getElementById("service-profile-ignored-title").hidden = true;
  });

  document.getElementById("service-profile-apply-btn").addEventListener("click", async (event) => {
    if (!(serviceProfileState?.profile?.validated || serviceProfileState?.profile?.heuristic_apply_enabled)) return;
    const button = event.currentTarget;
    button.disabled = true;
    document.getElementById("service-signature-status").textContent = "Verificando assinaturas digitais dos serviços...";
    document.getElementById("service-signature-review").hidden = false;
    document.getElementById("service-signature-findings").replaceChildren();
    try {
      const findings = await Qboa.auditUnsignedServices();
      renderServiceSignatureAudit(findings);
    } catch (error) {
      document.getElementById("service-signature-status").textContent = `A auditoria falhou; por segurança, o preset não foi aplicado. Detalhe: ${String(error)}`;
      document.getElementById("service-signature-apply-btn").disabled = true;
    } finally {
      button.disabled = false;
    }
  });

  document.getElementById("service-signature-cancel-btn").addEventListener("click", () => {
    document.getElementById("service-signature-review").hidden = true;
    document.getElementById("service-signature-findings").replaceChildren();
    serviceSignatureFindings = [];
  });

  document.getElementById("service-signature-apply-btn").addEventListener("click", async (event) => {
    const button = event.currentTarget;
    if (!(serviceProfileState?.profile?.validated || serviceProfileState?.profile?.heuristic_apply_enabled)) return;
    const profile = document.getElementById("service-profile-select").value;
    const selectedToDisable = Array.from(
      document.querySelectorAll("#service-signature-findings input[type='checkbox']:checked:not(:disabled)")
    ).map((input) => input.dataset.serviceName).filter(Boolean);
    const aggressive = profile === "aggressive";
    const selectedSummary = selectedToDisable.length
      ? ` Além do preset, serão desativados estes serviços selecionados: ${selectedToDisable.join(", ")}.`
      : "";
    const message = (aggressive
      ? "O preset Agressivo pode restringir funcionalidades opcionais do Windows. Confira a prévia, confirme que os recursos afetados não são necessários e mantenha um meio de recuperação disponível."
      : "O QBoa aplicará somente as alterações exibidas na prévia e registrará os tipos de inicialização anteriores para permitir a reversão.")
      + selectedSummary + " Deseja continuar?";
    const title = aggressive ? "Aplicar preset Agressivo?" : "Aplicar preset de serviços?";
    if (!(await askConfirmation(message, title))) return;
    button.disabled = true;
    try {
      await Qboa.applyServiceProfile(profile, selectedToDisable);
      appendConsole("Preset de serviços aplicado; alterações registradas no snapshot para possível reversão.");
      document.getElementById("service-signature-review").hidden = true;
      await loadServiceProfileState();
    } catch (error) {
      appendConsole(String(error));
      document.getElementById("service-signature-status").textContent = `Não foi possível aplicar as alterações: ${String(error)}`;
    } finally {
      button.disabled = false;
    }
  });

  document.getElementById("service-profile-defaults-btn").addEventListener("click", async () => {
    if (!serviceProfileState?.profile?.defaults_validated) return;
    if (!(await askConfirmation("Restaurar o padrão do Windows?", "Restaurar serviços do Windows?"))) return;
    try {
      await Qboa.restoreWindowsServiceDefaults();
      appendConsole("Padrão do Windows restaurado.");
    } catch (error) {
      appendConsole(String(error));
    }
  });

  document.getElementById("service-profile-undo-btn").addEventListener("click", async () => {
    if (!(serviceProfileState?.profile?.validated || serviceProfileState?.profile?.heuristic_apply_enabled)) return;
    if (!(await askConfirmation("Desfazer minhas alterações?", "Desfazer alterações?"))) return;
    try {
      await Qboa.undoServiceProfileChanges();
      appendConsole("Alterações desfeitas.");
    } catch (error) {
      appendConsole(String(error));
    }
  });
})();
