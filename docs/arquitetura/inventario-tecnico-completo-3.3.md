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
- Identidade da placa-mãe em três dimensões independentes: marca comercial, fabricante declarado no SMBIOS e fabricante industrial/OEM, cada uma com origem e confiança próprias.
- Produto/modelo, versão e número de série da placa-mãe; o serial é dado sensível.
- Fabricante/fornecedor declarado pelo BIOS, versão e data de lançamento.
- Modo de inicialização do firmware (UEFI/Legacy), quando identificável.
- TPM: presença, prontidão, habilitação, ativação, versão de especificação e fabricante, como propriedades independentes.
- Secure Boot como estado reportado por fonte UEFI confiável; não inferir conformidade, suporte ou estado a partir de ausência de dados.
- Não confundir fabricante do computador (SMBIOS Type 1) com qualquer uma das três identidades da placa-mãe.

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

- Números de série e endereços MAC podem ser coletados e preservados no inventário local quando disponíveis, mas sua inclusão em exportações é opcional e vem desativada por padrão.
- Cada exportação deve oferecer uma opção explícita para incluir esses identificadores, explicar que podem identificar o equipamento ou a interface de rede e aplicar a escolha somente à exportação solicitada, sem alterar o inventário original.
- A opção deve estar disponível para cada formato de exportação e não pode ser ativada implicitamente por preferências anteriores sem indicação visível ao usuário.

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

## 11. Modelo de dados tipado e contratos por categoria

Esta seção é normativa para a implementação. Os nomes representam os tipos Rust canônicos; JSON usa snake_case. Estados, classificações e unidades conhecidas devem ser enums/tipos, não strings livres.

### 11.1 Tipos escalares e unidades

| Conceito | Tipo | Regra |
|---|---|---|
| Identificadores internos | UUID v4 | Nunca usar hostname como chave |
| Data/hora | DateTime<Utc>, RFC 3339 | Sempre UTC |
| Duração | u64 | Milissegundos |
| Capacidade/tamanho | u64 | Bytes |
| Frequência | u64 ou f64 | MHz |
| Taxa de link | u64 | Bits por segundo |
| Percentual | f64 | 0–100; rejeitar NaN/infinito |
| Endereço IP | std::net::IpAddr | Representação canônica |
| Versões externas | Option<String> | Não presumir formato semântico |

Normalizar strings vazias para ausência quando a fonte as usa como “não informado”. Preservar códigos desconhecidos em raw_value quando úteis. Não converter ausência em zero, false ou string vazia.

### 11.2 Evidência comum

Tipos comuns:

- InventoryValueState: Available, NotDetected, NotApplicable, Unavailable, Error.
- InventoryConfidence: Confirmed, Identified, Estimated, Unavailable.
- InventorySource: provider: String, member: Option<String>, native_code: Option<String>.
- Observed<T>: value: Option<T>, state: InventoryValueState, confidence: InventoryConfidence, source: Option<InventorySource>, collected_at: DateTime<Utc>, note: Option<String>.
- InventoryIssue: code: String estável, message: String segura para UI, source: Option<InventorySource>, native_code: Option<String>.
- CategoryCollection<T>: state, started_at, finished_at, duration_ms, data: Option<T>, issues: Vec<InventoryIssue>, warnings: Vec<String>.

Invariantes de Observed<T>:
- available exige value; confiança confirmada/identificada, ou estimada com nota explicativa.
- not_detected, not_applicable, unavailable e error exigem value = None.
- not_detected significa enumeração concluída sem correspondência; não prova ausência física.
- not_applicable significa que a propriedade não se aplica ao contexto.
- unavailable significa que a fonte não expõe o dado ou não há método suportado.
- error significa que uma consulta aplicável falhou e exige InventoryIssue.
- confidence = unavailable somente com state = unavailable.
- Uma propriedade com erro não invalida as demais propriedades da categoria.
- Uma coleção disponível pode ser vazia somente quando a enumeração terminou com sucesso. Falha de enumeração é error, nunca lista vazia.
- Itens de uma coleção podem ter propriedades indisponíveis sem invalidar os outros itens.

### 11.3 Raiz persistida

TechnicalInventory:
- schema_version: u32 (inicialmente 1)
- inventory_id: UUID
- app_version: String
- machine: MachineIdentity
- started_at, finished_at: DateTime<Utc>
- duration_ms: u64
- collection_status: InventoryRunStatus
- categories: InventoryCategories
- warnings: Vec<String>

