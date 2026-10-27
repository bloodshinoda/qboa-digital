# Contrato do laudo de diagnóstico — QBoa Digital 3.2

**Status:** proposta inicial para revisão  
**Baseline auditado:** `main` em `8fd31bbbebf3facd211c30024d68d922f8c89f21` (QBoa Digital 3.1.2)  
**Escopo:** persistência local e ciclo de vida dos relatórios de diagnóstico. Este documento não autoriza mudanças na limpeza, nos perfis de serviços ou no executor de registro.

## 1. Objetivos e princípios

1. Um laudo concluído deve continuar disponível depois de fechar e reabrir o QBoa.
2. Um laudo parcial deve sobreviver a falhas de coletores e, quando possível, a cancelamento solicitado pelo usuário.
3. Ausência de dado não significa resultado saudável nem falha de hardware.
4. Coleta, interpretação e apresentação são camadas distintas. O laudo armazena evidências e estados; a interface não inventa recomendações.
5. O formato deve ser versionado e permitir futuras migrações sem invalidar relatórios antigos.
6. O relatório é local. Não enviar dados à rede nem incluir identificadores de hardware sensíveis sem decisão explícita futura.
7. A persistência do laudo é separada do journal de alterações reversíveis mantido pelo módulo de segurança.

## 2. Envelope do relatório

Cada arquivo de laudo representa uma execução de diagnóstico.

| Campo | Tipo | Obrigatório | Regra |
|---|---|---:|---|
| `schema_version` | inteiro | sim | Começa em `1`; descreve o formato persistido, não a versão do aplicativo. |
| `report_id` | string | sim | Identificador único, estável e seguro para nome de arquivo. |
| `app_version` | string | sim | Versão do QBoa que produziu o laudo. |
| `mode` | enum | sim | `quick` ou `complete`. |
| `execution_status` | enum | sim | `running`, `completed`, `partial`, `cancelled` ou `failed`. `running` é transitório e não deve ser apresentado como laudo final. |
| `started_at` | string RFC 3339 | sim | Data/hora com fuso explícito; armazenar UTC. |
| `finished_at` | string RFC 3339 ou null | sim | Nulo enquanto a execução não terminar. |
| `duration_ms` | inteiro ou null | sim | Duração monotônica, não negativa; nula se não disponível. |
| `system` | objeto | sim | Identificação não sensível do sistema no momento da coleta. |
| `results` | array | sim | Resultados por categoria, inclusive estados indisponíveis. |
| `checks` | array | sim | Verificações adicionais, como SFC e DISM. Array vazio é válido para modo rápido. |
| `issues` | array | sim | Falhas, avisos e interrupções que afetaram a execução. Array vazio quando não houver. |

### 2.1 Identificação do sistema

Objeto `system`:

- `os_name`: string ou null.
- `os_version`: string ou null.
- `os_build`: string ou null.
- `os_edition`: string ou null.
- `architecture`: string ou null, se coletada de forma confiável.

Não armazenar serial de placa-mãe, número de série de disco, nome de usuário, caminho do perfil, endereço MAC ou outros identificadores persistentes nesta versão do contrato. Não criar uma impressão digital do equipamento.

### 2.2 Resultados por categoria

Cada item de `results` mantém o formato existente sempre que possível:

- `category`: enum atual: `sistema`, `armazenamento`, `memoria_ram`, `placa_de_video`, `processador`, `temperatura`.
- `status`: `ok`, `atencao`, `falha`, `indisponivel` ou `informativo`.
- `evidence`: array de strings com fatos observados, valores e explicações de indisponibilidade.
- `source`: array de strings com as fontes efetivamente usadas.

Os estados são mutuamente distintos. `informativo` significa que houve coleta sem uma avaliação de saúde; não equivale a `ok`. `indisponivel` significa que não foi possível obter evidência suficiente. Uma leitura ausente não deve ser convertida em zero.

### 2.3 Verificações adicionais

Cada item de `checks` contém:

- `name`: nome estável e legível da verificação.
- `status`: enum de estado acima.
- `evidence`: resumo do resultado ou da razão de indisponibilidade.
- `source`: executável/provedor usado.
- `exit_code`: inteiro ou null, quando houver processo externo.
- `duration_ms`: inteiro ou null.
- `started_at` e `finished_at`: strings RFC 3339 ou null, se o executor puder fornecê-las.

Campos adicionais de execução são opcionais para compatibilidade com os resultados existentes; campos já conhecidos não devem ser descartados ao salvar.

### 2.4 Problemas da execução

Cada item de `issues` contém:

- `severity`: `info`, `warning` ou `error`.
- `component`: identificador estável do coletor/verificação.
- `code`: código legível por máquina, estável e sem texto localizado.
- `message`: explicação legível em português.
- `occurred_at`: timestamp RFC 3339 ou null.

Não gravar stack traces, comandos com dados pessoais ou conteúdo arbitrário de ambiente. Mensagens devem ser limitadas e sanitizadas.

## 3. Semântica do estado global

- `completed`: todas as etapas esperadas para o modo solicitado terminaram; indisponibilidade de um sensor opcional, por si só, não impede este estado se registrada explicitamente.
- `partial`: a execução terminou, mas uma ou mais etapas obrigatórias falharam ou não puderam terminar; os resultados válidos permanecem disponíveis.
- `cancelled`: o usuário solicitou cancelamento e a execução encerrou de forma controlada. Resultados obtidos até então devem ser preservados.
- `failed`: não foi possível produzir um relatório utilizável, por exemplo falha fatal antes de qualquer resultado.
- `running`: estado transitório opcional para recuperação após encerramento inesperado. Na abertura seguinte, uma execução persistida como `running` deve ser marcada como interrompida/partial, com um item em `issues`; nunca apresentada como concluída.

