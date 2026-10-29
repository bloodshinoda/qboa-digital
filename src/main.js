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
let activeDiagnosticTarget = null;
let activeDiagnosticMode = "quick";
let memoryTestSessionSnapshot = null;

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

  async listStorageDisks() {
    return invoke("list_storage_disks");
  },

  async runStorageDiagnostic(diskId) {
    return invoke("run_storage_diagnostic", { diskId });
  },

  async getStorageDiagnosticReports() {
    return invoke("get_storage_diagnostic_reports");
  },

  async startMemoryTest(profile) {
    return invoke("start_memory_test", {
      profile,
      memoryBudgetBytes: null,
      preferredBlockBytes: null,
      seed: null
    });
  },

  async getMemoryTestSession() {
    return invoke("get_memory_test_session");
  },

  async cancelMemoryTest(sessionId) {
    return invoke("cancel_memory_test", { sessionId });
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

  // Diagnóstico possui cards e fluxo próprios; não renderizar também a aba antiga definida no JSON.
  structure.sections.filter((section) => section.id !== "diagnostico").forEach((section) => {
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

  const diagnosticTab = document.createElement("button");
  diagnosticTab.className = "tab" + (activeSection === "diagnostico" ? " active" : "");
  diagnosticTab.textContent = "🩺 Diagnóstico";
  diagnosticTab.addEventListener("click", () => {
    activeSection = "diagnostico";
    renderTabs();
    document.getElementById("section-summary").textContent = "Testes de hardware e saúde do sistema";
    renderCards();
  });
  tabsEl.appendChild(diagnosticTab);

  const contributionTab = document.createElement("button");
  contributionTab.className = "tab" + (activeSection === "sobre" ? " active" : "");
  contributionTab.textContent = "ℹ️ Sobre";
  contributionTab.addEventListener("click", () => {
    activeSection = "sobre";
    renderTabs();
    document.getElementById("section-summary").textContent = "Sobre o QBoa Digital · versão 3.1.2";
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
  dashboard.classList.toggle("contribution-view", activeSection === "sobre");
  document.querySelector(".levels").hidden = activeSection !== "limpeza";
  syncExecutionControls();
  cardsEl.innerHTML = "";
  if (activeSection === "servicos" || activeSection === "capacidades") { if (activeSection === "servicos" && !windowsServices.length) void loadWindowsServices(); return; }
  if (activeSection === "diagnostico") {
    recoveryPanel.hidden = true;
    const tests = [
      { id: "armazenamento", name: "Armazenamento", desc: "Espaço livre e informações disponíveis sobre os volumes." },
      { id: "memoria_ram", name: "Memória RAM", desc: "Memória total e disponível durante a coleta." },
      { id: "teste_memoria", name: "Integridade da RAM", desc: "Executa padrões de integridade em blocos de memória do próprio processo, com progresso e cancelamento." },
      { id: "placa_de_video", name: "GPU", desc: "Adaptadores gráficos detectados e leituras disponíveis." },
      { id: "processador", name: "Processador", desc: "Identificação do processador e dados disponíveis." },
      { id: "temperatura", name: "Temperaturas", desc: "Leituras de sensores compatíveis, quando expostas pelo sistema." },
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
    return;
  }
  if (activeSection === "sobre") {
    dashboard.classList.add("contribution-view");
    recoveryPanel.hidden = true;
    cardsEl.innerHTML = `
      <article class="contribution-panel about-panel">
        <div class="contribution-copy">
          <span class="eyebrow">Sobre o aplicativo</span>
          <h2>QBoa Digital <span class="about-version">3.1.2</span></h2>
          <p>Utilitário independente de limpeza, manutenção, privacidade e diagnóstico do Windows, com foco em ferramentas práticas e atenção à segurança das operações.</p>
          <div class="about-meta">
            <p><strong>Versão:</strong> 3.1.2</p>
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

function renderStorageReport(report, append = false) {
  const list = document.getElementById("diagnostic-results");
  if (!append) {
    list.replaceChildren();
  } else {
    const separator = document.createElement("li");
    separator.className = "storage-report-separator";
    separator.textContent = "Próxima unidade · " + (report.disk_name || report.volume || report.disk_id || "disco");
    list.appendChild(separator);
  }

  function addItem(title, detail, meta = "") {
    const item = document.createElement("li");
    const heading = document.createElement("strong");
    heading.textContent = title;
    item.appendChild(heading);
    if (detail) {
      const paragraph = document.createElement("p");
      paragraph.textContent = detail;
      item.appendChild(paragraph);
    }
    if (meta) {
      const source = document.createElement("small");
      source.textContent = meta;
      item.appendChild(source);
    }
    list.appendChild(item);
    return item;
  }

  const fmt = (value, digits = 2) => Number.isFinite(Number(value))
    ? Number(value).toLocaleString("pt-BR", { maximumFractionDigits: digits })
    : "Indisponível";
  const bytes = (value) => Number.isFinite(Number(value)) && Number(value) >= 0
    ? (Number(value) / (1024 ** 3)).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " GiB"
    : "Indisponível";
  const smartValue = (value, suffix = "") => value === null || value === undefined
    ? "Indisponível"
    : fmt(value) + suffix;
  const classNames = {
    notebook_hdd: "HD de notebook",
    desktop_hdd: "HD de desktop",
    sata_ssd: "SSD SATA",
    nvme: "SSD NVMe",
    usb: "Armazenamento USB",
    unknown: "Classe desconhecida"
  };
  const threshold = { notebook_hdd: 60, desktop_hdd: 120, sata_ssd: 400, nvme: 1500 };

  addItem("Unidade: " + (report.disk_name || report.disk_id || "Disco"),
    (report.volume || "Volume indisponível") + " · Capacidade: " + bytes(report.capacity_bytes),
    "Classe: " + (classNames[report.class] || "Desconhecida") + " · Barramento: " +
      (report.bus_type || "Indisponível") + " · ID: " + (report.disk_id || "Indisponível"));

  if (report.backend === "fake" || report.execution_status === "simulated") {
    addItem("MODO SIMULADO", "Valores fictícios para validar a interface; nenhum hardware foi medido.");
  }

  addItem("As oito medições", "Arquivo temporário local; leituras diretas no Windows usando E/S sem cache de arquivos.");
  const names = {
    seq_small_write: "Sequencial pequeno · gravação · 128 KiB · fila 1",
    seq_small_read: "Sequencial pequeno · leitura · 128 KiB · fila 1",
    seq_large_write: "Sequencial grande · gravação · 1 MiB · fila 8",
    seq_large_read: "Sequencial grande · leitura · 1 MiB · fila 8",
    random_small_write: "Aleatório pequeno · gravação · 4 KiB · fila 1",
    random_small_read: "Aleatório pequeno · leitura · 4 KiB · fila 1",
    random_large_write: "Aleatório grande · gravação · 4 KiB · fila 32",
    random_large_read: "Aleatório grande · leitura · 4 KiB · fila 32"
  };
  const measurements = Array.isArray(report.measurements) ? report.measurements : [];
  Object.keys(names).forEach((kind) => {
    const measurement = measurements.find((entry) => entry.kind === kind);
    if (!measurement) {
      addItem(names[kind], "Indisponível: medição não retornada.");
      return;
    }
    let detail = measurement.value === null || measurement.value === undefined
      ? "Indisponível. " + (measurement.reason || "Nenhum resultado válido.")
      : fmt(measurement.value) + " " + (measurement.unit || "");
    let meta = measurement.bytes_transferred === null || measurement.bytes_transferred === undefined
      ? ""
      : "Transferidos: " + bytes(measurement.bytes_transferred) + " · Duração: " + fmt(measurement.duration_ms) + " ms";
    if ((kind === "seq_large_read" || kind === "seq_large_write") && threshold[report.class]) {
      const speedStatus = measurement.value === null || measurement.value === undefined
        ? "Faixa de referência ≥ " + threshold[report.class] + " MB/s; resultado indisponível"
        : (Number(measurement.value) >= threshold[report.class] ? "Dentro" : "Abaixo") +
          " da faixa de referência para " + classNames[report.class] + " (≥ " + threshold[report.class] + " MB/s)";
      meta += (meta ? " · " : "") + speedStatus;
    } else if ((kind === "seq_large_read" || kind === "seq_large_write") && (report.class === "usb" || report.class === "unknown")) {
      meta += (meta ? " · " : "") + "Sem faixa de desempenho para esta classe";
    }
    addItem(names[kind], detail, meta);
  });

  const smart = report.smart || {};
  addItem("SMART", smart.usable ? "Dados SMART utilizáveis retornados." : "Dados SMART utilizáveis indisponíveis; isso não significa que o disco esteja saudável.",
    smart.source ? "Fontes: " + smart.source : "Fonte SMART indisponível.");
  addItem("Previsão de falha SMART", smart.predicted_failure === true ? "O provedor sinalizou previsão de falha."
    : smart.predicted_failure === false ? "O provedor não sinalizou previsão de falha."
    : "Indisponível: o provedor não expôs o campo.");
  addItem("Setores realocados", smartValue(smart.reallocated));
  addItem("Setores pendentes", smartValue(smart.pending));
  addItem("Setores incorrigíveis", smartValue(smart.uncorrectable));
  addItem("Erros CRC da interface", smartValue(smart.interface_crc_before) + " → " + smartValue(smart.interface_crc_after),
    "Comparação antes/depois do teste; aumento indica revisar cabo/porta.");
  addItem("Desgaste SMART", smart.wear_percent === null || smart.wear_percent === undefined
    ? "Indisponível" : smartValue(smart.wear_percent, "%") + " de desgaste",
    smart.wear_limit_percent === null || smart.wear_limit_percent === undefined
      ? "Limite do dispositivo indisponível" : "Limite informado: " + smartValue(smart.wear_limit_percent, "%"));
  addItem("Temperatura SMART", smartValue(smart.temperature_c, " °C"));
  addItem("Modo PIO", smart.pio_mode === true ? "Detectado" : smart.pio_mode === false ? "Não detectado" : "Indisponível");
  addItem("Link SATA lento", smart.slow_sata_link === true ? "Detectado" : smart.slow_sata_link === false ? "Não detectado" : "Indisponível");
  if (smart.usb_slow === true) addItem("Interface USB", "Velocidade baixa; revisar cabo/gabinete. Não é usada para condenar o disco.");

  const attributes = Array.isArray(smart.attributes) ? smart.attributes : [];
  if (attributes.length) {
    const item = addItem("Atributos SMART brutos", attributes.length + " atributo(s) retornado(s).");
    const details = document.createElement("details");
    const summary = document.createElement("summary");
    summary.textContent = "Ver todos os atributos SMART";
    const rows = document.createElement("ul");
    attributes.forEach((attribute) => {
      const row = document.createElement("li");
      row.textContent = "ID " + attribute.id + " · " + (attribute.name || "Atributo") +
        " · atual " + (attribute.current ?? "Indisponível") +
        " · pior " + (attribute.worst ?? "Indisponível") +
        " · limite " + (attribute.threshold ?? "Indisponível") +
        " · bruto " + (attribute.raw_value ?? "Indisponível");
      rows.appendChild(row);
    });
    details.append(summary, rows);
    item.appendChild(details);
  }
  const metrics = Array.isArray(smart.reliability_metrics) ? smart.reliability_metrics : [];
  if (metrics.length) {
    const item = addItem("Métricas de confiabilidade adicionais", metrics.length + " métrica(s).");
    const rows = document.createElement("ul");
    metrics.forEach((metric) => {
      const row = document.createElement("li");
      row.textContent = (metric.name || "Métrica") + ": " + (metric.value ?? "Indisponível");
      rows.appendChild(row);
    });
    item.appendChild(rows);
  }

  if (report.space_available === false) addItem("Benchmark não executado",
    "Espaço livre insuficiente para criar o arquivo de teste com a margem de segurança.");
  (report.issues || []).forEach((issue) => addItem("Aviso de coleta", String(issue)));
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
  try {
    const storageOnly = activeDiagnosticTarget === "armazenamento";
    const reportPromise = storageOnly ? Promise.resolve([]) : Qboa.getDiagnosticReports();
    const storagePromise = Qboa.getStorageDiagnosticReports().catch(() => []);
    const results = await Promise.all([reportPromise, storagePromise]);
    const reports = results[0];
    const storageReports = results[1];
    list.replaceChildren();
    const combined = [
      ...storageReports.map((report) => ({ report, storage: true })),
      ...reports.map((report) => ({ report, storage: false }))
    ].sort((a, b) => String(b.report.started_at || "").localeCompare(String(a.report.started_at || "")));

    if (!combined.length) {
      const empty = document.createElement("li");
      empty.textContent = "Nenhum laudo salvo ainda.";
      list.appendChild(empty);
    }
    combined.forEach(({ report, storage }) => {
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
        : (report.mode === "complete" ? "Completo" : "Rápido") + " · " + diagnosticExecutionLabel(report.execution_status);
      button.append(date, detail);
      button.addEventListener("click", () => {
        if (storage) {
          renderStorageReport(report);
          document.getElementById("diagnostic-status").textContent =
            "Laudo de armazenamento recuperado · " + (report.report_id || "ID indisponível");
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
      if (activeDiagnosticTarget === "armazenamento" && storageReports.length) {
        renderStorageReport(storageReports[0]);
        document.getElementById("diagnostic-status").textContent =
          "Último laudo de armazenamento recuperado · " + (storageReports[0].report_id || "ID indisponível");
      } else if (reports.length) {
        renderDiagnosticReport(reports[0]);
        document.getElementById("diagnostic-status").textContent =
          "Último laudo recuperado · " + diagnosticExecutionLabel(reports[0].execution_status);
      } else if (storageReports.length) {
        renderStorageReport(storageReports[0]);
        document.getElementById("diagnostic-status").textContent =
          "Último laudo de armazenamento recuperado · " + (storageReports[0].report_id || "ID indisponível");
      }
    }
    return activeDiagnosticTarget === "armazenamento" ? storageReports : reports;
  } catch (error) {
    const item = document.createElement("li");
    item.textContent = "Não foi possível carregar o histórico: " + String(error);
    list.replaceChildren(item);
    return [];
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
    model: disk.model || disk.Model || "Disco",
    volume: volume || (driveLetter ? String(driveLetter).toUpperCase() + ":\\" : ""),
    capacity_bytes: disk.capacity_bytes ?? disk.CapacityBytes ?? disk.size_bytes ?? disk.Size,
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
  refreshButton.disabled = true;
  list.replaceChildren();
  const loading = document.createElement("li");
  loading.className = "storage-disk-empty";
  loading.textContent = "Localizando unidades...";
  list.appendChild(loading);
  runButton.disabled = true;

  try {
    let result = await Qboa.listStorageDisks();
    if (!Array.isArray(result) && result && Array.isArray(result.disks)) result = result.disks;
    const entries = Array.isArray(result) ? result.flat(Infinity).map(normalizeStorageDisk).filter(Boolean) : [];
    const disks = Array.from(new Map(entries.map((disk) => [disk.id, disk])).values());
    list.replaceChildren();

    if (!disks.length) {
      const empty = document.createElement("li");
      empty.className = "storage-disk-empty";
      empty.textContent = "Nenhuma unidade com letra de volume foi identificada. Atualize a lista ou confira se o Windows expõe o disco.";
      list.appendChild(empty);
    }

    disks.forEach((disk, index) => {
      const item = document.createElement("li");
      item.className = "storage-disk-option";
      const label = document.createElement("label");
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = disk.id;
      checkbox.dataset.storageDisk = "true";
      checkbox.checked = preserveSelection ? previous.has(disk.id) : index === 0;
      const description = document.createElement("span");
      description.className = "storage-disk-description";
      const name = document.createElement("strong");
      name.textContent = (disk.volume ? disk.volume + " · " : "") + disk.model;
      const meta = document.createElement("small");
      const bytes = Number(disk.capacity_bytes);
      const capacity = disk.capacity_bytes !== undefined && disk.capacity_bytes !== null && Number.isFinite(bytes) && bytes > 0
        ? (bytes / (1024 ** 3)).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " GiB"
        : "capacidade indisponível";
      const classNames = {
        notebook_hdd: "HD notebook", desktop_hdd: "HD desktop", sata_ssd: "SSD SATA",
        nvme: "NVMe", usb: "USB", unknown: "classe desconhecida"
      };
      meta.textContent = (classNames[disk.class] || "classe desconhecida") + " · " + disk.bus_type +
        " · " + capacity + (disk.smart_available === true ? " · SMART disponível" : "");
      description.append(name, meta);
      label.append(checkbox, description);
      item.appendChild(label);
      list.appendChild(item);
      checkbox.addEventListener("change", () => {
        if (selectAll) {
          const boxes = Array.from(list.querySelectorAll('input[type="checkbox"][data-storage-disk]'));
          selectAll.checked = boxes.length > 0 && boxes.every((box) => box.checked);
          selectAll.indeterminate = boxes.some((box) => box.checked) && !selectAll.checked;
        }
        runButton.disabled = selectedStorageDiskIds().length === 0;
      });
    });

    if (selectAll) {
      const boxes = Array.from(list.querySelectorAll('input[type="checkbox"][data-storage-disk]'));
      selectAll.checked = boxes.length > 0 && boxes.every((box) => box.checked);
      selectAll.indeterminate = boxes.some((box) => box.checked) && !selectAll.checked;
      selectAll.disabled = boxes.length === 0;
    }
    runButton.disabled = selectedStorageDiskIds().length === 0;
    document.getElementById("diagnostic-status").textContent = disks.length
      ? disks.length + " unidade(s) identificada(s). Marque uma ou mais para testar."
      : "Nenhuma unidade elegível encontrada.";
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
  }
}

function openDiagnosticModal(target = null, mode = "quick") {
  activeDiagnosticTarget = target;
  activeDiagnosticMode = mode;
  const modal = document.getElementById("diagnostic-modal");
  const title = document.getElementById("diagnostic-modal-title");
  const intro = document.getElementById("diagnostic-modal-intro");
  const runButton = document.getElementById("diagnostic-run-selected");
  const oldPicker = document.getElementById("storage-disk-picker");
  if (oldPicker) oldPicker.remove();
  const memoryPanel = document.getElementById("memory-test-panel");
  const isMemoryTest = target === "teste_memoria";
  memoryPanel.hidden = !isMemoryTest;
  modal.querySelector(".diagnostic-modal-actions").hidden = isMemoryTest;
  document.getElementById("diagnostic-results").hidden = isMemoryTest;
  modal.querySelector(".diagnostic-saved-reports").hidden = isMemoryTest;

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
    label.textContent = "Unidades a testar";
    label.htmlFor = "storage-disk-list";
    const list = document.createElement("ul");
    list.id = "storage-disk-list";
    list.className = "storage-disk-list";
    list.setAttribute("aria-label", "Selecione as unidades para o teste");
    picker.append(toolbar, label, list);
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
    memoria_ram: ["Teste de memória RAM", "Analisa os dados de memória expostos pelo Windows. Não é um teste de estresse nem substitui o Diagnóstico de Memória do Windows."],
    teste_memoria: ["Teste de integridade da RAM", "Executa padrões A01–A05 em buffers próprios do QBoa, conforme o perfil selecionado. O teste não acessa endereços físicos, não certifica toda a RAM e pode ser cancelado a qualquer momento."],
    placa_de_video: ["Teste de GPU", "Consulta os adaptadores gráficos detectados e as leituras disponíveis. Não executa benchmark nem teste de estresse gráfico."],
    processador: ["Teste de processador", "Consulta a identificação e os dados disponíveis do processador. Não executa benchmark nem teste de estresse."],
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
    void loadMemoryTestSession();
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

function renderMemoryTestSession(session) {
  if (!session) {
    memoryTestSessionSnapshot = null;
    return;
  }
  if (memoryTestSessionSnapshot &&
      memoryTestSessionSnapshot.session_id !== session.session_id &&
      ["running", "cancelling"].includes(memoryTestSessionSnapshot.lifecycle)) {
    // Não deixar um evento atrasado de uma sessão antiga sobrescrever a ativa.
    if (session.session_id !== memoryTestSessionSnapshot.session_id) return;
  }
  memoryTestSessionSnapshot = session;
  const active = ["running", "cancelling"].includes(session.lifecycle);
  const profile = document.getElementById("memory-test-profile");
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
  document.getElementById("memory-test-progress-percent").textContent = percent + "%";
  const tested = formatMemoryBytes(progress?.memory_tested_bytes);
  const budget = formatMemoryBytes(progress?.memory_budget_bytes);
  const block = progress?.current_block_bytes
    ? " · bloco atual " + formatMemoryBytes(progress.current_block_bytes)
    : "";
  document.getElementById("memory-test-progress-detail").textContent = progress
    ? tested + " de " + budget + " contabilizados · " + (progress.blocks_completed || 0) + " bloco(s) concluído(s)" + block + " · " + (progress.mismatches_total || 0) + " divergência(s)"
    : "Aguardando a primeira atualização do runner.";
  document.getElementById("memory-test-progress-phase").textContent =
    session.lifecycle === "cancelling" ? "Cancelamento solicitado; aguardando o término seguro do bloco atual." :
    session.lifecycle === "running" ? memoryPhaseLabel(progress?.phase) :
    session.lifecycle === "failed" ? "Falha ao iniciar a sessão" : "Execução finalizada";

  const result = document.getElementById("memory-test-result");
  const details = document.getElementById("memory-test-result-details");
  details.replaceChildren();
  if (session.report) {
    const report = session.report;
    const summary = document.createElement("p");
    summary.textContent = memoryTestStatusLabel(report.status);
    result.replaceChildren(summary);
    const meta = document.createElement("p");
    meta.className = "memory-test-report-meta";
    meta.textContent = "Perfil " + report.profile + " · " + formatMemoryBytes(report.memory_tested_bytes) + " testados · " + report.duration_ms + " ms · " + report.mismatches_total + " divergência(s) agregada(s)";
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
  } else if (session.error) {
    result.textContent = session.error;
  } else if (session.lifecycle === "running" || session.lifecycle === "cancelling") {
    result.textContent = session.lifecycle === "cancelling"
      ? "Cancelamento solicitado. A sessão ainda está encerrando com segurança."
      : "O teste está sendo executado. Você pode cancelar a qualquer momento.";
  } else {
    result.textContent = "A sessão ainda não tem um relatório final.";
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
  const result = document.getElementById("memory-test-result");
  const start = document.getElementById("memory-test-start");
  start.disabled = true;
  result.textContent = "Solicitando início da sessão…";
  try {
    const session = await Qboa.startMemoryTest(profile);
    renderMemoryTestSession(session);
    await loadMemoryTestSession();
  } catch (error) {
    result.textContent = String(error);
    start.disabled = false;
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
  const modal = document.getElementById("diagnostic-modal");
  modal.classList.add("hidden");
  modal.setAttribute("aria-hidden", "true");
}

async function runDiagnostic(mode, target = null) {
  const status = document.getElementById("diagnostic-status");
  document.getElementById("diagnostic-results").replaceChildren();
  const runButton = document.getElementById("diagnostic-run-selected");
  runButton.disabled = true;
  status.textContent = "Verificando...";
  try {
    if (target === "armazenamento") {
      const selectedIds = selectedStorageDiskIds();
      if (!selectedIds.length) throw new Error("Marque uma ou mais unidades antes de iniciar o teste.");
      let completed = 0;
      const failures = [];
      for (let index = 0; index < selectedIds.length; index++) {
        const diskId = selectedIds[index];
        status.textContent = "Testando unidade " + (index + 1) + " de " + selectedIds.length + " · " + diskId + "...";
        try {
          const report = await Qboa.runStorageDiagnostic(diskId);
          renderStorageReport(report, completed > 0 || failures.length > 0);
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
      await loadDiagnosticHistory(false);
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
  void listen("qboa-memory-test-progress", (event) => {
    renderMemoryTestSession(event.payload?.session);
  }).catch((error) => console.error("Falha ao receber progresso do teste de RAM:", error));
  void listen("qboa-memory-test-state", (event) => {
    renderMemoryTestSession(event.payload?.session);
  }).catch((error) => console.error("Falha ao receber estado do teste de RAM:", error));
  void listen("qboa-memory-test-finished", (event) => {
    renderMemoryTestSession(event.payload?.session);
  }).catch((error) => console.error("Falha ao receber resultado do teste de RAM:", error));

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

  document.getElementById("diagnostic-run-selected").addEventListener("click", () => {
    void runDiagnostic(activeDiagnosticMode, activeDiagnosticTarget);
  });
  document.getElementById("memory-test-start").addEventListener("click", () => {
    void startMemoryTestFromUi();
  });
  document.getElementById("memory-test-cancel").addEventListener("click", () => {
    void cancelMemoryTestFromUi();
  });
  document.getElementById("memory-test-profile").addEventListener("change", () => {
    const descriptions = {
      fast: "Rápido · 512 MiB · A01/A03 em cada bloco e A04 amostrado. Cobertura parcial.",
      standard: "Padrão · 2 GiB · executa A01–A05 nos blocos concluídos.",
      extended: "Prolongado · 4 GiB · dois passes de A01–A05 e seeds diferentes no A04."
    };
    document.getElementById("memory-test-profile-description").textContent =
      descriptions[document.getElementById("memory-test-profile").value];
  });
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
})();
