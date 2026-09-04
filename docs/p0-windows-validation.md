# Matriz de validacao P0 no Windows

Este documento prepara a validacao manual do P0 em uma VM Windows descartavel. Ele nao executa alteracoes automaticamente e nao substitui um snapshot externo da VM.

## Regras da VM

- Usar Windows 10 ou 11 atualizado, com checkpoint externo da VM antes de cada grupo de testes.
- Usar uma conta administrativa separada e registrar o nome da VM, build do Windows e arquitetura.
- Confirmar que a Protecao do Sistema esta habilitada antes do teste de Restore Point.
- Registrar os estados antes e depois em arquivos de evidencia, sem incluir credenciais ou dados pessoais.
- Nao usar a maquina de trabalho principal.
- Nao testar com dados de producao, politicas corporativas ou tarefas agendadas de terceiros.
- Interromper o grupo se um criterio de restauracao falhar; nao prosseguir para o proximo teste.

## Limite dos testes automatizados atuais

`cargo test` cobre apenas memoria e estruturas de controle:

- snapshots fake de Registro para valor existente, inexistente e troca de tipo;
- estados fake de servicos;
- estados fake de tarefas agendadas;
- rollback sem snapshot e rollback repetido;
- allowlist de estrategias de rollback;
- IDs independentes e cancelamento logico de duas execucoes;
- resolucao de tarefas e ordem dos presets.

Os testes nao chamam Registro, Service Control Manager, Task Scheduler, PowerShell, DISM, SFC, System Restore ou `taskkill`. Em Linux, as implementacoes Windows sao excluidas por `cfg(target_os = "windows")`. Portanto, toda a matriz abaixo exige Windows real, exceto onde indicado.

## Matriz

| ID | Teste | Pre-condicoes | Acao controlada | Estado esperado antes | Estado esperado depois | Aprovacao objetiva | Risco |
|---|---|---|---|---|---|---|---|
| W-REG-01 | Registro: valor existente | VM em checkpoint; criar uma chave de teste dedicada; valor `REG_DWORD` conhecido | Capturar snapshot; alterar o valor pela tarefa; executar rollback | Hive, caminho, nome, existencia, tipo e valor registrados | Valor existe com o mesmo tipo e valor original | Comparacao estruturada de todos os campos do snapshot, nao apenas texto | Medio: altera Registro, limitado a chave dedicada |
| W-REG-02 | Registro: valor inexistente | Chave de teste sem o `value_name` | Capturar snapshot; criar/alterar o valor; executar rollback | `exists=false`, sem tipo e sem valor | O valor nao existe; a chave nao precisa ser removida | `GetValueNames()` nao contem o nome e o snapshot pos-rollback informa `exists=false` | Medio |
| W-REG-03 | Registro: tipo preservado | Dois casos dedicados: `REG_DWORD` e `REG_EXPAND_SZ` ou `REG_MULTI_SZ` | Capturar; substituir por outro tipo; executar rollback | Tipo original conhecido | Tipo e valor retornam exatamente ao original | `GetValueKind()` e valor normalizado coincidem com a evidencia anterior | Medio |
| W-SVC-01 | Servico: Automatic + Running | Escolher servico nao critico e documentar estado; preferir servico de teste instalado na VM | Capturar snapshot; alterar; executar rollback | Startup `Automatic`/`Auto`, estado `Running` | Startup `Automatic`, estado `Running` | `Get-CimInstance Win32_Service` e `Get-Service` coincidem antes/depois | Alto: parar/iniciar servico |
| W-SVC-02 | Servico: Automatic + Stopped | Mesmo procedimento, com servico parado | Capturar; alterar; rollback | `Automatic`, `Stopped` | `Automatic`, `Stopped` | Nenhum start implicito; ambos os campos coincidem | Alto |
| W-SVC-03 | Servico: Manual + Running | Servico de teste em `Manual` e iniciado | Capturar; alterar; rollback | `Manual`, `Running` | `Manual`, `Running` | Startup e estado coincidem, inclusive apos aguardar estabilizacao | Alto |
| W-SVC-04 | Servico: Manual + Stopped | Servico de teste em `Manual` e parado | Capturar; alterar; rollback | `Manual`, `Stopped` | `Manual`, `Stopped` | Nao iniciar o servico durante o rollback | Alto |
| W-SVC-05 | Servico: Disabled + Stopped | Servico de teste desabilitado | Capturar; alterar; rollback | `Disabled`, `Stopped` | `Disabled`, `Stopped` | O rollback nao pode iniciar servico desabilitado | Alto |
| W-SVC-06 | Servico inexistente | Usar um nome que nao exista e confirmar com `Get-Service -ErrorAction SilentlyContinue` | Capturar; executar fluxo; rollback se houver alteracao | `exists=false` | Continua inexistente; nenhuma criacao silenciosa | Consulta final nao encontra o nome | Medio |
| W-TASK-01 | Tarefa habilitada | Usar tarefa agendada de teste dedicada | Capturar; desabilitar; rollback | Tarefa existe e esta habilitada | Mesma tarefa existe e esta habilitada | `Get-ScheduledTask` confirma exatamente `TaskPath`, `TaskName` e estado | Medio |
| W-TASK-02 | Tarefa desabilitada | Tarefa de teste existente e desabilitada | Capturar; alterar; rollback | Existe e esta desabilitada | Continua desabilitada | O rollback nao habilita a tarefa | Medio |
| W-TASK-03 | Tarefa inexistente | Caminho/nome inexistente | Capturar; executar fluxo; rollback | `exists=false` | Continua inexistente | Nenhuma tarefa e criada ou habilitada | Baixo |
| W-PART-01 | Falha parcial + rollback | Snapshot externo; estrategia reversivel com pelo menos duas alteracoes observaveis; etapa de falha controlada | Executar A, B e provocar falha em C; observar journal; aguardar rollback | A e B no estado original; C ainda nao aplicado | A e B restaurados; C sem rollback inventado | Resultado e `failed_and_rolled_back`, ou `failed_rollback_partial/error` com evidencia correspondente | Alto |
| W-RP-01 | Restore Point | Protecao do Sistema habilitada; espaco suficiente; checkpoint externo | Criar ponto com descricao unica; consultar pontos de restauracao | Nenhum ponto com aquela descricao | Ponto identificavel pela descricao e timestamp aproximado | O comando retorna sucesso e `Get-ComputerRestorePoint` encontra o ponto | Alto: altera infraestrutura de recuperacao |
| W-PS-01 | Execucao PowerShell | VM com permissao administrativa e logs habilitados | Executar uma tarefa PowerShell reversivel; guardar stdout/stderr e exit code | Processo inexistente; snapshot capturado antes | Processo termina; evento final corresponde ao exit code; snapshot permanece legivel | Exit code, journal e eventos sao consistentes; nenhuma mensagem de erro fica oculta | Medio/Alto |
| W-DISM-01 | DISM ScanHealth | VM em checkpoint; energia estavel; sem manutencao concorrente | Executar somente `DISM /Online /Cleanup-Image /ScanHealth` | Integridade registrada no log | Processo termina sem alterar configuracoes reversiveis | Exit code e saida sao completos; nenhum processo DISM fica ativo | Medio |
| W-DISM-02 | DISM RestoreHealth | Somente VM descartavel; imagem do sistema preservada | Executar `RestoreHealth` manualmente, fora de uma validacao destrutiva automatica | Estado da imagem e logs registrados | Reparacao concluida ou falha explicita; sem estado `completed` falso | Exit code, log DISM e journal concordam; rollback nao e alegado para a reparacao | Alto |
| W-SFC-01 | SFC | VM em checkpoint; nenhuma atualizacao concorrente | Executar `sfc /scannow` | Integridade e log inicial registrados | SFC termina e seu resultado e preservado | Exit code e texto final registrados; cancelamento nao deixa `sfc.exe` ativo | Alto |
| W-CAN-01 | Cancelamento do processo principal | Iniciar tarefa longa e observar `execution_id` e PID | Solicitar cancelamento uma vez; aguardar evento final | Estado `running`, processo principal ativo | `cancelled` somente depois de processo encerrado | `task-cancelled` so aparece apos `WaitForSingleObject`/wait equivalente e sem PID ativo | Alto |
| W-CAN-02 | Arvore de descendentes | Tarefa que inicia PowerShell e ferramenta filha; monitorar por PID | Cancelar durante a ferramenta filha | PowerShell e descendente ativos | Processo principal e todos os descendentes encerrados | `tasklist`/Process Explorer nao encontra a arvore; nenhum processo continua alterando o sistema | Alto |
| W-CAN-03 | Cancelamento nao confirmado | Instrumentar falha de `taskkill` ou negar encerramento em ambiente de teste | Solicitar cancelamento | Processo ainda ativo | Evento de erro, nao `task-cancelled` | O journal nao afirma cancelamento enquanto houver processo relacionado | Alto |
| W-CON-01 | Execucoes concorrentes do mesmo task_id | Permitir duas chamadas backend separadas e registrar os dois IDs | Iniciar o mesmo `task_id` duas vezes; cancelar somente a primeira | Dois `execution_id` e dois estados independentes | Primeira termina cancelada; segunda continua e termina conforme seu resultado | Eventos, PID, journal e cancelamento da primeira nao aparecem na segunda | Alto |
| W-CON-02 | Cancelamento ambiguo pelo task_id | Duas execucoes ativas do mesmo task_id | Enviar cancelamento sem `execution_id` | Duas correspondencias ativas | Backend rejeita a solicitacao ambigua | Nenhuma das duas execucoes e cancelada por engano | Medio |

