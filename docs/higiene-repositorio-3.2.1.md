# Higiene do repositório — auditoria F1

**Data da revisão:** 2026-11-09  
**Referência da linha estável:** QBoa Digital 3.2.2  
**Referência da linha de trabalho:** `formula-1/f1-01-cleanup-progress`  
**Commit F1 auditado:** `82ccf54d9c34e63e9385872156271033d2383d80`  
**Base observada:** `main` em `714a87092e3a808fcb6211da56bad8554282f386`

Este documento registra uma fotografia do repositório e os resultados da auditoria estática. Não substitui a validação funcional no Windows nem autoriza refatorações fora do escopo aprovado.

## Estado de branches

A listagem remota de 2026-11-09 contém nove branches:

| Branch | Situação observada | Orientação |
|---|---|---|
| `main` | Linha estável, protegida | Preservar como referência pública |
| `formula-1/f1-01-cleanup-progress` | Linha ativa F1; 87 commits à frente de main no snapshot | Preservar; PR #28 permanece aberto |
| `feat/matriz-servicos-coletor` | Pesquisa isolada; quatro commits exclusivos e grande divergência | Preservar como fonte de pesquisa; não fazer merge direto |
| `feat/windows-update-chuck` | Implementação anterior do Windows Update | PR #29 encerrado como substituído pela F1; branch preservada |
| `feat/3.2.2-ui-resize-taskbar-progress` | Trabalho de interface anterior | PR #26 encerrado como substituído pelo PR #27, já integrado; branch preservada |
| `ajustezinhos/3.2.2-progresso-e-frases-qboa` | Trabalho incorporado ao PR #27 | Histórico preservado |
| `docs/f0-baseline-3.2.1` | Documentação incorporada à linha principal | Histórico preservado |
| `docs/readme-modos-limpeza` | Documentação incorporada à linha principal | Histórico preservado |
| `fix/f1-eventos-logs-atividade-ptbr` | Trabalho incorporado ao PR #25 | Histórico preservado |

Branches antigas não foram removidas nesta revisão: o histórico e os commits exclusivos foram preservados enquanto se confirma a ausência de conteúdo ainda útil. A exclusão de referências remotas deve ser uma operação separada, depois da conferência final dos PRs e commits.

## Pull requests

| PR | Estado após a revisão | Decisão |
|---|---|---|
| #28 — contratos comuns F1 | Aberto, draft | Manter em revisão; não integrar até fechar as lacunas de contrato e preparação |
| #29 — Windows Update | Fechado, não mergeado | Substituído pela implementação mais recente validada na F1 |
| #26 — resize/taskbar | Fechado, não mergeado | Substituído pelo PR #27, já integrado |
| #27 — progresso, resize e mensagens 3.2.2 | Integrado | Mantido |
| #23, #24 e #25 | Integrados | Mantidos |
| #9 — matriz de serviços | Fechado, não mergeado | Pesquisa preservada na branch correspondente |

## CI e distribuição

`.github/workflows/windows-build.yml` é o workflow canônico observado. A execução F1 de 2026-11-09, run [42695994662](https://github.com/bloodshinoda/qboa-digital/actions/runs/42695994662), concluiu com sucesso o job Windows: verificação de sintaxe JavaScript, self-test do coletor de armazenamento, rustfmt, testes Rust, instalador e pacote portátil.

A etapa `prepare-draft-release` foi ignorada, conforme esperado para uma execução de branch e não de tag.

O fluxo usa `npm ci`, `Cargo.lock` e testes Rust com `--locked --all-targets`. Os artefatos Windows são instalador NSIS e executável portátil individual.

## Achados técnicos da auditoria F1

### Prioridade alta

- O frontend oferece o estado “Pronto para iniciar”, mas o executor ainda solicita confirmação de falha do ponto de restauração durante a execução.
- A confirmação do ResetBase também permanece dentro do executor. Isso não corresponde integralmente ao contrato de confirmações antecipadas.
- Tipos de eventos e confirmações em `operation_contracts.rs` ainda não governam todos os caminhos reais.
- O resultado estruturado da operação é criado, mas parte do fluxo ainda descarta esse retorno; métricas finais dependem de mensagens/texto.

### Prioridade média

- Duas funções em `windows_update.rs` que aparentam ser testes não possuem atributo `#[test]`; portanto, não são executadas por `cargo test`.
- Metadados de pacote ainda identificam a build F1 como 3.2.2. Antes de distribuir builds experimentais, adotar identificador interno sem criar versão pública.
- O workflow não valida explicitamente a correspondência entre a versão da tag e a versão dos metadados do pacote.
- A CI não executa Clippy nem testes comportamentais de frontend.
- O build do monitor térmico depende de toolchain .NET externa e baixa o instalador PawnIO durante o build, com verificação SHA-256.

### Robustez

- Existem chamadas `unwrap()` em caminhos de execução e acesso a locks compartilhados. Revisar por contexto; não substituir mecanicamente.
- `main.rs` e `main.js` permanecem grandes. A extração modular deve ser planejada como etapa arquitetural própria, com testes de caracterização.

## Segurança e dependências

- O workflow observado usa permissões restritas para o fluxo normal.
- O reparo do Windows Update exige elevação e restringe a alteração a políticas locais conhecidas.
- O instalador PawnIO tem hash SHA-256 verificado antes do uso.
- `Cargo.lock` e `package-lock.json` estão versionados.
- `THIRD_PARTY_NOTICES.md` registra LibreHardwareMonitorLib 0.9.6 e MPL-2.0.

A revisão foi estática e não constitui auditoria exaustiva de licenças transitivas, cadeia de suprimentos ou regras de proteção do GitHub.

## Próximas ações

1. Antecipar para a preparação as decisões sobre ponto de restauração e ResetBase, preservando o fluxo fire-and-forget.
2. Conectar eventos, confirmações, resultados e métricas estruturados aos caminhos reais.
3. Corrigir os testes Windows Update sem `#[test]`.
4. Definir identificação interna de build experimental e validar tags contra metadados.
5. Atualizar a fotografia deste inventário após a próxima rodada de integração.
6. Só então revisar a remoção das branches históricas já incorporadas.

## Limites desta revisão

- Nenhuma branch foi apagada e nenhum histórico foi reescrito.
- PRs #26 e #29 foram encerrados como substituídos, sem merge.
- PR #28 permanece aberto e sem merge.
- Nenhuma release pública ou tag foi criada, movida ou removida.
- A build F1 passou no CI e o mantenedor confirmou a validação funcional desta rodada.
