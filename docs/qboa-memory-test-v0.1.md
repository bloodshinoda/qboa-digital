# QBoa Memory Test — contrato técnico v0.1

## Objetivo

Adicionar um teste de integridade da memória RAM executado durante a sessão atual do Windows, sem reinicialização, sem driver e sem acesso à memória física ou a processos de terceiros. O resultado é uma evidência de teste, não um certificado de que toda a RAM física está perfeita.

## Limites de segurança

- Suporte inicial: Windows 10/11 x64.
- Não exigir elevação de administrador.
- Trabalhar somente com buffers alocados pelo próprio processo.
- Não exigir `VirtualLock`; não tentar impedir o sistema de paginar memória.
- Orçamento cumulativo configurável até o total de RAM física detectada no perfil personalizado, com teto operacional de segurança de 1 TiB por sessão. Esse orçamento representa bytes processados ao longo dos blocos, não alocação simultânea.
- Alocação dinâmica em blocos; tamanho padrão de 16 MiB, adaptável entre 4 e 256 MiB conforme a disponibilidade.
- Considerar a memória física e disponível continuamente; interromper novas alocações e reduzir/encerrar o teste diante de pressão de memória.
- Uma única sessão e um único worker ativos por vez.
- Cancelamento cooperativo; sempre liberar os buffers ao encerrar.
- Não implementar Rowhammer, escrita fora dos buffers, leitura/escrita de endereços físicos ou acesso a outros processos.

A reserva mínima de memória do sistema e o cálculo final do limite adaptativo precisam ser validados em testes antes de serem considerados política definitiva. O teto operacional de 1 TiB é um limite de segurança, não uma meta obrigatória. O perfil personalizado usa como máximo o total de RAM física que o Windows reporta; a alocação simultânea segue limitada pela disponibilidade e pela reserva de segurança.

## Algoritmos

- **A01 — padrões sólidos:** preencher e verificar com `0x00` e `0xFF`.
- **A02 — walking bits:** testar bits individuais em padrões de 32/64 bits, incluindo complemento.
- **A03 — tabuleiro de xadrez:** padrões alternados `0xAA` e `0x55`.
- **A04 — PRNG determinístico:** padrão pseudoaleatório reproduzível; registrar algoritmo/versão e seed.
- **A05 — inversões móveis:** gravar padrão, verificar inversões em ordem direta e reversa.
- **A06 — modulo-X:** desabilitado até revisão e validação independente.

Cada rotina deve verificar limites de índice e nunca ler/escrever fora do buffer próprio. A04 deve produzir exatamente a mesma sequência para a mesma versão e seed.

## Perfis

Os perfis devem descrever cobertura explícita, e não apenas uma duração estimada.

- **Rápido:** A01 e A03; amostragem de A04. Indicar claramente que a cobertura é parcial.
- **Padrão:** A01–A05 em blocos selecionados de acordo com o orçamento adaptativo.
- **Prolongado:** A01–A05 com passes adicionais e seeds diferentes no A04.
- **Personalizado:** um passe de A01–A05 sobre o orçamento definido pelo usuário, limitado ao total de RAM física reportada.
- **A06:** não faz parte de nenhum perfil enquanto não for validado.

Os critérios exatos de seleção de blocos, passes e conclusão precisam ser implementados como configuração explícita e cobertos por testes; a interface não deve prometer cobertura total da RAM física.

## Resultado e erros

Estados finais possíveis:

- `passed`: todas as etapas planejadas foram concluídas e nenhuma divergência foi encontrada.
- `errors_detected`: uma ou mais divergências foram encontradas.
- `cancelled`: o usuário cancelou a sessão.
- `inconclusive`: não foi possível concluir a cobertura planejada, por exemplo por pressão de memória ou interrupção externa.
- `failed`: falha operacional impediu a execução confiável.

Nunca marcar como aprovado um teste cancelado, incompleto ou que detectou divergências. Erros encontrados antes de uma interrupção permanecem no laudo. Uma divergência é uma anomalia observada e, isoladamente, não prova que o módulo físico de RAM esteja defeituoso.

Manter contador agregado total de divergências e no máximo 100 registros detalhados por sessão. Cada registro detalhado deve conter algoritmo, versão/parâmetros relevantes, passe, bloco, offset dentro do buffer virtual, valor esperado, valor observado, seed quando aplicável e timestamp. O offset não é endereço físico e deve ser rotulado como tal.

## Laudo persistente

O laudo é independente do diário de alterações de `safety.rs` e deve sobreviver ao fechamento inesperado do aplicativo. Formato inicial: JSON versionado, gravação atômica e recuperação de relatório parcial quando viável.

Campos mínimos:
- versão do esquema e do engine;
- início, fim e duração;
- estado final e motivo de parada;
- perfil, algoritmos e parâmetros/seeds;
- memória solicitada e efetivamente testada;
- etapas concluídas e cobertura declarada;
- total agregado e amostra limitada de divergências;
- versão/build do Windows;
- avisos e limitações aplicáveis.

Não armazenar conteúdo bruto da RAM nem inventar endereços físicos.

## Arquitetura prevista

Separar engine Rust da integração Tauri e da coleta de inventário. Módulos sugeridos: configuração/perfis, alocação adaptativa, padrões/algoritmos, runner, segurança/cancelamento, erros/progresso e persistência do laudo. A camada Tauri deve expor iniciar, consultar sessão/progresso e cancelar, com eventos próprios de progresso. Não executar um teste longo dentro do comando síncrono de diagnóstico existente.

A integração visual só deve ocorrer depois que os testes unitários do núcleo passarem. A implementação não deve alterar o diagnóstico de armazenamento existente.

## Critérios mínimos de aceitação

- Testes unitários para padrões A01–A05 e limites dos buffers.
- Reprodutibilidade do A04 por versão e seed.
- Detecção de divergência simulada sem escrita fora dos limites.
- Teto rígido de 8 GiB e alocação adaptativa.
- Cancelamento libera buffers e não reporta aprovação.
- Pressão de memória interrompe ou reduz a alocação de forma segura.
- Limite de 100 detalhes sem perder o contador agregado.
- Laudos persistidos com perfil, parâmetros, cobertura e motivo de parada.
- UI responsiva, progresso por eventos e compatibilidade Windows 10/11 x64.
