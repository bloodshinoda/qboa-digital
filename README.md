# Qboa Digital

![Qboa Digital](src-tauri/icons/logo.png)

Versão atual: `0.9.0`

## Arquitetura

Qboa Digital é um utilitário de manutenção do Windows desenvolvido com Tauri, Rust, HTML, CSS e JavaScript puro. O aplicativo segue uma arquitetura pequena em camadas:

- interface frontend e seleção de tarefas
- metadados do registro de tarefas e resolução de presets
- mecanismo de tarefas e execução de comandos
- gerenciador de segurança para snapshots, pontos de restauração e registro de alterações
- integração com o Windows por meio de comandos nativos e PowerShell

## Mecanismo de tarefas

O backend disponibiliza um registro central de tarefas com metadados como:

- task id
- category
- risk
- necessidade de privilégios administrativos
- reversibilidade
- necessidade de ponto de restauração
- estratégia de rollback
- participação em presets

As tarefas são resolvidas pelo ID da tarefa, sem permitir comandos arbitrários vindos do frontend. O backend mantém a lista de tarefas e executores; `src/data/qboa-structure.json` fornece os metadados usados pela interface.

## Operações disponíveis

O aplicativo atualmente oferece:

- limpeza de temporários, caches de navegadores, arquivos recentes de aplicativos, histórico do Executar e caminhos recentes do Explorer
- limpeza de arquivos temporários do Windows Update
- limpeza de componentes do Windows com `DISM /StartComponentCleanup`
- verificação e reparo da imagem do Windows com DISM
- verificação de arquivos do sistema com `SFC /scannow`
- ajuste de serviços, tarefas agendadas e políticas de telemetria
- políticas de privacidade relacionadas a Recall e Copilot, quando disponíveis
- consulta de informações do sistema

Operações de limpeza removem apenas arquivos temporários e caches definidos pelo aplicativo. A limpeza padrão não remove pastas fixadas ou a estrutura de pastas recentes do Quick access/Explorer, nem histórico, formulários, preenchimento automático, cookies, armazenamento local ou sessões dos navegadores.

## Modelo de segurança

A camada de segurança possui proteção para operações destrutivas:

- pontos de restauração quando necessário
- snapshots JSON do estado anterior para Registro, serviços e tarefas agendadas reversíveis
- validação do estado após o rollback
- identificador único por execução (`execution_id`)
- cancelamento associado à execução e à árvore de processos
- log técnico de execução em `%LOCALAPPDATA%\Qboa Digital\logs\execution.log`

O journal de alterações e o estado das execuções ainda ficam em memória durante a sessão. Os snapshots ficam em `%TEMP%\qboa-digital\snapshots`.

## Pontos de restauração

A Restauração do Sistema do Windows é usada como proteção de nível sistêmico antes de alterações arriscadas. O Qboa identifica seus próprios pontos de restauração usando uma convenção consistente de descrição, como:

- Qboa Digital — Antes da tarefa: X

Isso não substitui pontos de restauração criados pelo usuário, e o aplicativo não remove automaticamente pontos de restauração existentes.

## Rollback

O rollback no Qboa possui duas camadas:

### Restauração do Sistema do Windows
Um mecanismo de proteção fornecido pelo próprio Windows.

### Rollback do Qboa
Uma camada própria de rollback para alterações reversíveis, como ajustes no registro e em serviços. As operações são registradas e gerenciadas pelo diário de alterações, sendo aplicadas em ordem inversa dentro de uma sessão.

No Windows, o rollback granular restaura o estado capturado das políticas de telemetria, serviços associados e tarefas agendadas processadas pelo Qboa. A operação só é marcada como concluída após validação. Limpeza de arquivos, verificações e reparos do sistema são operações sem reversão granular; nesses casos, o ponto de restauração é a proteção disponível.

## Presets

O aplicativo oferece três modos de preset:

- Express
- Normal
- Turbo

