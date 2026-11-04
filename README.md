# QBoa Digital

![QBoa Digital](src-tauri/icons/logo.png)

Versão atual: `3.2.2` (versão do pacote: `3.2.2`)

[![made-with-rust](https://img.shields.io/badge/Made%20with-Rust-1f425f.svg)](https://www.rust-lang.org/) [![made-with-javascript](https://img.shields.io/badge/Made%20with-JavaScript-1f425f.svg)](https://www.javascript.com) 

## QBoa Digital 3.2.2

A versão 3.2.2 refina a experiência de execução: permite redimensionar o Registro de Atividades por arraste, apresenta o progresso na barra de tarefas do Windows e exibe mensagens contextuais bem-humoradas no popup de progresso, com variações e animação sutil. Mantém o percentual real das operações separado das mensagens descontraídas.

> O QBoa Digital mantém diagnóstico separado de limpeza e manutenção.

## Em desenvolvimento — QBoa Pro 3.3

A próxima etapa é ampliar o QBoa com ferramentas voltadas ao trabalho técnico:

- Inventário técnico completo do computador e do Windows.
- Diagnóstico de rede e conectividade.
- Central de reparo do Windows, reunindo fluxos guiados de SFC, DISM e Windows Update.
- Auditoria de programas instalados e itens de inicialização, inicialmente informativa.
- Personalização de componentes e configurações do Windows.
- Laudo técnico em PDF com logo próprio e informações OEM.
- Atualização assistida do aplicativo.

Itens como auditoria de drivers, comparação de inventários, backup/restauração de configurações e auditoria de segurança podem entrar em incrementos posteriores. Gestão de ordens de serviço e clientes fica para uma etapa futura. O escopo pode ser ajustado durante o desenvolvimento.

## Screenshots

Capturas da interface do QBoa Digital. As imagens são hospedadas no repositório do hotsite para manter uma única cópia dos arquivos.

<div align="center">
  <img src="https://raw.githubusercontent.com/qboadigital/qboadigital.github.io/main/assets/limpeza.png" alt="Tela de limpeza do QBoa Digital" width="720" />
  <p><strong>Limpeza</strong></p>
</div>

<div align="center">
  <img src="https://raw.githubusercontent.com/qboadigital/qboadigital.github.io/main/assets/capacidades.png" alt="Inventário de capacidades do Windows no QBoa Digital" width="720" />
  <p><strong>Capacidades do Windows</strong></p>
</div>

<div align="center">
  <img src="https://raw.githubusercontent.com/qboadigital/qboadigital.github.io/main/assets/diagn%C3%B3stico.png" alt="Tela de diagnóstico do QBoa Digital" width="720" />
  <p><strong>Diagnóstico</strong></p>
</div>

<div align="center">
  <img src="https://raw.githubusercontent.com/qboadigital/qboadigital.github.io/main/assets/servi%C3%A7os.png" alt="Gerenciamento de serviços do Windows no QBoa Digital" width="720" />
  <p><strong>Serviços do Windows</strong></p>
</div>

## Downloads — versão 3.2.2

- **[Instalador Windows x64 (.exe)](https://github.com/bloodshinoda/qboa-digital/releases/download/v3.2.2/qboa-digital-3.2.2-x64-installer.exe)** — instalação tradicional.
- **[Versão portátil Windows x64 (.exe)](https://github.com/bloodshinoda/qboa-digital/releases/download/v3.2.2/qboa-digital-3.2.2-x64-portable.exe)** — executável portátil distribuído como arquivo individual.
- [Notas da versão e todos os arquivos da release](https://github.com/bloodshinoda/qboa-digital/releases/tag/v3.2.2).

### Destaques da versão 3.2.2

- Redimensionamento do Registro de Atividades por alça de arraste, com altura persistida entre sessões.
- Indicador de progresso na barra de tarefas do Windows, com estado indeterminado ou percentual real.
- Mensagens contextuais com cinco variações por etapa, sem repetição imediata, além de easter eggs raros com QBoa e o dízimo.
- Animação discreta e respeito à preferência do Windows por movimento reduzido.

### Destaques da versão 3.2.1

- Progresso por etapas nas limpezas média e pesada, incluindo percentual real quando informado pelo DISM.
- Resumo final da execução e separação entre eventos de progresso e saída técnica.
- Limpeza de histórico de navegadores compatíveis, preservando favoritos, senhas e arquivos baixados.
- Confirmação para fechar navegadores antes da limpeza, respeitando a decisão de preservar os dados em uso.
- Correções nas rotinas de telemetria e privacidade avançada, com reporte de falhas por componente.
- Ajustes na execução silenciosa da Limpeza de Disco do Windows e no fluxo de inicialização com splash screen.

### Destaques da versão 3.2.0

- Laudos de diagnóstico em JSON versionado, persistidos localmente e preservados entre sessões.
- Histórico de laudos com recuperação de execuções interrompidas e preservação de resultados parciais.
- Teste de integridade da memória RAM com perfis Rápido, Padrão, Prolongado e Personalizado, além de inventário por módulo.
- Teste de carga controlada da CPU com telemetria, acompanhamento térmico, limite de segurança e laudo próprio.
- Teste de estabilidade da GPU via Direct3D 11 Compute Shader, com validação independente dos resultados e proteção térmica.
- Diagnóstico completo integrado: aviso prévio de operação demorada, confirmação, progresso por etapa, cancelamento e resumo consolidado.
- Diagnósticos individuais continuam disponíveis; o fluxo completo não mistura diagnóstico com limpeza ou manutenção.
- O laudo registra evidências e estados observados, sem emitir recomendações automáticas.
- Painel de progresso centralizado, com contraste aprimorado e progresso indeterminado quando a ferramenta não informa percentual real.
- Diálogos de confirmação e avisos de risco priorizados sobre o painel de progresso, que fica oculto enquanto a pessoa precisa decidir.
- Bloqueio de execuções concorrentes tanto na interface quanto no backend Rust.
- Descrições em linguagem simples para serviços conhecidos na prévia de ajustes de serviços.
- Diálogos de confirmação próprios da interface, com cancelamento seguro e suporte à tecla Escape.
- Resumo final das tarefas de limpeza, com indicação visual de sucesso, falha ou cancelamento.
- Aba **Sobre** com versão, autoria, link do projeto e QR Code Pix para apoiar o desenvolvimento.
- Diagnóstico informativo de armazenamento, tratando dados parciais ou inconsistentes sem transformar ausência de dados em valores falsos.
- Leitura opcional de temperatura da CPU quando um provedor compatível, como LibreHardwareMonitor ou OpenHardwareMonitor, está ativo.
- Proteções por ponto de restauração quando disponível, snapshots e rollback de alterações reversíveis.

O diagnóstico de armazenamento informa capacidade e espaço livre e oferece medições de desempenho; não substitui SMART nem verificação de integridade do sistema de arquivos. Os testes de CPU e GPU dependem de telemetria térmica compatível e aplicam limites conservadores de segurança. O teste de GPU exige Direct3D 11 Feature Level 11_0 e sensor térmico compatível; não cobre rasterização, VRAM ou todos os recursos gráficos. Os resultados não certificam integralmente os componentes.

## Arquitetura

QBoa Digital é um utilitário de manutenção do Windows desenvolvido com Tauri, Rust, HTML, CSS e JavaScript. A aplicação separa a interface, a orquestração das tarefas, os mecanismos nativos e as operações que dependem de recursos do próprio Windows:

- interface frontend e seleção de tarefas;
- registro central de tarefas, metadados de risco e associação das tarefas aos modos de limpeza;
- orquestração e acompanhamento das execuções no backend Rust;
- mecanismo nativo de limpeza em Rust para temporários, caches e resíduos conhecidos;
- integração controlada com APIs do Windows e ferramentas de manutenção, como DISM e SFC, quando a operação exige esses recursos;
- gerenciamento de serviços, políticas e tarefas agendadas;
- armazenamento local dos laudos de diagnóstico, independente do journal de alterações reversíveis.

A limpeza cotidiana não depende do Cleanmgr nem de scripts PowerShell para percorrer e remover os diretórios nativos cobertos pelo motor Rust. Ferramentas externas continuam sendo utilizadas apenas nas operações em que são necessárias.

## Mecanismo de tarefas

O backend disponibiliza um registro central de tarefas com metadados como:

- task id
- category
- risk
- necessidade de privilégios administrativos
- reversibilidade
- necessidade de ponto de restauração
- estratégia de rollback
- associação aos modos de limpeza

As tarefas são resolvidas pelo ID da tarefa, sem permitir comandos arbitrários vindos do frontend. O backend mantém a lista de tarefas e executores; `src/data/qboa-structure.json` fornece os metadados usados pela interface.

## Operações disponíveis

O aplicativo organiza as operações em áreas distintas:

### Limpeza

O motor nativo em Rust oferece três níveis:

- **Leve:** temporários e caches comuns com baixo risco.
- **Intermediária:** inclui itens adicionais, como downloads temporários do Windows Update e caches conhecidos de aplicativos Adobe.
- **Pesada:** reúne resíduos avançados e operações de manutenção mais agressivas, incluindo verificações/reparos com DISM e SFC quando selecionados.

A limpeza pesada aplica verificações de segurança antes de remover resíduos de instalações anteriores do Windows. O DISM `/ResetBase` é opcional e só deve ser oferecido em ambiente compatível, com confirmação explícita; em sistemas modificados, restritivos ou de condição incerta, a operação é ignorada para preservar possibilidades de rollback.

A limpeza de navegadores cobre os perfis e caminhos reconhecidos pelo aplicativo, incluindo histórico e caches compatíveis. Dados pessoais como favoritos, senhas e downloads não fazem parte da limpeza de histórico. Quando um navegador precisa ser fechado para liberar arquivos em uso, a interface solicita confirmação.

A limpeza Adobe é seletiva: alcança apenas caches conhecidos em locais previstos, sem remover projetos, presets, plugins ou conteúdo de trabalho.

### Serviços e privacidade

- ajuste de serviços do Windows com referências por edição/perfil, prévia das mudanças e confirmação antes da aplicação;
- ajustes de tarefas agendadas e políticas de telemetria;
- políticas de privacidade relacionadas a Recall e Copilot, quando disponíveis;
- consulta de informações e capacidades do Windows.

Identificação de fornecedor ou divergência em relação à referência não significa, por si só, que um serviço deva ser desativado. Itens desconhecidos ou fora das condições de segurança são preservados.

### Diagnósticos

Os diagnósticos são independentes da limpeza e da manutenção:

- armazenamento: informações, temperatura quando disponível, SMART e medições de desempenho;
- memória RAM: inventário por módulo e teste próprio com perfis de duração;
- processador: teste de carga controlada com monitoramento e limites de segurança;
- GPU: teste de estabilidade por Direct3D 11 Compute Shader, dentro dos recursos suportados.

Os laudos são persistidos localmente e podem ser consultados posteriormente. O diagnóstico completo avisa sobre a duração, solicita confirmação, apresenta progresso e consolida os resultados ao final. Os laudos registram evidências e estados observados; não emitem recomendações automáticas nem executam limpeza.

## Modelo de segurança

A camada de segurança possui proteção para operações destrutivas:

- pontos de restauração quando necessário
- snapshots JSON do estado anterior para Registro, serviços e tarefas agendadas reversíveis
- validação do estado após o rollback
- identificador único por execução (`execution_id`)
- cancelamento associado à execução e à árvore de processos
- log técnico de execução em `%LOCALAPPDATA%\QBoa Digital\logs\execution.log`

O journal de alterações e o estado das execuções ainda ficam em memória durante a sessão. Os snapshots ficam em `%TEMP%\qboa-digital\snapshots`.

## Pontos de restauração

A Restauração do Sistema do Windows é usada como proteção de nível sistêmico antes de alterações arriscadas. O QBoa identifica seus próprios pontos de restauração usando uma convenção consistente de descrição, como:

- QBoa Digital — Antes da tarefa: X

Isso não substitui pontos de restauração criados pelo usuário, e o aplicativo não remove automaticamente pontos de restauração existentes.

## Rollback

O rollback no QBoa possui duas camadas:

### Restauração do Sistema do Windows
Um mecanismo de proteção fornecido pelo próprio Windows.

### Rollback do QBoa
Uma camada própria de rollback para alterações reversíveis, como ajustes no registro e em serviços. As operações são registradas e gerenciadas pelo diário de alterações, sendo aplicadas em ordem inversa dentro de uma sessão.

No Windows, o rollback granular restaura o estado capturado das políticas de telemetria, serviços associados e tarefas agendadas processadas pelo QBoa. A operação só é marcada como concluída após validação. Limpeza de arquivos, verificações e reparos do sistema são operações sem reversão granular; nesses casos, o ponto de restauração é a proteção disponível.

## Modos de limpeza

A limpeza é organizada em três modos, que definem o conjunto e o nível de intervenção das operações executadas:

- **Leve:** remove temporários e caches comuns, priorizando operações de baixo risco.
- **Média:** amplia a limpeza para itens adicionais, como downloads temporários do Windows Update e caches conhecidos de aplicativos Adobe.
- **Pesada:** reúne resíduos avançados e operações de manutenção mais agressivas, incluindo verificações e reparos com DISM e SFC quando selecionados.

Os modos são exclusivos da área de limpeza. Diagnósticos, gerenciamento de serviços, privacidade e demais ferramentas são áreas independentes e não fazem parte de uma sequência automática de limpeza.

As operações moderadas ou avançadas podem exigir confirmação na interface. A limpeza pesada aplica verificações de segurança antes de remover resíduos de instalações anteriores do Windows; o DISM `/ResetBase` permanece opcional e depende de compatibilidade e confirmação explícita.

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
- `qboa-digital-windows-portable`: executável portátil `.exe`

O workflow é executado em pushes para `main`, pull requests para `main`, execução manual e tags `v*`. Ao publicar uma tag `v*`, o workflow lê a versão de `src-tauri/tauri.conf.json`, compila os pacotes `qboa-digital-<versão>-x64-installer.exe` e `qboa-digital-<versão>-x64-portable.exe` e cria uma release em rascunho com ambos os arquivos anexados. Depois, revise e publique o rascunho em [Releases](https://github.com/bloodshinoda/qboa-digital/releases). Os artefatos temporários das execuções também ficam disponíveis no GitHub Actions por 14 dias. A versão portátil é distribuída como `.exe` individual, não como ZIP, e ainda exige que o Microsoft Edge WebView2 Runtime esteja instalado no Windows.

## Higiene e manutenção do repositório

O estado dos workflows, dependências, documentação e classificação das branches está registrado em [docs/higiene-repositorio-3.2.1.md](docs/higiene-repositorio-3.2.1.md).

## Validação P0 no Windows

A matriz de validação manual está em [docs/p0-windows-validation.md](docs/p0-windows-validation.md). Ela cobre snapshots e rollback, serviços, tarefas agendadas, Restore Point, PowerShell, DISM, SFC, cancelamento, árvores de processos e execuções concorrentes. Os testes que alteram o Windows devem ser executados somente em uma VM descartável com checkpoint externo.

## Sobre e autoria

**QBoa Digital 3.2.1** é um utilitário independente para limpeza, manutenção, privacidade e diagnóstico do Windows.

- **Autor e desenvolvedor:** Vilson de Oliveira Junior
- **Tecnologias:** Tauri, Rust, JavaScript, HTML e CSS
- **Repositório:** [github.com/bloodshinoda/qboa-digital](https://github.com/bloodshinoda/qboa-digital)

## Como contribuir

O projeto aceita contribuições com código, testes, ideias, documentação e relatos de problemas:

**Repositório:** [github.com/bloodshinoda/qboa-digital](https://github.com/bloodshinoda/qboa-digital)

Também é possível contribuir via Pix. Escaneie o QR Code abaixo com o aplicativo do seu banco:

![QR Code para contribuição via Pix](src/data/pix-qrcode.png)

Código Pix copia e cola:

```text
00020126330014BR.GOV.BCB.PIX0111072603319765204000053039865802BR62070503***63041548
```
