# F0 — Baseline arquitetural da 3.2.1

**Status:** inventário documental inicial concluído; nenhuma alteração funcional autorizada nesta etapa.  
**Versão observada:** QBoa Digital 3.2.1.  
**Branch de documentação:** `docs/f0-baseline-3.2.1`.  
**Base:** branch `main`, commit `37eefc99280601bf709852546bc8ec8c7fb1e1ac` (referência confirmada antes da criação desta branch).  
**Arquivo principal:** `src-tauri/src/main.rs`, 2.557 linhas no momento da auditoria.  
**Cargo:** pacote `qboa-digital` 3.2.1, Rust 2021, Tauri 2; binário declarado explicitamente em `src/main.rs`.

## Objetivo

Registrar o comportamento observável e os contratos existentes antes de qualquer extração de `main.rs`. Esta documentação é a referência de comparação para a migração arquitetural da linha 3.3, preservando compatibilidade com a 3.2.1.

A F0 documenta:

1. contratos Rust serializados e formatos persistidos;
2. comandos Tauri registrados e seus parâmetros/retornos;
3. eventos Tauri emitidos e respectivos envelopes;
4. dependências entre módulos, estado compartilhado e fluxos críticos;
5. invariantes que a migração precisa preservar.

## Escopo e limites

- Esta etapa é documental. Não altera Rust, JavaScript, JSON de perfis, configuração Tauri, dependências ou formatos de persistência.
- Não move funções nem módulos.
- Não muda IDs de tarefas, nomes de comandos, nomes de eventos ou parâmetros.
- Não modifica a tag `v3.2.1`.
- Não incorpora a branch `feat/matriz-servicos-coletor`.
- Diagnósticos permanecem separados de limpeza e manutenção.
- O documento registra o que o código faz hoje, não o que a arquitetura futura deveria fazer.

## Documentos desta baseline

- [Contratos e persistência](./contratos-e-persistencia.md)
- [Comandos Tauri](./comandos-tauri.md)
- [Eventos Tauri](./eventos-tauri.md)
- [Mapa de dependências e ordem de migração](./dependencias-e-migracao.md)

## Fontes de código auditadas

Os hashes abaixo são os blobs consultados na branch `main` durante a auditoria. Eles permitem identificar exatamente os arquivos usados como evidência, independentemente de futuras mudanças.

| Arquivo | Blob SHA |
|---|---|
| `src-tauri/src/main.rs` | `7783483c48e056feafcb96630a43853818d9e2c6` |
| `src-tauri/Cargo.toml` | `246d1753d105e7f653053d6451e79f8f298fddda` |
| `src-tauri/tauri.conf.json` | `71678d5b17f7a6df262d2ce7279a741f1e8125b1` |
| `src/main.js` | `8d31f8454a0f8a66bdfa08c684293fbc58e19430` |
| `src-tauri/src/task_registry.rs` | `9da402ac9869a662bb8b438b75a451f01543ffd0` |
| `src-tauri/src/safety.rs` | `b041357440d617dc3ba4a1133f9479deb029cdf3` |
| `src-tauri/src/registry_executor.rs` | `c3d0043274462358a028176de6475be54827017f` |
| `src-tauri/src/diagnostics.rs` | `db1a81e89ee95232a20e106a5a18a4878bc2d41d` |
| `src-tauri/src/diagnostics_store.rs` | `568197634e016494fcc649ff5448c99940d1d0ed` |
| `src-tauri/src/complete_diagnostic.rs` | `397f7c288f7d7230fef58e06b23a69fbf0e38f27` |
| `src-tauri/src/storage_diagnostic.rs` | `24005ccf5c6056a5f89484efe6507d0923391496` |
| `src-tauri/src/memory_test/session.rs` | `a845729c22e48046e724bfcb2790cc7219b16eff` |
| `src-tauri/src/memory_test/runner.rs` | `247c8fe6efbce4eb73b20c441fd21df57482fce5` |
| `src-tauri/src/cpu_diagnostic.rs` | `ecdc5082bfdeb192959760978474c171cc5bb42f` |
| `src-tauri/src/gpu_diagnostic.rs` | `48c1aedaeb2ecc03000f9a5944186fff1ab24f5d` |
| `src-tauri/src/capability_inventory.rs` | `2a07ca6faf31598cef4aac29f9146750092f638a` |
| `src-tauri/src/memory_inventory.rs` | `908a78b4a63f846aaddf458e89a75c66daba4138` |
| `src-tauri/src/service_manager.rs` | `7a95f1896c53c14e50574541d3afcf656ae3ce0b` |
| `src-tauri/src/service_profiles.rs` | `601b8d3962b47aa1f7766c536be85e12c4f3e4e8` |
| `src-tauri/src/tweak_profiles.rs` | `c6cd17f26d990ad5b640855fcdb8a8f8ebd546e9` |

## Critério de conclusão da F0

A F0 será considerada documentalmente concluída quando:

- os comandos listados coincidirem com o `generate_handler!` do baseline;
- os eventos listados coincidirem com os emissores Rust e listeners do frontend;
- os contratos de retorno e os formatos persistidos estiverem identificados;
- as dependências e os estados compartilhados estiverem registrados;
- as lacunas de teste de caracterização estiverem explícitas;
- nenhuma alteração funcional tiver sido introduzida.

**Importante:** esta documentação não afirma que testes ou CI foram executados nesta etapa. O inventário foi feito por inspeção estática dos arquivos identificados acima. A execução dos testes de baseline será registrada separadamente antes da primeira extração de código.
