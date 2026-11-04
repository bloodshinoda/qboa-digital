# Eventos Tauri — baseline 3.2.1

O frontend `src/main.js` registra listeners para dez canais Tauri: `qboa-event` e três canais por teste de hardware (RAM, CPU e GPU).

## 1. Operações e tarefas: `qboa-event`

Emissor: `main.rs::emit_event`.  
Payload serializado: `EventPayload`.

| Campo | Tipo |
|---|---|
| `event` | string |
| `task_id` | string |
| `execution_id` | string |
| `message` | string |
| `ok` | bool |
| `progress` | string ou null |
| `risk` | string |

Valores de `event` observados no código:

| Evento | Uso |
|---|---|
| `task-started` | Uma tarefa foi registrada e iniciou o fluxo em background |
| `task-output` | Mensagem, aviso ou saída de execução |
| `task-progress` | Atualização de progresso textual |
| `task-completed` | Execução terminou com sucesso |
| `task-error` | Falha no snapshot, executor, journal ou finalização |
| `task-cancelled` | Cancelamento confirmado ou decisão do usuário de não prosseguir |
| `restore-point-created` | Ponto de restauração criado |
| `restore-point-confirmation-required` | Falha ao criar ponto; aguarda decisão do frontend |
| `reset-base-confirmation-required` | Solicita confirmação específica para DISM /ResetBase |

O campo `event` no payload repete a categoria emitida; o frontend deve continuar recebendo o objeto completo.

### Fluxo de confirmação

1. O backend emite `restore-point-confirmation-required` e registra um canal de resposta indexado por `execution_id`.
2. O frontend chama `respond_restore_point_failure(execution_id, proceed)`.
3. O backend remove o canal pendente e envia a decisão para a thread de execução.
4. A confirmação de `/ResetBase` segue o mesmo padrão por meio de `respond_reset_base_confirmation`.

A correlação por `execution_id` é essencial. Não trocar por estado global único ou confirmação sem identificador.

## 2. Teste de memória

Canais:

- `qboa-memory-test-progress`
- `qboa-memory-test-state`
- `qboa-memory-test-finished`

Emissor: `memory_test/session.rs`.

Envelope comum:

```json
{
  "session_id": "identificador da sessão",
  "session": {
    "session_id": "identificador da sessão",
    "lifecycle": "running | cancelling | completed | failed",
    "profile": "perfil",
    "started_at": "timestamp",
    "progress": null,
    "report": null,
    "error": null
  }
}
```

O objeto interno é `MemoryTestSessionSnapshot`; os campos concretos de `progress` e `report` são definidos no runner e devem ser preservados integralmente. O canal `progress` é emitido durante a execução; `state` comunica mudanças de estado; `finished` carrega o snapshot terminal.

## 3. Teste de CPU

Canais:

- `qboa-cpu-test-progress`
- `qboa-cpu-test-state`
- `qboa-cpu-test-finished`

Emissor: `cpu_diagnostic.rs`.

Envelope:

```json
{
  "session_id": "identificador da sessão",
  "session": {
    "session_id": "identificador da sessão",
    "lifecycle": "estado atual",
    "profile": "quick | standard | extended",
    "started_at": "timestamp",
    "elapsed_ms": 0,
    "target_duration_ms": 0,
    "progress_percent": 0,
    "latest_sample": null,
    "iterations": 0,
    "calculation_errors": 0,
    "report": null,
    "error": null
  }
}
```

O tipo concreto interno é `CpuTestSessionSnapshot`. Os campos e estados existentes não podem ser reduzidos durante a extração.

## 4. Teste de GPU

Canais:

- `qboa-gpu-test-progress`
- `qboa-gpu-test-state`
- `qboa-gpu-test-finished`

Emissor: `gpu_diagnostic.rs`.

Envelope:

```json
{
  "session_id": "identificador da sessão",
  "session": {
    "session_id": "identificador da sessão",
    "lifecycle": "estado atual",
    "profile": "quick | standard | extended",
    "started_at": "timestamp",
    "elapsed_ms": 0,
    "target_duration_ms": 0,
    "progress_percent": 0,
    "latest_sample": null,
    "dispatches": 0,
    "validation_errors": 0,
    "report": null,
    "error": null
  }
}
```

O tipo concreto interno é `GpuTestSessionSnapshot`.

## 5. Compatibilidade do frontend

Listeners localizados em `src/main.js`:

- `listen("qboa-event", ...)`
- `listen("qboa-memory-test-progress", ...)`
- `listen("qboa-memory-test-state", ...)`
- `listen("qboa-memory-test-finished", ...)`
- `listen("qboa-cpu-test-progress", ...)`
- `listen("qboa-cpu-test-state", ...)`
- `listen("qboa-cpu-test-finished", ...)`
- `listen("qboa-gpu-test-progress", ...)`
- `listen("qboa-gpu-test-state", ...)`
- `listen("qboa-gpu-test-finished", ...)`

Não há evento de progresso Tauri identificado para o diagnóstico geral nem para o diagnóstico de armazenamento nesta baseline; esses comandos retornam/persistem o resultado por suas APIs atuais.

## 6. Regras para migração

1. Preservar os dez nomes de canais.
2. Preservar os nove valores de evento de tarefa e os três ciclos progress/state/finished por hardware.
3. Preservar os envelopes e os nomes dos campos.
4. Preservar correlação por `execution_id` para operações e por `session_id` para testes de hardware.
5. Não centralizar todos os eventos em um novo envelope durante a extração.
6. Não trocar o ciclo de sessão por evento de tarefa: são contratos diferentes.