InventoryRunStatus: Completed, Partial, Failed.

MachineIdentity:
- computer_name, manufacturer, model: Observed<String>
- device_kind: Observed<DeviceKind>
- is_virtual_machine: Observed<bool>

DeviceKind: Desktop, Laptop, Tablet, Workstation, Server, VirtualMachine, Unknown.

InventoryCategories contém uma CategoryCollection tipada para cada chave estável:
- system: SystemCategory
- firmware: FirmwareCategory
- cpu: CpuCategory
- memory: MemoryCategory
- gpu: GpuCategory
- storage: StorageCategory
- displays: DisplayCategory
- network: NetworkCategory
- audio_peripherals: AudioPeripheralCategory
- power: PowerCategory
- software: SoftwareCategory
- drivers: DriverCategory

Não incluir serial SMBIOS, UUID de hardware ou MAC na identidade resumida da máquina.

### 11.4 Contratos das doze categorias

Todos os campos escalares abaixo são Observed<T>, exceto quando indicado como Vec<T> ou estrutura composta. Campos que a fonte não expõe devem ser unavailable, não omitidos silenciosamente.

**A. Sistema e equipamento — SystemCategory**
- os_name, os_edition, os_version, display_version: Observed<String>
- build_number, ubr: Observed<u32>
- architecture: Observed<OsArchitecture> (X86, X64, Arm64, Unknown)
- ui_language, system_locale: Observed<String>
- install_date, boot_time: Observed<DateTime<Utc>>
- computer_name, domain_or_workgroup: Observed<String>
- device_kind: Observed<DeviceKind>
- Não coletar usuário logado, SID, nome de perfil ou caminhos pessoais.

**B. Placa-mãe e firmware — FirmwareCategory**

`MotherboardInfo`:
- `commercial_brand: Observed<String>` — marca sob a qual o modelo específico da placa-mãe é comercializado ao comprador (por exemplo, PICHAU). Não significa necessariamente quem fabricou fisicamente a placa.
- `smbios_manufacturer: Observed<String>` — fabricante declarado pela própria implementação SMBIOS na estrutura Type 2 (Baseboard Information), campo Manufacturer. É uma declaração do firmware, não uma verificação independente da fábrica.
- `oem_manufacturer: Observed<String>` — fabricante industrial/ODM/OEM que efetivamente projetou ou produziu a placa, somente quando uma fonte documental confiável identifica explicitamente essa entidade.
- `product: Observed<String>` — produto/modelo identificado na estrutura SMBIOS Type 2, campo Product.
- `version: Observed<String>` — revisão/versão declarada na estrutura SMBIOS Type 2, campo Version.
- `serial_number: Observed<Sensitive<String>>` — serial declarado na estrutura SMBIOS Type 2, campo Serial Number; manter sensível e sujeito às regras de exportação da seção 11.6.

**Fontes confiáveis e precedência para a identidade da placa-mãe**
1. **SMBIOS Type 2 (fonte primária para os campos declarados pelo firmware):** coletar Manufacturer, Product, Version e Serial Number diretamente da tabela SMBIOS obtida por `GetSystemFirmwareTable(RSMB)`. Selecionar a estrutura cujo Board Type indique Motherboard (valor `0x0A`); não assumir que a primeira estrutura Type 2 é a placa-mãe quando houver múltiplas estruturas. Registrar a origem como SMBIOS Type 2 e o campo/offset correspondente.
2. **Marca comercial:** preencher somente com evidência vinculada ao modelo exato, como catálogo/ficha técnica oficial do fabricante da marca, página oficial do produto, etiqueta/embalagem original ou documentação comercial do equipamento. Uma identificação manual/documental fornecida pelo técnico pode ser registrada apenas se o fluxo do produto preservar que a origem é declarada pelo usuário. SMBIOS Type 1 Manufacturer identifica o fabricante/marca do computador completo e não pode ser copiado para a marca comercial da placa-mãe. SMBIOS Type 2 Manufacturer também não prova, sozinho, a marca comercial.
3. **Fabricante industrial/OEM:** preencher somente quando documentação oficial do titular da marca, do fabricante industrial ou documentação técnica rastreável identificar explicitamente o OEM/ODM para aquele modelo/revisão. Catálogos de distribuidores, fóruns, bancos de dados comunitários, semelhança visual, códigos de PCB, país de fabricação e suposições por chipset não são evidência suficiente.
4. **Sem evidência suficiente:** manter `value = None` com estado `unavailable` e nota objetiva. Não copiar um fabricante para os outros campos, não inferir OEM a partir de SMBIOS, e não converter desconhecido em `not_detected`.

