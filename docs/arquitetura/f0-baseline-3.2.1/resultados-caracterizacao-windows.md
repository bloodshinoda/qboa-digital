# Resultados de caracterização F0 — Windows

**Baseline de código:** `22712114e70cab86b292f10944185f49df775ee5` (branch `docs/f0-baseline-3.2.1`).  
**Execução:** GitHub Actions, runner `windows-latest`, workflow `Windows build`.  
**Run:** [41873086854](https://github.com/bloodshinoda/qboa-digital/actions/runs/41873086854).  
**Data:** 2026-11-04.  
**Escopo:** validação automatizada disponível no runner Windows. Não equivale a smoke test interativo no computador físico do usuário.

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

## Matriz dos 12 pontos de caracterização

| # | Contrato | Situação |
|---:|---|---|
| 1 | Inventário dos 42 comandos no `generate_handler!` | **Inspeção estática documentada**; ainda sem teste automatizado de igualdade |
| 2 | Envelope e nomes de `qboa-event` | **Inspeção estática documentada**; ainda sem teste de runtime |
| 3 | Eventos de tarefa e eventos de sessão RAM/CPU/GPU | **Inspeção estática documentada**; ainda sem teste de runtime |
| 4 | `run_task` retorna antes da conclusão e fornece `execution_id` | **PENDENTE** — exige teste de integração |
| 5 | Confirmação de ponto de restauração correlacionada por `execution_id` | **PENDENTE** — exige exercício do fluxo |
| 6 | Confirmação de DISM `/ResetBase` correlacionada por `execution_id` | **PENDENTE** — exige exercício controlado do fluxo |
| 7 | Cancelamento por `execution_id` e rejeição de execução ambígua | **PENDENTE** — exige execução e cancelamento reais |
| 8 | Journal/snapshots separados das pastas de laudos | **Inspeção estática documentada**; persistência real ainda não exercitada |
| 9 | Diagnóstico geral persiste estado inicial, checkpoints e estado terminal | **PENDENTE** — exige execução e inspeção dos arquivos gerados |
| 10 | Compatibilidade JSON e persistência do diagnóstico de armazenamento | **Parcial** — autoteste do coletor passou; round-trip do relatório persistido ainda não foi caracterizado |
| 11 | Exclusão mútua RAM/CPU/GPU/armazenamento/tarefas | **PENDENTE** — exige testes concorrentes em runtime |
| 12 | API de serviço individual separada da API de perfis | **Inspeção estática documentada**; ainda sem teste de comportamento |

## Conclusão

A CI confirma que a baseline compila no Windows, passa no formatador e na suíte Rust existente, executa o autoteste das oito medições de armazenamento e gera os artefatos Windows.

**A F0 ainda não está liberada para F1.** Os contratos dinâmicos dos itens 4–7, 9, 11 e parte dos itens 8, 10 e 12 precisam de caracterização em runtime. Os itens estáticos 1–3 e 12 também devem ganhar verificações automatizadas quando viável.

Nenhuma alteração funcional foi feita nesta validação. A tag `v3.2.1` permanece intocada.
