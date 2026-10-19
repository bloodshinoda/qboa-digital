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
let selectedTweakTaskIds = new Set();
let debloatProfileQueue = null;

const Qboa = {
  async analyze() {
    return invoke("get_tasks");
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

  async getCleanupFindings() {
    return invoke("get_cleanup_findings");
  },

  async runDiagnosticCheck(mode) {
    return invoke("run_diagnostic_check", { mode });
  },

  async deleteCleanupFinding(findingId) {
    return invoke("delete_cleanup_finding", { findingId, confirmed: true });
  },

  async getServiceProfileState() {
    return invoke("get_service_profile_state");
  },

  async previewServiceProfile(profile) {
    return invoke("preview_service_profile", { profile });
  },

  async applyServiceProfile(profile) {
    return invoke("apply_service_profile", { profile });
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
  serviceTab.textContent = "Serviços";
  serviceTab.addEventListener("click", async () => {
    activeSection = "servicos";
    renderTabs();
    document.getElementById("section-summary").textContent = "Alterar perfil de serviços do Windows";
    renderCards();
    if (!serviceProfileState) await loadServiceProfileState();
  });
  tabsEl.appendChild(serviceTab);

  const contributionTab = document.createElement("button");
  contributionTab.className = "tab" + (activeSection === "contribuir" ? " active" : "");
  contributionTab.textContent = "♡ Contribuir";
  contributionTab.addEventListener("click", () => {
    activeSection = "contribuir";
    renderTabs();
    document.getElementById("section-summary").textContent = "Contribua com código, testes, ideias ou Pix.";
    renderCards();
  });
  tabsEl.appendChild(contributionTab);
}

function renderCards() {
  const cardsEl = document.getElementById("cards");
  const dashboard = document.querySelector(".dashboard");
  const recoveryPanel = document.querySelector(".recovery-panel");
  const profileControls = document.getElementById("debloat-profile-controls");
  const serviceProfilePanel = document.getElementById("service-profile-panel");
  profileControls.hidden = activeSection !== "desengordurar";
  serviceProfilePanel.hidden = activeSection !== "servicos";
  dashboard.hidden = activeSection === "servicos";
  document.getElementById("preset-run-btn").disabled = activeSection === "servicos";
  cardsEl.innerHTML = "";
  if (activeSection === "servicos") return;
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
      activeSection === "desengordurar" &&
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

async function executeTask(item, cardEl) {
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
    appendConsole("Aguarde a conclusão do perfil de Desengordurar.");
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
  document.getElementById("debloat-profile-run-btn").disabled = false;
  document.getElementById("debloat-profile-status").textContent = message;
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
    const debloatItems = structure.sections.find((section) => section.id === "desengordurar").items;
    const items = ids.map((id) => debloatItems.find((item) => item.id === id));
    if (items.some((item) => !item)) throw new Error("O catálogo retornou uma tarefa desconhecida.");
    debloatProfileQueue = { items, currentTaskId: null };
    status.textContent = "Executando tarefas do perfil.";
    await runNextDebloatProfileTask();
  } catch (error) {
    button.disabled = false;
    status.textContent = String(error);
  }
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
      appendConsole(`Nenhuma tarefa disponível para o preset ${activeLevel}.`);
      return;
    }

    if (activeLevel === "turbo" && !(await askConfirmation("O preset Turbo executará alterações avançadas no Windows. Deseja continuar?", "Executar preset Turbo?"))) {
      return;
    }

    try {
      const ids = await Qboa.runPreset(activeLevel);
      appendConsole(`Preset ${activeLevel} iniciado: ${ids.length} tarefas em sequência.`);
    } catch (error) {
      appendConsole(String(error));
    }
  });

  document.getElementById("debloat-profile-select").addEventListener("change", () => {
    if (activeSection === "desengordurar") renderCards();
  });
  document.getElementById("debloat-profile-run-btn").addEventListener("click", () => {
    void startDebloatProfile();
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

function findingCategoryLabel(category) {
  return {
    broken_shortcut: "Atalho quebrado",
    invalid_association: "Associação inválida",
    orphan_uninstall: "Desinstalação órfã"
  }[category] || "Achado";
}

function findingRiskLabel(risk) {
  return {
    low: "Baixo",
    moderate: "Moderado",
    high: "Alto"
  }[risk] || risk;
}

function findingProfileLabels(profiles) {
  return profiles.map((profile) => ({
    basic: "Básico",
    complete: "Completo",
    deep: "Profundo"
  }[profile] || profile)).join(", ");
}

async function refreshCleanupFindings() {
  const list = document.getElementById("cleanup-findings");
  list.replaceChildren();
  const findings = await Qboa.getCleanupFindings();
  if (!findings.length) {
    const empty = document.createElement("li");
    empty.textContent = "Nenhum problema encontrado.";
    list.appendChild(empty);
    return;
  }

  findings.forEach((finding) => {
    const item = document.createElement("li");
    item.className = "cleanup-finding";
    const details = document.createElement("div");
    const heading = document.createElement("strong");
    heading.textContent = `${findingCategoryLabel(finding.category)} · ${findingRiskLabel(finding.risk)}`;
    const path = document.createElement("p");
    path.textContent = finding.path;
    const reason = document.createElement("p");
    reason.textContent = finding.reason;
    const profiles = document.createElement("small");
    profiles.textContent = `Perfis: ${findingProfileLabels(finding.profiles)}`;
    details.append(heading, path, reason, profiles);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Excluir";
    remove.addEventListener("click", async () => {
      if (!finding.id || !(await askConfirmation(`Excluir este achado?\n${finding.path}`, "Excluir achado?"))) return;
      remove.disabled = true;
      try {
        await Qboa.deleteCleanupFinding(finding.id);
        await refreshCleanupFindings();
        appendConsole("Achado excluído.");
      } catch (error) {
        remove.disabled = false;
        appendConsole(String(error));
      }
    });

    item.append(details, remove);
    list.appendChild(item);
  });
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
    !serviceProfileState?.profile?.validated ||
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
    const formFactor = system.form_factor ? ` · ${system.form_factor}` : "";
    details.textContent = `${system.os} ${system.version} (${system.build}) · ${system.edition}${formFactor}`;
    status.textContent = profile.validated
      ? "Perfil validado disponível."
      : "Nenhum perfil validado. Aplicação indisponível.";
    controls.forEach((control) => { control.disabled = !profile.validated; });
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
      startActivity(taskDisplayName(payload.task_id));
      document.querySelector(`[data-task-id="${payload.task_id}"]`)?.classList.add("running");
    }
    if (payload.event === "restore-point-confirmation-required") {
      void (async () => {
        const proceed = await askConfirmation(
          `${payload.message || "Não foi possível criar um ponto de restauração."}\n\nSe prosseguir, a tarefa será executada sem essa proteção. Deseja continuar?`,
          "Ponto de restauração indisponível"
        );
        try {
          await Qboa.respondRestorePointFailure(payload.execution_id, proceed);
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
    if (!confirmationModal.classList.contains("hidden")) {
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

  document.getElementById("detect-cleanup-btn").addEventListener("click", async (event) => {
    const button = event.currentTarget;
    button.disabled = true;
    try {
      await refreshCleanupFindings();
      appendConsole("Verificação concluída.");
    } catch (error) {
      appendConsole(String(error));
    } finally {
      button.disabled = false;
    }
  });

  document.getElementById("diagnostic-quick-btn").addEventListener("click", () => {
    void runDiagnostic("quick");
  });
  document.getElementById("diagnostic-complete-btn").addEventListener("click", () => {
    void runDiagnostic("complete");
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
    document.getElementById("service-profile-apply-btn").disabled = true;
    document.getElementById("service-profile-preview").replaceChildren();
    document.getElementById("service-profile-ignored").replaceChildren();
    document.getElementById("service-profile-preview-status").textContent = "";
    document.getElementById("service-profile-ignored-title").hidden = true;
  });

  document.getElementById("service-profile-apply-btn").addEventListener("click", async () => {
    if (!serviceProfileState?.profile?.validated) return;
    const profile = document.getElementById("service-profile-select").value;
    if (profile === "custom" || !(await askConfirmation("Aplicar este perfil de serviços?", "Aplicar perfil de serviços?"))) return;
    try {
      await Qboa.applyServiceProfile(profile);
      appendConsole("Perfil de serviços aplicado.");
    } catch (error) {
      appendConsole(String(error));
    }
  });

  document.getElementById("service-profile-defaults-btn").addEventListener("click", async () => {
    if (!serviceProfileState?.profile?.validated) return;
    if (!(await askConfirmation("Restaurar o padrão do Windows?", "Restaurar serviços do Windows?"))) return;
    try {
      await Qboa.restoreWindowsServiceDefaults();
      appendConsole("Padrão do Windows restaurado.");
    } catch (error) {
      appendConsole(String(error));
    }
  });

  document.getElementById("service-profile-undo-btn").addEventListener("click", async () => {
    if (!serviceProfileState?.profile?.validated) return;
    if (!(await askConfirmation("Desfazer minhas alterações?", "Desfazer alterações?"))) return;
    try {
      await Qboa.undoServiceProfileChanges();
      appendConsole("Alterações desfeitas.");
    } catch (error) {
      appendConsole(String(error));
    }
  });
})();
