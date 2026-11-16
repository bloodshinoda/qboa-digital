# Contrato — Inventário Técnico Completo 3.3

**Branch:** `v3.3-dev`  
**Etapa:** definição de escopo e contrato, antes da implementação  
**Natureza:** coleta informativa, somente leitura

## 1. Objetivo

O Inventário Técnico Completo é a primeira ferramenta profissional planejada para o Pro 3.3. Deve produzir uma fotografia organizada e verificável do equipamento, do Windows e dos componentes instalados, útil para suporte técnico, manutenção, comparação entre coletas e composição de relatórios.

O inventário descreve o que foi identificado. Ele **não executa testes de estabilidade**, não avalia saúde por meio de carga, não altera configurações e não realiza limpeza ou reparos.

## 2. Limites e separação de responsabilidades

- **Inventário:** identifica e registra componentes, configurações e softwares encontrados.
- **Diagnóstico:** executa verificações, medições ou testes e registra resultados e evidências.
- **Limpeza/manutenção:** executa ações que alteram o sistema, sempre sob os contratos próprios desse domínio.

O inventário pode reutilizar coletores já existentes, mas não deve transformar testes de CPU, RAM, GPU ou armazenamento em etapas obrigatórias da coleta. A coleta básica precisa ser informativa, previsível e relativamente rápida.

A primeira implementação deve permanecer independente do licenciamento. A restrição comercial Pro será aplicada pela camada de produto/licença quando essa etapa for implementada; não espalhar verificações de licença pelos coletores.

## 3. Escopo funcional

A coleta será organizada nas doze categorias abaixo. Cada categoria pode conter itens identificados, itens não encontrados e itens cuja consulta não foi possível.

### 3.1 Equipamento e sistema operacional
- Fabricante, modelo e tipo de equipamento (desktop, notebook ou máquina virtual, quando houver evidência).
- Nome do dispositivo, quando disponível.
- Nome/edição do Windows, versão, build, arquitetura e idioma.
- Data/hora da coleta e versão do QBoa responsável pela coleta.

### 3.2 Placa-mãe e firmware
- Fabricante, modelo/produto e versão da placa-mãe.
- Fabricante, versão e data do BIOS/UEFI.
- Modo de firmware, quando identificável.
- TPM e Secure Boot como capacidades/estados reportados pelo sistema, sem inferir conformidade.

### 3.3 Processador
- Nome/modelo, fabricante e identificador reportado.
- Arquitetura, sockets, núcleos físicos e processadores lógicos.
- Frequências reportadas (base/máxima), quando disponíveis.
- Recursos de virtualização reportados pelo processador/firmware.

### 3.4 Memória RAM
- Capacidade física total e disponível no momento da coleta.
- Quantidade de slots reportados e módulos enumerados.
- Capacidade, fabricante, part number, tipo, formato e velocidades reportadas por módulo.
- Largura de dados/total e tensão configurada, quando expostas.
- Timings, XMP/EXPO e demais parâmetros somente quando uma fonte confiável os disponibilizar; nunca estimar ou inventar valores.

### 3.5 GPU
- Todos os adaptadores gráficos identificados.
- Fabricante, nome/modelo, tipo integrado/dedicado quando houver evidência, versão do driver e memória reportada.
- Adaptador ativo/principal, se a API permitir identificar sem ambiguidade.
- Recursos/API gráfica identificáveis sem executar benchmark.

### 3.6 Armazenamento e volumes
- Discos físicos, modelo, fabricante, tipo/barramento e capacidade.
- Partições e volumes, sistema de arquivos, capacidade, espaço livre e letra/ponto de montagem.
- Identificação de SSD/HDD/NVMe somente quando a fonte permitir.
- SMART/saúde não será presumido a partir de modelo ou capacidade; medições de desempenho continuam no diagnóstico de armazenamento.

### 3.7 Monitores
- Monitores enumerados, fabricante/modelo identificados via EDID quando disponível.
- Resolução atual, resolução nativa reportada, frequência e profundidade de cor quando acessíveis.
- Distinguir monitor conectado de registro histórico/desconectado quando a API permitir.

