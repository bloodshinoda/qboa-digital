# Identificação conservadora de serviços

## Fontes de referência

As listas fornecidas pelo usuário são referências de configuração para **Windows 10 Pro** e **Windows 11 Pro**. Elas são usadas como pistas de pertencimento à família Windows, não como prova de configuração padrão de fábrica e não como autorização para mudar o tipo de inicialização. Os valores `Auto`, `demand`, `delayed-auto` e `Disabled` descrevem o que aqueles scripts tentam aplicar.

A captura `iot-ltsc-service-baseline.json` é uma observação medida de uma instalação Windows 11 IoT Enterprise LTSC 2024. Ela descreve aquela máquina naquele momento; não é, por si só, uma imagem limpa nem uma baseline universal do LTSC.

## Regra de decisão

1. Consultar o inventário real do Windows antes de exibir a lista.
2. Classificar como terceiro conhecido apenas quando houver uma assinatura reconhecida por nome interno, executável ou caminho conhecido.
3. Classificar como Microsoft assinado apenas quando a verificação Authenticode for válida e o certificado indicar Microsoft.
4. Tudo o mais fica como fornecedor não identificado. Não inferir que é serviço do Windows nem de terceiro pela ausência numa lista.
5. Não gerar recomendações de inicialização com base somente nas listas Pro.
6. Antes de aplicar uma mudança de perfil, exigir prévia atual, mostrar as alterações, permitir seleção explícita e revalidar no backend. A confirmação visual não substitui as verificações de segurança no backend.

## Assinaturas de terceiros observadas

| Fornecedor | Nomes internos/arquivos reconhecidos | Normalização de versão |
|---|---|---|
| AMD | `amdsata`, `AMDSAFD`, `amdsbs`, `amdwps`, `amdwddmg`, `AMDRyzenMasterDriverV20`, `amdi2c`, `amdgpio3`, `AmdK8`, `amdpsp`, `AmdPPM`, `AMDXE`, `amdxata`, `amdgpio2`, `AMD External Events Utility`, `AMD Crash Defender Service`, `amdacpbus`, `amdfendrmgr`, `amdfendr`, `AtiHDAudioService` | Nomes internos conhecidos e nomes de arquivos de driver; não classificar por substring genérica `amd`. |
| Apple | `Apple Mobile Device Service`, `AppleMobileDeviceService.exe` | Nome/executável estável; pasta Mobile Device Support é evidência auxiliar. |
| Claude | `CoworkVMService`, `cowork-svc.exe` | Ignorar a versão no diretório `Claude_<versão>_...`. |
| Dropbox | `dbupdate`, `dbupdatem`, `DropboxUpdate.exe` | Nomes internos e executável; não depender da versão do produto. |
| Google | Prefixos `GoogleUpdaterService`, `GoogleUpdaterInternalService`; `GoogleChromeElevationService`, `updater.exe`, `elevation_service.exe` | Remover mentalmente o sufixo numérico de versão do nome interno; confirmar com executável/caminho quando disponível. |
| Glarysoft | `GUBootService`, `GUBootStartup`, `GUPMService`, `GUMemfilesService` | Reconhecer também o driver `.sys`; não exigir versão literal da pasta. |
| PDF24 | `PDF24`, `pdf24.exe` | Nome/executável e pasta-base. |
| Valve / Steam | `Steam Client Service`, `steamservice.exe` | Nome/executável e pasta Common Files\Steam. |

## Limites

- A assinatura do fornecedor não determina se o serviço é seguro para desativar.
- Um serviço de terceiros pode ser necessário para hardware, autenticação, sincronização ou software em uso.
- A presença numa lista Pro não prova que o serviço exista ou tenha o mesmo estado no LTSC.
- Serviços opcionais e componentes de driver podem não aparecer em todas as instalações.
- Não há desligamento, restauração ou aplicação automática decorrente desta classificação.
