# Qboa Digital

## Arquitetura

Qboa Digital é um utilitário de manutenção do Windows desenvolvido com Tauri, Rust, HTML, CSS e JavaScript puro. O aplicativo segue uma arquitetura pequena em camadas:

- interface frontend e seleção de tarefas
- metadados do registro de tarefas e resolução de presets
- mecanismo de tarefas e execução de comandos
- gerenciador de segurança para backups, pontos de restauração e registro de alterações
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

As tarefas são resolvidas pelo ID da tarefa, sem permitir comandos arbitrários vindos do frontend.

## Operações disponíveis

O aplicativo atualmente oferece:

- limpeza de temporários, caches de navegadores, MRUs e Lixeira
- limpeza de arquivos temporários do Windows Update
- limpeza de componentes do Windows com `DISM /StartComponentCleanup`
- verificação e reparo da imagem do Windows com DISM
- verificação de arquivos do sistema com `SFC /scannow`
- ajuste de serviços, tarefas agendadas e políticas de telemetria
- políticas de privacidade relacionadas a Recall e Copilot, quando disponíveis
- consulta de informações do sistema

Operações de limpeza removem apenas arquivos temporários e caches definidos pelo aplicativo. Histórico, formulários, preenchimento automático, cookies, armazenamento local e sessões dos navegadores não são removidos pela limpeza padrão.

## Modelo de segurança

A camada de segurança foi projetada para registrar alterações e proteger operações destrutivas:

- pontos de restauração quando necessário
- metadados de backup para alterações reversíveis
- diário de alterações para o histórico de execução
- registros de rollback e ordem de reversão por sessão

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

No Windows, o rollback granular restaura políticas de telemetria, serviços associados e tarefas agendadas processadas pelo Qboa. Limpeza de arquivos, verificações e reparos do sistema são operações sem reversão granular; nesses casos, o ponto de restauração é a proteção disponível.

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
- a saída de stdout e stderr é transmitida para a interface durante a execução
- processos em execução podem ser cancelados pela interface

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

## GitHub Actions

O workflow `.github/workflows/windows-build.yml` compila o aplicativo Windows em `windows-latest` e publica dois artefatos:

- `qboa-digital-windows-installer`: instalador NSIS `.exe`
- `qboa-digital-windows-portable`: ZIP com o `.exe` portátil e DLLs de runtime

Ele é executado automaticamente em pushes para `main` e tags de versão como `v0.2.0`. Também pode ser iniciado manualmente na aba **Actions**, usando **Run workflow**. Depois que o job terminar, baixe o artefato na execução do workflow. Extraia o ZIP portátil inteiro e mantenha o `.exe` junto das DLLs incluídas. O build portátil ainda exige que o Microsoft Edge WebView2 Runtime esteja instalado no Windows.

## Como contribuir

O projeto aceita contribuições com código, testes, ideias, documentação e relatos de problemas:

**Repositório:** [github.com/bloodshinoda/qboa-digital](https://github.com/bloodshinoda/qboa-digital)

Também é possível contribuir via Pix. Escaneie o QR Code abaixo com o aplicativo do seu banco:

![QR Code para contribuição via Pix](src/data/pix-qrcode.png)

Código Pix copia e cola:

```text
00020126330014BR.GOV.BCB.PIX0111072603319765204000053039865802BR62070503***63041548
```