## Evidencias obrigatorias

Para cada teste guardar:

- build do Windows, arquitetura e versao do PowerShell;
- `task_id`, `execution_id`, PID principal e timestamp;
- snapshot JSON antes e depois;
- stdout, stderr e exit code separados;
- journal antes, durante e depois;
- resultado das consultas de verificacao;
- resultado do checkpoint externo da VM.

Nao considerar um teste aprovado somente por uma mensagem textual do aplicativo. A aprovacao exige comparacao do estado observado no Windows.

## Riscos encontrados durante a preparacao

- A captura e o rollback Windows ainda nao foram executados neste ambiente Linux.
- O fluxo usa PowerShell e comandos nativos que precisam de validacao sintatica e semantica no Windows.
- `taskkill /T /F` precisa ser observado com Process Explorer ou `tasklist`; exit code sozinho nao prova que toda a arvore terminou.
- DISM RestoreHealth e SFC podem modificar arquivos/componentes do sistema e nao possuem rollback granular.
- Restore Point depende de Protecao do Sistema, espaco disponivel e politicas do Windows.
- O journal atual e em memoria; reiniciar o aplicativo entre captura e rollback invalida a evidencia.
- O frontend atual nao envia `execution_id` no cancelamento; em execucoes ambiguas o backend deve rejeitar a operacao.

## Limitacoes desta etapa

- Nenhum comando destrutivo foi executado.
- Nenhuma nova funcionalidade foi implementada.
- A matriz nao substitui testes automatizados Windows reais.
- Falha parcial de sequencias irreversiveis continua sem rollback granular por design; esse ponto pertence a uma etapa posterior do Task Engine.
- A checagem global de `cargo fmt --check` pode apontar formatacao pre-existente em `src-tauri/src/task_registry.rs`; esse arquivo deve permanecer fora desta etapa.
