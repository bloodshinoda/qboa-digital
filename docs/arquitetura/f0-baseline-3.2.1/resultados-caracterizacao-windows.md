# Resultados de caracterização F0 — Windows

**Baseline de código:** `22712114e70cab86b292f10944185f49df775ee5` (branch `docs/f0-baseline-3.2.1`).  
**Execução automatizada:** GitHub Actions, runner `windows-latest`, workflow `Windows build`.  
**Run:** [41873086854](https://github.com/bloodshinoda/qboa-digital/actions/runs/41873086854).  
**Data:** 2026-11-04.  
**Escopo:** validação automatizada no runner Windows e caracterização manual reportada pelo usuário no Windows físico. Os resultados manuais são registrados conforme observação do usuário; não houve coleta independente dos logs nesta documentação.

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

**Limpeza pesada / DISM — PASSOU conforme relato do usuário**

- O fluxo apresentou alerta de operação irreversível.
- O usuário observou a criação do ponto de restauração e a limpeza de navegadores sem problemas.
- O DISM executou `/Online /Cleanup-Image /ScanHealth` e, em seguida, `/Online /Cleanup-Image /RestoreHealth`.
- Ao chegar à etapa de rebase, o QBoa apresentou um alerta específico; o usuário confirmou e informou que o rebase foi executado corretamente.
- A linha de comando capturada durante a execução confirmou `/ScanHealth`; a execução de `/ResetBase` foi relatada pelo usuário após a confirmação, mas o trecho correspondente do log DISM ainda não foi anexado à evidência.

**Persistência de laudos — PASSOU conforme relato do usuário.** O usuário confirmou que o teste de persistência realizado no QBoa funcionou corretamente. Os detalhes de quais arquivos/checkpoints foram inspecionados não foram anexados.

**Cancelamento de tarefas — PASSOU conforme relato do usuário.** O usuário confirmou que o teste de cancelamento funcionou corretamente. O cenário específico e a evidência de rejeição de identificadores ambíguos ainda não foram registrados.

**Exclusividade de operações — PASSOU conforme relato do usuário.** O usuário confirmou que o teste de exclusividade funcionou corretamente. A matriz exata de pares de operações exercitados não foi anexada.

## Matriz dos 12 pontos de caracterização

| # | Contrato | Situação |
|---:|---|---|
| 1 | Inventário dos 42 comandos no `generate_handler!` | **Inspeção estática documentada**; ainda sem teste automatizado de igualdade |
| 2 | Envelope e nomes de `qboa-event` | **Inspeção estática documentada**; ainda sem teste de runtime |
| 3 | Eventos de tarefa e eventos de sessão RAM/CPU/GPU | **Inspeção estática documentada**; ainda sem teste de runtime |
| 4 | `run_task` retorna antes da conclusão e fornece `execution_id` | **PENDENTE** — exige teste de integração específico |
| 5 | Confirmação de ponto de restauração correlacionada por `execution_id` | **Parcial** — fluxo de ponto de restauração observado; correlação interna ainda não verificada |
| 6 | Confirmação de DISM `/ResetBase` correlacionada por `execution_id` | **Parcial** — alerta e execução confirmada pelo usuário; correlação interna e log completo ainda não verificados |
| 7 | Cancelamento por `execution_id` e rejeição de execução ambígua | **Parcial** — cancelamento funcional confirmado pelo usuário; rejeição de identificador ambíguo ainda não verificada |
| 8 | Journal/snapshots separados das pastas de laudos | **Parcial** — persistência de laudos confirmada; separação física entre journal e laudos ainda precisa de inspeção |
| 9 | Diagnóstico geral persiste estado inicial, checkpoints e estado terminal | **Parcial** — persistência de laudos confirmada; cobertura de todos os estados/checkpoints ainda não detalhada |
| 10 | Compatibilidade JSON e persistência do diagnóstico de armazenamento | **Parcial** — autoteste do coletor passou e persistência de laudos foi confirmada; round-trip específico do relatório de armazenamento ainda não foi caracterizado |
| 11 | Exclusão mútua RAM/CPU/GPU/armazenamento/tarefas | **Parcial** — exclusividade funcional confirmada pelo usuário; matriz completa de combinações ainda não registrada |
| 12 | API de serviço individual separada da API de perfis | **Inspeção estática documentada**; ainda sem teste de comportamento |

## Conclusão

A CI confirma que a baseline compila no Windows, passa no formatador e na suíte Rust existente, executa o autoteste das oito medições de armazenamento e gera os artefatos Windows. Os testes manuais já confirmaram o fluxo principal da limpeza pesada com confirmação do rebase, persistência de laudos, cancelamento e exclusividade, conforme relato do usuário.

**A F0 ainda não está liberada para F1.** Restam caracterizações específicas de `run_task`/eventos, correlação das confirmações, rejeição de identificador ambíguo, separação física entre journal e laudos, round-trip do relatório de armazenamento, cobertura da matriz de exclusividade e comportamento das APIs de serviço. Os itens estáticos 1–3 e 12 também devem ganhar verificações automatizadas quando viável.

Nenhuma alteração funcional foi feita nesta validação. A tag `v3.2.1` permanece intocada.
