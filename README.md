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

## Presets

O aplicativo oferece três modos de preset:

- Express
- Normal
- Turbo

Express é intencionalmente limitado a operações de baixo risco. Normal adiciona manutenção de rotina. Turbo inclui tarefas mais avançadas e proteção reforçada.

## Segurança

Princípios de segurança:

- o frontend envia apenas IDs de tarefas
- o Rust resolve e valida a execução
- a interface não monta comandos arbitrários de shell
- CSP habilitada na configuração do Tauri
- a elevação administrativa é mantida pelo manifest do Windows quando necessária

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
