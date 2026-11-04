# Contratos e persistência — baseline 3.2.1

Este documento descreve contratos existentes no código. Não é uma proposta de contrato novo.

## 1. Contrato de tarefa

Definido em `task_registry.rs`.

### Tipos serializados

- `TaskModule`: `Cleanup`, `Debloat`, `Diagnostic`.
- `TaskCategory`: `Cleanup`, `Debloat`, `Diagnostic`.
- `TaskRisk`: `Safe`, `Moderate`, `Advanced`; `as_str()` produz `safe`, `moderate`, `advanced`.
- `TweakProfile`: `Conservador`, `Equilibrado`, `Agressivo`, `Personalizado`, serializado em snake_case.
- `ExecutorType`: `Command`, `PowerShell`, `Sequence`, `Registry`.
- `ImpactArea`: `Space`, `Performance`, `Privacy`, `Appearance`, `Energy`, `Stability`.
- `RebootRequirement`: `No`, `Recommended`, `Required`.

### Estrutura `Task`

Campos públicos serializados:

`id`, `name`, `description`, `module`, `category`, `risk`, `requires_admin`, `estimated_duration`, `reversible`, `creates_restore_point`, `rollback_strategy`, `rollback_on_failure`, `dependencies`, `executor`, `impact`, `requirements`, `verification`, `reboot`, `profiles`, `presets`, `tweak_profiles`.

`TaskRequirements` contém `admin`, `windows_versions` e `hardware`. `ExpectedImpact` contém `area` e `description`.

### Catálogo

- O runtime cria o catálogo por `TaskRegistry::try_new()`.
- `TaskRegistry::from_catalog_json()` também existe, mas a existência desse construtor não significa que o catálogo JSON da interface seja a fonte executável do runtime.
- `resolve()`, `list()` e `preset_tasks()` são a API usada pelo backend.
- O catálogo valida executores, dependências e presets; testes internos cobrem IDs conhecidos, tarefas desconhecidas, dependências ausentes/cíclicas e executores inválidos.
- O despacho efetivo dos executores permanece em `main.rs::execute_task`.

## 2. Resultado de início de tarefa

O comando `run_task` retorna `TaskResult`, com os campos:

| Campo | Tipo | Significado atual |
|---|---|---|
| `ok` | bool | Indica que a solicitação foi aceita/iniciada, não que o trabalho em background terminou |
| `output` | string | Mensagem inicial, atualmente `Tarefa iniciada: {nome}` |
| `task_id` | string | ID estável do catálogo |
| `execution_id` | string | ID desta execução |
| `risk` | string | Rótulo textual do risco |
| `restore_point_created` | bool | Indica que a tarefa requer/cria ponto de restauração; não comprova isoladamente o resultado final |
| `change_id` | string ou null | Atualmente inicializado como null nesse retorno |

**Invariante de compatibilidade:** não reinterpretar `ok` como sucesso final da operação durante a migração. A conclusão real é comunicada pelos eventos `qboa-event`.

## 3. Envelope de evento de tarefa

Definido em `main.rs::EventPayload`, emitido no canal Tauri `qboa-event`.

| Campo | Tipo |
|---|---|
| `event` | string |
| `task_id` | string |
| `execution_id` | string |
| `message` | string |
| `ok` | bool |
| `progress` | string ou null |
| `risk` | string |

O tipo de evento também aparece no campo `event`; não se deve remover essa redundância sem alteração coordenada do frontend.

## 4. Contratos de segurança e execução

Definidos em `safety.rs`.

- `ExecutionHandle`: `execution_id`, `task_id`, `status`, `cancelled`.
- `ExecutionState`: `execution_id`, `task_id`, `status`, `started_at`, `cancelled`, `process_id`.
- `ChangeRecord`: `id`, `execution_id`, `timestamp`, `task_id`, `description`, `backup_location`, `reversible`, `rollback_status`, `result`.
- `RollbackStatus`: `Pending`, `Applied`, `RollbackAvailable`, `RollingBack`, `RolledBack`, `RollbackFailed`, `RollbackPartial`, `Unknown`, `Irreversible`.
- `SystemSnapshot`: estratégia e coleções de snapshots de Registro, serviços, tarefas agendadas e recursos/funcionalidades.

O journal é persistido pelo próprio `SafetyManager`; o caminho padrão fica sob os dados locais do aplicativo. A escrita usa arquivo temporário e substituição no destino, com tratamento específico para Windows. O journal é separado dos laudos de diagnóstico.

O campo de existência do Registro tem serialização legada compatível: grava booleano e aceita booleano ou strings antigas `present`/`absent` na leitura.

## 5. Contrato de diagnóstico geral e laudo

Definido em `diagnostics.rs` e `diagnostics_store.rs`; o contrato de produto mais detalhado está em [contrato-laudo-diagnostico-3.2.md](../../contrato-laudo-diagnostico-3.2.md).

Envelope `StoredDiagnosticReport`:

- `schema_version` (atualmente 1);
- `report_id`;
- `app_version`;
- `mode` (`quick` ou `complete`);
- `execution_status` (`running`, `completed`, `partial`, `cancelled`, `failed`);
- `started_at`, `finished_at`, `duration_ms`;
- `system`;
- `results`, `checks`, `issues`.

