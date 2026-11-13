# Baseline arquitetural — QBoa Digital 3.3-dev

**Branch auditada:** `v3.3-dev`  
**Referência de código:** `a0da1770342df62cdd7f3887f1af0ac51370b250`  
**Arquivo principal auditado:** `src-tauri/src/main.rs` — blob `f869fc2799bb2c2681263fa9b1ffa6e1d13968fe`  
**Estado:** inventário estático atualizado; caracterização comportamental específica desta revisão ainda precisa ser repetida.

> Este documento substitui a baseline antiga como referência para o trabalho 3.3. A pasta `f0-baseline-3.2.1` é preservada como registro histórico da etapa anterior.

## 1. Retrato do código

| Indicador | Estado observado |
|---|---:|
| Linhas em `main.rs` | 3.378 |
| Módulos declarados diretamente em `main.rs` | 21 |
| Comandos Tauri definidos diretamente em `main.rs` | 44 |
| Comandos registrados no `generate_handler!` | 45 |
| Comandos registrados e definidos em outro módulo | 1 — `complete_diagnostic::update_complete_diagnostic_report` |
| Linhas em `src/main.js` | 4.190 |
| Listeners Tauri no frontend | 10 |
| Testes unitários em `main.rs` | 20 |
| Testes em `operation_contracts.rs` | 11 |

Os números são uma fotografia estática da referência indicada, não métricas dinâmicas nem afirmação de que toda a suíte foi executada nesta revisão.

## 2. Módulos Rust existentes

Declarados pelo binário em `main.rs`:

- `capability_inventory`
- `cleanup_engine`
- `cleanup_findings`
- `cleanup_progress`
- `complete_diagnostic`
- `cpu_diagnostic`
- `diagnostics`
- `diagnostics_store`
- `gpu_diagnostic`
- `memory_inventory`
- `memory_test`
- `operation_contracts`
- `registry_executor`
- `safety`
- `service_manager`
- `service_profiles`
- `storage_diagnostic`
- `system_compat`
- `task_registry`
- `tweak_profiles`
- `windows_update`

O arquivo `lib.rs` exporta somente `memory_test`, `safety` e `task_registry`. O binário declara novamente os módulos que usa. A propriedade e a fronteira entre crate de biblioteca e binário ainda precisam ser decididas antes de mover módulos; não fazer uma reorganização mecânica de `mod`/`pub mod`.

## 3. Superfície pública Tauri

Os 45 handlers registrados atualmente são:

**Limpeza, preparação e segurança**
- `get_tasks`
- `prepare_cleanup`
- `cancel_cleanup_preparation`
- `run_task`
- `set_shutdown_on_complete`
- `respond_restore_point_failure`
- `get_running_browsers`
- `prepare_browser_cleanup`
- `cancel_task`
- `get_debloat_profile_tasks`
- `create_restore_point`
- `rollback_session`
- `get_session_history`

**Diagnóstico geral e armazenamento**
- `run_diagnostic_check`
- `complete_diagnostic::update_complete_diagnostic_report`
- `get_diagnostic_reports`
- `list_storage_disks`
- `run_storage_diagnostic`
- `get_storage_diagnostic_reports`

**Memória**
- `start_memory_test`
- `get_memory_test_session`
- `cancel_memory_test`
- `get_memory_inventory`

**CPU**
- `get_cpu_inventory`
- `install_pawnio`
- `start_cpu_test`
- `get_cpu_test_session`
- `cancel_cpu_test`
- `get_cpu_diagnostic_reports`

**GPU**
- `get_gpu_inventory`
- `start_gpu_test`
- `get_gpu_test_session`
- `cancel_gpu_test`
- `get_gpu_diagnostic_reports`

**Capacidades e serviços**
- `get_capability_inventory`
- `list_windows_services`
- `manage_windows_service`
- `repair_windows_update`
- `get_service_profile_state`
- `preview_service_profile`
- `apply_service_profile`
- `restore_windows_service_defaults`
- `undo_service_profile_changes`

**Inicialização e desenvolvimento**
- `open_devtools`
- `finish_startup`

Compatibilidade obrigatória durante extrações: manter nomes, argumentos, tipos serializados, retornos, erros observáveis e registro no frontend. Qualquer alteração intencional de API deve ser tratada como mudança separada, não escondida dentro da refatoração.

## 4. Contratos e estado compartilhado observados

