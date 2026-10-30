# Changelog — QBoa Digital

Todas as mudanças relevantes do QBoa Digital são registradas neste arquivo.

## [3.2.0] — 2026-10-30

### Diagnóstico e hardware
- Adiciona teste de integridade da memória RAM com perfis Rápido, Padrão, Prolongado e Personalizado, inventário por módulo e progresso/cancelamento.
- Adiciona teste de carga controlada da CPU com telemetria térmica, limite de segurança e relatório persistente.
- Adiciona teste de estabilidade da GPU via Direct3D 11 Compute Shader, com validação dos resultados e interrupção por limite térmico ou perda de telemetria.
- Integra o Diagnóstico completo em uma sequência com confirmação prévia, progresso por etapa, cancelamento e resumo consolidado.
- Mantém os diagnósticos separados das operações de limpeza e manutenção; não gera recomendações automáticas.

### Laudos e histórico
- Define contrato JSON versionado para os laudos, com identificação do sistema, resultados, evidências, fontes e problemas por componente.
- Persiste relatórios localmente em arquivos independentes, com gravação segura e checkpoints durante a execução.
- Adiciona histórico local e recuperação de relatórios interrompidos como parciais, preservando os dados já coletados.
- Mantém os laudos individuais dos motores de RAM, CPU e GPU, além do laudo consolidado.

### Validação e distribuição
- Amplia o CI do Windows com testes do coletor de armazenamento, formatação Rust, testes automatizados e build do aplicativo.
- Atualiza os metadados de versão para 3.2.0 e padroniza a geração dos artefatos de release a partir da versão do Tauri.
- Mantém a distribuição Windows x64 em instalador NSIS e executável portátil individual.

### Limitações conhecidas
- A aprovação dos testes cobre apenas as regiões/buffers efetivamente exercitados e não certifica integralmente a RAM, CPU ou GPU.
- O teste de GPU exige Direct3D 11 Feature Level 11_0 e sensor térmico compatível; não testa rasterização nem toda a VRAM.
- A validação final de longa duração e dos limites térmicos deve ser realizada em hardware físico representativo.

## [3.1.2] — 2026-10-27

### Serviços do Windows
- Corrige a prévia de ajustes no Windows 11 IoT Enterprise LTSC, que podia ocultar todas as sugestões de alteração.
- Restringe as sugestões heurísticas aos serviços cujo estado atual coincide com o valor de referência do catálogo; divergências são ignoradas.
- Identifica fornecedores conhecidos (AMD, Apple, Claude, Dropbox, Google, Glarysoft, PDF24 e Valve/Steam) e expõe evidências de identificação.
- Mantém a prévia e a confirmação explícita antes de aplicar mudanças; identificação de fornecedor não equivale a recomendação para desativar serviços.

### Validação
- Adiciona testes para fornecedores identificados, serviços desconhecidos e compatibilidade das sugestões com a referência LTSC.

## [3.1.1] — 2026-10-26

### Interface e experiência de uso
- Melhora o contraste dos avisos e notificações com fundo escuro translúcido, desfoque e contornos mais legíveis.
- Centraliza o painel de progresso durante a execução.
- Oculta temporariamente o painel de progresso enquanto diálogos de confirmação ou avisos de risco estão abertos, evitando sobreposição de elementos.
- Dá prioridade visual aos diálogos de confirmação e de risco.
- Exibe descrições em linguagem simples para serviços conhecidos na prévia de ajustes.

### Execução e segurança
- Impede o início de tarefas concorrentes enquanto outra operação está em andamento, com validação no frontend e no backend Rust.
- Melhora o tratamento de solicitações de execução e de erros ao iniciar tarefas.
- Mantém as confirmações para operações que exigem atenção e as proteções existentes para alterações reversíveis.

### Documentação e distribuição
- Atualiza a versão do aplicativo para 3.1.1.
- Padroniza os nomes dos arquivos de release para facilitar a publicação e os links do hotsite.

## [3.1.0]
- Aprimora o inventário de capacidades e recursos do Windows.
- Melhora a apresentação dos serviços e das informações de diagnóstico.
- Adiciona resumo final das tarefas com estado de sucesso, falha ou cancelamento.
- Adiciona a aba Sobre com versão, autoria e QR Code Pix.
- Melhora diálogos de confirmação e diagnóstico de armazenamento.

## [3.0]
- Evolui os scripts BAT para um aplicativo desktop baseado em Tauri e Rust.
- Introduz presets Express, Normal e Turbo.
- Reúne limpeza, privacidade, diagnóstico e manutenção do Windows em uma interface própria.
- Adiciona proteções para operações avançadas, incluindo pontos de restauração quando disponíveis e rollback para alterações reversíveis.
