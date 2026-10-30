# Diagnóstico de CPU v0.1

## Objetivo

Executar uma carga computacional controlada e registrar o comportamento observado do processador. Este módulo é independente da limpeza e não produz recomendações de manutenção.

## Perfis

| Perfil | Duração | Uso |
|---|---:|---|
| Rápido | 5 minutos | Triagem inicial |
| Padrão | 15 minutos | Diagnóstico básico sob carga |
| Estendido | 30 minutos | Observação térmica prolongada |

O perfil padrão é de 15 minutos. O usuário inicia e cancela o teste explicitamente.

## Pré-requisitos de telemetria

A temperatura é lida de sensores de CPU publicados por uma instância em execução de LibreHardwareMonitor ou Open Hardware Monitor.

A ordem de descoberta é:

1. API REST local em `http://127.0.0.1:8085/data.json`, com identificação do hardware por `HardwareId` e leitura térmica por `RawValue`. No LibreHardwareMonitor, habilite `Options > Remote Web Server > Run` e configure o IP de escuta como `127.0.0.1`. Não exponha o servidor de sensores em interfaces de rede.
2. WMI em `root\\OpenHardwareMonitor` ou `root\\LibreHardwareMonitor`, como alternativa para instalações que exponham esses namespaces

O QBoa não inventa temperatura a partir de zonas ACPI genéricas. Se não houver sensor de CPU identificado e fornecendo uma leitura válida, o teste acompanhado de carga não começa.

A identificação do processador usa `Win32_Processor`. A utilização total usa `Win32_PerfFormattedData_PerfOS_Processor.PercentProcessorTime`, e a frequência instantânea reportada é consultada em `Win32_PerfFormattedData_Counters_ProcessorInformation.ProcessorFrequency`.

## Carga e segurança

- Um trabalhador de cálculo por thread lógico, limitado a 64 trabalhadores.
- Prioridade de thread abaixo do normal no Windows.
- Rotinas inteiras com duas rotas aritméticas independentes, comparadas para registrar divergências.
- Contagem de verificações concluídas e divergências.
- Coleta de telemetria em intervalos de aproximadamente um segundo.
- Cancelamento pelo usuário.
- Interrupção se a temperatura atingir 85 °C, se a leitura térmica ficar ausente por três amostras ou se a coleta parar de responder.

O limite de 85 °C é uma proteção conservadora definida pelo QBoa. Não representa o TjMax nem a especificação térmica oficial do fabricante.

Os trabalhadores são interrompidos e aguardados mesmo quando ocorre uma falha na coleta ou na persistência do laudo.

## Dados registrados

Cada amostra contém:

- Tempo decorrido em milissegundos e timestamp UTC.
- Temperatura em °C e nome do sensor selecionado.
- Utilização total em porcentagem.
- Frequência reportada em MHz.

O laudo agrega temperatura mínima, média e máxima; utilização média e máxima; pico de frequência; quantidade de verificações; divergências; duração; perfil e motivo de encerramento.

## Persistência

Os laudos são salvos em JSON em:

`<AppData do QBoa>/diagnostics/cpu-reports/`

A gravação usa arquivo temporário, sincronização e substituição atômica. Enquanto o teste está ativo, checkpoints são gravados a cada 15 segundos. Um laudo encontrado com status `running` na inicialização seguinte é apresentado como parcial, preservando as amostras já gravadas.

## Isolamento

O teste de CPU não inicia enquanto houver limpeza/manutenção, teste de RAM ou benchmark de armazenamento. Essas operações também são impedidas de começar enquanto o teste de CPU estiver ativo. O diagnóstico não dispara limpeza nem recomenda alterações no sistema.

## Limitações

Este é um teste sintético de triagem, não um benchmark comparativo nem certificação de estabilidade universal. Um resultado concluído significa apenas que não foram observadas divergências nesta carga, neste hardware e durante o intervalo executado. Não substitui testes especializados, diferentes cargas de instrução ou validação prolongada.