**`BiosInfo`**
- `vendor`, `version` e `release_date`: campos da estrutura SMBIOS Type 0 (BIOS Information), respectivamente Vendor, BIOS Version e BIOS Release Date. Preservar o texto original reportado; não confundir fornecedor do BIOS com fabricante da placa-mãe ou do computador.
- `mode: Observed<FirmwareMode>`: API nativa `GetFirmwareType`; `FirmwareMode`: Uefi, Legacy, Unknown. Falha de consulta é unavailable/error, nunca inferência por versão do Windows ou presença de partições.
- TPM: consultar o provedor Windows de TPM por API/WMI nativo diretamente (sem PowerShell), mantendo `present`, `ready`, `enabled`, `activated`, `spec_version` e `manufacturer` independentes. Registrar classe/propriedade e namespace como origem; ausência do provedor, falha e TPM não detectado são estados distintos.
- `secure_boot_enabled: Observed<bool>`: consultar o estado da variável UEFI `SecureBoot` por API nativa de firmware quando o modo for UEFI. Valor 1 = habilitado; valor 0 = desabilitado. Falha de leitura, variável indisponível ou falta de privilégio não equivale a false. Em modo Legacy, registrar not_applicable. Não usar apenas a existência de chave de Registro como prova primária do estado atual.
- Não inferir compatibilidade com Windows 11, integridade da cadeia de boot ou conformidade de segurança a partir desses campos.

**Fontes normativas e documentação técnica**
- DMTF, SMBIOS Reference Specification: Type 2 (Baseboard Information) e Type 0 (BIOS Information): https://www.dmtf.org/standards/smbios
- Microsoft Learn, SMBIOS e significado dos campos de fabricante do sistema/baseboard: https://learn.microsoft.com/pt-br/windows-hardware/drivers/bringup/smbios
- Microsoft Learn, Computer Hardware IDs e distinção entre Manufacturer Type 1 e Baseboard Manufacturer Type 2: https://learn.microsoft.com/pt-br/windows-hardware/drivers/install/computer-hardware-ids

**C. CPU — CpuCategory**
- processors: Vec<ProcessorInfo>, suportando múltiplos sockets.
- ProcessorInfo: name, manufacturer, processor_id, socket, architecture, physical_cores, logical_processors, base_clock_mhz, max_clock_mhz, virtualization_firmware_enabled, virtualization_supported, l1_cache_bytes, l2_cache_bytes, l3_cache_bytes.
- Frequências e contagens são dados reportados pela fonte, não benchmark nem frequência instantânea.
- Não coletar temperatura, consumo ou utilização como requisito do inventário.

**D. RAM — MemoryCategory**
- total_physical_bytes, available_physical_bytes, total_virtual_bytes, available_virtual_bytes, reported_slot_count.
- modules: Vec<MemoryModuleInfo>.
- MemoryModuleInfo: locator, bank_label, capacity_bytes, manufacturer, part_number, memory_type, form_factor, configured_speed_mhz, smbios_speed_mhz, data_width_bits, total_width_bits, configured_voltage_mv.
- MemoryTechnology: DDR, DDR2, DDR3, DDR4, DDR5, LPDDR3, LPDDR4, LPDDR4X, LPDDR5, LPDDR5X, Unknown; preservar código SMBIOS bruto.
- MemoryFormFactor: Dimm, Sodimm, Onboard, RowOfChips, Unknown.
- Timings CAS e perfis XMP/EXPO ficam unavailable sem fonte confiável; nunca deduzir pelo clock.

**E. GPU — GpuCategory**
- adapters: Vec<GpuAdapterInfo>.
- GpuAdapterInfo: name, vendor, device_id, pnp_device_id: Observed<Sensitive<String>>, adapter_kind, driver_provider, driver_version, driver_date, dedicated_memory_bytes, shared_memory_bytes, is_primary, active_outputs.
- GpuAdapterKind: Integrated, Dedicated, Virtual, Unknown.
- graphics_apis: Vec<GraphicsApiInfo>, somente capacidades obtidas por consulta leve e confiável.
- Memória compartilhada nunca deve ser apresentada como VRAM dedicada. is_primary só pode ser true quando a API resolver sem ambiguidade.
- Não executar shader, benchmark ou teste de estabilidade para preencher inventário.