- `operation_contracts.rs` define o ciclo de vida específico de limpeza/manutenção, progresso, referências de execução, confirmações, envelope de evento e resultado.
- O próprio módulo declara que diagnósticos mantêm modelos e ciclos de vida próprios. Não unificar os domínios por conveniência.
- `task_registry.rs` concentra catálogo/tipos de tarefas, módulos, categorias, riscos, executores, requisitos e perfis.
- `safety.rs` mantém journal de alterações, snapshots, estado de execução, PID e suporte a cancelamento/rollback.
- `diagnostics_store.rs` persiste laudos de diagnóstico; não confundir essa persistência com o journal de alterações.
- `main.rs` ainda mantém estado global via `OnceLock`, `Mutex` e atomics, além de coordenação de início de operação e preparação de limpeza.
- RAM, CPU e GPU mantêm sessões próprias; armazenamento e diagnóstico geral também possuem fluxos próprios.
- O gate de início e as verificações cruzadas de concorrência estão distribuídos entre `main.rs` e os módulos de sessão. Devem ser caracterizados como um único invariante antes de extração.

## 5. Eventos Tauri observados

O frontend registra dez listeners:

- `qboa-event`
- `qboa-memory-test-progress`
- `qboa-memory-test-state`
- `qboa-memory-test-finished`
- `qboa-cpu-test-progress`
- `qboa-cpu-test-state`
- `qboa-cpu-test-finished`
- `qboa-gpu-test-progress`
- `qboa-gpu-test-state`
- `qboa-gpu-test-finished`

Os eventos de operação usam o envelope `CleanupEventEnvelope`, com `event`, `task_id`, `execution_id`, `message`, `ok`, `progress` e `risk`. Eventos de sessão de hardware usam seus próprios snapshots e identificadores de sessão.

Valores encontrados no emissor de operações incluem `task-started`, `task-output`, `task-progress`, `task-completed`, `task-error`, `task-cancelled`, `restore-point-created`, `restore-point-confirmation-required`, `shutdown-scheduled` e `shutdown-error`. O inventário de eventos deve ser confirmado por testes de contrato e não apenas por busca textual.

## 6. Runtime e composição ainda concentrados em main.rs

O arquivo principal ainda acumula:
- inicialização do Tauri e registro de comandos;
- comandos de múltiplos domínios;
- estado global e gates de concorrência;
- coordenação do ciclo de vida de limpeza/manutenção;
- execução de processos e PowerShell, captura/encaminhamento de saída e monitoramento;
- logging e emissão de eventos;
- partes de inventário, diagnósticos e operações de sistema.

Portanto, o objetivo não é apenas dividir o arquivo em módulos menores. A separação precisa distinguir contratos, estado, runtime de processos, casos de uso/orquestração, adaptadores de plataforma e borda Tauri.

## 7. CI e validação

O workflow `.github/workflows/windows-build.yml` para push/PR em `main` e `v3.3-dev`, além de tags `v*` e execução manual. Ele executa validação JavaScript, autoteste do coletor de armazenamento, `cargo fmt --check`, `cargo test --locked --all-targets` e build Windows. Builds de desenvolvimento incluem DevTools; tags de release não.

O usuário informou que o build #775 passou. Isso é evidência de build aprovado pelo usuário, mas não substitui os novos testes de caracterização descritos no plano.

## 8. Pontos arquiteturais que precisam de decisão

1. Definir a fronteira canônica entre biblioteca e binário: hoje `lib.rs` exporta três módulos e `main.rs` declara 21.
2. Definir o dono do estado global e do gate de exclusão mútua.
3. Definir um Process Runtime reutilizável sem acoplar domínio a `AppHandle`.
4. Definir contratos comuns de execução sem apagar as diferenças entre limpeza e diagnósticos.
5. Manter persistências distintas: journal/snapshots de segurança, laudos gerais e relatórios/sessões de hardware.
6. Preservar integralmente a superfície Tauri e os dez listeners durante a migração.

## 9. Critério de saída desta atualização F0

- [x] Referência de código e branch registradas.
- [x] Inventário estático de módulos, comandos, eventos e arquivos centrais atualizado.
- [x] Divergência entre `lib.rs` e `main.rs` documentada.
- [ ] Matriz de caracterização executada contra esta referência.
- [ ] Testes de contrato adicionados para os invariantes ainda não cobertos.
- [ ] CI verde após a inclusão dos testes.

A F0 documental está atualizada. A liberação para extração de código depende dos itens pendentes, não apenas deste inventário.
