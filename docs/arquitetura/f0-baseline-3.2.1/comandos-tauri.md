# Comandos Tauri — baseline 3.2.1

Fonte de verdade do registro: `src-tauri/src/main.rs::main`, macro `tauri::generate_handler!`. O frontend auditado é `src/main.js`.

**Total registrado: 42 comandos.** Os nomes abaixo são os identificadores públicos invocados pelo frontend; a migração não pode renomeá-los nem alterar os parâmetros/retornos sem mudança coordenada de API.

## Tarefas, limpeza e segurança

| Comando | Parâmetros Rust | Retorno |
|---|---|---|
| `get_tasks` | — | `Result<Vec<Task>, String>` |
| `run_task` | `app: AppHandle`, `task_id: String`, `shutdown_on_complete: bool` | `Result<TaskResult, String>` |
| `respond_restore_point_failure` | `execution_id: String`, `proceed: bool` | `Result<(), String>` |
| `respond_reset_base_confirmation` | `execution_id: String`, `proceed: bool` | `Result<(), String>` |
| `get_running_browsers` | — | `Result<Vec<String>, String>` |
| `prepare_browser_cleanup` | `close_running: bool` | `Result<String, String>` |
| `cancel_task` | `task_id: String`, `execution_id: Option<String>` | `Result<String, String>` |
| `run_preset` | `app: AppHandle`, `preset_id: String` | `Result<Vec<String>, String>` |
| `get_debloat_profile_tasks` | `profile: TweakProfile`, `selected_ids: Vec<String>` | `Result<Vec<String>, String>` |
| `create_restore_point` | — | `Result<String, String>` |
| `rollback_session` | — | `Result<Vec<String>, String>` |
| `get_session_history` | — | `Result<Vec<ChangeRecord>, String>` |
| `finish_startup` | `app: AppHandle` | `Result<(), String>` |

## Diagnóstico geral

| Comando | Parâmetros Rust | Retorno |
|---|---|---|
| `run_diagnostic_check` | `app: AppHandle`, `mode: DiagnosticMode` | `Result<StoredDiagnosticReport, String>` |
| `update_complete_diagnostic_report` | `app`, `report_id: String`, `results: Vec<DiagnosticResult>`, `issues: Vec<DiagnosticIssue>`, `system: Option<DiagnosticSystem>`, `execution_status: String`, `duration_ms: Option<u64>` | `Result<StoredDiagnosticReport, String>` |
| `get_diagnostic_reports` | `app: AppHandle` | `Result<Vec<StoredDiagnosticReport>, String>` |

O comando `update_complete_diagnostic_report` é implementado em `complete_diagnostic.rs` e registrado com caminho qualificado no `generate_handler!`.

## Armazenamento

| Comando | Parâmetros Rust | Retorno |
|---|---|---|
| `list_storage_disks` | — | `Result<serde_json::Value, String>` |
| `run_storage_diagnostic` | `app: AppHandle`, `disk_id: String` | `Result<serde_json::Value, String>` |
| `get_storage_diagnostic_reports` | `app: AppHandle` | `Result<serde_json::Value, String>` |

O executor/coletor PowerShell do armazenamento ainda está implementado em `main.rs`; avaliação e persistência ficam em `storage_diagnostic.rs`.

## Memória

| Comando | Parâmetros Rust | Retorno |
|---|---|---|
| `start_memory_test` | `app`, `profile: MemoryTestProfile`, `memory_budget_bytes: Option<u64>`, `preferred_block_bytes: Option<usize>`, `seed: Option<u64>` | `Result<MemoryTestSessionSnapshot, String>` |
| `get_memory_test_session` | — | `Option<MemoryTestSessionSnapshot>` |
| `cancel_memory_test` | `app`, `session_id: Option<String>` | `Result<MemoryTestSessionSnapshot, String>` |
| `get_memory_inventory` | — | `Result<MemoryInventory, String>` |

## CPU

| Comando | Parâmetros Rust | Retorno |
|---|---|---|
| `get_cpu_inventory` | `app: AppHandle` | `Result<CpuInventory, String>` |
| `install_pawnio` | `app: AppHandle` | `Result<String, String>` |
| `start_cpu_test` | `app`, `profile: CpuTestProfile` | `Result<CpuTestSessionSnapshot, String>` |
| `get_cpu_test_session` | — | `Option<CpuTestSessionSnapshot>` |
| `cancel_cpu_test` | `app`, `session_id: Option<String>` | `Result<CpuTestSessionSnapshot, String>` |
| `get_cpu_diagnostic_reports` | `app: AppHandle` | `Result<Vec<CpuTestReport>, String>` |

## GPU

| Comando | Parâmetros Rust | Retorno |
|---|---|---|
| `get_gpu_inventory` | `app: AppHandle` | `Result<GpuInventory, String>` |
| `start_gpu_test` | `app`, `profile: GpuTestProfile` | `Result<GpuTestSessionSnapshot, String>` |
| `get_gpu_test_session` | — | `Option<GpuTestSessionSnapshot>` |
| `cancel_gpu_test` | `app`, `session_id: Option<String>` | `Result<GpuTestSessionSnapshot, String>` |
| `get_gpu_diagnostic_reports` | `app: AppHandle` | `Result<Vec<GpuTestReport>, String>` |

## Inventário e serviços

| Comando | Parâmetros Rust | Retorno |
|---|---|---|
| `get_capability_inventory` | — | `Result<CapabilityInventory, String>` |
| `list_windows_services` | — | `Result<Vec<ServiceEntry>, String>` |
| `manage_windows_service` | `name: String`, `action: String`, `start_type: Option<String>` | `Result<String, String>` |
| `get_service_profile_state` | — | `Result<ServiceProfileState, String>` |
| `preview_service_profile` | `profile: ProductProfile` | `Result<ServicePreview, String>` |
| `apply_service_profile` | `profile: ProductProfile`, `changes: Vec<ServiceProfileOverride>` | `Result<(), String>` |
| `restore_windows_service_defaults` | — | `Result<(), String>` |
| `undo_service_profile_changes` | — | `Result<(), String>` |

## Observações importantes

- `run_task` inicia o trabalho em background e retorna um `TaskResult` de início; a conclusão chega por evento.
- `run_preset` retorna a lista de IDs selecionados e executa as tarefas sequencialmente em background.
- `cancel_task` aceita `execution_id` opcional; quando omitido, o backend tenta resolver a execução pela tarefa e pode rejeitar a ambiguidade.
- Testes de RAM, CPU e GPU têm comandos e sessões separados.
- O frontend `src/main.js` contém invocações diretas para a maior parte dos comandos. Na busca literal por `invoke("nome_do_comando")`, não foram encontradas chamadas diretas para `run_preset` e `get_debloat_profile_tasks`; ambos permanecem registrados no backend e podem ser APIs legadas ou chamadas por outro caminho. Essa distinção deve ser revalidada antes de qualquer remoção. A inspeção estática encontrou também listeners para os eventos descritos em [eventos-tauri.md](./eventos-tauri.md).
- Tipos e campos serializados estão detalhados em [contratos-e-persistencia.md](./contratos-e-persistencia.md).

## Regra para extração

A primeira migração dos comandos deve ser puramente estrutural: transferir a função para o módulo de comandos, preservar nome, assinatura, tipo de retorno e comportamento, e atualizar somente o caminho de registro no `generate_handler!` quando necessário.
