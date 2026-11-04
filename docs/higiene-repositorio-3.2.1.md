# Higiene do repositório — linha 3.2.1

**Referência:** QBoa Digital 3.2.1  
**Data da revisão:** 2026-11-04  
**Branch de trabalho:** `main`

Este documento registra a organização do repositório após a estabilização da linha 3.2.1. Não altera o escopo funcional do aplicativo.

## CI e automação

`.github/workflows/windows-build.yml` é o workflow canônico para a linha principal. Ele executa em pushes para `main`, pull requests para `main`, execução manual e tags `v*`.

O fluxo canônico cobre:

- instalação reproduzível das dependências JavaScript com `npm ci`;
- toolchain Rust estável para Windows MSVC;
- verificação de formatação Rust;
- testes Rust com `--locked --all-targets`, respeitando `Cargo.lock`;
- self-test do coletor de armazenamento, executando as oito medições em arquivo temporário descartável;
- build Windows;
- geração dos artefatos instalador NSIS e portátil como executáveis individuais;
- criação de release em rascunho quando uma tag `v*` é publicada.

Os workflows legados `windows-ci.yml`, `qboa-windows-artifacts.yml` e `qboa-memory-test.yml` foram removidos da branch principal porque repetiam verificações/builds ou apontavam para branches antigas de funcionalidades já incorporadas. O histórico dos arquivos permanece disponível no Git.

**Atenção:** o workflow de release continua reagindo a tags `v*`. Antes de mover ou recriar uma tag existente, confirme que a automação não criará um rascunho duplicado. A divergência histórica da tag `v3.2.1` foi conscientemente deixada sem correção nesta revisão, pois a próxima linha de versão será 3.3.

## Branches

A listagem abaixo reflete as branches encontradas durante a revisão. A classificação não significa que as referências remotas já tenham sido apagadas: a integração disponível nesta sessão permite consultar branches, mas não oferece operação de exclusão de refs.

| Branch | Classificação | Destino |
|---|---|---|
| `main` | Ativa | Única linha de integração e CI. |
| `docs/contrato-laudo-32` | Referência | Preservar temporariamente como histórico do contrato de laudos. O documento também está em `main`. |
| `feat/catalogo-identificacao-servicos` | Pesquisa preservada | Manter até avaliar dependências e eventual reaproveitamento seletivo. |
| `feat/inventario-capacidades-windows` | Pesquisa preservada | Manter até avaliar dependências e eventual reaproveitamento seletivo. |
| `feat/auditoria-assinaturas-servicos` | Adiada/legada | PR #8 fechado; preservar somente até confirmar que não há commits exclusivos úteis. |
| `feat/matriz-servicos-coletor` | Adiada/legada | PR #9 fechado; coleta de baseline fica como pesquisa futura. |
| `feat/cleanup-tiered-profiles` | Incorporada/legada | Funcionalidade integrada à linha principal; candidata à exclusão. |
| `feat/native-rust-cleanup` | Incorporada/legada | Motor nativo integrado à linha principal; candidata à exclusão. |
| `feat/qboa-memory-test` | Incorporada/legada | Diagnóstico de memória integrado; candidata à exclusão. |
| `feat/gpu-stability-diagnostic` | Incorporada/legada | Diagnóstico de GPU integrado; candidata à exclusão. |
| `feat/complete-diagnostic-flow` | Incorporada/legada | Fluxo completo integrado; candidata à exclusão. |
| `feat/3.2.1-cleanup-progress-summary` | Incorporada/legada | Progresso e resumo integrados; candidata à exclusão. |
| `fix/cleanmgr-timeout` | Substituída | Motor nativo substituiu a dependência da rotina de limpeza via Cleanmgr; candidata à exclusão. |
| `release/3.1.2` | Abandonada | Linha não utilizada; candidata à exclusão. |

Os PRs #8 e #9 foram fechados sem merge e suas branches foram preservadas para consulta. Não reabrir nem mesclar essas linhas integralmente sem nova revisão de dependências e compatibilidade com a arquitetura atual.

## Dependências e licenças

- `package-lock.json` e `src-tauri/Cargo.lock` são mantidos no repositório.
- A instalação do frontend no CI usa `npm ci`.
- Os testes Rust no CI usam `--locked` para impedir atualização implícita do lockfile.
- `THIRD_PARTY_NOTICES.md` registra a dependência LibreHardwareMonitorLib 0.9.6 e sua licença MPL-2.0.

Esta revisão não substitui uma auditoria completa das licenças transitivas de todas as dependências. Essa verificação deve ser feita antes de introduzir novos componentes distribuídos com o aplicativo.

## Documentação de referência

- [README](../README.md): visão geral, arquitetura, operações e desenvolvimento.
- [CHANGELOG](../CHANGELOG.md): histórico das versões.
- [Contrato do laudo 3.2](contrato-laudo-diagnostico-3.2.md): formato e persistência dos diagnósticos.
- [Matriz P0 Windows](p0-windows-validation.md): roteiro de validação manual em VM descartável.
- [Identificação de serviços Windows](identificacao-servicos-windows.md): critérios e evidências de identificação.

## Limites desta revisão

- Não houve alteração funcional nos motores de limpeza, serviços ou diagnóstico.
- Não foram apagadas branches remotas; a exclusão precisa ser feita na interface do GitHub ou por uma operação Git com permissão de escrita em refs.
- Não foi executada uma nova validação manual do aplicativo no Windows nesta etapa.
- O CI deve validar os commits desta revisão antes de considerar a higiene concluída.
