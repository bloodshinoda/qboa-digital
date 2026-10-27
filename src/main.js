const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let structure = null;
let activeSection = "limpeza";
let activeLevel = "normal";
let pendingTask = null;
let pendingConfirmationResolver = null;
let activeTaskId = null;
let activeExecutionId = null;
let launchPending = false;
let presetSequenceRunning = false;
let presetExpectedCount = 0;
let presetTerminalCount = 0;
let currentOperationName = "";
let currentOperationStage = "";
let consoleBuffer = "";
let executionOutput = [];
let executionToastTimer = null;
let activityLabel = "";
let activityDots = "";
let activityTimer = null;
let serviceProfileState = null;
let currentServiceProfilePreview = null;
let editedServiceProfileChanges = [];
let windowsServices = [];
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

  async getSessionHistory() {
    return invoke("get_session_history");
  },

  async runDiagnosticCheck(mode) {
    return invoke("run_diagnostic_check", { mode });
  },

  async getDiagnosticReports() {
    return invoke("get_diagnostic_reports");
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
  contributionTab.className = "tab" + (activeSection === "sobre" ? " active" : "");
  contributionTab.textContent = "ℹ️ Sobre";
  contributionTab.addEventListener("click", () => {
    activeSection = "sobre";
    renderTabs();
    document.getElementById("section-summary").textContent = "Sobre o QBoa Digital · versão 3.1.1";
    renderCards();
  });
  tabsEl.appendChild(contributionTab);
}

