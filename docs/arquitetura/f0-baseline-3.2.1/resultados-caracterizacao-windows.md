# Resultados de caracterização F0 — Windows

**Baseline de código:** `22712114e70cab86b292f10944185f49df775ee5` (branch `docs/f0-baseline-3.2.1`).  
**Execução automatizada:** GitHub Actions, runner `windows-latest`, workflow `Windows build`.  
**Run:** [41873086854](https://github.com/bloodshinoda/qboa-digital/actions/runs/41873086854).  
**Data:** 2026-11-04.  
**Escopo:** validação automatizada no runner Windows e bateria de caracterização manual executada pelo usuário no Windows físico, com scripts PowerShell e verificações práticas orientadas durante a validação da F0.

## Resultado automatizado

**Resultado: PASSOU**

| Etapa | Resultado |
|---|---|
| Instalação de dependências npm | PASSOU |
| Autoteste do coletor de armazenamento em arquivo temporário | PASSOU — oito medições executadas e validadas |
| `cargo fmt --check` | PASSOU |
| `cargo test --locked --all-targets` | PASSOU |
| Build do aplicativo Windows / instalador NSIS | PASSOU |
| Empacotamento do executável portátil | PASSOU |

O autoteste de armazenamento usou um arquivo temporário descartável; não foi um benchmark destrutivo do disco físico. O job `prepare-draft-release` foi ignorado porque a execução não foi disparada por uma tag.

## Caracterização manual no Windows físico

**Resultado geral: PASSOU**, conforme confirmação do usuário após executar a bateria de scripts PowerShell e verificações práticas fornecidas durante a validação.

Cenários exercitados e aprovados:

- **Ciclo de tarefa:** início assíncrono, retorno do comando, identificação da execução por `execution_id` e recebimento dos eventos correspondentes.
- **Confirmações correlacionadas:** fluxo de confirmação de ponto de restauração e confirmação específica de DISM `/ResetBase`, sem prosseguir com a operação irreversível quando não autorizada.
- **Cancelamento:** cancelamento associado à execução correta, encerramento/liberação do estado e tratamento de identificador ambíguo.
- **Persistência:** gravação e recuperação de laudos/checkpoints após reabrir o aplicativo, incluindo verificação da separação entre journal de alterações e armazenamento de laudos.
- **Armazenamento:** compatibilidade do formato JSON e persistência dos resultados do diagnóstico.
- **Exclusividade:** bloqueios entre testes de RAM, CPU, GPU, armazenamento e tarefas incompatíveis.
- **Serviços:** manutenção da separação entre API de gerenciamento individual de serviços e API de aplicação/gerenciamento de perfis.
- **Limpeza pesada / DISM:** alerta de operação irreversível, fluxo de ponto de restauração, limpeza de navegadores, etapas `/ScanHealth` e `/RestoreHealth`, confirmação explícita e execução do rebase conforme autorização do usuário.

Os resultados dos cenários manuais são registrados com base na confirmação explícita do usuário. Os scripts foram executados no Windows físico; os logs brutos não foram anexados à PR, portanto este documento não os apresenta como evidência independente coletada pelo CI.

## Matriz dos 12 pontos de caracterização

| # | Contrato | Situação |
|---:|---|---|
| 1 | Inventário dos 42 comandos no `generate_handler!` | **PASSOU** — conferência estática do registro e correspondência com a baseline |
| 2 | Envelope e nomes de `qboa-event` | **PASSOU** — contrato conferido e comportamento validado |
| 3 | Eventos de tarefa e eventos de sessão RAM/CPU/GPU | **PASSOU** — canais, ciclos e payloads conferidos/validados |
| 4 | `run_task` retorna antes da conclusão e fornece `execution_id` | **PASSOU** — ciclo assíncrono validado |
| 5 | Confirmação de ponto de restauração correlacionada por `execution_id` | **PASSOU** — confirmação validada |
| 6 | Confirmação de DISM `/ResetBase` correlacionada por `execution_id` | **PASSOU** — alerta, decisão e bloqueio/prosseguimento conforme confirmação validados |
| 7 | Cancelamento por `execution_id` e rejeição de execução ambígua | **PASSOU** — cancelamento e tratamento de identificação validados |
| 8 | Journal/snapshots separados das pastas de laudos | **PASSOU** — persistência e separação verificadas |
| 9 | Diagnóstico geral persiste estado inicial, checkpoints e estado terminal | **PASSOU** — persistência validada após reabertura |
| 10 | Compatibilidade JSON e persistência do diagnóstico de armazenamento | **PASSOU** — formato e persistência validados |
| 11 | Exclusão mútua RAM/CPU/GPU/armazenamento/tarefas | **PASSOU** — bloqueios entre operações incompatíveis validados |
| 12 | API de serviço individual separada da API de perfis | **PASSOU** — separação das APIs conferida e validada |

## Conclusão

A CI confirma que a baseline compila no Windows, passa no formatador e na suíte Rust existente, executa o autoteste das oito medições de armazenamento e gera os artefatos Windows. A bateria manual no Windows físico, executada com os scripts PowerShell e verificações práticas, foi confirmada pelo usuário como aprovada em todos os cenários da matriz.

**A caracterização F0 está concluída e liberada para a etapa seguinte da Fórmula 1 (3.3).**

Nenhuma alteração funcional foi feita nesta validação. A tag `v3.2.1` permanece intocada.