**F. Armazenamento — StorageCategory**
- physical_disks: Vec<PhysicalDiskInfo>; volumes: Vec<VolumeInfo>.
- PhysicalDiskInfo: model, manufacturer, serial_number: Observed<Sensitive<String>>, firmware_revision, interface_type, media_type, bus_type, capacity_bytes, logical_sector_bytes, physical_sector_bytes.
- StorageMediaType: Hdd, Ssd, Scm, Unknown.
- VolumeInfo: volume_id, drive_letter, mount_points, label, file_system, capacity_bytes, free_bytes, associated_disk_ids.
- Não presumir NVMe/SSD pela velocidade ou nome. Não executar SMART prolongado, benchmark, leitura integral, reparo ou teste de superfície.

**G. Monitores — DisplayCategory**
- displays: Vec<DisplayInfo>.
- DisplayInfo: manufacturer, model, friendly_name, edid_product_code, serial_number: Observed<Sensitive<String>>, connection_type, connection_state, is_primary, current_mode, native_mode.
- DisplayConnectionState: Connected, Disconnected, Unknown.
- DisplayMode: width_px: u32, height_px: u32, refresh_rate_millihz: u32, bits_per_color: Option<u8>.
- Não apresentar monitor desconectado como conectado; não inferir modelo sem EDID.

**H. Rede — NetworkCategory**
- adapters: Vec<NetworkAdapterInfo>.
- NetworkAdapterInfo: name, description, manufacturer, interface_kind, is_physical, status, link_speed_bps, mac_address: Observed<Sensitive<String>>, driver_provider, driver_version, dhcp_enabled, ipv4: Vec<IpConfiguration>, ipv6: Vec<IpConfiguration>, gateways: Vec<IpAddr>, dns_servers: Vec<IpAddr>, interface_metric.
- IpConfiguration: address: IpAddr, prefix_length: u8, is_temporary: Option<bool>.
- NetworkLinkState: Up, Down, Dormant, NotPresent, Unknown.
- Enumerar adaptadores físicos e virtuais. Não executar ping, DNS ativo, traceroute, teste de porta ou throughput.

**I. Áudio e periféricos — AudioPeripheralCategory**
- audio_devices: Vec<AudioDeviceInfo>; peripherals: Vec<PeripheralInfo>.
- AudioDeviceInfo: name, manufacturer, device_id, device_role, is_default, state, driver_version.
- AudioDeviceRole: Input, Output, Controller, Unknown.
- PeripheralInfo: name, manufacturer, device_class, bus_type, connection_state, driver_version, is_present.
- Classes relevantes: USB, Bluetooth, câmera, biometria, touchscreen, caneta, teclado e mouse.
- Não coletar teclas, imagens, áudio, impressões digitais ou conteúdo de dispositivos.

**J. Bateria e energia — PowerCategory**
- batteries: Vec<BatteryInfo>; ac_power_state: Observed<AcPowerState>; active_power_scheme: Observed<PowerSchemeInfo>.
- BatteryInfo: name, manufacturer, chemistry, design_capacity_mwh, full_charge_capacity_mwh, remaining_capacity_mwh, charge_percent, cycle_count, status.
- AcPowerState: Online, Offline, Unknown.
- Em desktop sem bateria, enumeração concluída vazia e estado not_applicable; não gerar falha.
- Não executar descarga nem alterar plano, suspensão ou política de energia.

**K. Softwares — SoftwareCategory**
- applications: Vec<InstalledApplicationInfo>.
- InstalledApplicationInfo: display_name, display_version, publisher, architecture, install_date, install_scope, source_registry, estimated_size_bytes, uninstall_entry_id.
- InstallScope: Machine, User, Unknown.
- uninstall_entry_id é opaco; nunca executar comandos de desinstalação.
- Deduplicar entradas equivalentes preservando escopo/origem quando diferentes. Não executar binários nem varrer diretórios pessoais. Não prometer cobertura de aplicativos portáteis.

**L. Drivers — DriverCategory**
- devices: Vec<DriverDeviceInfo>.
- DriverDeviceInfo: device_instance_id: Sensitive<String>, friendly_name, device_class, manufacturer, driver_provider, driver_version, driver_date, inf_name, problem_code: Observed<u32>, status, is_present.
- PnpDeviceState: Started, Disabled, Error, Unknown.
- O identificador PnP é sensível e deve ser omitido nas exportações padrão.
- Não instalar, atualizar, remover ou habilitar drivers.

### 11.5 Estados das categorias e execução

