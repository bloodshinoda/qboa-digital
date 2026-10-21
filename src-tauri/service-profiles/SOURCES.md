# Fontes e licença dos presets de serviços

Os catálogos candidatos do Windows 10 Home e Pro usam como referência histórica as configurações Black Viper distribuídas por **WinServiceConfigurator**, de Madbomb122. Os valores foram adaptados para o formato interno do QBoa e filtrados para excluir serviços dinâmicos, estados de inicialização não suportados e serviços protegidos.

Fontes:
- Repositório: https://github.com/madbomb122/WinServiceConfigurator
- Configuração padrão histórica do Windows 10: https://github.com/madbomb122/WinServiceConfigurator/blob/main/Win%2010/Default-Windows%2010.csv
- Black Viper Safe Desktop: https://github.com/madbomb122/WinServiceConfigurator/blob/main/Win%2010/BlackViper-Desktop-Safe.csv
- Black Viper Safe Laptop: https://github.com/madbomb122/WinServiceConfigurator/blob/main/Win%2010/BlackViper-Laptop-Safe.csv
- Black Viper Tweaked Desktop: https://github.com/madbomb122/WinServiceConfigurator/blob/main/Win%2010/BlackViper-Desktop-Tweaked.csv

Mapeamento dos quatro controles do QBoa:
- **Padrão:** referência informativa; não aplica a configuração histórica como se fosse o padrão atual.
- **Seguro:** usa o Safe comum a desktop e notebook; diferenças específicas de hardware são omitidas.
- **Refinado:** parte do Safe e só usa diferenças do Safe Desktop quando desktop e ausência de bateria forem confirmados.
- **Agressivo:** usa Tweaked Desktop; diferenças em relação ao perfil portátil só entram quando desktop e ausência de bateria forem confirmados.

A configuração padrão de referência é histórica (Windows 10 April 2018 Update). Ela **não** é tratada como baseline confiável para restaurar padrões em versões modernas. O QBoa compara o tipo de inicialização atual com o baseline histórico e omite regras divergentes; isso é uma heurística conservadora, não uma prova de equivalência entre builds.

Os catálogos permanecem com aplicação bloqueada até validação prática. Os arquivos de Windows 11 não herdam automaticamente as regras do Windows 10: a fonte contém uma lista padrão do Windows 11, mas não fornece perfis Safe/Tweaked equivalentes atuais. Regras condicionais a desktop exigem evidência de hardware inequívoca e ausência de bateria; se a detecção for inconclusiva, essas regras são omitidas.

## Aviso de licença da fonte

As configurações derivam de dados distribuídos no repositório WinServiceConfigurator, cujo README declara licença MIT com condição adicional de preservar o aviso de copyright e o link original de doação.

Copyright (c) 2022 Madbomb122 - WinServiceConfigurator Script.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice(s), this permission notice and any original donation link shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

Link original de doação exigido pela licença: https://www.amazon.com/gp/registry/wishlist/YBAYWBJES5DE/

As configurações de serviço também se baseiam no trabalho histórico de Charles "Black Viper" Sparks: https://www.blackviper.com/