function syncExecutionControls() {
  const running = Boolean(activeTaskId || launchPending || presetSequenceRunning);
  const presetButton = document.getElementById("preset-run-btn");
  const cancelButton = document.getElementById("cancel-task-btn");

  cancelButton.hidden = !activeTaskId;
  cancelButton.disabled = !activeTaskId;
  presetButton.disabled = running || activeSection === "servicos";
  const progress = document.getElementById("operation-progress");
  if (progress) progress.hidden = !running;
  if (running) updateOperationProgress(currentOperationName || taskDisplayName(activeTaskId), currentOperationStage || "Preparando a operação…", null);
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
  document.querySelector(".levels").hidden = activeSection !== "limpeza";
  syncExecutionControls();
  cardsEl.innerHTML = "";
  if (activeSection === "servicos" || activeSection === "capacidades") { if (activeSection === "servicos" && !windowsServices.length) void loadWindowsServices(); return; }
  if (activeSection === "sobre") {
    dashboard.classList.add("contribution-view");
    recoveryPanel.hidden = true;
    cardsEl.innerHTML = `
      <article class="contribution-panel about-panel">
        <div class="contribution-copy">
          <span class="eyebrow">Sobre o aplicativo</span>
          <h2>QBoa Digital <span class="about-version">3.1.1</span></h2>
          <p>Utilitário independente de limpeza, manutenção, privacidade e diagnóstico do Windows, com foco em ferramentas práticas e atenção à segurança das operações.</p>
          <div class="about-meta">
            <p><strong>Versão:</strong> 3.1.1</p>
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
  return ["limpeza_leve", "limpeza_media", "limpeza_pesada", "limpeza_windows_update"].includes(taskId);
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

function showCleanupResult(taskId, status, detail = "") {
  const successful = status === "success"; const cancelled = status === "cancelled";
  const title = successful ? "Limpeza concluída com sucesso" : cancelled ? "Limpeza cancelada" : "A limpeza terminou com falha";
  const modal = document.getElementById("execution-summary-modal");
  document.getElementById("execution-summary-title").textContent = title;
  document.getElementById("execution-summary-task").textContent = taskDisplayName(taskId);
  document.getElementById("execution-summary-status").textContent = successful ? "Todas as etapas reportadas pela tarefa foram concluídas." : cancelled ? "A execução foi cancelada antes de terminar." : (detail || "O QBoa informou uma falha durante a execução.");
  const lines = executionOutput.map((line) => String(line).trim()).filter(Boolean);
  document.getElementById("execution-summary-output").textContent = lines.length ? lines.join("\n") : (detail || "A tarefa não forneceu detalhes adicionais.");
  modal.classList.remove("hidden"); modal.setAttribute("aria-hidden", "false");
  showExecutionToast(successful ? "success" : cancelled ? "warning" : "error", title, taskDisplayName(taskId));
}

function closeExecutionSummary() {
  const modal = document.getElementById("execution-summary-modal");
  modal.classList.add("hidden"); modal.setAttribute("aria-hidden", "true");
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

function updateOperationProgress(taskName, stage, progressValue) {
  const task = document.getElementById("operation-progress-task");
  const stageEl = document.getElementById("operation-progress-stage");
  const bar = document.getElementById("operation-progress-bar");
  const percentEl = document.getElementById("operation-progress-percent");
  if (!task || !stageEl || !bar || !percentEl) return;
  if (taskName) task.textContent = taskName;
  if (stage) stageEl.textContent = stage;
  const source = String(progressValue ?? stage ?? "");
  const match = source.match(/(?:^|\s)(\d{1,3}(?:[.,]\d+)?)\s*%/);
  if (match) {
    const percent = Math.max(0, Math.min(100, Number(match[1].replace(",", "."))));
    bar.classList.add("determinate"); bar.style.width = `${percent}%`;
    percentEl.textContent = `${Math.round(percent)}%`;
  } else {
    bar.classList.remove("determinate"); bar.style.width = "";
    percentEl.textContent = "Em andamento";
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
  if (!(activeTaskId || launchPending || presetSequenceRunning)) return false;
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
  if (operationIsBusy(item?.name)) return false;
  launchPending = true; currentOperationName = item.name; currentOperationStage = "Preparando a operação…"; syncExecutionControls();
  if (["limpeza_leve", "limpeza_media", "limpeza_pesada"].includes(item.id)) {
    await prepareBrowserCleanupIfNeeded();
  }
  if (cardEl) cardEl.classList.add("running");
  activeTaskId = item.id;
  currentOperationName = item.name; currentOperationStage = "Solicitando início ao mecanismo…";
  updateOperationProgress(currentOperationName, currentOperationStage, null);
  writeStatus(`> Rodando "${item.name}" (nível: ${activeLevel})...`);
  startActivity(item.name);
  closeRiskModal();
  const shutdownOnComplete = document.getElementById("shutdown-on-complete").checked;

  try {
    const result = await Qboa.runTask(item.id, shutdownOnComplete);
    const output = result.output || "(sem saída)";
    activeExecutionId = result.execution_id || activeExecutionId;
    launchPending = false; currentOperationStage = "Operação em execução; aguardando atualizações reais do mecanismo."; syncExecutionControls();
    appendConsole(output);
    return true;
  } catch (err) {
    stopActivity();
    appendConsole(`Erro ao chamar o backend: ${err}`);
    if (cardEl) cardEl.classList.remove("running");
    activeTaskId = null; activeExecutionId = null; launchPending = false; currentOperationName = ""; currentOperationStage = ""; syncExecutionControls();
    return false;
  }
}

async function runTask(item, cardEl) {
  if (operationIsBusy(item?.name)) return;
  if (requiresConfirmation(item)) {
    showRiskModal(item, cardEl);
    return;
  }

  await executeTask(item, cardEl);
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
    if (operationIsBusy(`Perfil ${presetDisplayName(activeLevel)}`)) return;
    launchPending = true; presetSequenceRunning = true; presetExpectedCount = 0; presetTerminalCount = 0;
    currentOperationName = `Perfil ${presetDisplayName(activeLevel)}`; currentOperationStage = "Preparando a sequência de tarefas…"; syncExecutionControls();

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
      launchPending = false; presetExpectedCount = ids.length;
      currentOperationName = `Perfil ${presetDisplayName(activeLevel)}`;
      currentOperationStage = `Executando sequência: ${presetTerminalCount} de ${presetExpectedCount} tarefas concluídas.`;
      if (presetTerminalCount >= presetExpectedCount) { presetSequenceRunning = false; currentOperationName = ""; currentOperationStage = ""; }
      syncExecutionControls(); appendConsole(`Perfil ${presetDisplayName(activeLevel)} iniciado: ${ids.length} tarefas em sequência.`);
    } catch (error) {
      appendConsole(String(error)); launchPending = false; presetSequenceRunning = false; presetExpectedCount = 0; presetTerminalCount = 0; currentOperationName = ""; currentOperationStage = ""; syncExecutionControls();
    }
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

function diagnosticExecutionLabel(status) {
  return {
    running: "em execução",
    completed: "concluído",
    partial: "parcial",
    cancelled: "cancelado",
    failed: "falhou"
  }[status] || "estado desconhecido";
}

async function loadDiagnosticHistory(selectLatest = false) {
  const list = document.getElementById("diagnostic-history");
  if (!list) return [];
  try {
    const reports = await Qboa.getDiagnosticReports();
    list.replaceChildren();
    if (!reports.length) {
      const empty = document.createElement("li");
      empty.textContent = "Nenhum laudo salvo ainda.";
      list.appendChild(empty);
      return reports;
    }
    reports.forEach((report) => {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "diagnostic-history-item";
      const date = document.createElement("span");
      date.textContent = new Date(report.started_at).toLocaleString("pt-BR");
      const detail = document.createElement("small");
      detail.textContent = `${report.mode === "complete" ? "Completo" : "Rápido"} · ${diagnosticExecutionLabel(report.execution_status)}`;
      button.append(date, detail);
      button.addEventListener("click", () => {
        renderDiagnosticReport(report);
        document.getElementById("diagnostic-status").textContent =
          `Laudo recuperado do histórico · ${diagnosticExecutionLabel(report.execution_status)} · ${report.report_id}`;
      });
      item.appendChild(button);
      list.appendChild(item);
    });
    if (selectLatest) {
      renderDiagnosticReport(reports[0]);
      document.getElementById("diagnostic-status").textContent =
        `Último laudo recuperado · ${diagnosticExecutionLabel(reports[0].execution_status)}`;
    }
    return reports;
  } catch (error) {
    const item = document.createElement("li");
    item.textContent = `Não foi possível carregar o histórico: ${String(error)}`;
    list.replaceChildren(item);
    return [];
  }
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
    await loadDiagnosticHistory(false);
    status.textContent = report.execution_status === "failed"
      ? "A execução falhou; o laudo e o motivo foram preservados."
      : mode === "quick"
      ? "Coleta rápida concluída. Informações não representam aprovação de saúde."
      : "Verificação completa finalizada; confira cada resultado e sua disponibilidade.";
  } catch (error) {
    status.textContent = String(error);
  } finally {
    buttons.forEach((item) => { item.disabled = false; });
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
  // Não bloqueia o vínculo dos controles da interface esperando o listener Tauri.
  // Se listen() demorar ou falhar, os botões de confirmação continuam funcionais.
  void listen("qboa-event", (event) => {
    const payload = event.payload;
    if (payload.event === "task-started") {
      executionOutput = []; activeTaskId = payload.task_id; activeExecutionId = payload.execution_id; launchPending = false;
      currentOperationName = presetSequenceRunning ? `Perfil ${presetDisplayName(activeLevel)} · ${taskDisplayName(payload.task_id)}` : taskDisplayName(payload.task_id);
      currentOperationStage = "Iniciando etapa…"; updateOperationProgress(currentOperationName, currentOperationStage, payload.progress); syncExecutionControls();
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
          }
        } catch (error) {
          appendConsole(`Não foi possível responder à confirmação: ${String(error)}`);
        }
      })();
    }


    if (payload.event === "task-output" && payload.message) {
      executionOutput.push(payload.message); currentOperationStage = payload.message;
      updateOperationProgress(currentOperationName, currentOperationStage, payload.progress || payload.message); appendConsole(payload.message);
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
      if (presetSequenceRunning) { presetTerminalCount += 1; currentOperationStage = `Sequência: ${presetTerminalCount}${presetExpectedCount ? ` de ${presetExpectedCount}` : ""} tarefa(s) finalizada(s).`; }
      if (isCleanupTask(payload.task_id)) {
        const resultStatus = payload.event === "task-completed" ? "success" : payload.event === "task-cancelled" ? "cancelled" : "error";
        showCleanupResult(payload.task_id, resultStatus, payload.message || "");
      }
      document.querySelector(`[data-task-id="${payload.task_id}"]`)?.classList.remove("running");
      if (activeTaskId === payload.task_id) activeTaskId = null;
      if (!payload.execution_id || payload.execution_id === activeExecutionId) activeExecutionId = null;
      if (presetSequenceRunning && presetExpectedCount > 0 && presetTerminalCount >= presetExpectedCount) { presetSequenceRunning = false; currentOperationName = ""; currentOperationStage = ""; }
      else if (!presetSequenceRunning) { currentOperationName = ""; currentOperationStage = ""; }
      syncExecutionControls();
      refreshHistory();
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
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    const confirmationModal = document.getElementById("confirmation-modal");
    const riskModal = document.getElementById("risk-modal");
    const serviceInventoryModal = document.getElementById("service-inventory-modal");
    const executionSummaryModal = document.getElementById("execution-summary-modal");
    const runningOperationModal = document.getElementById("running-operation-modal");
    if (!runningOperationModal.classList.contains("hidden")) { closeRunningOperationPopup(); }
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
})();