- available: coleta concluída e dados retornados; campos individuais ainda podem estar indisponíveis.
- not_detected: enumeração válida sem elementos correspondentes.
- not_applicable: categoria/propriedade não se aplica ao contexto.
- unavailable: plataforma/provedor não oferece método suportado.
- error: falha operacional; preencher issues com código e origem.
- Completed: todas as categorias processadas sem erro operacional; not_detected/not_applicable/unavailable são resultados válidos.
- Partial: pelo menos uma categoria falhou, mas outra foi processada.
- Failed: falha da orquestração ou nenhuma categoria pôde ser processada. Preservar metadados e erros; não criar inventário vazio apresentado como sucesso.

### 11.6 Identificadores sensíveis e exportação

Tipos:
- Sensitive<T> { value: T, classification: SensitiveDataKind }
- SensitiveDataKind: HardwareSerial, NetworkMac, PnpInstanceId.
- InventoryExportOptions { include_sensitive_identifiers: bool }, default false.

O inventário local pode preservar serial, MAC e identificadores PnP quando coletados. O exportador deve aplicar redação em uma cópia/visão de exportação, sem mutar o histórico. A opção deve ser explícita em cada exportação, começar sempre false e informar quais classes serão incluídas. Preferências anteriores não podem ativá-la silenciosamente.

### 11.7 Serialização, compatibilidade e validação

- schema_version = 1 na primeira versão. Mudança incompatível exige incremento e migração explícita.
- Campos opcionais ausentes são None; não serializar strings vazias como substituto.
- Enums em snake_case; valores numéricos preservam unidades e não são armazenados formatados.
- Ordenar coleções por identificador estável/nome normalizado; nunca usar posição de enumeração como identidade.
- Validar: percentuais 0–100; prefixo IP dentro do limite da família; free_bytes <= capacity_bytes quando ambos conhecidos; finished_at >= started_at; duração coerente; consistência entre state e value.
- Limitar tamanhos de strings, listas e arquivos; tratar dados externos como não confiáveis; nenhum campo pode ser interpretado como comando ou caminho executável.
- Relatório de versão futura não pode ser sobrescrito silenciosamente. Abrir somente se o leitor puder preservar os dados sem perda; caso contrário, informar incompatibilidade.
- Testes obrigatórios: round-trip JSON; todos os estados; estado/valor inconsistente; categoria parcial; lista vazia válida versus falha de enumeração; enum desconhecido; timestamps; limites numéricos; redação; schema futuro; arquivo truncado/corrompido.

### 11.8 Limites para a implementação

- Criar módulo independente technical_inventory para modelos, validação e orquestração; coletores separados por domínio.
- Não concentrar consultas em main.rs.
- Não reutilizar structs de teste/diagnóstico que carregam sensores, sessões, progresso ou benchmark. Adaptar somente lógica de identificação.
- memory_inventory.rs pode fornecer dados tipados de módulos, sem tornar seu formato atual de UI o contrato persistido.
- capability_inventory.rs continua dono da aba Capacidades; compartilhar funções de baixo nível quando adequado, sem acoplar o formato CapabilityInventory à persistência do inventário.
- Primeira fatia: modelos comuns, serialização, validação e testes unitários. Sem UI e sem coletar todas as categorias nesta etapa.

## 12. Matriz de integração com Capacidades

A aba **Capacidades** continua independente, disponível no Free e no Pro. Ela responde se um recurso ou dispositivo foi detectado, está desabilitado ou não pôde ser confirmado. O Inventário Técnico descreve componentes e configurações com dados tipados e maior profundidade. Uma mesma evidência pode alimentar as duas experiências, mas os relatórios e seus contratos não se misturam.

### 12.1 Resumo de sistema já apresentado em Capacidades

O objeto `CapabilityInventory.system` contém resumos de OS, arquitetura, fabricante, modelo, placa-mãe, processador, memória, GPU e tipo de equipamento. Esses campos são strings de apresentação e não devem ser copiados diretamente para o inventário.

| Dado resumido em Capacidades | Destino tipado no Inventário | Regra |
|---|---|---|
| `os_name`, `os_version`, `os_build`, `architecture` | `system` | Recoletar em campos próprios, preservando edição, DisplayVersion, build e UBR separadamente. |
| `manufacturer`, `model`, `device_type` | `machine` e `system.device_kind` | Reaproveitar a evidência de identificação, não a string formatada; preservar confiança e origem. |
| `motherboard` | `firmware.motherboard` | Não importar resumo formatado. Coletar Type 2 e manter `commercial_brand`, `smbios_manufacturer` e `oem_manufacturer` independentes; marca comercial/OEM exigem evidência documental própria. |
| `processor` | `cpu.processors[]` | Não importar resumo com clock/núcleos concatenados; coletar cada propriedade tipada. |
| `memory` | `memory` | Não importar texto agregado; enumerar módulos e capacidades individualmente. |
| `gpu` | `gpu.adapters[]` | Enumerar todos os adaptadores e distinguir memória dedicada/compartilhada. |

