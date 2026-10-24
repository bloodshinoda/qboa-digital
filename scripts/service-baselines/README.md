# Coleta de baseline de serviços do Windows

Este diretório prepara a coleta real por instalação; **não contém valores presumidos nem gera perfis de otimização**. Execute em cada VM limpa, sem debloat, e preserve o JSON original como evidência da captura.

## Coletor

Execute em PowerShell elevado na VM-alvo:

\`\`\`powershell
Set-ExecutionPolicy -Scope Process Bypass
.\Collect-ServiceBaseline.ps1 -Environment "win10-22h2-home" -ExpectedBuild "19045" -ExpectedEditionId "Core" -OutputPath ".\captures\win10-22h2-home.json"
\`\`\`

Use nomes de ambiente únicos. Exemplos para as capturas solicitadas:

| Ambiente | Build esperada | EditionID esperado |
|---|---:|---|
| Windows 10 22H2 Home | 19045 | Core |
| Windows 10 22H2 Pro | 19045 | Professional |
| Windows 10 22H2 Pro for Workstations | 19045 | ProfessionalWorkstation |
| Windows 11 26H2 Home | validar antes da coleta | Core |
| Windows 11 26H2 Pro | validar antes da coleta | Professional |
| Windows 11 26H2 Pro for Workstations | validar antes da coleta | ProfessionalWorkstation |
| Windows 10 Enterprise LTSC 2021 | 19044 | EnterpriseS |
| Windows 11 Enterprise LTSC 2024 | 26100 | EnterpriseS |

A build de Windows 11 26H2 deve ser confirmada numa imagem oficial correspondente antes de fixar \`-ExpectedBuild\`; não trate o número informado no pedido como confirmado por este script. Para comparação adicional de diferenças entre versões, colete Windows 11 24H2 e 25H2 em instalações limpas separadas e nomeie as capturas explicitamente. Não misture capturas de versões cumulativamente atualizadas com imagens limpas sem registrar essa diferença.

Se o build ou EditionID real não corresponder aos argumentos esperados, o script aborta. Para primeira inspeção exploratória, omita os parâmetros \`-ExpectedBuild\` e \`-ExpectedEditionId\`, examine os metadados e depois repita a captura com as expectativas corretas.

O coletor registra metadados do SO e, para cada chave em \`HKLM\\SYSTEM\\CurrentControlSet\\Services\`, coleta \`Start\`, \`DelayedAutostart\`, \`Type\`, presença e árvore de valores de \`TriggerInfo\`, \`DependOnService\`, \`DisplayName\` e \`ImagePath\`. Instâncias com sufixo hexadecimal de usuário/sessão (por exemplo \`CDPUserSvc_5c1e7\`) são excluídas; a chave-modelo sem sufixo permanece. O rótulo do serviço é calculado diretamente dos valores de registro. Cada observação vem marcada \`source: measured\`.

> Faça as capturas em VMs descartáveis e limpas, sem scripts de debloat. Registre ISO, idioma, arquitetura, data da instalação e atualização cumulativa usada junto dos artefatos; a build por si só pode não identificar todos os componentes.

## Merge e testes

Em qualquer máquina com Python 3.10+:

\`\`\`bash
python scripts/service-baselines/merge_service_baselines.py --out build/service-baselines/matrix captures/*.json
python -m unittest discover -s scripts/service-baselines -p 'test_*.py' -v
\`\`\`

O merge produz:

- \`service-matrix.csv\`: uma linha por serviço, uma coluna por ambiente, células vazias quando o serviço não existe, sufixo \`(Trigger)\`, \`varies_between_envs\` e \`source\` como mapa JSON por ambiente;
- \`service-observations.csv\`: formato longo com os campos medidos e a proveniência por observação;
- \`capture-metadata.json\`: metadados de SO e lista de arquivos de captura.

O merge recusa capturas que não declarem \`source: measured\`, ambientes duplicados, serviços duplicados e instâncias dinâmicas por sessão. Ele **não** inventa valores para células ausentes, não faz analogia entre edições, não deriva alvos Safe/Recommended/Tweaked e não altera os JSONs de perfil do QBoa.

## Decisões pendentes antes de derivar perfis

1. A aplicação atual aceita apenas \`automatic\`, \`manual\` e \`disabled\`; não converta \`Automatic (Delayed)\` para \`Automatic\` silenciosamente. A implementação precisa de um tipo explícito e de leitura/escrita de \`DelayedAutostart\` antes de suportá-lo.
2. \`parse_edition\` atualmente mapeia \`EnterpriseS\` para \`Enterprise\`, sem distinguir LTSC. Recomenda-se um identificador de edição/canal separado (por exemplo \`enterprise_ltsc\`) ou um campo de produto/canal, sem reaproveitar o perfil Enterprise comum por analogia. O mapeamento precisa ser acompanhado por seleção de perfil, compatibilidade, snapshots e testes.
3. Antes de gerar alvos, construir o grafo reverso de \`DependOnService\`: um serviço não pode ser desativado se for dependência de outro serviço que ficará Automatic, além da lista de proteção existente. A regra deve ser validada novamente no caminho de aplicação, não apenas na prévia.
4. A lista de serviços protegidos deve ser uma única fonte de verdade compartilhada pela gestão manual e pelos perfis.

## Proveniência e licença dos dados históricos

Os dados históricos candidatos que já existem no projeto derivam de **WinServiceConfigurator**, de Madbomb122, e do trabalho de Black Viper. Isso não transforma tais dados em medições das versões atuais do Windows. Mantenha a distinção entre baseline observada e política do perfil.

- Repositório-fonte: https://github.com/madbomb122/WinServiceConfigurator
- Referência Black Viper: https://www.blackviper.com/
- Copyright: Copyright (c) 2022 Madbomb122 - WinServiceConfigurator Script.
- Licença MIT e condição de redistribuição: preservar o aviso de copyright, o aviso de permissão e o link original de doação em cópias ou partes substanciais.
- Link original de doação exigido pela licença: https://www.amazon.com/gp/registry/wishlist/YBAYWBJES5DE/

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice(s) and this permission notice and any original donation link shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