### 3.8 Rede
- Adaptadores físicos e virtuais, fabricante/modelo e estado.
- Tipo de conexão, velocidade de link reportada e driver.
- Endereços IPv4/IPv6, máscara/prefixo, gateway e DNS configurados.
- DHCP e métricas de interface quando disponíveis.
- Não realizar ping, teste de rota, DNS ativo ou medição de throughput: isso pertence ao Diagnóstico de Rede.

### 3.9 Áudio e periféricos
- Controladores/dispositivos de áudio e endpoints enumerados.
- Teclados, mouses, USB e periféricos relevantes identificáveis.
- Bluetooth, câmera, biometria, touchscreen e caneta, quando presentes.
- Diferenciar dispositivo presente, desabilitado, registrado mas ausente e não detectado.

### 3.10 Bateria e energia
- Presença de bateria, capacidade reportada, nível/carga e estado.
- Fonte de alimentação/estado AC, quando disponível.
- Plano de energia ativo e configurações relevantes que possam ser consultadas sem alteração.
- Em desktop ou ambiente sem bateria, registrar como não aplicável, não como falha.

### 3.11 Softwares instalados
- Aplicativos encontrados nas fontes padrão de registro de desinstalação do Windows.
- Nome, versão, fabricante, arquitetura, escopo (usuário/máquina) e data de instalação, quando disponíveis.
- Não executar programas para descobrir versão nem varrer diretórios indiscriminadamente.
- Distinguir ausência de informação de ausência do software.

### 3.12 Drivers
- Dispositivos PnP e respectivos drivers associados.
- Nome do dispositivo, classe, fabricante/provedor, versão e data do driver.
- Estado/problema reportado pelo Plug and Play.
- Destacar dispositivos com erro ou driver ausente sem tentar corrigir.

## 4. Contrato de evidência

Cada dado coletado deve preservar, sempre que aplicável:

- **Valor:** valor tipado, sem depender de texto formatado para cálculos.
- **Rótulo:** nome amigável para apresentação.
- **Origem:** API, classe WMI/CIM, propriedade, chave de registro ou outra fonte concreta.
- **Coletado em:** instante UTC da coleta.
- **Confiança:** `confirmed`, `identified`, `estimated` ou `unavailable`.
- **Estado:** `available`, `not_detected`, `not_applicable`, `unavailable` ou `error`.
- **Observação:** limitação ou contexto necessário para interpretar o valor.

Regras:
- Não converter `unavailable` em zero, falso ou string vazia.
- Não tratar `not_detected` como prova de ausência física.
- Não apresentar estimativas como valores confirmados.
- Se fontes divergirem, preservar a origem e registrar a divergência; não escolher silenciosamente um valor.
- Falha de uma categoria não deve invalidar as demais. A coleta deve terminar como concluída, parcial ou falha, com erros por categoria.

## 5. Estratégia de coleta

A implementação será nativa em Rust e APIs do Windows. Fontes previstas, conforme a categoria:

- WMI/CIM acessado diretamente, sem iniciar PowerShell.
- SetupAPI e Configuration Manager para enumeração Plug and Play e drivers.
- APIs nativas do Windows para sistema, energia, rede e dispositivos.
- DXGI e APIs gráficas para adaptadores e capacidades de GPU.
- EDID para identificação de monitores.
- Registro de desinstalação do Windows para softwares instalados.

Não usar scripts PowerShell como mecanismo de coleta do inventário. Não instalar serviços, drivers ou componentes auxiliares para coletar informações. Consultas devem ser somente leitura e tolerar APIs/propriedades ausentes em diferentes edições do Windows, inclusive LTSC.

## 6. Modelo lógico do relatório

O contrato lógico deve permitir evoluir o esquema sem invalidar históricos:

- `schema_version`: versão do formato persistido.
- `inventory_id`: identificador único e seguro.
- `app_version`: versão do QBoa coletor.
- `machine`: identificação resumida da máquina.
- `started_at` e `finished_at`: timestamps UTC.
- `duration_ms`: duração total.
- `collection_status`: `completed`, `partial` ou `failed`.
- `categories`: resultados independentes por categoria.
- `warnings`: limitações e divergências gerais.

