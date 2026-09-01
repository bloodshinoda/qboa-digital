# Qboa Digital — esqueleto Tauri

Ponto de partida pra migrar o Qboa do `.bat` pra um app portable de verdade
(sem CMD visível, sem depender de Chromium embutido — usa o WebView2 nativo
do Windows).

## Estrutura

```
qboa-tauri/
├── package.json
├── src/                        ← frontend (HTML/CSS/JS puro)
│   ├── index.html
│   ├── style.css
│   ├── main.js
│   └── data/qboa-structure.json  ← as 3 seções (Limpar/Tunar/Auditoria) e seus itens
└── src-tauri/                  ← backend Rust
    ├── Cargo.toml
    ├── build.rs                ← embute o manifest de admin no .exe
    ├── qboa.manifest           ← requireAdministrator (UAC)
    ├── tauri.conf.json
    └── src/main.rs             ← mapeia cada task_id pro comando real do Windows
```

## Setup (rodar no Windows, com Rust + Node instalados)

```powershell
# 1. Instalar dependências do Rust/Tauri
rustup update
cargo install tauri-cli --version "^2"

# 2. Instalar a CLI do Tauri via npm (usa o package.json)
npm install

# 3. Rodar em modo dev
npm run tauri dev

# 4. Gerar o .exe final (fica em src-tauri/target/release/)
npm run tauri build
```

Pré-requisitos do lado Windows: **WebView2 Runtime** (já vem instalado de
fábrica no W10/11 atualizado — se não tiver, o instalador do Tauri baixa
automaticamente via `webviewInstallMode` já configurado no
`tauri.conf.json`) e o **MSVC Build Tools** (necessário pra compilar Rust no
Windows).

## O que já está pronto

- As 3 abas (Limpeza / Desengordurar / Diagnóstico) com todos os itens já
  batizados e vindos do `qboa-structure.json`.
- Paleta de cores aplicada via CSS (`--qboa-green`, `--qboa-red`,
  `--rainbow`) — todos os hex agora são reais: `--qboa-green: #005930`
  (verde da garrafa), `--qboa-red: #eb070b` e o `--rainbow` extraído
  diretamente do ícone (`#f75629 → #fa2195`).
- `src-tauri/icons/icon-source.png` já está no repo (o PNG que você mandou).
  Falta só rodar `npm run tauri icon src-tauri/icons/icon-source.png` no
  Windows pra gerar os tamanhos que o `tauri.conf.json` espera.
- Painel de console na parte de baixo da janela, pra mostrar a saída dos
  comandos (mantém a pegada "terminal" mesmo numa UI gráfica).
- `main.rs` já mapeia todo `task_id` pro comando Windows correspondente
  (`dism`, `sfc`, `cleanmgr`, `chkdsk`, `dxdiag`, `msinfo32`, PowerShell pros
  itens sem CLI direta).
- Manifest de UAC (`qboa.manifest`) configurado pra pedir elevação de admin
  automaticamente ao abrir o `.exe` — sem isso `dism`/`sfc`/`chkdsk` falham
  silenciosamente.

## O que falta (marcado como `TODO` no `main.rs`)

- **Storage Sense** (`debito_automatico`), **efeitos visuais**
  (`corte_de_gastos`), **Game Mode/Fast Startup** (`regime_especial`),
  **telemetria** (`discricao_fiscal`) e **debloat** (`sonegacao`) não têm
  CLI nativa simples — precisam de chamadas específicas de registro/API que
  são as mesmas que você já usa no `.bat`/winutils atual. Copiar a lógica de
  lá pra cá é o próximo passo natural.
- **Windows Memory Diagnostic** (`exame_de_sanidade`) exige reboot — vale
  tratar como um fluxo separado (avisar o usuário antes) em vez de rodar
  direto como os outros.
- **Streaming de progresso**: hoje `run_task` só devolve o resultado no
  final (comandos como `dism`/`chkdsk` demoram e não mostram progresso em
  tempo real). Pra isso, trocar pra `Command::spawn()` + eventos Tauri
  (`app.emit()`) lendo stdout linha a linha.
- **Ícones**: rode `npm run tauri icon <caminho-do-logo.png>` pra gerar os
  ícones referenciados no `tauri.conf.json` (`icons/32x32.png`, etc.) — não
  incluí nenhum aqui.
- **Níveis (Express/Normal/Turbo)**: os botões já existem na UI, mas ainda
  não disparam nada — falta decidir se cada nível roda um preset de tasks em
  lote (e nessa ordem) ou só muda algum comportamento visual.