Express é intencionalmente limitado a operações de baixo risco. Normal adiciona manutenção de rotina. Turbo inclui tarefas mais avançadas e proteção reforçada.

As sequências atuais são:

- **Express:** limpeza leve e informações do sistema
- **Normal:** telemetria, limpeza média e informações do sistema
- **Turbo:** telemetria e limpeza pesada, incluindo Windows Update, DISM Scan, limpeza de componentes, DISM Restore e SFC

As tarefas de um preset são executadas sequencialmente pelo backend. Tarefas moderadas e avançadas exigem confirmação na interface.

## Segurança

Princípios de segurança:

- o frontend envia apenas IDs de tarefas
- o Rust resolve e valida a execução
- a interface não monta comandos arbitrários de shell
- CSP habilitada na configuração do Tauri
- a elevação administrativa é mantida pelo manifest do Windows quando necessária
- a saída dos processos é transmitida para a interface durante a execução
- o backend registra PID, comando, status e falhas no log técnico
- processos em execução podem ser cancelados pela interface; no Windows, a árvore é encerrada com `taskkill /T /F`
- o PowerShell Core pode validar a sintaxe dos scripts sem executá-los:

```powershell
[System.Management.Automation.Language.Parser]::ParseInput($script, [ref]$tokens, [ref]$errors)
```

## Desenvolvimento

```bash
npm install
npm run dev
```

## Testes

```bash
cd src-tauri
cargo test
cargo check
```

## Requisitos do Windows

- Windows 10 ou 11
- runtime do WebView2
- toolchain do Rust
- Build Tools do MSVC para compilação no Windows
- privilégios de administrador para tarefas como DISM, SFC e alterações no registro

## Compilação

```bash
npm run build
```

Esse comando gera um build local do Tauri em uma máquina compatível. O empacotamento específico para Windows e a execução real das operações do Windows só são válidos no Windows.

### Imagens do instalador NSIS

O Tauri/NSIS usa estas dimensões para imagens personalizadas do instalador:

- `headerImage`: `150 x 57 px`
- `sidebarImage`: `164 x 314 px`

Essas imagens ainda não estão configuradas em `tauri.conf.json`. O logo principal do aplicativo está em `src-tauri/icons/logo.png`.

## GitHub Actions

O workflow `.github/workflows/windows-build.yml` compila o aplicativo Windows em `windows-latest` e publica dois artefatos:

- `qboa-digital-windows-installer`: instalador NSIS `.exe`
- `qboa-digital-windows-portable`: ZIP com o `.exe` portátil e DLLs de runtime

Ele é executado automaticamente em pushes para `main` e tags de versão como `v0.9.0`. Também pode ser iniciado manualmente na aba **Actions**, usando **Run workflow**. Depois que o job terminar, baixe o artefato na execução do workflow. Extraia o ZIP portátil inteiro e mantenha o `.exe` junto das DLLs incluídas. O build portátil ainda exige que o Microsoft Edge WebView2 Runtime esteja instalado no Windows.

## Validação P0 no Windows

A matriz de validação manual está em [docs/p0-windows-validation.md](docs/p0-windows-validation.md). Ela cobre snapshots e rollback, serviços, tarefas agendadas, Restore Point, PowerShell, DISM, SFC, cancelamento, árvores de processos e execuções concorrentes. Os testes que alteram o Windows devem ser executados somente em uma VM descartável com checkpoint externo.

## Como contribuir

O projeto aceita contribuições com código, testes, ideias, documentação e relatos de problemas:

**Repositório:** [github.com/bloodshinoda/qboa-digital](https://github.com/bloodshinoda/qboa-digital)

Também é possível contribuir via Pix. Escaneie o QR Code abaixo com o aplicativo do seu banco:

![QR Code para contribuição via Pix](src/data/pix-qrcode.png)

Código Pix copia e cola:

```text
00020126330014BR.GOV.BCB.PIX0111072603319765204000053039865802BR62070503***63041548
```