Cada categoria registra seu próprio estado, duração, itens, fontes e avisos. Cada item mantém valor tipado, estado, confiança, origem e timestamp. Campos opcionais devem aceitar ausência real, sem preenchimento artificial.

O inventário terá persistência própria. Não reutilizar o journal de alterações de `safety.rs` nem gravar inventários como se fossem laudos de diagnóstico em `diagnostics_store.rs`.

## 7. Experiência de uso

A interface terá uma área própria de Inventário Técnico, separada das abas de Diagnóstico e Capacidades.

A tela deve oferecer:
- Resumo da máquina com os principais componentes.
- Doze categorias expansíveis, com visão resumida e detalhes técnicos.
- Estados visuais claros para disponível, não detectado, não aplicável, indisponível e erro.
- Origem/confiança acessíveis nos detalhes, sem poluir a leitura principal.
- Ação explícita para realizar nova coleta.
- Cópia de resumo e detalhes.
- Histórico local de coletas, com data, duração e estado.
- Comparação entre duas coletas, destacando alterações sem afirmar que uma mudança é defeito.
- Exportação JSON e CSV; relatório PDF legível para uso profissional.

Abrir ou exportar um relatório histórico não deve disparar uma nova coleta. Nova coleta deve ser sempre uma ação explícita do usuário.

## 8. Persistência, privacidade e limites

- Persistir localmente no diretório de dados do aplicativo.
- Não enviar inventário automaticamente para servidores.
- Não incluir conteúdo de arquivos pessoais, credenciais, chaves, tokens ou histórico de navegação.
- Tratar endereços de rede e identificadores de hardware como dados potencialmente sensíveis na exportação; a interface deve deixar claro o que será exportado.
- Limitar tamanho e validar identificadores/arquivos ao carregar históricos.
- Relatório corrompido ou de versão futura não pode derrubar a tela nem ser sobrescrito silenciosamente.

A política de retenção e a inclusão de identificadores persistentes como número de série e endereço MAC deverão ser decididas antes de habilitar esses campos na exportação padrão.

## 9. Reuso e integração com o código existente

Pontos existentes a avaliar e reutilizar sem acoplamento indevido:

- `capability_inventory.rs`: evidências de capacidades, TPM, Secure Boot, virtualização, dispositivos e recursos do Windows.
- `memory_inventory.rs`: módulos e parâmetros reportados de RAM.
- `cpu_diagnostic.rs`: identificação de CPU e informações já coletadas; separar inventário de disponibilidade de sensores/teste.
- `gpu_diagnostic.rs`: adaptadores e drivers; separar inventário de telemetria e teste de estabilidade.
- `storage_diagnostic.rs`: identificação de discos/volumes; separar inventário de benchmark e avaliação.
- `diagnostics_store.rs`: referência de práticas de persistência, não repositório compartilhado.

A camada de apresentação não deve depender diretamente de comandos PowerShell nem conhecer detalhes de APIs Windows. Os coletores devem produzir modelos tipados e testáveis; a orquestração reúne resultados parciais e persiste a fotografia.

## 10. Etapas de implementação e validação

1. Fechar o contrato de dados e os estados de coleta.
2. Criar testes unitários de serialização, classificação de estados, dados ausentes, divergência e falha parcial.
3. Implementar coletores por categoria, começando por equipamento/SO, placa-mãe/BIOS, CPU e RAM.
4. Integrar GPU, armazenamento, monitores, rede, áudio/periféricos, bateria/energia, softwares e drivers.
5. Implementar persistência e histórico sem recoleta silenciosa.
6. Implementar interface, comparação e exportações.
7. Validar em Windows 10/11, incluindo LTSC, diferentes edições e máquinas físicas/virtuais.

Critérios de aceite:
- Nenhuma categoria indisponível impede a apresentação das demais.
- Nenhum valor ausente é inventado ou silenciosamente convertido.
- Coleta não altera o sistema e não depende de PowerShell.
- Históricos continuam legíveis após atualização do aplicativo.
- Exportações correspondem exatamente à coleta selecionada.
- Nenhum teste de carga, benchmark ou reparo é disparado pelo inventário.
