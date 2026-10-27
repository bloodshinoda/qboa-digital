# Changelog — QBoa Digital

Todas as mudanças relevantes do QBoa Digital são registradas neste arquivo.

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