### 12.2 Capacidades de dispositivos e destino no Inventário

| ID atual em Capacidades | Destino no Inventário | O que não pode ser confundido |
|---|---|---|
| `bluetooth` | `audio_peripherals.peripherals[]` e `drivers.devices[]` | Recurso Bluetooth detectado não substitui fabricante, modelo, estado PnP e driver do adaptador. |
| `wifi`, `ethernet` | `network.adapters[]` e `drivers.devices[]` | A capacidade resumida não substitui enumeração de interfaces, endereços e configuração passiva de rede. |
| `touchscreen`, `pen`, `camera`, `biometrics` | `audio_peripherals.peripherals[]` e `drivers.devices[]` | Presença/estado do dispositivo não autoriza coletar conteúdo, imagens, digitais ou entradas do usuário. |
| `audio` | `audio_peripherals.audio_devices[]` e `drivers.devices[]` | Distinguir controlador, endpoint de entrada/saída e driver. |
| `battery` | `power.batteries[]` | A presença resumida não substitui capacidade, carga, química e estado; desktop sem bateria é resultado não aplicável, não erro. |
| `tpm` | `firmware.tpm` | Presença, pronto, habilitado, ativado, versão e fabricante são propriedades independentes. |
| `secure_boot` | `firmware.secure_boot_enabled` e `firmware.bios.mode` | Não inferir Secure Boot desabilitado quando a consulta falhar ou o firmware não for UEFI. |
| `virtualization_firmware` | `cpu.processors[].virtualization_firmware_enabled` e `virtualization_supported` | Capacidade suportada e habilitação no firmware são fatos distintos. |
| `hypervisor_active` | Sem equivalência direta no esquema atual | Hipervisor ativo no Windows não significa que o próprio computador seja uma máquina virtual. Não preencher `machine.is_virtual_machine` com esse dado. |
| `hyper_v`, `wsl`, `virtual_machine_platform`, `sandbox` | Permanecem em Capacidades por enquanto | São estados de recursos/componentes opcionais do Windows; não há campo equivalente no contrato atual das 12 categorias. Não forçar sua inclusão em software instalado nem em identificação de hardware. |

### 12.3 Regras de implementação e compartilhamento

1. **Capacidades permanece dona da aba e do seu contrato.** Não migrar seus cards, estados ou resumo para a tela do Inventário e não condicionar Capacidades à licença Pro.
2. **Não chamar `capability_inventory::collect()` pelo Inventário.** O coletor atual de Capacidades executa PowerShell e usa cmdlets como `Get-PnpDevice`, `Get-CimInstance`, `Get-Tpm`, `Confirm-SecureBootUEFI` e `Get-WindowsOptionalFeature`. Isso viola o contrato do Inventário, que exige coleta nativa em Rust/APIs Windows e não pode depender de PowerShell.
3. **Reutilizar evidência, não resumos formatados.** Quando a mesma fonte nativa atender aos dois domínios, extrair a consulta para um módulo de baixo nível compartilhado e adaptar seu resultado aos dois modelos. Não fazer um domínio depender do JSON/struct do outro.
4. **Preservar semânticas específicas.** `registered_not_present` em Capacidades não equivale automaticamente a `not_detected` no Inventário; desconhecido/erro de consulta não pode virar ausência confirmada.
5. **Uma coleta não dispara a outra.** Abrir Capacidades não cria histórico de inventário; executar o Inventário não atualiza nem altera o estado visual de Capacidades. Cada experiência mantém seu próprio fluxo, relatório e disponibilidade.
6. **Sem duplicar trabalho desnecessariamente.** Se futuramente houver um serviço comum de enumeração nativa PnP, ele poderá alimentar Capacidades e Inventário em uma única consulta por execução, desde que preserve os dois contratos e não introduza acoplamento de licença.
7. **Migração de fonte é uma etapa separada.** A substituição futura de PowerShell em Capacidades pode aproveitar os módulos nativos criados para o Inventário, mas não deve ser incluída como requisito para concluir a primeira versão do Inventário.
