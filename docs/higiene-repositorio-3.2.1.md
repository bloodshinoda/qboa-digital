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

A listagem remota conferida em 2026-11-04 contém somente duas branches:

| Branch | Situação | Orientação |
|---|---|---|
| `main` | Linha principal | Única linha ativa de integração e CI. |
| `feat/matriz-servicos-coletor` | Pesquisa isolada | Preservada deliberadamente; não integrar em bloco nem reabrir o PR #9 sem revisão específica. |

As demais branches antigas foram excluídas do remoto após a revisão de 3.2.1. A documentação e os recursos já incorporados permanecem no histórico de `main`; a exclusão das referências não remove commits do histórico.

### Branch preservada: `feat/matriz-servicos-coletor`

A comparação com `main` em 2026-11-04 encontrou quatro commits exclusivos nessa branch, com quatro arquivos adicionados:

- `scripts/service-baselines/Collect-ServiceBaseline.ps1`: coleta metadados de serviços Windows.
- `scripts/service-baselines/README.md`: procedimento, matriz de ambientes e critérios de proveniência.
- `scripts/service-baselines/merge_service_baselines.py`: consolidação dos dados coletados.
- `scripts/service-baselines/test_merge_service_baselines.py`: testes do consolidador.

A branch está 4 commits à frente e 304 atrás de `main`, portanto divergiu consideravelmente da linha atual. Ela deve ser tratada como fonte de pesquisa seletiva, não como candidata a merge direto. O PR #9 permanece fechado.


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
- As branches antigas classificadas como incorporadas, substituídas ou abandonadas foram removidas do remoto; somente `main` e `feat/matriz-servicos-coletor` permanecem.
- Não foi executada uma nova validação manual do aplicativo no Windows nesta etapa.
- O CI deve validar os commits desta revisão antes de considerar a higiene concluída.
