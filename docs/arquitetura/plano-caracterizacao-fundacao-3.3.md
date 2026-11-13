# Plano de caracterização — fundação 3.3

**Base:** `v3.3-dev`  
**Baseline estática:** `docs/arquitetura/f0-baseline-3.3-dev/README.md`  
**Status:** plano preparado; cenários abaixo ainda precisam ser ligados a testes executáveis e validados.

## Objetivo

Criar uma rede de segurança antes de mover responsabilidades para fora de `main.rs`. Os testes devem observar comportamento público e invariantes, não a organização interna atual. Assim, os testes continuam úteis depois da extração.

## Matriz obrigatória

| ID | Contrato a preservar | Caracterização exigida |
|---|---|---|
| C01 | Superfície Tauri | Conferir os 45 handlers registrados; nomes e parâmetros devem continuar compatíveis com os invokes do frontend. |
| C02 | Aceite assíncrono | `run_task` aceita o início e retorna identidade da execução sem aguardar a tarefa inteira; distinguir aceite de conclusão. |
| C03 | Identidade | `task_id` identifica a tarefa; `execution_id` identifica uma ocorrência. Eventos e cancelamentos devem permanecer correlacionados. |
| C04 | Preparação de limpeza | Token é de uso único, expira, pertence ao `task_id` preparado e não pode autorizar outra tarefa. |
| C05 | Ponto de restauração | Falha exige decisão explícita; recusa não inicia a tarefa; autorização deve ser vinculada à execução correta. |
| C06 | ResetBase | Autorização explícita e escopo irreversível não podem ser ampliados por refatoração; cobrir caminho autorizado e negado. |
| C07 | Cancelamento | Cancelar a execução correta, rejeitar identificador inexistente/ambíguo e não classificar cancelamento como sucesso. |
| C08 | Exclusão mútua | RAM, CPU, GPU, armazenamento e limpeza/manutenção não podem entrar em combinações incompatíveis, inclusive em chamadas concorrentes. |
| C09 | Process Runtime | Preservar captura de stdout/stderr, saída incremental, PID, código/resultado de término, cancelamento e encerramento de árvore quando aplicável. |
| C10 | Eventos de operação | Preservar canal `qboa-event`, nomes, envelope, ordem relevante e identificadores. |
| C11 | Eventos de hardware | Preservar canais progress/state/finished e snapshots de RAM, CPU e GPU, correlacionados por `session_id`. |
| C12 | Persistência de segurança | Journal, snapshots e histórico de alterações sobrevivem ao reinício; manter compatibilidade com dados legados. |
| C13 | Persistência de laudos | Laudos gerais, armazenamento, CPU, GPU e memória permanecem recuperáveis e separados do journal de alterações. |
| C14 | Diagnósticos independentes | Diagnósticos não devem herdar preparação, confirmações ou ciclo de limpeza/manutenção. |
| C15 | Serviços | Gerenciamento individual e aplicação de perfis permanecem APIs distintas; preservar preview, aplicação, restauração e undo. |
| C16 | Inicialização | Splash, janela principal, foco e DevTools continuam funcionando; DevTools não deve ficar disponível em release. |

## Como transformar a matriz em testes

1. **Testes puros de contrato:** estados/transições, validação de tokens, classificação de resultado, serialização de envelopes e regras de progresso.
2. **Testes de integração com doubles:** substituir processos, filesystem, relógio e emissão de eventos por adaptadores controláveis. Não depender de Windows físico para testar regra de negócio.
3. **Testes de compatibilidade:** snapshots JSON e nomes de eventos/canais; fixtures de journal e laudos antigos.
4. **Testes de concorrência:** barreiras/sincronização determinística para disparar inícios simultâneos; evitar testes baseados em `sleep`.
5. **Testes Windows:** reservar CI Windows para comportamento de plataforma, PowerShell, encerramento de processos, Registro e build real.
6. **Validação manual:** manter apenas cenários que dependem de interação com Windows real ou risco de sistema; registrar build, ambiente, resultado e evidência.

## Critérios de aceite

- Cada contrato tem ao menos um teste automatizado determinístico ou uma justificativa explícita para validação exclusivamente Windows/manual.
- Testes não dependem de ordem global nem compartilham arquivos temporários fixos.
- Testes de persistência usam diretórios temporários isolados e fixtures versionadas.
- Nenhum teste executa limpeza destrutiva, DISM real, alteração real do Registro ou benchmark no disco físico.
- O conjunto passa em `cargo fmt --check` e `cargo test --locked --all-targets` no workflow Windows.
- Antes de cada extração, os testes relevantes passam; depois da extração, passam novamente sem enfraquecer asserts.

## Ordem de execução recomendada

### F0 — fechar caracterização

1. Mapear cada handler com assinatura e consumidores em `src/main.js`.
2. Localizar e congelar fixtures de persistência e payloads JSON.
3. Cobrir contratos puros ainda sem teste.
4. Adicionar testes de concorrência determinísticos para o gate e sessões.
5. Validar o fluxo de processos por adaptador/double e manter testes Windows para as particularidades do SO.
6. Rodar a suíte completa no CI e só então marcar F0 como liberada.

### F1 — extração de funções puras

Após F0 liberada, extrair apenas funções sem `AppHandle`, estado global, filesystem, Registro ou processo. Cada extração deve:
- mover uma responsabilidade pequena;
- manter assinatura/comportamento público;
- preservar os testes existentes;
- adicionar teste unitário no novo módulo quando fizer sentido;
- passar fmt, testes e build antes da próxima extração.

### Etapas seguintes (não executar ainda)

- **F2 — Estado da aplicação:** organizar propriedade e ciclo de vida de estado sem mudar semântica de concorrência.
- **F3 — Process Runtime:** encapsular criação, captura, cancelamento e término de processos.
- **F4 — Aplicação/orquestração:** separar resolução de tarefa, políticas, adapters e dispatcher.
- **F5 — Comandos Tauri:** agrupar comandos por domínio, mantendo os 45 contratos públicos.
- **F6 — Composição:** reduzir `main.rs` a composição do app, inicialização e registro de comandos.

## Regras de migração

- Não renomear comandos/eventos nem mudar argumentos/retornos no mesmo commit de extração.
- Não unificar ciclo de limpeza com ciclo de diagnóstico.
- Não mover todo o estado para um novo singleton ou mega-gerenciador.
- Não introduzir trait/abstração sem pelo menos dois consumidores reais ou um limite de plataforma concreto.
- Não criar uma estrutura de pastas inteira antecipadamente; criar módulos conforme uma responsabilidade é extraída e testada.
- Código legível é desejável, mas funcionamento, segurança e desempenho vêm primeiro.