Uma falha de uma consulta opcional não deve invalidar todas as outras coletas. Falhas individuais devem aparecer no resultado correspondente ou em `issues`.

## 4. Persistência local

Diretório lógico proposto:

`<app_data_dir>/diagnostics/reports/<report_id>.json`

Usar o diretório de dados do aplicativo resolvido pelo Tauri, nunca um caminho fixo com nome de usuário. Não gravar dentro da pasta de instalação nem depender de permissões administrativas para armazenar o laudo.

Regras de escrita:

1. Serializar o envelope completo em JSON UTF-8.
2. Escrever primeiro em arquivo temporário no mesmo diretório.
3. Fazer flush/sync quando suportado e substituir o destino de forma atômica.
4. Nunca sobrescrever outro laudo com o mesmo identificador.
5. Se a gravação falhar, mostrar erro explícito; não informar que o laudo foi salvo.
6. Se um arquivo estiver corrompido, preservar o arquivo original, ignorá-lo no carregamento e registrar aviso visível/logável. Não apagar automaticamente dados.
7. Ordenar o histórico por `started_at` decrescente. Uma falha ao ler um laudo não deve impedir carregar os demais.
8. A lista/histórico pode ser reconstruída a partir dos arquivos; não exigir um índice separado como única fonte da verdade.

A política de retenção (limite por quantidade ou idade) fica deliberadamente pendente. Até a decisão, não excluir relatórios automaticamente.

## 5. Privacidade e segurança

- Armazenamento local, sem sincronização ou telemetria.
- Não incluir dados pessoais nem identificadores de hardware sensíveis.
- Validar o identificador antes de usá-lo em caminhos; rejeitar separadores, `..` e nomes inválidos.
- Limitar o tamanho máximo aceito por arquivo e o tamanho de strings/arrays ao carregar.
- Tratar JSON inválido, versão futura desconhecida e campos inesperados sem travar a inicialização.
- Não executar comandos ou caminhos derivados de campos do laudo. O arquivo é dado, nunca instrução.
- Não misturar o relatório diagnóstico com snapshots/journal de alterações do Windows.

## 6. Compatibilidade e migração

- `schema_version` é obrigatório em arquivos persistidos.
- Versão atual inicial: `1`.
- Campos opcionais ausentes devem ser tratados como desconhecidos, nunca preenchidos com valores que impliquem sucesso.
- Versão menor/igual à suportada: validar e carregar; preservar os dados reconhecidos.
- Versão superior à suportada: não sobrescrever nem migrar automaticamente; mostrar que o arquivo foi criado por uma versão mais nova.
- Migrações futuras devem ser explícitas, testadas e não destrutivas.
- O formato interno atual `DiagnosticReport` (`mode`, `results`, `checks`) pode continuar sendo usado durante a coleta, mas a persistência deve usar um envelope separado para não acoplar o contrato histórico à implementação do coletor.

## 7. Exemplo ilustrativo

O exemplo abaixo ilustra a estrutura; valores e resultados são fictícios e não constituem diagnóstico real.

```json
{
  "schema_version": 1,
  "report_id": "20261027T143000Z-example",
  "app_version": "3.2.0",
  "mode": "quick",
  "execution_status": "completed",
  "started_at": "2026-10-27T14:30:00Z",
  "finished_at": "2026-10-27T14:30:02Z",
  "duration_ms": 2000,
  "system": {
    "os_name": "Microsoft Windows",
    "os_version": "10.0",
    "os_build": "26100",
    "os_edition": "IoTEnterpriseS",
    "architecture": "x86_64"
  },
  "results": [
    {
      "category": "sistema",
      "status": "informativo",
      "evidence": ["Sistema identificado"],
      "source": ["Win32_OperatingSystem e registro do Windows"]
    },
    {
      "category": "temperatura",
      "status": "indisponivel",
      "evidence": ["Leitura de temperatura confiável não disponível."],
      "source": ["Provedor não disponível."]
    }
  ],
  "checks": [],
  "issues": []
}
```

O identificador no exemplo é ilustrativo. A implementação deve gerar IDs únicos sem depender do relógio como única garantia de unicidade.

## 8. Critérios de aceite do contrato

- [ ] Uma execução rápida pode ser serializada e recuperada sem perder categoria, status, evidência ou origem.
- [ ] Uma execução completa mantém cada verificação e sua indisponibilidade/erro.
- [ ] Execução parcial e cancelada são representáveis sem fingir conclusão.
- [ ] Fechar e reabrir o app não apaga o histórico.
- [ ] Um arquivo corrompido não impede abrir os outros laudos.
- [ ] Uma versão de esquema desconhecida não é sobrescrita.
- [ ] O contrato não armazena recomendação automática, segredo ou identificador sensível do equipamento.
- [ ] O relatório não substitui nem modifica o journal de alterações.

## 9. Decisões ainda abertas antes da implementação

1. Retenção: manter todos os laudos ou adotar limite configurável.
2. Experiência de interface: mostrar o último laudo na tela de diagnóstico e abrir um histórico separado.
3. Identificador: selecionar implementação que garanta unicidade e seja compatível com as dependências existentes.
4. Recuperação de execução interrompida: registrar um envelope `running` em cada etapa ou salvar apenas checkpoints controlados. A escolha deve equilibrar robustez e escrita em disco.

Estas decisões não bloqueiam a revisão dos campos centrais, mas devem ser fechadas antes de finalizar a persistência.
