# Dependências e ordem segura de migração — baseline 3.2.1

## 1. Visão estrutural atual

`src-tauri/Cargo.toml` declara o binário em `src/main.rs`. O arquivo principal tem 2.557 linhas e concentra:

- registro dos módulos;
- contratos locais de tarefa/evento;
- estado global;
- execução de processos e PowerShell;
- logs e cancelamento;
- implementação de limpeza e manutenção;
- despacho e orquestração de tarefas;
- comandos Tauri;
- registro dos comandos no Builder.

Os módulos de domínio já existem, mas a camada de aplicação e a fronteira Tauri ainda estão fortemente concentradas no `main.rs`.

## 2. Mapa dos módulos

| Módulo | Responsabilidade observada | Acoplamentos relevantes |
|---|---|---|
| `task_registry` | Tipos de tarefa, catálogo, validação de dependências e presets | Consumido por `main`, `registry_executor`, `tweak_profiles` |
| `safety` | Execuções, cancelamento, PID, snapshots, journal e rollback | Consumido por `main` e `registry_executor` |
| `registry_executor` | Leitura/aplicação de mutações do Registro com backend | Depende de `safety` e `task_registry`; invocado pelo executor em `main` |
| `cleanup_engine` | Limpeza nativa, caches e caminhos conhecidos | Invocado pelo executor em `main` |
| `cleanup_findings` | Coleta e aplicação de achados de limpeza | Invocado por helpers em `main` |
| `system_compat` | Avaliação de compatibilidade/conservadorismo do ambiente | Consultado pelo fluxo de execução em `main` |
| `tweak_profiles` | Resolve seleção de tarefas por perfil | Depende de `task_registry`; chamado pelo comando em `main` |
| `diagnostics` | Coleta e execução de diagnóstico geral com checkpoints | Chamado por `run_diagnostic_check` |
| `diagnostics_store` | Envelope, checkpoint, save/load dos laudos gerais | Depende dos tipos de `diagnostics`; usado também por `complete_diagnostic` |
| `complete_diagnostic` | Atualização do laudo completo existente | Depende de `diagnostics_store`, tipos de `diagnostics` e timestamp de armazenamento |
| `storage_diagnostic` | Tipos, avaliação e persistência de relatórios de armazenamento | Coletor/orquestração ainda parcialmente em `main` |
| `memory_test` | Runner e sessão de teste de RAM | `session` emite eventos Tauri; comando em `main` aplica exclusão mútua com CPU/GPU/tarefas |
| `memory_inventory` | Inventário de módulos e capacidade física de RAM | Usado por comando e validação de orçamento customizado em `main` |
| `cpu_diagnostic` | Inventário, telemetria, execução, sessão, cancelamento e persistência CPU | GPU reutiliza helpers do módulo CPU; comando em `main` coordena exclusão mútua |
| `gpu_diagnostic` | Inventário, Direct3D 11, telemetria, sessão, cancelamento e persistência GPU | Depende de helpers de `cpu_diagnostic`; comando em `main` coordena exclusão mútua |
| `capability_inventory` | Inventário de capacidades e resumo do sistema | Chamado diretamente por `get_capability_inventory`; não depende de `service_profiles` |
| `service_manager` | Listagem, classificação e ações diretas sobre serviços | API independente chamada pelos comandos de serviços |
| `service_profiles` | Detecção de edição, baseline, prévia, aplicação, restauração e undo de perfis | Backend próprio; comandos de perfil em `main` |
| `main` | Composição atual, runtime, comandos e orquestração | Conecta praticamente todos os fluxos |

**Correção importante em relação ao mapa preliminar:** o comando `get_capability_inventory` chama `capability_inventory::collect()` diretamente. Não foi encontrada dependência direta com `service_profiles` nesse fluxo.

## 3. Estado global e exclusão mútua

Atualmente `main.rs` possui:

