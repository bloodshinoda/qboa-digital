# F1 — Contrato do fluxo de execução e confirmações

## Objetivo

Permitir que uma operação de limpeza/manutenção seja preparada e autorizada antes de começar. Depois do início, o fluxo deve ser previsível (*fire-and-forget*): o usuário acompanha o progresso, pode solicitar cancelamento e pode alterar a opção de desligamento ao concluir, sem receber confirmações previsíveis no meio da execução.

Este contrato se aplica somente às tarefas de limpeza/manutenção. Diagnósticos mantêm seus próprios ciclos de vida e controles.

## Sequência esperada

1. **Seleção da tarefa** — identificar tarefa, risco e operações potencialmente irreversíveis.
2. **Confirmação de risco** — explicar escopo e impacto antes de qualquer alteração.
3. **Preparação dos navegadores** — consultar navegadores em execução e obter autorização para fechá-los; registrar a decisão.
4. **Proteção do sistema** — tentar criar o ponto de restauração antes de iniciar a tarefa. Se falhar, apresentar a falha e pedir autorização explícita para prosseguir sem ele. Recusa encerra a preparação sem iniciar a limpeza.
5. **Operações irreversíveis previstas** — quando a tarefa e a avaliação de compatibilidade indicarem que ResetBase poderá ser executado, apresentar a confirmação antes do início. A recusa deve excluir ResetBase da execução ou cancelar a tarefa, conforme a política de segurança definida para o modo; nunca perguntar durante a faxina.
6. **Resumo de preparação** — mostrar as decisões tomadas e pedir o último aceite para iniciar.
7. **Execução** — iniciar uma única vez. Não solicitar novamente decisões já tomadas na preparação.
8. **Finalização** — emitir um único resultado terminal: concluída, parcial, cancelada ou falhou.

## Implementação da preparação

A F1 prepara operações de limpeza e manutenção antes de liberar o botão **Iniciar**:

- `prepare_cleanup` resolve a tarefa e tenta criar o ponto de restauração antes da execução.
- A preparação retorna um token de uso único, vinculado ao `task_id`, e informa se o ponto foi criado ou se houve falha.
- O token expira após 15 minutos; tokens expirados são descartados e o armazenamento mantém no máximo 32 preparações pendentes.
- Em caso de falha, a interface solicita autorização explícita ainda na preparação. Recusar descarta o token e encerra sem iniciar alterações.
- `run_task` consome o token, valida a tarefa e exige autorização explícita quando o ponto não pôde ser criado. Não existe segunda tentativa nem confirmação tardia no executor.
- A decisão sobre `/ResetBase` também fica vinculada à mesma preparação, evitando reclassificar o ambiente entre a confirmação e o início.
- Diagnósticos permanecem fora desse fluxo e preservam seu ciclo de vida independente.

Testes de regressão cobrem falha sem autorização, prosseguimento autorizado, reutilização do ponto criado, vínculo do token à tarefa, consumo único do token e separação entre manutenção e diagnóstico.

## Popup de execução

O popup central de progresso deve concentrar:

- tarefa e etapa atual;
- progresso agregado;
- opção **Desligar ao concluir**, alterável durante a execução;
- ação **Parar**, que solicita cancelamento real ao executor.

O estado de desligamento deve ser consultado no momento da finalização, não capturado somente no início. O desligamento só pode ser agendado após conclusão bem-sucedida ou parcial explicitamente considerada concluída pela política; nunca após cancelamento ou falha. O usuário deve conseguir desmarcar a opção antes do agendamento.

O cancelamento deve ter estado visível (Solicitando parada…) e impedir cliques repetidos. O popup não deve desaparecer enquanto o executor ainda estiver encerrando processos ou concluindo rollback.

## Contratos de eventos e Registro de Atividades

- Cada etapa tem identidade estável e emite início/resultado uma única vez.
- Confirmações pertencem à preparação; eventos de confirmação durante execução são considerados regressão.
- Métricas são agregadas a partir de resultados estruturados por etapa. Não somar linhas de métricas repetidas nem inferir contagens de texto livre.
- Saídas de progresso mutável (por exemplo, percentual do DISM) substituem a linha anterior; mensagens de estado e resultado permanecem no histórico.
- Cancelamento solicitado pelo usuário é estado terminal cancelled, não falha do executor.
- Falhas parciais de itens ocupados devem ser distinguidas de falhas de etapa.
- O resumo final deve corresponder às métricas efetivamente persistidas pelo executor.

## Progresso global das operações

O percentual exibido representa a operação inteira, nunca o percentual isolado da etapa atual.

- O progresso começa em 0% e só chega a 100% quando o executor conclui a operação.
- Cada etapa recebe um peso; a troca de etapa consolida o peso da etapa anterior. A limpeza leve é representada como uma etapa única (1/1), pois o motor atual não fornece percentuais internos confiáveis para suas sub-rotinas.
- DISM RestoreHealth, DISM ResetBase e SFC usam o percentual real da ferramenta somente dentro do peso reservado àquela etapa.
- Etapas sem percentual próprio avançam por conclusão: o peso é consolidado quando a próxima etapa começa.
- O percentual global é monotônico: nenhuma atualização posterior pode fazê-lo regredir.
- Cancelamento e falha preservam o último percentual alcançado; não simulam conclusão.
- Diagnósticos mantêm seus ciclos de progresso independentes.

Pesos atuais:

| Operação | Etapa | Peso |
| --- | --- | ---: |
| Limpeza média | Cada uma das 4 etapas | 25% |
| Limpeza pesada | Etapas 1 a 5 | 6% cada |
| Limpeza pesada | DISM RestoreHealth | 35% |
| Limpeza pesada | DISM ResetBase | 15% |
| Limpeza pesada | SFC | 20% |

A soma dos pesos de cada operação é 100%. Se ResetBase for omitido por compatibilidade, sua etapa é concluída imediatamente e o peso é consolidado na transição para SFC.

## Invariantes de segurança

- Nenhuma operação destrutiva ou irreversível começa antes da autorização correspondente.
- Falha ao criar ponto de restauração nunca é convertida silenciosamente em autorização.
- Configurações e políticas administradas por domínio/WSUS não devem ser sobrescritas por este fluxo.
- Nenhum desligamento é agendado para execução cancelada ou falha.
- Diagnósticos permanecem independentes da execução de limpeza.

## Plano de implementação

1. Introduzir preparação explícita e transportar as decisões para o executor.
2. Mover os controles de desligamento e cancelamento para o popup; permitir atualizar a opção de desligamento em tempo real.
3. Normalizar eventos/métricas para evitar duplicações no Registro de Atividades e nos resumos.
4. Adicionar testes unitários e de regressão para cada transição e invariantes.
5. Validar no Windows: sucesso, falha do ponto de restauração, recusa de ResetBase, cancelamento e desligamento ativado/desativado.

Este documento define o contrato; a implementação deve seguir em commits pequenos, com CI Windows verde antes de solicitar nova validação de campo.