`DiagnosticSystem` contém `os_name`, `os_version`, `os_build`, `os_edition`, `architecture`. `DiagnosticIssue` contém `severity`, `component`, `code`, `message`, `occurred_at`.

O comando de diagnóstico cria e salva o relatório em estado `running`, persiste checkpoints durante a coleta, aplica resultado/falha e salva novamente. Os relatórios gerais ficam em `app_data_dir/diagnostics/reports`, com ID validado, limite de tamanho e substituição segura de arquivo.

O comando `update_complete_diagnostic_report` só aceita laudo de modo `complete` e estados conhecidos. Não executa testes; atualiza resultados/ocorrências e persiste o laudo.

## 6. Contrato de armazenamento

Definido em `storage_diagnostic.rs`.

- `StorageReport`: `disk_id`, `class`, `measurements`, `smart`, `recommendation`, `reasons`, `execution_status`, `space_available`.
- `DiskClass`: `notebook_hdd`, `desktop_hdd`, `sata_ssd`, `nvme`, `usb`, `unknown`.
- `MeasurementKind`: oito medições (sequenciais/aleatórias, leitura/escrita, blocos pequeno/grande).
- `SmartData`: disponibilidade e evidências SMART, temperatura, desgaste e indicadores de interface.
- `Recommendation`: `ok`, `inconclusivo`, `revisar`, `reparar`, `trocar`.

A resposta Tauri de armazenamento é `serde_json::Value`, não o tipo Rust diretamente. O comando injeta `recommendation` e `reasons` no JSON e persiste relatórios em `app_data_dir/diagnostics/storage-reports`. O schema do relatório de armazenamento está na versão 1.

## 7. Contratos de sessões de hardware

### Memória

- `MemoryTestSessionSnapshot`: `session_id`, `lifecycle`, `profile`, `started_at`, `progress`, `report`, `error`.
- O runner usa `MemoryTestConfig`, `MemoryTestProgress` e `MemoryTestReport`.
- Uma sessão ativa/cancelando bloqueia uma segunda sessão; estados terminais podem ser substituídos.
- O evento de sessão encapsula `session_id` e `session`.

### CPU

- `CpuInventory`: identificação do processador, núcleos, threads, clock e disponibilidade/provedor de sensores.
- `CpuTestProfile`: `quick`, `standard`, `extended`.
- `CpuTestSessionSnapshot`: sessão, lifecycle, perfil, tempos, progresso, última amostra, contadores, relatório e erro.
- `CpuTestReport`: schema_version, report_id, identificação da CPU, perfil/duração, estado, amostras, iterações, erros e métricas térmicas/uso/frequência.
- Relatórios têm schema_version 1 e são persistidos pelo módulo de CPU.

### GPU

- `GpuInventory`: adaptadores e disponibilidade/provedor de telemetria.
- `GpuTestProfile`: `quick`, `standard`, `extended`.
- `GpuTestSessionSnapshot`: sessão, lifecycle, perfil, tempos, progresso, última amostra, dispatches, erros, relatório e erro.
- `GpuTestReport`: schema_version, report_id, adaptadores/drivers, API, perfil/duração, estado, dispatches, erros, amostras, métricas e avisos.
- Relatórios são persistidos pelo módulo de GPU.

**Atenção:** memória, CPU e GPU possuem registries de sessão próprios. Não são o mesmo estado global de tarefas de limpeza/manutenção.

## 8. Contratos de inventário e serviços

### Inventário de capacidades

`CapabilityInventory` agrega `SystemSummary` e `Capability`. Cada capacidade possui `id`, `label`, `status`, `confidence`, `summary`, `sources`, `details`. Estados incluem detectado, desativado, registrado/não presente, não detectado, desconhecido e não aplicável.

### Inventário de memória

`MemoryInventory` contém total físico e módulos. Cada módulo pode conter localização, capacidade, fabricante, part number, tipo, velocidade configurada, largura e outros dados disponíveis; campos não confiáveis permanecem opcionais.

### Serviços

- `ServiceEntry`: nome, nome de exibição, estado, tipo de inicialização, classificação Microsoft/protegido, razão de proteção, fornecedor, classificação e evidências.
- `ServiceProfileState`: `system`, `profile`, `products`.
- `ServicePreview`: perfil, aplicabilidade, validação, mensagem, serviços e itens ignorados.
- `ServiceProfileOverride`: serviço e tipo de inicialização.

Perfis e baseline possuem sua própria persistência. A gestão direta de serviço (`service_manager`) e a aplicação de perfis (`service_profiles`) são APIs distintas.

## 9. Regras de compatibilidade para a migração

1. Não renomear campos serializados ou variantes de enums.
2. Não mudar versões de schema persistido nesta migração.
3. Não misturar paths ou formatos de journal, laudos gerais, relatórios de armazenamento, CPU/GPU e baseline de serviços.
4. Não converter respostas `serde_json::Value` em structs públicos sem coordenar o frontend.
5. Não tratar ausência de telemetria como resultado saudável.
6. Não mudar lifecycle ou política de exclusão mútua de sessões.
7. Não mover responsabilidades de execução para a camada de apresentação.