| Estado | Tipo | Papel |
|---|---|---|
| `BROWSER_CLEANUP_ENABLED` | `AtomicBool` | Injeta variável de ambiente que habilita/desabilita limpeza de cache de browser no processo |
| `STORAGE_SCRIPT_COUNTER` | `AtomicU64`, Windows | Evita colisão de nomes de scripts temporários de armazenamento |
| `SAFETY_STATE` | `OnceLock<Arc<Mutex<SafetyManager>>>` | Estado de execução, cancelamento, PID, snapshots e journal |
| `OPERATION_START_GATE` | `OnceLock<Mutex<()>>` | Serializa o início de operações/testes concorrentes |
| `RESTORE_POINT_RESPONSES` | `OnceLock<Mutex<HashMap<execution_id, Sender<bool>>>>` | Correlaciona confirmação de falha ao criar ponto de restauração |
| `RESET_BASE_RESPONSES` | `OnceLock<Mutex<HashMap<execution_id, Sender<bool>>>>` | Correlaciona confirmação de DISM /ResetBase |

Além desses estados, os módulos de memória, CPU e GPU mantêm registries próprios de sessão, com `OnceLock` e `Mutex`.

### Exclusão mútua observada

- Início de teste de RAM, CPU e GPU usa `OPERATION_START_GATE`.
- Testes de RAM/CPU/GPU verificam se existe tarefa de limpeza/manutenção ativa por meio do `SafetyManager`.
- RAM verifica atividade de CPU e GPU.
- CPU verifica atividade de GPU e RAM.
- GPU verifica atividade de RAM e CPU.
- Armazenamento verifica atividade de RAM, CPU e GPU.
- `run_preset` impede iniciar quando há tarefa ativa e executa tarefas sequencialmente.
- A exclusão mútua é distribuída entre `main.rs` e registries de sessão; não é um serviço centralizado único.

**Risco:** mover apenas `start_task` ou apenas os comandos de testes pode alterar a janela de concorrência ou permitir operações incompatíveis. O gate e todas as verificações devem ser tratados como um único contrato comportamental.

## 4. Runtime de processos e fluxo de tarefa

### Runtime atual em `main.rs`

Funções e responsabilidades identificadas:

- `decode_process_output`: decodificação de saída, incluindo caminhos específicos de Windows.
- `forward_output`: leitura assíncrona de stdout/stderr.
- `configure_hidden_console`: configuração de console oculto no Windows.
- `execution_log_path`, `local_timestamp`, `write_execution_log`: logs técnicos.
- `run_command`: cria processo, injeta `QBOA_CLEAN_BROWSER_CACHE`, registra PID no `SafetyManager`, encaminha saída e monitora cancelamento.
- `terminate_process_tree`: encerra árvore de processos (Windows: `taskkill /T /F`).
- `run_powershell`: aplica preâmbulo UTF-8 e delega a `run_command`.
- `run_system_command`: caminho de execução de ferramentas do sistema.
- `emit_event`: publica envelope de tarefa em `qboa-event`.

### Fluxo de tarefa

1. `run_task` resolve o ID no `TaskRegistry`.
2. `start_task` adquire o gate de início e registra execução no `SafetyManager`.
3. A execução real é despachada em `tauri::async_runtime::spawn`.
4. Se a tarefa exige reversibilidade/Registro, captura snapshot.
5. Se necessário, tenta criar ponto de restauração; em caso de falha, aguarda confirmação correlacionada pelo ID da execução.
6. Executa a tarefa pelo dispatcher `execute_task`.
7. Emite saída/progresso.
8. Registra/atualiza alterações no journal, quando aplicável.
9. Finaliza o estado e emite evento terminal.

O comando `run_task` devolve o identificador de execução logo após o agendamento. Não aguarda o término do trabalho em background.

## 5. Acoplamentos críticos e risco de extração

| Área | Acoplamento | Risco se extraído isoladamente |
|---|---|---|
| Processos | `run_command` ↔ PID no `SafetyManager` ↔ cancelamento | Processo pode sobreviver ao cancelamento ou não ser associado à execução |
| Tarefa | `start_task` ↔ gate ↔ snapshots ↔ confirmações ↔ journal ↔ eventos | Mudança de ordem pode remover proteção ou quebrar UI |
| Registro | `registry_executor` ↔ `safety` ↔ catálogo | Mutação sem snapshot/journal correto |
| Laudos | `diagnostics` ↔ `diagnostics_store` ↔ `complete_diagnostic` | Laudo parcial pode deixar de persistir ou incompatibilidade de schema |
| RAM/CPU/GPU | registries próprios ↔ gate global ↔ comandos | Duas cargas concorrentes ou cancelamento inconsistente |
| CPU/GPU | GPU usa helpers de telemetria/PowerShell do módulo CPU | Extração/movimentação pode quebrar compilação ou duplicar comportamento |
| Armazenamento | script/coleta em `main`, avaliação/persistência em módulo | Contrato JSON pode mudar silenciosamente |
| Serviços | `service_manager` e `service_profiles` são fluxos diferentes | Misturar ação individual com aplicação de perfil e alterar proteções |
| Frontend | nomes/assinaturas Tauri e eventos | UI deixa de invocar ou interpretar corretamente |

## 6. Ordem segura de migração

A ordem abaixo é uma sequência de extração, não uma autorização para implementar tudo de uma vez.

### F0 — caracterização (esta etapa)

- Congelar referência e hashes.
- Inventariar comandos, eventos, contratos, persistência e dependências.
- Identificar testes existentes e lacunas.
- Nenhuma mudança funcional.

### F1 — funções puras e auxiliares

Extrair apenas funções sem estado global, sem `AppHandle` e sem efeito colateral, com testes de caracterização. Candidatos devem ser confirmados por análise de chamada antes da extração. Não mover funções de processo ou de tarefa nesta fase.

### F2 — estado da aplicação

Isolar os seis estados globais listados acima preservando tipo, inicialização, escopo, lock, ordenação e política de concorrência. Não converter para outro mecanismo de estado nesta etapa.

### F3 — runtime de processo

Extrair processo, PowerShell, saída, logs e cancelamento como uma unidade. Preservar registro/remoção de PID e comportamento de término de árvore. Não alterar o protocolo de cancelamento.

### F4 — motor e orquestração de tarefas

Extrair em ordem: resolução de tarefa, adaptadores de execução, despacho `execute_task`, e por último `start_task`. Manter a ordem de snapshot, ponto de restauração, confirmação, execução, journal, finalização e eventos.

### F5 — comandos Tauri por domínio

Mover wrappers para módulos de comandos mantendo nomes, parâmetros, tipos de retorno e efeitos. Registrar os mesmos 42 comandos no `generate_handler!`. Os comandos de sessão devem continuar chamando os registries de domínio corretos.

### F6 — inicialização

Reduzir `main.rs` a composição do Builder, registro de comandos e inicialização apenas depois das extrações anteriores.

## 7. Testes de caracterização que faltam antes de F1

Os testes unitários existentes dos módulos não substituem testes de contrato de integração. Antes da primeira extração, adicionar ou executar verificações para:

1. inventário estático dos 42 comandos registrados;
2. nomes e formato do envelope `qboa-event`;
3. nomes dos nove eventos de tarefa e dos nove eventos de sessão;
4. `run_task` retorna antes da conclusão e fornece `execution_id`;
5. confirmação de ponto de restauração correlacionada por `execution_id`;
6. confirmação de `/ResetBase` correlacionada por `execution_id`;
7. cancelamento por `execution_id` e rejeição de execução ambígua;
8. journal e snapshots permanecem fora das pastas de laudo;
9. diagnóstico geral persiste estado inicial, checkpoints e estado terminal;
10. armazenamento mantém JSON e persistência atuais;
11. exclusão mútua RAM/CPU/GPU/armazenamento/tarefas;
12. serviço individual e perfil continuam APIs separadas.

## 8. Portões de aprovação por extração

Cada etapa deve ser um commit pequeno na branch de migração, com:

- `cargo fmt --check`;
- `cargo check`;
- `cargo test`;
- CI Windows;
- inspeção do diff para confirmar ausência de alteração funcional;
- verificação dos contratos Tauri afetados;
- smoke test manual do fluxo correspondente quando houver interação com Windows.

Não avançar para a próxima etapa com falha aberta. A tag `v3.2.1` permanece intocada; a documentação e a migração partem da branch `main`.
